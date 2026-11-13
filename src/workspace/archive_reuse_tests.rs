use super::*;
use crate::jar::test_support::ArchiveProbe;
use crate::{ClasspathEntry, PlatformEntry, Project, ResolveStatus};
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use zip::{ZipWriter, write::SimpleFileOptions};

const LIB: &[u8] = include_bytes!("../../tests/fixtures/classes/Lib.class");

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "java-engine-archive-reuse-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn archive(entries: &[(&str, &[u8])], jmod: bool) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    let bytes = zip.finish().unwrap().into_inner();
    if jmod { [b"JM\x01\x00".as_slice(), &bytes].concat() } else { bytes }
}
fn id(name: &str) -> CompilationId {
    CompilationId::new(name, "main")
}
fn workspace(inputs: &[ProjectConfig]) -> WorkspaceConfig {
    WorkspaceConfig {
        compilations: inputs
            .iter()
            .enumerate()
            .map(|(index, inputs)| CompilationConfig {
                id: id(&format!("scope{index}")),
                inputs: inputs.clone(),
                dependencies: vec![],
            })
            .collect(),
        ..Default::default()
    }
}
fn key(path: &Path) -> String {
    path.canonicalize().unwrap().to_str().unwrap().to_owned()
}
fn input(snapshot: &WorkspaceSnapshot, index: usize) -> &ProjectSnapshot {
    snapshot.scope(&id(&format!("scope{index}"))).unwrap().inputs()
}
fn renamed_lib() -> Vec<u8> {
    let mut bytes = LIB.to_vec();
    let at = bytes.windows(3).position(|bytes| bytes == b"run").unwrap();
    bytes[at..at + 3].copy_from_slice(b"ran");
    bytes
}

#[test]
fn workspace_reuses_successful_shared_archives_but_charges_every_occurrence() {
    let f = Fixture::new();
    let jar = f.write("shared.jar", archive(&[("lib/Lib.class", LIB)], false));
    let jmod = f.write("shared.jmod", archive(&[("classes/lib/Lib.class", LIB)], true));
    let config = ProjectConfig {
        platform: vec![PlatformEntry::Jmod(jmod.clone())],
        classpath: vec![ClasspathEntry::Jar(jar.clone()), ClasspathEntry::Jar(jar.clone())],
        ..Default::default()
    };
    let probe = ArchiveProbe::new();
    let snapshot = WorkspaceSnapshot::load(&workspace(&[config.clone(), config])).unwrap();
    assert_eq!(probe.paths(), [jmod.canonicalize().unwrap(), jar.canonicalize().unwrap()]);
    assert_eq!(probe.captures().len(), 6);
    for index in 0..2 {
        let input = input(&snapshot, index);
        assert_eq!(
            input.usage,
            (3, fs::metadata(&jmod).unwrap().len() + 2 * fs::metadata(&jar).unwrap().len(), 3 * LIB.len() as u64)
        );
        assert_eq!(input.shadowed_classes().len(), 2);
        assert_eq!(input.shadowed_classes()[0], input.shadowed_classes()[1]);
        assert!(input.class_origins()[0].ends_with("shared.jmod!/classes/lib/Lib.class"));
    }
    let origin = &input(&snapshot, 0).class_origins()[0];
    assert!(std::ptr::eq(
        input(&snapshot, 0).project().class_signature(origin).unwrap(),
        input(&snapshot, 1).project().class_signature(origin).unwrap()
    ));
}

#[test]
fn project_clone_shares_immutable_signatures_and_replacement_is_isolated() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Project>();
    send_sync::<WorkspaceSnapshot>();
    let mut original = Project::new();
    original.add_class_bytes("lib", LIB).unwrap();
    original.insert("Use.java", "class Use extends lib.Lib { void call() { run(); } }");
    let mut cloned = original.clone();
    assert!(std::ptr::eq(original.class_signature("lib").unwrap(), cloned.class_signature("lib").unwrap()));
    let prepared = original.prepare();
    let byte = original.file("Use.java").unwrap().text().find("run()").unwrap() as u32;
    cloned.add_class_bytes("lib", &renamed_lib()).unwrap();
    assert_eq!(cloned.resolve_at("Use.java", byte).status, ResolveStatus::Unresolved);
    assert_eq!(prepared.resolve_at("Use.java", byte).status, ResolveStatus::Resolved);
    assert!(cloned.remove_class("lib"));
    assert!(original.class_signature("lib").is_some());
    assert_eq!(prepared.resolve_at("Use.java", byte).status, ResolveStatus::Resolved);
}

#[test]
fn exact_target_release_and_archive_format_are_separate_reads() {
    let f = Fixture::new();
    let jar = f.write(
        "versions.jar",
        archive(
            &[
                ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\nMulti-Release: true\n\n"),
                ("lib/Lib.class", LIB),
                ("META-INF/versions/9/lib/Lib.class", LIB),
                ("META-INF/versions/17/lib/Lib.class", LIB),
            ],
            false,
        ),
    );
    let inputs: Vec<_> = [8, 8, 9, 17, 17, 7]
        .into_iter()
        .map(|target_release| ProjectConfig {
            classpath: vec![ClasspathEntry::Jar(jar.clone())],
            jar_options: crate::JarOptions { target_release, ..Default::default() },
            ..Default::default()
        })
        .collect();
    let probe = ArchiveProbe::new();
    let snapshot = WorkspaceSnapshot::load(&workspace(&inputs)).unwrap();
    assert_eq!(probe.paths().len(), 4);
    for (index, suffix) in [
        "!/lib/Lib.class",
        "!/lib/Lib.class",
        "!/META-INF/versions/9/lib/Lib.class",
        "!/META-INF/versions/17/lib/Lib.class",
        "!/META-INF/versions/17/lib/Lib.class",
        "!/lib/Lib.class",
    ]
    .iter()
    .enumerate()
    {
        assert!(input(&snapshot, index).class_origins()[0].ends_with(suffix));
    }
    drop(probe);
    let probe = ArchiveProbe::new();
    let plain = f.write("format.data", archive(&[("lib/Lib.class", LIB)], false));
    let jar_config = ProjectConfig { classpath: vec![ClasspathEntry::Jar(plain.clone())], ..Default::default() };
    let jmod_config = ProjectConfig { platform: vec![PlatformEntry::Jmod(plain)], ..Default::default() };
    let error = WorkspaceSnapshot::load(&workspace(&[jar_config, jmod_config])).unwrap_err();
    assert_eq!(error.input.unwrap().kind, crate::ProjectLoadErrorKind::Jmod);
    assert_eq!(probe.paths().len(), 2);
}

#[test]
fn tighter_limits_on_shared_jar_and_jmod_preserve_reader_errors() {
    let f = Fixture::new();
    for jmod in [false, true] {
        let name = if jmod { "classes/lib/Lib.class" } else { "lib/Lib.class" };
        // The resource, not a selected class, determines the largest entry.
        let jar = f.write("limits.data", archive(&[(name, LIB), ("ignored", &[0; 4096])], jmod));
        let loose = if jmod {
            ProjectConfig { platform: vec![PlatformEntry::Jmod(jar.clone())], ..Default::default() }
        } else {
            ProjectConfig { classpath: vec![ClasspathEntry::Jar(jar.clone())], ..Default::default() }
        };
        for limit in 0..7 {
            let mut tight = loose.clone();
            match limit {
                0 => {
                    tight.jar_options.max_entry_bytes = 1024;
                    tight.jmod_options.max_entry_bytes = 1024;
                }
                1 => {
                    tight.jar_options.max_entries = 1;
                    tight.jmod_options.max_entries = 1;
                }
                2 => {
                    tight.jar_options.max_archive_bytes = 1;
                    tight.jmod_options.max_archive_bytes = 1;
                }
                3 => {
                    tight.jar_options.max_total_uncompressed_bytes = 4200;
                    tight.jmod_options.max_total_uncompressed_bytes = 4200;
                }
                4 => tight.limits.max_input_bytes = 1,
                5 => tight.limits.max_entries = 1,
                6 => tight.limits.max_archive_expanded_bytes = 4200,
                _ => unreachable!(),
            }
            let expected = ProjectSnapshot::load(&tight).unwrap_err();
            let actual = WorkspaceSnapshot::load(&workspace(&[loose.clone(), tight])).unwrap_err().input.unwrap();
            assert_eq!(actual, expected, "jmod={jmod}, limit={limit}");
        }
        let loaded = ProjectSnapshot::load(&loose).unwrap();
        for limit in 0..3 {
            let mut config = workspace(&[loose.clone(), loose.clone()]);
            match limit {
                0 => config.max_entries = loaded.usage.0 * 2 - 1,
                1 => config.max_input_bytes = loaded.usage.1 * 2 - 1,
                2 => config.max_archive_expanded_bytes = loaded.usage.2 * 2 - 1,
                _ => unreachable!(),
            }
            assert!(WorkspaceSnapshot::load(&config).is_err(), "jmod={jmod}, aggregate={limit}");
        }
    }
}

#[test]
fn a_shared_archive_changed_during_load_is_redecoded() {
    let f = Fixture::new();
    let jar = f.write("shared.jar", archive(&[("lib/Lib.class", LIB)], false));
    let config = ProjectConfig { classpath: vec![ClasspathEntry::Jar(jar.clone())], ..Default::default() };
    let probe = ArchiveProbe::new();
    probe.after_read(move || fs::write(jar, archive(&[("lib/Lib.class", &renamed_lib())], false)).unwrap());
    let snapshot = WorkspaceSnapshot::load(&workspace(&[config.clone(), config])).unwrap();
    assert_eq!(probe.paths().len(), 2);
    let origin = &input(&snapshot, 0).class_origins()[0];
    let first = input(&snapshot, 0).project().class_signature(origin).unwrap();
    let second = input(&snapshot, 1).project().class_signature(origin).unwrap();
    assert!(first.methods.iter().any(|method| method.name == "run"));
    assert!(second.methods.iter().any(|method| method.name == "ran"));
    assert!(!std::ptr::eq(first, second));
}

#[test]
fn changed_or_removed_shared_archive_is_not_masked_and_old_snapshot_survives() {
    let f = Fixture::new();
    let jar = f.write("shared.jar", archive(&[("lib/Lib.class", LIB)], false));
    let project = ProjectConfig { classpath: vec![ClasspathEntry::Jar(jar.clone())], ..Default::default() };
    let config = workspace(&[project.clone(), project]);
    let old = WorkspaceSnapshot::load(&config).unwrap();
    let origin = input(&old, 0).class_origins()[0].clone();
    for remove in [false, true] {
        fs::write(&jar, archive(&[("lib/Lib.class", LIB)], false)).unwrap();
        let probe = ArchiveProbe::new();
        let path = jar.clone();
        probe.after_read(move || {
            if remove {
                fs::remove_file(path).unwrap();
            } else {
                fs::write(path, b"bad archive").unwrap();
            }
        });
        assert!(WorkspaceSnapshot::load(&config).is_err());
        assert!(
            input(&old, 0)
                .project()
                .class_signature(&origin)
                .unwrap()
                .methods
                .iter()
                .any(|method| method.name == "run")
        );
    }
}

#[cfg(unix)]
#[test]
fn every_shared_archive_occurrence_rechecks_symlinks() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let jar = f.write("shared.jar", archive(&[("lib/Lib.class", LIB)], false));
    let alias = f.0.join("alias.jar");
    symlink(&jar, &alias).unwrap();
    let first = ProjectConfig { classpath: vec![ClasspathEntry::Jar(jar.clone())], ..Default::default() };
    let second = ProjectConfig { classpath: vec![ClasspathEntry::Jar(alias)], ..Default::default() };
    assert_eq!(
        WorkspaceSnapshot::load(&workspace(&[first.clone(), second])).unwrap_err().input.unwrap().kind,
        crate::ProjectLoadErrorKind::InvalidPath
    );
    let replacement = f.write("replacement.jar", archive(&[("lib/Lib.class", LIB)], false));
    let probe = ArchiveProbe::new();
    probe.after_read(move || {
        fs::remove_file(&jar).unwrap();
        symlink(replacement, jar).unwrap();
    });
    assert_eq!(
        WorkspaceSnapshot::load(&workspace(&[first.clone(), first])).unwrap_err().input.unwrap().kind,
        crate::ProjectLoadErrorKind::InvalidPath
    );
    assert_eq!(probe.paths().len(), 1);
}

#[test]
fn exact_limits_reuse_and_effective_source_budgets_still_apply() {
    let f = Fixture::new();
    let jar_bytes = archive(&[("lib/Lib.class", LIB), ("ignored", &[0; 4096])], false);
    let jar = f.write("shared.jar", &jar_bytes);
    let loose = ProjectConfig { classpath: vec![ClasspathEntry::Jar(jar)], ..Default::default() };
    let mut exact = loose.clone();
    exact.jar_options.max_archive_bytes = jar_bytes.len() as u64;
    exact.jar_options.max_entries = 2;
    exact.jar_options.max_entry_bytes = 4096;
    exact.jar_options.max_total_uncompressed_bytes = (LIB.len() + 4096) as u64;
    let probe = ArchiveProbe::new();
    WorkspaceSnapshot::load(&workspace(&[loose.clone(), exact.clone()])).unwrap();
    assert_eq!(probe.paths().len(), 1);
    assert_eq!(probe.captures().len(), 2);
    drop(probe);
    let source = "class Use {}";
    f.write("src/Use.java", source);
    exact.source_roots.push(f.0.join("src"));
    exact.limits.max_input_bytes = (jar_bytes.len() + source.len() - 1) as u64;
    // Source consumption constrains the effective byte allowance for a hit.
    let expected = ProjectSnapshot::load(&exact).unwrap_err();
    assert_eq!(WorkspaceSnapshot::load(&workspace(&[loose.clone(), exact])).unwrap_err().input.unwrap(), expected);
    let mut tight = loose.clone();
    tight.jar_options.max_entries = 1;
    tight.jar_options.max_entry_bytes = 0;
    tight.jar_options.max_total_uncompressed_bytes = 0;
    let expected = ProjectSnapshot::load(&tight).unwrap_err();
    assert!(expected.detail.contains("entry count limit exceeded"));
    assert_eq!(WorkspaceSnapshot::load(&workspace(&[loose, tight])).unwrap_err().input.unwrap(), expected);
}

#[test]
fn newly_selected_malformed_version_and_distinct_path_origins_are_not_reused() {
    let f = Fixture::new();
    let bytes = archive(
        &[
            ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\nMulti-Release: true\n\n"),
            ("lib/Lib.class", LIB),
            ("META-INF/versions/17/lib/Lib.class", b"invalid class"),
        ],
        false,
    );
    let jar = f.write("versions.jar", bytes);
    let old = ProjectConfig {
        classpath: vec![ClasspathEntry::Jar(jar)],
        jar_options: crate::JarOptions { target_release: 8, ..Default::default() },
        ..Default::default()
    };
    let mut new = old.clone();
    new.jar_options.target_release = 17;
    let expected = ProjectSnapshot::load(&new).unwrap_err();
    let error = WorkspaceSnapshot::load(&workspace(&[old, new])).unwrap_err().input.unwrap();
    assert_eq!(error, expected);
    assert!(error.detail.contains("ClassFile"));

    let bytes = archive(&[("lib/Lib.class", LIB)], false);
    let first = f.write("a.jar", &bytes);
    let second = f.write("b.jar", &bytes);
    let alias = f.0.join("./a.jar");
    let configs: Vec<_> = [first.clone(), alias, second.clone()]
        .into_iter()
        .map(|path| ProjectConfig { classpath: vec![ClasspathEntry::Jar(path)], ..Default::default() })
        .collect();
    let probe = ArchiveProbe::new();
    let snapshot = WorkspaceSnapshot::load(&workspace(&configs)).unwrap();
    assert_eq!(probe.paths(), [first.canonicalize().unwrap(), second.canonicalize().unwrap()]);
    assert_eq!(probe.captures().len(), 3);
    assert_eq!(input(&snapshot, 0).class_origins(), input(&snapshot, 1).class_origins());
    assert_ne!(input(&snapshot, 0).class_origins(), input(&snapshot, 2).class_origins());
}

#[test]
fn fifo_byte_and_entry_caps_and_oversized_bypass_only_change_decode_work() {
    use crate::jar::ArchiveReads;
    let f = Fixture::new();
    let bytes = archive(&[("lib/Lib.class", LIB)], false);
    let paths: Vec<_> =
        ["a.jar", "b.jar", "c.jar"].into_iter().map(|path| f.write(path, &bytes).canonicalize().unwrap()).collect();
    let options = crate::JarOptions::default();
    for (max_bytes, max_entries) in [(bytes.len() * 2, 16), (usize::MAX, 2)] {
        let mut cache = ArchiveReads::with_limits(max_bytes, max_entries);
        let probe = ArchiveProbe::new();
        for index in [0, 1, 0, 2, 1, 0] {
            let loaded = cache.read_jar(&paths[index], &options).unwrap();
            assert_eq!(loaded.classes[0].origin, format!("{}!/lib/Lib.class", paths[index].display()));
        }
        // Hits do not promote FIFO entries. Inserting c evicts a, not b.
        assert_eq!(probe.paths(), [paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[0].clone()]);
        assert_eq!(probe.captures().len(), 6);
    }
    let large = f.write("oversized.jar", archive(&[("lib/Lib.class", LIB), ("ignored", &[0; 4096])], false));
    let mut cache = ArchiveReads::with_limits(bytes.len(), 16);
    let probe = ArchiveProbe::new();
    for path in [&paths[0], &large, &large, &paths[0]] {
        assert_eq!(cache.read_jar(path, &options).unwrap().classes.len(), 1);
    }
    // Oversized inputs do not flush useful entries and are never rejected.
    assert_eq!(probe.paths(), [paths[0].clone(), large.clone(), large]);
    assert_eq!(probe.captures().len(), 4);
}

#[test]
fn failures_are_not_cached_and_tighter_limit_misses_parse_captured_bytes() {
    use crate::jar::ArchiveReads;
    let f = Fixture::new();
    let jar = f.write("shared.jar", archive(&[("lib/Lib.class", LIB)], false));
    let options = crate::JarOptions::default();
    let mut cache = ArchiveReads::default();
    let probe = ArchiveProbe::new();
    cache.read_jar(&jar, &options).unwrap();
    let tight = crate::JarOptions { max_entries: 0, ..options.clone() };
    for _ in 0..2 {
        let error = cache.read_jar(&jar, &tight).unwrap_err();
        assert_eq!(error.kind, crate::JarErrorKind::LimitExceeded);
    }
    cache.read_jar(&jar, &options).unwrap();
    assert_eq!(probe.paths().len(), 3);
    assert_eq!(probe.captures().len(), 4);
    fs::write(&jar, b"broken archive").unwrap();
    for _ in 0..2 {
        assert!(cache.read_jar(&jar, &options).is_err());
    }
    fs::write(&jar, archive(&[("lib/Lib.class", LIB)], false)).unwrap();
    cache.read_jar(&jar, &options).unwrap();
    assert_eq!(probe.paths().len(), 6);
    assert_eq!(probe.captures().len(), 7);
}

#[test]
fn reload_and_overlay_sharing_preserve_old_source_and_binary_snapshots() {
    let f = Fixture::new();
    let jar = f.write("shared.jar", archive(&[("lib/Lib.class", LIB)], false));
    let source = "class Use extends lib.Lib { void call() { run(); } }";
    let path = key(&f.write("src/Use.java", source));
    let first = ProjectConfig {
        classpath: vec![ClasspathEntry::Jar(jar.clone())],
        source_roots: vec![f.0.join("src")],
        ..Default::default()
    };
    let second = ProjectConfig { classpath: first.classpath.clone(), ..Default::default() };
    let config = workspace(&[first, second]);
    let probe = ArchiveProbe::new();
    let old = WorkspaceSnapshot::load(&config).unwrap();
    let prepared = old.prepare();
    let byte = source.find("run()").unwrap() as u32;
    assert_eq!(prepared.scope(&id("scope0")).unwrap().resolve_at(&path, byte).status, ResolveStatus::Resolved);
    let overlay_text = source.replace("run()", "ran()");
    let overlay = old
        .with_source_overlays(&[SourceOverlay { owner: id("scope0"), path: path.clone(), text: overlay_text.clone() }])
        .unwrap();
    let origin = &input(&old, 0).class_origins()[0];
    assert!(std::ptr::eq(
        input(&old, 0).project().class_signature(origin).unwrap(),
        input(&overlay, 0).project().class_signature(origin).unwrap()
    ));
    assert_eq!(probe.paths().len(), 1);
    assert_eq!(probe.captures().len(), 2);
    fs::write(&jar, archive(&[("lib/Lib.class", &renamed_lib())], false)).unwrap();
    fs::write(&path, &overlay_text).unwrap();
    let next = WorkspaceSnapshot::load(&config).unwrap();
    assert_eq!(probe.paths().len(), 2);
    assert_eq!(probe.captures().len(), 4);
    assert_eq!(next.scope(&id("scope0")).unwrap().resolve_at(&path, byte).status, ResolveStatus::Resolved);
    assert_eq!(overlay.scope(&id("scope0")).unwrap().resolve_at(&path, byte).status, ResolveStatus::Unresolved);
    assert_eq!(prepared.scope(&id("scope0")).unwrap().resolve_at(&path, byte).status, ResolveStatus::Resolved);
    fs::remove_file(&path).unwrap();
    let mut removed = config.clone();
    for compilation in &mut removed.compilations {
        compilation.inputs.classpath.clear();
    }
    let removed = WorkspaceSnapshot::load(&removed).unwrap();
    assert!(input(&removed, 0).source_paths().is_empty());
    assert!(input(&removed, 0).class_origins().is_empty());
    fs::write(&jar, b"bad archive").unwrap();
    assert!(WorkspaceSnapshot::load(&config).is_err());
    assert_eq!(prepared.scope(&id("scope0")).unwrap().resolve_at(&path, byte).status, ResolveStatus::Resolved);
}

#[test]
fn load_session_recaptures_reuses_and_rejects_tighter_limits() {
    let f = Fixture::new();
    let jar = f.write("cache.jar", archive(&[("lib/Lib.class", LIB)], false));
    let cfg = ProjectConfig { classpath: vec![ClasspathEntry::Jar(jar.clone())], ..Default::default() };
    let config = workspace(&[cfg]);
    let mut session = WorkspaceLoadSession::default();
    let probe = ArchiveProbe::new();
    let first = session.load(&config).unwrap();
    let second = session.load(&config).unwrap();
    assert_eq!(probe.captures().len(), 2);
    assert_eq!(probe.paths().len(), 1);
    assert_eq!(input(&first, 0).usage, input(&second, 0).usage);
    let mut tight = config.clone();
    tight.max_input_bytes = 1;
    assert!(session.load(&tight).is_err());
    assert!(WorkspaceSnapshot::load(&tight).is_err());
    fs::write(&jar, archive(&[("lib/Lib.class", &renamed_lib())], false)).unwrap();
    let changed = session.load(&config).unwrap();
    let fresh = WorkspaceSnapshot::load(&config).unwrap();
    assert_eq!(input(&changed, 0).usage, input(&fresh, 0).usage);
    let origin = &input(&first, 0).class_origins()[0];
    assert!(!std::ptr::eq(
        input(&first, 0).project().class_signature(origin).unwrap(),
        input(&changed, 0).project().class_signature(origin).unwrap()
    ));
    fs::write(&jar, b"not an archive").unwrap();
    assert!(session.load(&config).is_err());
    fs::write(&jar, archive(&[("lib/Lib.class", LIB)], false)).unwrap();
    assert!(session.load(&config).is_ok());
}

#[test]
fn load_session_miss_parses_captured_bytes_not_mutated_disk() {
    let f = Fixture::new();
    let jar = f.write("changing.jar", archive(&[("lib/Lib.class", LIB)], false));
    let config =
        workspace(&[ProjectConfig { classpath: vec![ClasspathEntry::Jar(jar.clone())], ..Default::default() }]);
    let mut session = WorkspaceLoadSession::default();
    let old = session.load(&config).unwrap();
    fs::write(&jar, archive(&[("lib/Lib.class", &renamed_lib())], false)).unwrap();
    let probe = ArchiveProbe::new();
    let changed_path = jar.clone();
    probe.after_capture(move || fs::write(changed_path, b"corrupted after capture").unwrap());
    let captured = session.load(&config).unwrap();
    assert_eq!(probe.captures().len(), 1);
    let origin = &input(&old, 0).class_origins()[0];
    assert_ne!(
        format!("{:?}", input(&old, 0).project().class_signature(origin)),
        format!("{:?}", input(&captured, 0).project().class_signature(origin))
    );
    assert!(session.load(&config).is_err());
}
#[test]
fn load_session_does_not_hide_source_inventory_drift() {
    let f = Fixture::new();
    let path = f.write("src/A.java", "class A {}");
    let config =
        workspace(&[ProjectConfig { source_roots: vec![path.parent().unwrap().to_owned()], ..Default::default() }]);
    let mut session = WorkspaceLoadSession::default();
    let old = session.load(&config).unwrap();
    let frozen = input(&old, 0).source_paths().to_vec();
    f.write("src/B.java", "class B {}");
    let next = session.load(&config).unwrap();
    assert_ne!(input(&next, 0).source_paths(), frozen.as_slice());
    // Existing editor compares this inventory and refuses drift, unchanged.
}

#[test]
fn load_session_warm_identity_budget_and_partial_failure_match_fresh() {
    let f = Fixture::new();
    let jar = f.write("a.jar", archive(&[("lib/Lib.class", LIB)], false));
    let other = f.write("b.jar", archive(&[("lib/Lib.class", &renamed_lib())], false));
    let src = f.write("src/A.java", "class A {}");
    let base = workspace(&[ProjectConfig {
        source_roots: vec![src.parent().unwrap().into()],
        classpath: vec![ClasspathEntry::Jar(jar.clone()), ClasspathEntry::Jar(other.clone())],
        ..Default::default()
    }]);
    let mut session = WorkspaceLoadSession::default();
    let original = session.load(&base).unwrap();
    fn fingerprint(w: &WorkspaceSnapshot) -> String {
        let mut parts = Vec::new();
        for (i, id) in w.ids.iter().enumerate() {
            let p = &w.inputs[i];
            parts.push(format!("{id:?}:{:?}:{:?}:{:?}", p.usage, p.class_origins(), p.shadowed_classes()));
            for path in p.source_paths() {
                parts.push(format!("{path}:{}", p.project().file(path).unwrap().text()));
            }
            for path in p.class_origins() {
                parts.push(format!("{:?}", p.project().class_signature(path)));
            }
        }
        format!("{parts:?}:{:?}", w.visibility)
    }
    let original_record = fingerprint(&original);
    let mut variants = Vec::new();
    for n in 0..8 {
        let mut c = base.clone();
        match n {
            0 => c.compilations[0].inputs.jar_options.max_entries = 0,
            1 => c.compilations[0].inputs.jar_options.max_entry_bytes = 1,
            2 => c.compilations[0].inputs.jar_options.max_total_uncompressed_bytes = 1,
            3 => c.max_archive_expanded_bytes = LIB.len() as u64,
            4 => c.compilations[0].inputs.jar_options.target_release = 8,
            5 => c.compilations[0].inputs.classpath.reverse(),
            6 => c.compilations[0].id = id("other-scope"),
            _ => c.compilations[0].inputs.platform = vec![PlatformEntry::Jmod(jar.clone())],
        }
        variants.push(c);
    }
    for c in variants {
        let a = session.load(&c).map(|w| fingerprint(&w)).map_err(|e| format!("{e:?}"));
        let b = WorkspaceSnapshot::load(&c).map(|w| fingerprint(&w)).map_err(|e| format!("{e:?}"));
        assert_eq!(a, b);
    }
    fs::write(&other, b"corrupt later archive").unwrap();
    assert!(session.load(&base).is_err());
    fs::write(&other, archive(&[("lib/Lib.class", LIB)], false)).unwrap();
    fs::write(&src, "class A { int changed; }").unwrap();
    assert_eq!(fingerprint(&session.load(&base).unwrap()), fingerprint(&WorkspaceSnapshot::load(&base).unwrap()));
    assert_eq!(fingerprint(&original), original_record);
}
