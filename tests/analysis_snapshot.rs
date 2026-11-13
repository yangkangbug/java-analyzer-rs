use java_engine::{
    AnalysisSnapshot, CompilationConfig, CompilationId, ProjectConfig, ResolveStatus, SourceOverlay,
    SourceOverlayErrorKind, WorkspaceConfig, WorkspaceSnapshot,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "java-engine-analysis-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, text: &str) -> String {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path.canonicalize().unwrap().to_str().unwrap().to_owned()
    }
    fn config(&self) -> WorkspaceConfig {
        let lib = CompilationId::new("lib", "main");
        let app = CompilationId::new("app", "main");
        WorkspaceConfig {
            compilations: vec![
                CompilationConfig {
                    id: lib.clone(),
                    inputs: ProjectConfig { source_roots: vec![self.0.join("lib")], ..Default::default() },
                    dependencies: vec![],
                },
                CompilationConfig {
                    id: app,
                    inputs: ProjectConfig { source_roots: vec![self.0.join("app")], ..Default::default() },
                    dependencies: vec![lib],
                },
            ],
            ..Default::default()
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn assert_send_sync<T: Send + Sync>() {}
#[test]
fn owned_views_survive_original_inputs_and_keep_transactional_versions() {
    assert_send_sync::<AnalysisSnapshot>();
    let f = Fixture::new();
    let before = "package p; public class Lib { public int value; }";
    let lib = f.write("lib/p/Lib.java", before);
    let source = "package q; import p.Lib; class App { void m(Lib x) { x.value++; } }";
    let app = f.write("app/q/App.java", source);
    let owner = CompilationId::new("lib", "main");
    let consumer = CompilationId::new("app", "main");
    let at = source.find("value++").unwrap() as u32;
    let analysis = {
        let inputs = WorkspaceSnapshot::load(&f.config()).unwrap();
        AnalysisSnapshot::new(inputs)
    };
    let old = analysis.scope(&consumer).unwrap().resolve_at(&app, at);
    assert_eq!(old.status, ResolveStatus::Resolved);
    let cloned = analysis.clone();
    drop(analysis);
    assert_eq!(cloned.scope(&consumer).unwrap().resolve_at(&app, at), old);
    let changed = cloned
        .with_source_overlays(&[SourceOverlay {
            owner: owner.clone(),
            path: lib.clone(),
            text: before.replace("value", "other"),
        }])
        .unwrap();
    assert_ne!(changed.scope(&consumer).unwrap().resolve_at(&app, at).status, ResolveStatus::Resolved);
    assert_eq!(cloned.scope(&consumer).unwrap().resolve_at(&app, at), old);
    for snap in [&cloned, &changed] {
        let full = snap.inputs().prepare();
        assert_eq!(
            snap.scope(&consumer).unwrap().resolve_at(&app, at),
            full.scope(&consumer).unwrap().resolve_at(&app, at)
        );
        assert_eq!(snap.scope(&owner).unwrap().resolve_at(&app, at).status, ResolveStatus::Unresolved);
    }
    let failure = SourceOverlay { owner: consumer.clone(), path: lib, text: before.into() };
    assert_eq!(cloned.with_source_overlays(&[failure]).err().unwrap().kind, SourceOverlayErrorKind::OwnerMismatch);
    assert_eq!(cloned.scope(&consumer).unwrap().resolve_at(&app, at), old);
    assert!(cloned.scope(&CompilationId::new("missing", "main")).is_none());
    assert_eq!(AnalysisSnapshot::load(&f.config()).unwrap().scope(&consumer).unwrap().resolve_at(&app, at), old);
}

#[test]
fn concurrent_readers_and_failed_reload_do_not_retire_old_state() {
    let f = Fixture::new();
    f.write("lib/p/Lib.java", "package p; public class Lib { public int value; }");
    let source = "package q; import p.Lib; class App { void m(Lib x) { x.value++; } }";
    let path = f.write("app/q/App.java", source);
    let id = CompilationId::new("app", "main");
    let at = source.find("value++").unwrap() as u32;
    let snapshot = AnalysisSnapshot::load(&f.config()).unwrap();
    let expected = snapshot.scope(&id).unwrap().resolve_at(&path, at);
    assert_eq!(expected.status, ResolveStatus::Resolved);
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let snapshot = snapshot.clone();
            let path = path.clone();
            let id = id.clone();
            let expected = expected.clone();
            std::thread::spawn(move || {
                for _ in 0..30 {
                    assert_eq!(snapshot.scope(&id).unwrap().resolve_at(&path, at), expected);
                }
            })
        })
        .collect();
    let mut bad = f.config();
    bad.compilations[0].dependencies.push(CompilationId::new("missing", "main"));
    assert!(AnalysisSnapshot::load(&bad).is_err());
    drop(f);
    assert_eq!(snapshot.scope(&id).unwrap().resolve_at(&path, at), expected);
    drop(snapshot);
    for thread in threads {
        thread.join().unwrap();
    }
}

#[test]
fn rejected_overlay_batches_preserve_inputs_and_budget_is_final_state_based() {
    let f = Fixture::new();
    let text = "package p; public class Lib { public int value; }";
    let path = f.write("lib/p/Lib.java", text);
    f.write("app/App.java", "class App {}");
    let mut config = f.config();
    config.compilations[0].inputs.limits.max_file_bytes = text.len() as u64;
    let snapshot = AnalysisSnapshot::load(&config).unwrap();
    let id = CompilationId::new("lib", "main");
    let original = SourceOverlay { owner: id.clone(), path: path.clone(), text: text.into() };
    let mut large = original.clone();
    large.text.push(' ');
    assert_eq!(snapshot.with_source_overlays(&[large]).err().unwrap().kind, SourceOverlayErrorKind::LimitExceeded);
    assert_eq!(
        snapshot.with_source_overlays(&[original.clone(), original]).err().unwrap().kind,
        SourceOverlayErrorKind::DuplicateSource
    );
    assert_eq!(snapshot.scope(&id).unwrap().inputs().project().file(&path).unwrap().text(), text);
    let mut total = config.clone();
    total.max_input_bytes = (text.len() + "class App {}".len()) as u64;
    total.compilations[0].inputs.limits.max_file_bytes += 10;
    let capped = AnalysisSnapshot::load(&total).unwrap();
    let grew = SourceOverlay { owner: id, path, text: format!("{text} ") };
    assert_eq!(capped.with_source_overlays(&[grew]).err().unwrap().kind, SourceOverlayErrorKind::LimitExceeded);
}

#[test]
fn isolated_binary_versions_keep_exact_context_across_updates() {
    let f = Fixture::new();
    let bytes = include_bytes!("fixtures/classes/Lib.class");
    for name in ["old", "new"] {
        fs::create_dir_all(f.0.join(name).join("lib")).unwrap();
    }
    fs::write(f.0.join("old/lib/Lib.class"), bytes).unwrap();
    let mut changed = bytes.to_vec();
    let pos = changed.windows(5).position(|b| b == b"value").unwrap();
    changed[pos..pos + 5].copy_from_slice(b"other");
    fs::write(f.0.join("new/lib/Lib.class"), changed).unwrap();
    let source = "class Use { void m(lib.Lib x) { x.value++; x.other++; } }";
    let paths = [f.write("a/Use.java", source), f.write("b/Use.java", source)];
    let ids = [CompilationId::new("a", "main"), CompilationId::new("b", "main")];
    let config = WorkspaceConfig {
        compilations: (0..2)
            .map(|i| CompilationConfig {
                id: ids[i].clone(),
                inputs: ProjectConfig {
                    source_roots: vec![f.0.join(["a", "b"][i])],
                    classpath: vec![java_engine::ClasspathEntry::Directory(f.0.join(["old", "new"][i]))],
                    ..Default::default()
                },
                dependencies: vec![],
            })
            .collect(),
        ..Default::default()
    };
    let old = AnalysisSnapshot::load(&config).unwrap();
    let next = old
        .with_source_overlays(&[SourceOverlay {
            owner: ids[0].clone(),
            path: paths[0].clone(),
            text: format!("{source} // edit"),
        }])
        .unwrap();
    for snapshot in [&old, &next] {
        let prepared = snapshot.inputs().prepare();
        for i in 0..2 {
            for (n, marker) in ["value++", "other++"].iter().enumerate() {
                let at = source.find(marker).unwrap() as u32;
                let r = snapshot.scope(&ids[i]).unwrap().resolve_at(&paths[i], at);
                assert_eq!(r, prepared.scope(&ids[i]).unwrap().resolve_at(&paths[i], at));
                assert_eq!(r.status == ResolveStatus::Resolved, i == n);
                if i == n {
                    assert!(r.targets[0].path.contains(["old", "new"][i]));
                }
            }
            assert_eq!(snapshot.scope(&ids[i]).unwrap().resolve_at(&paths[1 - i], 0).status, ResolveStatus::Unresolved);
        }
    }
}
