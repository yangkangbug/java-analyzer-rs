use java_engine::{
    ClasspathEntry, CompilationConfig, CompilationId, PlatformEntry, ProjectConfig, ResolveStatus, SourceOverlay,
    SourceOverlayErrorKind, WorkspaceConfig, WorkspaceSnapshot,
};
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
        let root = std::env::temp_dir().join(format!(
            "java-engine-overlays-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) -> String {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path.canonicalize().unwrap().to_str().unwrap().to_owned()
    }
    fn compilation(
        &self,
        project: &str,
        source_set: &str,
        root: &str,
        dependencies: Vec<CompilationId>,
    ) -> CompilationConfig {
        CompilationConfig {
            id: id(project, source_set),
            inputs: ProjectConfig { source_roots: vec![self.0.join(root)], ..Default::default() },
            dependencies,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn id(project: &str, set: &str) -> CompilationId {
    CompilationId::new(project, set)
}
fn overlay(owner: &CompilationId, path: &str, text: &str) -> SourceOverlay {
    SourceOverlay { owner: owner.clone(), path: path.to_owned(), text: text.to_owned() }
}
fn single(f: &Fixture) -> WorkspaceConfig {
    WorkspaceConfig { compilations: vec![f.compilation("app", "main", "src", vec![])], ..Default::default() }
}
fn source<'a>(snapshot: &'a WorkspaceSnapshot, owner: &CompilationId, path: &str) -> &'a java_engine::ParsedFile {
    snapshot.scope(owner).unwrap().inputs().project().file(path).unwrap()
}

#[test]
fn full_buffers_replace_only_the_derived_snapshot_and_preserve_old_prepared_views() {
    let f = Fixture::new();
    let before = "class A { int value; int read() { return value; } }";
    let after = "// 😀\r\nclass A { int changed; int read() { return changed; } }";
    let path = f.write("src/A.java", before);
    let owner = id("app", "main");
    let base = WorkspaceSnapshot::load(&single(&f)).unwrap();
    let old_prepared = base.prepare();
    let old = old_prepared.scope(&owner).unwrap().resolve_at(&path, before.rfind("value").unwrap() as u32);
    assert_eq!(old.status, ResolveStatus::Resolved);
    let next = base.with_source_overlays(&[overlay(&owner, &path, after)]).unwrap();
    let file = source(&next, &owner, &path);
    assert_eq!(file.text(), after);
    assert!(file.tokens_cover_text());
    assert_eq!(file.root(), java_engine::ParsedFile::parse(after).root());
    let prepared = next.prepare();
    let result = prepared.scope(&owner).unwrap().resolve_at(&path, after.rfind("changed").unwrap() as u32);
    assert_eq!(result.status, ResolveStatus::Resolved);
    assert_eq!(result.targets[0].name, "changed");
    assert_eq!(result, next.scope(&owner).unwrap().resolve_at(&path, after.rfind("changed").unwrap() as u32));
    assert_eq!(old_prepared.scope(&owner).unwrap().resolve_at(&path, before.rfind("value").unwrap() as u32), old);
    assert_eq!(source(&base, &owner, &path).text(), before);
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    let without = base.with_source_overlays(&[]).unwrap();
    assert_eq!(source(&without, &owner, &path).text(), before);
    let still_changed = next.with_source_overlays(&[]).unwrap();
    assert_eq!(source(&still_changed, &owner, &path).text(), after);
}

#[test]
fn malformed_and_empty_buffers_are_retained_without_changing_disk() {
    let f = Fixture::new();
    let path = f.write("src/A.java", "class A {}");
    let owner = id("app", "main");
    let base = WorkspaceSnapshot::load(&single(&f)).unwrap();
    for text in ["class A { void m( { ???", ""] {
        let next = base.with_source_overlays(&[overlay(&owner, &path, text)]).unwrap();
        let parsed = source(&next, &owner, &path);
        assert_eq!(parsed.text(), text);
        assert!(parsed.tokens_cover_text());
        if !text.is_empty() {
            assert!(!parsed.errors().is_empty());
        }
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), "class A {}");
}

#[test]
fn invalid_ownership_duplicates_and_missing_files_reject_the_entire_batch() {
    let f = Fixture::new();
    let path = f.write("src/A.java", "class A {}");
    let other = f.write("other/B.java", "class B {}");
    let owner = id("app", "main");
    let other_owner = id("other", "main");
    let mut config = single(&f);
    config.compilations.push(f.compilation("other", "main", "other", vec![]));
    let base = WorkspaceSnapshot::load(&config).unwrap();
    let valid = overlay(&owner, &path, "class Changed {}");
    for (invalid, expected) in [
        (overlay(&id("missing", "main"), &path, ""), SourceOverlayErrorKind::UnknownCompilation),
        (overlay(&owner, &other, ""), SourceOverlayErrorKind::OwnerMismatch),
        (overlay(&other_owner, &path, ""), SourceOverlayErrorKind::OwnerMismatch),
        (overlay(&owner, "Missing.java", ""), SourceOverlayErrorKind::UnknownSource),
        (overlay(&owner, &path.replace("/A.java", "/./A.java"), ""), SourceOverlayErrorKind::UnknownSource),
        (valid.clone(), SourceOverlayErrorKind::DuplicateSource),
    ] {
        let error = base.with_source_overlays(&[valid.clone(), invalid]).unwrap_err();
        assert_eq!(error.kind, expected);
        assert!(error.path.is_some());
        assert_eq!(source(&base, &owner, &path).text(), "class A {}");
    }
}

#[test]
fn overlays_keep_owner_context_and_visibility_without_any_input_reread() {
    let f = Fixture::new();
    let library_text = "package dep; public class Library extends lib.Lib { public void call() { run(); } }";
    let library = f.write("lib/Library.java", library_text);
    let app_text = "package app; class App extends dep.Library { void f() { call(); } }";
    let app = f.write("app/App.java", app_text);
    let test = f.write("test/Test.java", "class Test {}");
    let mut jar = ZipWriter::new(Cursor::new(Vec::new()));
    jar.start_file("lib/Lib.class", SimpleFileOptions::default()).unwrap();
    jar.write_all(include_bytes!("fixtures/classes/Lib.class")).unwrap();
    let jar_path = f.write("dep.jar", jar.finish().unwrap().into_inner());
    let mut jmod = ZipWriter::new(Cursor::new(Vec::new()));
    jmod.start_file("classes/lib/Lib.class", SimpleFileOptions::default()).unwrap();
    jmod.write_all(include_bytes!("fixtures/classes/Lib.class")).unwrap();
    let jmod_path = f.write("platform.jmod", [b"JM\x01\x00".as_slice(), &jmod.finish().unwrap().into_inner()].concat());
    let lib_owner = id("lib", "main");
    let app_owner = id("app", "main");
    let test_owner = id("app", "test");
    let mut lib = f.compilation("lib", "main", "lib", vec![]);
    lib.inputs.classpath = vec![ClasspathEntry::Jar(PathBuf::from(jar_path))];
    lib.inputs.platform = vec![PlatformEntry::Jmod(PathBuf::from(jmod_path))];
    let config = WorkspaceConfig {
        compilations: vec![
            lib,
            f.compilation("app", "main", "app", vec![lib_owner.clone()]),
            f.compilation("app", "test", "test", vec![app_owner.clone()]),
        ],
        ..Default::default()
    };
    let base = WorkspaceSnapshot::load(&config).unwrap();
    let old_prepared = base.prepare();
    assert_eq!(
        base.with_source_overlays(&[overlay(&app_owner, &library, library_text)]).unwrap_err().kind,
        SourceOverlayErrorKind::OwnerMismatch
    );
    let old = old_prepared.scope(&app_owner).unwrap().resolve_at(&library, library_text.rfind("run").unwrap() as u32);
    assert_eq!(old.status, ResolveStatus::Resolved);
    assert!(old.targets[0].path.ends_with("platform.jmod!/classes/lib/Lib.class"));
    // Remove every source and binary input. Derived construction/query must use
    // only the loaded identities, source trees, signatures and visibility graph.
    fs::remove_dir_all(&f.0).unwrap();
    let changed = library_text.replace("call", "renamed");
    let changed_app = app_text.replace("call", "renamed");
    let next = base
        .with_source_overlays(&[overlay(&lib_owner, &library, &changed), overlay(&app_owner, &app, &changed_app)])
        .unwrap();
    let prepared = next.prepare();
    let actual = prepared.scope(&app_owner).unwrap().resolve_at(&library, changed.rfind("run").unwrap() as u32);
    assert_eq!(actual, old);
    assert_eq!(
        prepared.scope(&app_owner).unwrap().resolve_at(&app, changed_app.rfind("renamed").unwrap() as u32).status,
        ResolveStatus::Resolved
    );
    assert_eq!(prepared.scope(&app_owner).unwrap().resolve_at(&test, 6).status, ResolveStatus::Unresolved);
    assert_eq!(prepared.scope(&test_owner).unwrap().resolve_at(&test, 6).status, ResolveStatus::Resolved);
    assert_eq!(
        old_prepared.scope(&app_owner).unwrap().resolve_at(&library, library_text.rfind("run").unwrap() as u32),
        old
    );
    let old_inputs = base.scope(&lib_owner).unwrap().inputs();
    let new_inputs = next.scope(&lib_owner).unwrap().inputs();
    assert_eq!(old_inputs.class_origins(), new_inputs.class_origins());
    assert_eq!(old_inputs.shadowed_classes().len(), 1);
    assert_eq!(old_inputs.shadowed_classes(), new_inputs.shadowed_classes());
    for origin in old_inputs.class_origins() {
        assert_eq!(old_inputs.project().class_signature(origin), new_inputs.project().class_signature(origin));
    }
}

#[test]
fn per_file_limit_uses_utf8_bytes_and_accepts_the_exact_boundary() {
    let f = Fixture::new();
    let path = f.write("src/A.java", "");
    let owner = id("app", "main");
    let mut config = single(&f);
    config.compilations[0].inputs.limits.max_file_bytes = 4;
    let base = WorkspaceSnapshot::load(&config).unwrap();
    assert!(base.with_source_overlays(&[overlay(&owner, &path, "😀")]).is_ok());
    assert_eq!(
        base.with_source_overlays(&[overlay(&owner, &path, "😀 ")]).unwrap_err().kind,
        SourceOverlayErrorKind::LimitExceeded
    );
}

#[test]
fn compilation_budget_counts_replacement_bytes_and_releases_removed_bytes() {
    let f = Fixture::new();
    let a = f.write("src/A.java", "//aaa");
    let b = f.write("src/B.java", "//bbb");
    let owner = id("app", "main");
    let mut config = single(&f);
    config.compilations[0].inputs.limits.max_input_bytes = 12;
    let base = WorkspaceSnapshot::load(&config).unwrap();
    let exact = base.with_source_overlays(&[overlay(&owner, &a, "//aaaaa")]).unwrap();
    assert_eq!(
        base.with_source_overlays(&[overlay(&owner, &a, "//aaaaaa")]).unwrap_err().kind,
        SourceOverlayErrorKind::LimitExceeded
    );
    let reduced = exact.with_source_overlays(&[overlay(&owner, &a, "")]).unwrap();
    assert!(reduced.with_source_overlays(&[overlay(&owner, &b, "//1234567890")]).is_ok());
    // Validate the final batch, not sequential intermediate usage. The grow
    // comes before the shrink and must still fit the same total budget.
    let batch = [overlay(&owner, &a, "//1234567890"), overlay(&owner, &b, "")];
    let forward = base.with_source_overlays(&batch).unwrap();
    let reverse = base.with_source_overlays(&batch.into_iter().rev().collect::<Vec<_>>()).unwrap();
    for path in [&a, &b] {
        assert_eq!(source(&forward, &owner, path).text(), source(&reverse, &owner, path).text());
    }
}

#[test]
fn workspace_budget_is_aggregate_across_compilations() {
    let f = Fixture::new();
    let a = f.write("a/A.java", "//aaa");
    let b = f.write("b/B.java", "//bbb");
    let a_owner = id("a", "main");
    let b_owner = id("b", "main");
    let config = WorkspaceConfig {
        compilations: vec![f.compilation("a", "main", "a", vec![]), f.compilation("b", "main", "b", vec![])],
        max_input_bytes: 12,
        ..Default::default()
    };
    let base = WorkspaceSnapshot::load(&config).unwrap();
    assert!(base.with_source_overlays(&[overlay(&a_owner, &a, "//aaaaa")]).is_ok());
    assert_eq!(
        base.with_source_overlays(&[overlay(&a_owner, &a, "//aaaa"), overlay(&b_owner, &b, "//bbbb")])
            .unwrap()
            .scope(&a_owner)
            .unwrap()
            .inputs()
            .source_paths()
            .len(),
        1
    );
    assert_eq!(
        base.with_source_overlays(&[overlay(&a_owner, &a, "//aaaaa"), overlay(&b_owner, &b, "//bbbb")])
            .unwrap_err()
            .kind,
        SourceOverlayErrorKind::LimitExceeded
    );
}

#[test]
fn temporary_loading_budget_does_not_become_a_compilation_limit() {
    let f = Fixture::new();
    let a = f.write("a/A.java", "//1234567890"); // 12 bytes, loaded first.
    let b = f.write("b/B.java", "//1234"); // 6 bytes, initially at remaining allowance.
    let a_owner = id("a", "main");
    let b_owner = id("b", "main");
    let mut first = f.compilation("a", "main", "a", vec![]);
    let mut second = f.compilation("b", "main", "b", vec![]);
    first.inputs.limits.max_input_bytes = 18;
    second.inputs.limits.max_input_bytes = 18;
    let config = WorkspaceConfig { compilations: vec![second, first], max_input_bytes: 18, ..Default::default() };
    let base = WorkspaceSnapshot::load(&config).unwrap();
    let batch = [overlay(&b_owner, &b, "//1234567890"), overlay(&a_owner, &a, "//1234")];
    let next = base.with_source_overlays(&batch).unwrap();
    let reverse = base.with_source_overlays(&batch.into_iter().rev().collect::<Vec<_>>()).unwrap();
    assert_eq!(source(&next, &b_owner, &b).text(), "//1234567890");
    assert_eq!(source(&reverse, &b_owner, &b).text(), "//1234567890");
}

#[test]
fn unchanged_binary_bytes_remain_charged_to_the_compilation_budget() {
    let f = Fixture::new();
    let source_path = f.write("src/A.java", "");
    let binary = include_bytes!("fixtures/classes/Lib.class");
    f.write("classes/lib/Lib.class", binary);
    let owner = id("app", "main");
    let mut config = single(&f);
    config.compilations[0].inputs.classpath = vec![ClasspathEntry::Directory(f.0.join("classes"))];
    config.compilations[0].inputs.limits.max_input_bytes = binary.len() as u64 + 4;
    let base = WorkspaceSnapshot::load(&config).unwrap();
    assert!(base.with_source_overlays(&[overlay(&owner, &source_path, "//aa")]).is_ok());
    assert_eq!(
        base.with_source_overlays(&[overlay(&owner, &source_path, "//aaa")]).unwrap_err().kind,
        SourceOverlayErrorKind::LimitExceeded
    );
}
