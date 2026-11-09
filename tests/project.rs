use java_engine::{ClasspathEntry, ProjectConfig, ProjectSnapshot, ResolveStatus};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "java-engine-project-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
    fn config(&self) -> ProjectConfig {
        ProjectConfig {
            source_roots: vec![self.0.join("src")],
            classpath: vec![ClasspathEntry::Directory(self.0.join("classes"))],
            ..Default::default()
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn key(path: &Path) -> String {
    path.canonicalize().unwrap().to_str().unwrap().to_owned()
}
const USE: &str = "package app; class Use extends Bridge { void call() { run(); } }";

#[test]
fn source_roots_and_classpath_resolve_without_registering_files_individually() {
    let f = Fixture::new();
    f.write("classes/lib/Lib.class", include_bytes!("fixtures/classes/Lib.class"));
    f.write("src/app/Bridge.java", "package app; public class Bridge extends lib.Lib {}");
    let use_file = f.write("src/app/Use.java", USE);
    f.write("src/ignored.txt", "not Java");
    let snapshot = ProjectSnapshot::load(&f.config()).unwrap();
    assert_eq!(snapshot.source_paths().len(), 2);
    let result = snapshot.resolve_at(&key(&use_file), USE.find("run()").unwrap() as u32);
    assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
    assert_eq!(result.targets[0].owner, "lib.Lib");
    assert_eq!(result.targets[0].path, key(&f.0.join("classes/lib/Lib.class")));
}

#[test]
fn reloading_removed_inputs_drops_stale_targets_but_keeps_old_snapshot() {
    let f = Fixture::new();
    let dependency = f.write("classes/lib/Lib.class", include_bytes!("fixtures/classes/Lib.class"));
    let bridge = f.write("src/app/Bridge.java", "package app; public class Bridge extends lib.Lib {}");
    let use_path = key(&f.write("src/app/Use.java", USE));
    let old = ProjectSnapshot::load(&f.config()).unwrap();
    let byte = USE.find("run()").unwrap() as u32;
    assert_eq!(old.resolve_at(&use_path, byte).status, ResolveStatus::Resolved);
    fs::remove_file(dependency).unwrap();
    let next = ProjectSnapshot::load(&f.config()).unwrap();
    assert_eq!(next.resolve_at(&use_path, byte).status, ResolveStatus::Unresolved);
    assert_eq!(old.resolve_at(&use_path, byte).status, ResolveStatus::Resolved);
    fs::remove_file(bridge).unwrap();
    let next = ProjectSnapshot::load(&f.config()).unwrap();
    assert_eq!(next.source_paths().len(), 1);
    assert_eq!(old.source_paths().len(), 2);
}

fn jar_bytes(class: &[u8]) -> Vec<u8> {
    use std::io::{Cursor, Write};
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};
    let mut jar = ZipWriter::new(Cursor::new(Vec::new()));
    jar.start_file("lib/Lib.class", SimpleFileOptions::default().compression_method(CompressionMethod::Deflated))
        .unwrap();
    jar.write_all(class).unwrap();
    jar.finish().unwrap().into_inner()
}
fn renamed_lib() -> Vec<u8> {
    let mut bytes = include_bytes!("fixtures/classes/Lib.class").to_vec();
    let at = bytes.windows(3).position(|window| window == b"run").unwrap();
    bytes[at..at + 3].copy_from_slice(b"ran");
    bytes
}

#[test]
fn jar_dependency_changes_and_classpath_removal_are_snapshot_scoped() {
    let f = Fixture::new();
    let jar = f.write("deps/library.jar", jar_bytes(include_bytes!("fixtures/classes/Lib.class")));
    f.write("src/app/Bridge.java", "package app; public class Bridge extends lib.Lib {}");
    let use_path = key(&f.write("src/app/Use.java", USE));
    let config = ProjectConfig {
        source_roots: vec![f.0.join("src")],
        classpath: vec![ClasspathEntry::Jar(jar.clone())],
        ..Default::default()
    };
    let byte = USE.find("run()").unwrap() as u32;
    let old = ProjectSnapshot::load(&config).unwrap();
    let result = old.resolve_at(&use_path, byte);
    assert_eq!(result.status, ResolveStatus::Resolved);
    assert_eq!(result.targets[0].path, format!("{}!/lib/Lib.class", key(&jar)));
    fs::write(&jar, jar_bytes(&renamed_lib())).unwrap();
    let next = ProjectSnapshot::load(&config).unwrap();
    assert_eq!(next.resolve_at(&use_path, byte).status, ResolveStatus::Unresolved);
    assert_eq!(old.resolve_at(&use_path, byte).status, ResolveStatus::Resolved);
    let no_dependency = ProjectConfig { classpath: vec![], ..config.clone() };
    assert_eq!(
        ProjectSnapshot::load(&no_dependency).unwrap().resolve_at(&use_path, byte).status,
        ResolveStatus::Unresolved
    );
    fs::write(&jar, b"broken archive").unwrap();
    assert!(ProjectSnapshot::load(&config).is_err());
    assert_eq!(old.resolve_at(&use_path, byte).status, ResolveStatus::Resolved);
}

#[test]
fn binary_classpath_order_is_explicit_and_shadowed_origins_are_reported() {
    let f = Fixture::new();
    let class = f.write("classes/lib/Lib.class", include_bytes!("fixtures/classes/Lib.class"));
    let jar = f.write("deps/library.jar", jar_bytes(&renamed_lib()));
    let use_path = key(&f.write("src/Use.java", "class Use extends lib.Lib { void call() { run(); } }"));
    let mut config = f.config();
    config.classpath.push(ClasspathEntry::Jar(jar.clone()));
    let first = ProjectSnapshot::load(&config).unwrap();
    assert_eq!(first.class_origins(), &[key(&class)]);
    assert_eq!(first.shadowed_classes().len(), 1);
    assert_eq!(first.shadowed_classes()[0].binary_name, "lib/Lib");
    assert_eq!(first.shadowed_classes()[0].selected_origin, key(&class));
    assert_eq!(first.shadowed_classes()[0].shadowed_origin, format!("{}!/lib/Lib.class", key(&jar)));
    let byte = first.project().file(&use_path).unwrap().text().find("run()").unwrap() as u32;
    assert_eq!(first.resolve_at(&use_path, byte).status, ResolveStatus::Resolved);
    config.classpath.reverse();
    let second = ProjectSnapshot::load(&config).unwrap();
    assert_eq!(second.resolve_at(&use_path, byte).status, ResolveStatus::Unresolved);
    assert_eq!(second.shadowed_classes()[0].selected_origin, format!("{}!/lib/Lib.class", key(&jar)));
}

#[test]
fn source_binary_collision_stays_ambiguous_and_duplicate_root_is_an_error() {
    use java_engine::ProjectLoadErrorKind;
    let f = Fixture::new();
    f.write("classes/lib/Lib.class", include_bytes!("fixtures/classes/Lib.class"));
    f.write("src/lib/Lib.java", "package lib; public class Lib {}");
    let source = "import lib.Lib; class Use { Lib value; }";
    let path = key(&f.write("src/Use.java", source));
    let config = f.config();
    let snapshot = ProjectSnapshot::load(&config).unwrap();
    assert_eq!(snapshot.resolve_at(&path, source.find("Lib value").unwrap() as u32).status, ResolveStatus::Ambiguous);
    let mut duplicate = config.clone();
    duplicate.source_roots.push(f.0.join("src/lib"));
    assert_eq!(ProjectSnapshot::load(&duplicate).unwrap_err().kind, ProjectLoadErrorKind::DuplicateSource);
}

#[test]
fn missing_wrong_path_utf8_module_and_class_name_fail_explicitly() {
    use java_engine::ProjectLoadErrorKind as K;
    let f = Fixture::new();
    let mut config = ProjectConfig { source_roots: vec![f.0.join("missing")], ..Default::default() };
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::Io);
    let source = f.write("src/Bad.java", [0xff]);
    config.source_roots = vec![source];
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::InvalidPath);
    config.source_roots = vec![f.0.join("src")];
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::InvalidUtf8);
    f.write("src/Bad.java", "class Bad {");
    let incomplete = ProjectSnapshot::load(&config).unwrap();
    assert!(!incomplete.project().file(&incomplete.source_paths()[0]).unwrap().errors().is_empty());
    let module = f.write("src/module-info.java", "module app {}");
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::UnsupportedModule);
    fs::remove_file(module).unwrap();
    f.write("classes/wrong/Lib.class", include_bytes!("fixtures/classes/Lib.class"));
    assert_eq!(ProjectSnapshot::load(&f.config()).unwrap_err().kind, K::ClassFile);
}

#[test]
fn filesystem_limits_apply_before_inputs_are_accepted() {
    use java_engine::{ProjectLimits, ProjectLoadErrorKind};
    let f = Fixture::new();
    f.write("src/deep/nested/A.java", "class A {}");
    for limits in [
        ProjectLimits { max_entries: 1, ..Default::default() },
        ProjectLimits { max_file_bytes: 1, ..Default::default() },
        ProjectLimits { max_input_bytes: 1, ..Default::default() },
        ProjectLimits { max_directory_depth: 1, ..Default::default() },
    ] {
        let config = ProjectConfig { source_roots: vec![f.0.join("src")], limits, ..Default::default() };
        assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, ProjectLoadErrorKind::LimitExceeded);
    }
    let mut config = f.config();
    config.limits.max_classpath_entries = 0;
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, ProjectLoadErrorKind::LimitExceeded);
}

#[cfg(unix)]
#[test]
fn directory_symlinks_and_link_entries_are_rejected() {
    use java_engine::ProjectLoadErrorKind;
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let file = f.write("real/A.java", "class A {}");
    symlink(f.0.join("real"), f.0.join("src")).unwrap();
    let config = ProjectConfig { source_roots: vec![f.0.join("src")], ..Default::default() };
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, ProjectLoadErrorKind::InvalidPath);
    fs::remove_file(f.0.join("src")).unwrap();
    fs::create_dir(f.0.join("src")).unwrap();
    symlink(file, f.0.join("src/A.java")).unwrap();
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, ProjectLoadErrorKind::InvalidPath);
}

#[test]
fn committed_source_to_jar_project_loads_without_a_jvm() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project");
    let config = ProjectConfig {
        source_roots: vec![root.join("src")],
        classpath: vec![ClasspathEntry::Jar(root.join("deps/library.jar"))],
        ..Default::default()
    };
    let snapshot = ProjectSnapshot::load(&config).unwrap();
    let path = key(&root.join("src/app/Use.java"));
    let source = snapshot.project().file(&path).unwrap().text();
    assert_eq!(snapshot.resolve_at(&path, source.find("run()").unwrap() as u32).status, ResolveStatus::Resolved);
    assert_eq!(snapshot.source_paths().len(), 2);
    assert_eq!(snapshot.class_origins().len(), 1);
    let mut zip = zip::ZipArchive::new(std::fs::File::open(root.join("deps/library.jar")).unwrap()).unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut zip.by_name("lib/Lib.class").unwrap(), &mut bytes).unwrap();
    assert_eq!(bytes, include_bytes!("fixtures/classes/Lib.class"));
}

#[test]
fn aggregate_archive_limits_include_ignored_entries_and_resources() {
    let f = Fixture::new();
    use std::io::{Cursor, Write};
    use zip::{ZipWriter, write::SimpleFileOptions};
    let lib = include_bytes!("fixtures/classes/Lib.class");
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("lib/Lib.class", lib.as_slice()),
        ("ignored-resource", &[0_u8; 17]),
        ("META-INF/versions/99/lib/Lib.class", b"ignored"),
    ] {
        archive.start_file(name, SimpleFileOptions::default()).unwrap();
        archive.write_all(bytes).unwrap();
    }
    let archive = archive.finish().unwrap().into_inner();
    let jar1 = f.write("one.jar", &archive);
    let jar2 = f.write("two.jar", &archive);
    let mut config =
        ProjectConfig { classpath: vec![ClasspathEntry::Jar(jar1), ClasspathEntry::Jar(jar2)], ..Default::default() };
    config.limits.max_archive_expanded_bytes = lib.len() as u64 + 17 + 7;
    assert!(ProjectSnapshot::load(&config).is_err());
    config.limits.max_archive_expanded_bytes *= 2;
    assert_eq!(ProjectSnapshot::load(&config).unwrap().shadowed_classes().len(), 1);
    config.limits.max_entries = 5;
    assert!(ProjectSnapshot::load(&config).is_err());
    config.limits.max_entries = 6;
    config.limits.max_input_bytes = archive.len() as u64 * 2 - 1;
    assert!(ProjectSnapshot::load(&config).is_err());
    config.limits.max_input_bytes += 1;
    assert!(ProjectSnapshot::load(&config).is_ok());
}

#[test]
fn directory_and_archive_origin_collision_cannot_overwrite_a_distinct_binary() {
    use java_engine::ProjectLoadErrorKind;
    use std::io::{Cursor, Write};
    use zip::{ZipWriter, write::SimpleFileOptions};
    let f = Fixture::new();
    let lib = include_bytes!("fixtures/classes/Lib.class");
    // Change the fixture's binary name, retaining its independent constant-pool shape.
    let name = b"META-INF/versions/17/lib/Lib";
    let position = lib.windows(7).position(|bytes| bytes == b"lib/Lib").unwrap();
    let mut directory_class = lib[..position - 2].to_vec();
    directory_class.extend_from_slice(&(name.len() as u16).to_be_bytes());
    directory_class.extend_from_slice(name);
    directory_class.extend_from_slice(&lib[position + 7..]);
    f.write("x.jar!/META-INF/versions/17/lib/Lib.class", directory_class);
    let mut jar = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\r\nMulti-Release: true\r\n\r\n".as_slice()),
        ("META-INF/versions/17/lib/Lib.class", lib.as_slice()),
    ] {
        jar.start_file(name, SimpleFileOptions::default()).unwrap();
        jar.write_all(bytes).unwrap();
    }
    let archive = f.write("x.jar", jar.finish().unwrap().into_inner());
    let mut config = ProjectConfig {
        classpath: vec![ClasspathEntry::Directory(f.0.join("x.jar!")), ClasspathEntry::Jar(archive)],
        ..Default::default()
    };
    for _ in 0..2 {
        assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, ProjectLoadErrorKind::ConflictingOrigin);
        config.classpath.reverse();
    }
}
