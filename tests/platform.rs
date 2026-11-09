use java_engine::{ClasspathEntry, PlatformEntry, ProjectConfig, ProjectSnapshot, ResolveStatus};
use std::{
    fs,
    io::{Cursor, Write},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use zip::{ZipWriter, write::SimpleFileOptions};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "java-engine-platform-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn write(&self, name: &str, data: impl AsRef<[u8]>) -> PathBuf {
        let p = self.0.join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, data).unwrap();
        p
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
// Minimal JVMS class signatures, assembled here without a Java compiler.
fn class(name: &str, method: &str) -> Vec<u8> {
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61];
    fn u(b: &mut Vec<u8>, n: u16) {
        b.extend(n.to_be_bytes());
    }
    fn utf(b: &mut Vec<u8>, s: &str) {
        b.push(1);
        u(b, s.len() as u16);
        b.extend(s.as_bytes());
    }
    u(&mut b, 7);
    utf(&mut b, name);
    b.push(7);
    u(&mut b, 1);
    utf(&mut b, "java/lang/Object");
    b.push(7);
    u(&mut b, 3);
    utf(&mut b, method);
    utf(&mut b, "()Ljava/lang/String;");
    u(&mut b, 0x21);
    u(&mut b, 2);
    u(&mut b, if name == "java/lang/Object" { 0 } else { 4 });
    u(&mut b, 0);
    u(&mut b, 0);
    u(&mut b, 1);
    u(&mut b, 0x101);
    u(&mut b, 5);
    u(&mut b, 6);
    u(&mut b, 0);
    u(&mut b, 0);
    b
}
fn archive(jmod: bool, method: &str) -> Vec<u8> {
    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("java/lang/Object.class", class("java/lang/Object", method)),
        ("java/lang/String.class", class("java/lang/String", "intern")),
        ("module-info.class", b"ignored module descriptor".to_vec()),
        ("resource", vec![0; 17]),
    ] {
        let name = if jmod { format!("classes/{name}") } else { name.into() };
        z.start_file(name, SimpleFileOptions::default()).unwrap();
        z.write_all(&data).unwrap();
    }
    let zip = z.finish().unwrap().into_inner();
    if jmod { [b"JM\x01\x00".as_slice(), &zip].concat() } else { zip }
}
const SOURCE: &str = "class Use { String value; void call() { toString(); } }";
fn source(f: &Fixture) -> (ProjectConfig, String) {
    let p = f.write("src/Use.java", SOURCE).canonicalize().unwrap().to_str().unwrap().to_owned();
    (ProjectConfig { source_roots: vec![f.0.join("src")], ..Default::default() }, p)
}
fn status(s: &ProjectSnapshot, p: &str, needle: &str) -> ResolveStatus {
    s.resolve_at(p, SOURCE.find(needle).unwrap() as u32).status
}
#[test]
fn explicit_platform_enables_java_lang_and_object_members_without_discovery() {
    let f = Fixture::new();
    let (mut c, p) = source(&f);
    let empty = ProjectSnapshot::load(&c).unwrap();
    assert_eq!(status(&empty, &p, "String"), ResolveStatus::Unresolved);
    assert_eq!(status(&empty, &p, "toString"), ResolveStatus::Unresolved);
    c.platform = vec![PlatformEntry::Jmod(f.write("java.base.jmod", archive(true, "toString")))];
    let s = ProjectSnapshot::load(&c).unwrap();
    assert_eq!(status(&s, &p, "String"), ResolveStatus::Resolved);
    assert_eq!(s.resolve_at(&p, SOURCE.find("String").unwrap() as u32).targets[0].owner, "java.lang.String");
    assert_eq!(status(&s, &p, "toString"), ResolveStatus::Resolved);
    let r = s.resolve_at(&p, SOURCE.find("toString").unwrap() as u32);
    assert_eq!(r.targets[0].owner, "java.lang.Object");
    assert!(r.targets[0].path.ends_with("java.base.jmod!/classes/java/lang/Object.class"));
    assert_eq!(s.class_origins().len(), 2);
}
#[test]
fn platform_precedes_application_and_preserves_source_ambiguity() {
    let f = Fixture::new();
    let (mut c, p) = source(&f);
    let platform = f.write("rt.jar", archive(false, "toString"));
    let later = f.write("later.jmod", archive(true, "other"));
    c.platform = vec![PlatformEntry::Jar(platform), PlatformEntry::Jmod(later)];
    c.classpath = vec![ClasspathEntry::Jar(f.write("app.jar", archive(false, "other")))];
    let s = ProjectSnapshot::load(&c).unwrap();
    assert_eq!(status(&s, &p, "toString"), ResolveStatus::Resolved);
    assert_eq!(s.shadowed_classes().len(), 4);
    assert!(s.shadowed_classes().iter().all(|v| v.selected_origin.contains("rt.jar!/")));
    c.platform.reverse();
    assert_eq!(status(&ProjectSnapshot::load(&c).unwrap(), &p, "toString"), ResolveStatus::Unresolved);
    f.write("src/String.java", "package java.lang; public class String {}");
    assert_eq!(status(&ProjectSnapshot::load(&c).unwrap(), &p, "String"), ResolveStatus::Ambiguous);
}
#[test]
fn platform_change_removal_and_failure_leave_old_snapshot_intact() {
    let f = Fixture::new();
    let (mut c, p) = source(&f);
    let path = f.write("base.jmod", archive(true, "toString"));
    c.platform = vec![PlatformEntry::Jmod(path.clone())];
    let old = ProjectSnapshot::load(&c).unwrap();
    fs::write(&path, archive(true, "other")).unwrap();
    assert_eq!(status(&ProjectSnapshot::load(&c).unwrap(), &p, "toString"), ResolveStatus::Unresolved);
    fs::write(&path, b"broken").unwrap();
    assert!(ProjectSnapshot::load(&c).is_err());
    c.platform.clear();
    assert_eq!(status(&ProjectSnapshot::load(&c).unwrap(), &p, "String"), ResolveStatus::Unresolved);
    assert_eq!(status(&old, &p, "toString"), ResolveStatus::Resolved);
}
#[test]
fn platform_and_application_share_all_aggregate_archive_limits() {
    let f = Fixture::new();
    let jmod = archive(true, "toString");
    let jar = archive(false, "toString");
    let mut c = ProjectConfig {
        platform: vec![PlatformEntry::Jmod(f.write("base.jmod", &jmod))],
        classpath: vec![ClasspathEntry::Jar(f.write("app.jar", &jar))],
        ..Default::default()
    };
    c.limits.max_classpath_entries = 1;
    assert!(ProjectSnapshot::load(&c).is_err());
    c.limits.max_classpath_entries = 2;
    c.limits.max_entries = 7;
    assert!(ProjectSnapshot::load(&c).is_err());
    c.limits.max_entries = 8;
    c.limits.max_input_bytes = (jmod.len() + jar.len()) as u64 - 1;
    assert!(ProjectSnapshot::load(&c).is_err());
    c.limits.max_input_bytes += 1;
    let expanded =
        (class("java/lang/Object", "toString").len() + class("java/lang/String", "intern").len() + 25 + 17) as u64 * 2;
    c.limits.max_archive_expanded_bytes = expanded - 1;
    assert!(ProjectSnapshot::load(&c).is_err());
    c.limits.max_archive_expanded_bytes = expanded;
    assert!(ProjectSnapshot::load(&c).is_ok());
}

#[test]
fn platform_origin_conflicts_with_directory_are_explicit_errors() {
    use java_engine::ProjectLoadErrorKind;
    let f = Fixture::new();
    let platform = f.write("base.jmod", archive(true, "toString"));
    f.write("base.jmod!/classes/java/lang/Object.class", class("classes/java/lang/Object", "other"));
    let c = ProjectConfig {
        platform: vec![PlatformEntry::Jmod(platform)],
        classpath: vec![ClasspathEntry::Directory(f.0.join("base.jmod!"))],
        ..Default::default()
    };
    assert_eq!(ProjectSnapshot::load(&c).unwrap_err().kind, ProjectLoadErrorKind::ConflictingOrigin);
    // Even a shadowed platform class must not give a different class the same origin.
    let first = f.write("first.jar", archive(false, "toString"));
    let mut c = c;
    c.platform.insert(0, PlatformEntry::Jar(first));
    assert_eq!(ProjectSnapshot::load(&c).unwrap_err().kind, ProjectLoadErrorKind::ConflictingOrigin);
}

#[test]
fn platform_path_and_per_archive_errors_keep_their_origin() {
    use java_engine::ProjectLoadErrorKind;
    let f = Fixture::new();
    let path = f.0.join("base.jmod");
    let mut c = ProjectConfig { platform: vec![PlatformEntry::Jmod(path.clone())], ..Default::default() };
    assert_eq!(ProjectSnapshot::load(&c).unwrap_err().kind, ProjectLoadErrorKind::Io);
    f.write("base.jmod", b"not a jmod");
    let err = ProjectSnapshot::load(&c).unwrap_err();
    assert_eq!(err.kind, ProjectLoadErrorKind::Jmod);
    assert_eq!(err.origin, path.canonicalize().unwrap().to_str().unwrap());
    f.write("base.jmod", archive(true, "toString"));
    c.jmod_options.max_entry_bytes = 1;
    assert_eq!(ProjectSnapshot::load(&c).unwrap_err().kind, ProjectLoadErrorKind::Jmod);
}
