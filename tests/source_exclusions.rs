//! Literal root-relative source selection. No JVM, external tools or input downloads.
use java_engine::{
    ClasspathEntry, CompilationConfig, CompilationId, ProjectConfig, ProjectLoadErrorKind as K, ProjectSnapshot,
    ResolveStatus, SourceOverlay, SourceOverlayErrorKind, WorkspaceConfig, WorkspaceLoadErrorKind, WorkspaceSnapshot,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "java-engine-selection-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
    fn config(&self, rules: &[&str]) -> ProjectConfig {
        ProjectConfig {
            source_roots: vec![self.0.join("src")],
            source_exclusions: rules.iter().map(|s| (*s).into()).collect(),
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
fn compilation(name: &str, inputs: ProjectConfig) -> CompilationConfig {
    CompilationConfig { id: CompilationId::new(name, "main"), inputs, dependencies: vec![] }
}
fn workspace(inputs: ProjectConfig) -> WorkspaceConfig {
    WorkspaceConfig { compilations: vec![compilation("app", inputs)], ..Default::default() }
}

#[test]
fn source_exclusions_validate_all_forbidden_components_without_io() {
    let f = Fixture::new(); // The configured src deliberately does not exist.
    for rule in [
        "", "/", "/a", "a/", "a//b", ".", "..", "a/./b", "a/../b", "a\\b", "a:b", "*.java", "a?", "a[b", "a]b", "a{b",
        "a}b",
    ] {
        let config = f.config(&[rule]);
        assert_eq!(config.validate_source_exclusions().unwrap_err().kind, K::InvalidPath, "{rule:?}");
        assert_eq!(
            ProjectSnapshot::load(&config).unwrap_err().kind,
            K::InvalidPath,
            "validation must precede source I/O: {rule:?}"
        );
    }
    for c in (0..=0x1f).chain(0x7f..=0x9f).map(|c| char::from_u32(c).unwrap()) {
        let rule = format!("a{c}b");
        assert_eq!(
            f.config(&[&rule]).validate_source_exclusions().unwrap_err().kind,
            K::InvalidPath,
            "Cc U+{:04X}",
            c as u32
        );
    }
}

#[test]
fn source_exclusions_preserve_literal_unicode_spaces_and_punctuation() {
    let f = Fixture::new();
    for rule in [
        " ",
        " spaced name /a.java",
        "中文/文件.java",
        "!literal",
        "--cancel-on-stdin-close",
        "...",
        "a..b",
        "é.java",
        "e\u{301}.java",
        "a\u{200d}b",
        "a\u{2028}b",
    ] {
        assert!(f.config(&[rule]).validate_source_exclusions().is_ok(), "{rule:?}");
    }
    assert!(f.config(&["a", "a/b", "a/b/C.java", "missing"]).validate_source_exclusions().is_ok());
    assert_eq!(f.config(&["a", "a"]).validate_source_exclusions().unwrap_err().kind, K::InvalidPath);
    assert!(f.config(&["a", "A"]).validate_source_exclusions().is_ok());
}

#[test]
fn source_exclusions_have_exact_rule_count_and_utf8_byte_bounds() {
    let f = Fixture::new();
    let mut config = f.config(&[]);
    config.source_exclusions = (0..1024).map(|i| format!("missing/{i}")).collect();
    assert!(config.validate_source_exclusions().is_ok());
    config.source_exclusions.push("last".into());
    assert_eq!(config.validate_source_exclusions().unwrap_err().kind, K::LimitExceeded);
    for rule in ["x".repeat(4096), format!("{}x", "中".repeat(1365))] {
        config.source_exclusions = vec![rule.clone()];
        assert!(config.validate_source_exclusions().is_ok());
        config.source_exclusions[0].push('x');
        assert_eq!(config.validate_source_exclusions().unwrap_err().kind, K::LimitExceeded);
    }
}

#[test]
fn source_exclusions_need_roots_but_do_not_check_root_existence() {
    let f = Fixture::new();
    assert!(ProjectConfig::default().validate_source_exclusions().is_ok());
    let mut config = f.config(&["missing"]);
    assert!(config.validate_source_exclusions().is_ok());
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::Io);
    config.source_roots.clear();
    config.classpath.push(ClasspathEntry::Directory(f.0.join("also-missing")));
    assert_eq!(config.validate_source_exclusions().unwrap_err().kind, K::InvalidPath);
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::InvalidPath);
}

#[test]
fn source_exclusions_are_exact_case_sensitive_component_paths() {
    let f = Fixture::new();
    // Distinct stored names also work on case-insensitive macOS filesystems.
    let paths = [
        "A.java",
        "Keep.java",
        "nested/A.java",
        "generated/B.java",
        "generated-more/C.java",
        "!literal.java",
        "--cancel-on-stdin-close",
        "中文/文件.java",
        "spaced name/D.java",
    ];
    for path in paths {
        f.write(&format!("src/{path}"), "class A {}");
    }
    let config = f.config(&[
        "A.java",
        "keep.java",
        "generated",
        "!literal.java",
        "--cancel-on-stdin-close",
        "中文/文件.java",
        "spaced name/D.java",
        "missing",
    ]);
    let selected = ProjectSnapshot::load(&config).unwrap();
    let mut expected =
        ["Keep.java", "nested/A.java", "generated-more/C.java"].map(|p| key(&f.0.join("src").join(p))).to_vec();
    expected.sort();
    assert_eq!(selected.source_paths(), expected);
    assert_eq!(ProjectSnapshot::load(&f.config(&[])).unwrap().source_paths().len(), 8);
}

#[test]
fn source_exclusions_compare_stored_unicode_spelling_without_normalization() {
    let f = Fixture::new();
    f.write("src/é.java", "class Accent {}");
    // Some filesystems expose a decomposed spelling. Only create one file, and
    // compare selection against the spelling actually returned by the walk.
    let stored = fs::read_dir(f.0.join("src")).unwrap().next().unwrap().unwrap();
    let spelling = stored.file_name().into_string().unwrap();
    let alternate = match spelling.as_str() {
        "é.java" => "e\u{301}.java",
        "e\u{301}.java" => "é.java",
        _ => panic!("unexpected stored spelling: {spelling:?}"),
    };
    assert_eq!(ProjectSnapshot::load(&f.config(&[alternate])).unwrap().source_paths(), &[key(&stored.path())]);
    assert!(ProjectSnapshot::load(&f.config(&[&spelling])).unwrap().source_paths().is_empty());
}

#[test]
fn source_exclusions_select_files_or_prune_real_directories_independent_of_extension() {
    let f = Fixture::new();
    let keep = key(&f.write("src/Keep.java", "class Keep {}"));
    f.write("src/file.java", [0xff]);
    f.write("src/dir.java/deep/module-info.java", "module no {}");
    f.write("src/readme.txt", [0xff]);
    let config = f.config(&["file.java", "dir.java", "dir.java/deep", "readme.txt"]);
    assert_eq!(ProjectSnapshot::load(&config).unwrap().source_paths(), &[keep]);
}

#[test]
fn source_exclusions_do_not_read_skipped_bytes_but_count_encountered_entries() {
    let f = Fixture::new();
    f.write("src/A.java", [0xff; 128]);
    f.write("src/deep/child/deeper/B.java", [0xff; 128]);
    let mut config = f.config(&["A.java", "deep"]);
    config.limits.max_file_bytes = 0;
    config.limits.max_input_bytes = 0;
    config.limits.max_directory_depth = 0;
    config.limits.max_entries = 2;
    assert!(ProjectSnapshot::load(&config).unwrap().source_paths().is_empty());
    config.limits.max_entries = 1;
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::LimitExceeded);
    config.limits.max_entries = 20;
    config.source_exclusions = vec!["A.java".into()];
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::LimitExceeded);
}

#[test]
fn source_exclusions_are_relative_to_every_root_including_nested_roots() {
    let f = Fixture::new();
    let a = key(&f.write("src/A.java", "class A {}"));
    let b = key(&f.write("src/nested/B.java", "class B {}"));
    f.write("other/nested/C.java", "class C {}");
    let d = key(&f.write("other/D.java", "class D {}"));
    let mut config = f.config(&["nested"]);
    config.source_roots.extend([f.0.join("src/nested"), f.0.join("other")]);
    let mut expected = vec![a, b, d];
    expected.sort();
    assert_eq!(ProjectSnapshot::load(&config).unwrap().source_paths(), expected);
    config.source_roots.reverse();
    assert_eq!(ProjectSnapshot::load(&config).unwrap().source_paths(), expected);
    config.source_exclusions.clear();
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::DuplicateSource);
    config.source_exclusions = vec!["missing".into()];
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::DuplicateSource);
}

#[test]
fn source_exclusions_do_not_relax_source_root_type_or_module_checks() {
    let f = Fixture::new();
    let module = f.write("src/module-info.java", "module app {}");
    let mut config = f.config(&[]);
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::UnsupportedModule);
    config.source_exclusions = vec!["other/module-info.java".into()];
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::UnsupportedModule);
    config.source_exclusions = vec!["module-info.java".into()];
    assert!(ProjectSnapshot::load(&config).unwrap().source_paths().is_empty());
    config.source_roots = vec![module];
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::InvalidPath);
}

#[test]
fn source_exclusions_never_filter_binary_directory_walks_or_binary_budgets() {
    let f = Fixture::new();
    f.write("src/A.java", "class A {}");
    let class = f.write("classes/lib/Lib.class", include_bytes!("fixtures/classes/Lib.class"));
    let mut config = f.config(&["A.java", "lib", "lib/Lib.class"]);
    config.classpath.push(ClasspathEntry::Directory(f.0.join("classes")));
    assert_eq!(ProjectSnapshot::load(&config).unwrap().class_origins(), &[key(&class)]);
    config.limits.max_directory_depth = 0;
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::LimitExceeded);
    config.limits.max_directory_depth = 10;
    config.limits.max_input_bytes = 0;
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::LimitExceeded);
}

#[cfg(unix)]
#[test]
fn source_exclusions_reject_encountered_symlinks_even_when_the_name_matches() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let target = f.write("elsewhere/A.java", "class A {}");
    fs::create_dir(f.0.join("src")).unwrap();
    for (name, destination) in [("A.java", target), ("dir", f.0.join("elsewhere")), ("dangling", f.0.join("missing"))] {
        let link = f.0.join("src").join(name);
        symlink(destination, &link).unwrap();
        assert_eq!(ProjectSnapshot::load(&f.config(&[name])).unwrap_err().kind, K::InvalidPath);
        fs::remove_file(link).unwrap();
    }
    symlink(f.0.join("elsewhere"), f.0.join("root-link")).unwrap();
    let mut config = f.config(&["A.java"]);
    config.source_roots = vec![f.0.join("root-link")];
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::InvalidPath);
}

#[cfg(unix)]
#[test]
fn source_exclusions_prune_without_visiting_symlinks_or_invalid_path_descendants() {
    use std::{
        ffi::OsString,
        os::unix::{ffi::OsStringExt, fs::symlink},
    };
    let f = Fixture::new();
    f.write("src/skip/deep/A.java", [0xff]);
    symlink(f.0.join("missing"), f.0.join("src/skip/link")).unwrap();
    fs::write(f.0.join("src/skip").join(OsString::from_vec(b"bad\xff.java".to_vec())), b"class Bad {}").unwrap();
    let mut config = f.config(&["skip"]);
    config.limits.max_entries = 1;
    config.limits.max_directory_depth = 0;
    assert!(ProjectSnapshot::load(&config).unwrap().source_paths().is_empty());
}

#[cfg(unix)]
#[test]
fn source_exclusions_do_not_mask_binary_symlinks() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.write("src/A.java", "class A {}");
    fs::create_dir(f.0.join("classes")).unwrap();
    symlink(f.0.join("missing"), f.0.join("classes/skip")).unwrap();
    let mut config = f.config(&["skip"]);
    config.classpath.push(ClasspathEntry::Directory(f.0.join("classes")));
    assert_eq!(ProjectSnapshot::load(&config).unwrap_err().kind, K::InvalidPath);
}

#[test]
fn source_exclusions_for_all_compilations_are_preflighted_before_any_input_io() {
    let f = Fixture::new();
    let early = compilation(
        "a",
        ProjectConfig { classpath: vec![ClasspathEntry::Jar(f.0.join("missing.jar"))], ..Default::default() },
    );
    let late = compilation("z", f.config(&["../bad"]));
    let mut config = WorkspaceConfig { compilations: vec![early, late], ..Default::default() };
    for _ in 0..2 {
        let error = WorkspaceSnapshot::load(&config).unwrap_err();
        assert_eq!(error.kind, WorkspaceLoadErrorKind::Input);
        assert_eq!(error.input.unwrap().kind, K::InvalidPath, "the missing earlier JAR must not be touched");
        config.compilations.reverse();
    }
}

#[test]
fn source_exclusions_are_per_compilation_and_keep_canonical_ownership_checks() {
    let f = Fixture::new();
    let a = key(&f.write("src/A.java", "class A {}"));
    let b = key(&f.write("src/B.java", "class B {}"));
    let mut config = WorkspaceConfig {
        compilations: vec![compilation("a", f.config(&["B.java"])), compilation("b", f.config(&["A.java"]))],
        ..Default::default()
    };
    let loaded = WorkspaceSnapshot::load(&config).unwrap();
    assert_eq!(loaded.scope(&CompilationId::new("a", "main")).unwrap().inputs().source_paths(), &[a]);
    assert_eq!(loaded.scope(&CompilationId::new("b", "main")).unwrap().inputs().source_paths(), &[b]);
    config.compilations[1].inputs.source_exclusions.clear();
    assert_eq!(WorkspaceSnapshot::load(&config).unwrap_err().kind, WorkspaceLoadErrorKind::DuplicateSource);
}

#[test]
fn source_exclusions_reload_keeps_old_prepared_views_and_excluded_overlays_cannot_add_sources() {
    let f = Fixture::new();
    let declaration = key(&f.write("src/A.java", "class A {}"));
    let text = "class Use { A value; }";
    let use_path = key(&f.write("src/Use.java", text));
    let at = text.find("A value").unwrap() as u32;
    let owner = CompilationId::new("app", "main");
    let mut config = workspace(f.config(&[]));
    let old = WorkspaceSnapshot::load(&config).unwrap();
    let prepared = old.prepare();
    let before = prepared.scope(&owner).unwrap().resolve_at(&use_path, at);
    assert_eq!(before.status, ResolveStatus::Resolved);
    config.compilations[0].inputs.source_exclusions.push("A.java".into());
    let next = WorkspaceSnapshot::load(&config).unwrap();
    assert_eq!(next.scope(&owner).unwrap().resolve_at(&use_path, at).status, ResolveStatus::Unresolved);
    assert_eq!(prepared.scope(&owner).unwrap().resolve_at(&use_path, at), before);
    let error = next
        .with_source_overlays(&[
            SourceOverlay { owner: owner.clone(), path: use_path.clone(), text: "class Changed {}".into() },
            SourceOverlay { owner: owner.clone(), path: declaration, text: "class A {}".into() },
        ])
        .unwrap_err();
    assert_eq!(error.kind, SourceOverlayErrorKind::UnknownSource);
    assert_eq!(next.scope(&owner).unwrap().inputs().project().file(&use_path).unwrap().text(), text);
    assert_eq!(prepared.scope(&owner).unwrap().resolve_at(&use_path, at), before);
}
