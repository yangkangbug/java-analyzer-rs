use java_engine::{Project, ResolveStatus, TextEdit};

fn project(source: &str) -> Project {
    let mut p = Project::new();
    for package in ["a", "b", "fallback"] {
        p.insert(
            format!("{package}/Item.java"),
            &format!("package {package}; public class Item {{ public int x; public static class Inner {{}} }}"),
        );
    }
    p.insert("Use.java", source);
    p
}
fn offset(source: &str, marker: &str) -> u32 {
    (source.find(marker).unwrap() + marker.len()) as u32
}
const BODY: &str = "class Use { /*type*/Item value; /*nested*/Item.Inner inner; int f(){return value./*field*/x;} }";

#[test]
fn distinct_single_imports_never_pick_by_order_or_loaded_subset() {
    for imports in [
        "import a.Item; import b.Item;",
        "import b.Item; import a.Item;",
        "import a.Item; import missing.Item;",
        "import missing.Item; import a.Item;",
        "import missing.Item; import absent.Item;",
    ] {
        let source = format!("{imports} import fallback.*; {BODY}");
        let p = project(&source);
        for marker in ["/*type*/", "/*nested*/", "/*field*/"] {
            let result = p.resolve_at("Use.java", offset(&source, marker));
            assert_ne!(result.status, ResolveStatus::Resolved, "{imports} {marker}: {result:?}");
            assert!(result.targets.is_empty(), "{imports} {marker}: {result:?}");
            assert_eq!(result, p.prepare().resolve_at("Use.java", offset(&source, marker)));
        }
    }
}

#[test]
fn repeated_identical_imports_and_unrelated_imports_preserve_exact_targets() {
    for imports in ["import a.Item;", "import a.Item; import a.Item;", "import b.Other; import a.Item; import a.Item;"]
    {
        let source = format!("{imports} {BODY}");
        let p = project(&source);
        for marker in ["/*type*/", "/*nested*/", "/*field*/"] {
            let result = p.resolve_at("Use.java", offset(&source, marker));
            assert_eq!(result.status, ResolveStatus::Resolved, "{marker}: {result:?}");
            assert_eq!(result.targets.len(), 1);
            assert_eq!(result.targets[0].owner, "a.Item");
            assert_eq!(result.targets[0].path, "a/Item.java");
        }
    }
}

#[test]
fn conflicting_import_edits_and_undo_match_fresh_without_changing_old_prepared_view() {
    let source = format!("import a.Item; {BODY}");
    let original = project(&source);
    let prepared = original.prepare();
    for inserted in ["import b.Item; ", "import missing.Item; "] {
        let mut changed = original.clone();
        changed.edit("Use.java", TextEdit { start_byte: 0, end_byte: 0, insert: inserted.into() }).unwrap();
        let text = format!("{inserted}{source}");
        let fresh = project(&text);
        for marker in ["/*type*/", "/*nested*/", "/*field*/"] {
            let at = offset(&text, marker);
            assert_eq!(changed.resolve_at("Use.java", at), fresh.resolve_at("Use.java", at));
            assert!(changed.resolve_at("Use.java", at).targets.is_empty());
            assert_eq!(prepared.resolve_at("Use.java", offset(&source, marker)).status, ResolveStatus::Resolved);
        }
        changed
            .edit("Use.java", TextEdit { start_byte: 0, end_byte: inserted.len() as u32, insert: String::new() })
            .unwrap();
        for marker in ["/*type*/", "/*nested*/", "/*field*/"] {
            assert_eq!(
                changed.resolve_at("Use.java", offset(&source, marker)),
                prepared.resolve_at("Use.java", offset(&source, marker))
            );
        }
    }
}

#[test]
fn lexical_shadowing_and_fully_qualified_types_keep_existing_precedence() {
    let source = "import a.Item; class Use { class Item {int x;} /*member*/Item value; void f(){class Item {} /*local*/Item x;} }";
    let p = project(source);
    let member = p.resolve_at("Use.java", offset(source, "/*member*/"));
    assert_eq!(member.status, ResolveStatus::Resolved);
    assert_eq!(member.targets[0].owner, "Use.Item");
    let local = p.resolve_at("Use.java", offset(source, "/*local*/"));
    assert_eq!(local.status, ResolveStatus::Resolved);
    assert_eq!(format!("{:?}", local.targets[0].kind), "LocalType");
    assert_eq!(local.targets[0].path, "Use.java");
    assert!(local.targets[0].owner.is_empty());
    assert_eq!(local.targets[0].start_byte, source.find("Item {}").unwrap() as u32);
    assert_eq!(local.targets[0].end_byte, local.targets[0].start_byte + 4);
    let qualified = "import a.Item; import b.Item; class Use { a./*qualified*/Item value; }";
    let p = project(qualified);
    let result = p.resolve_at("Use.java", offset(qualified, "/*qualified*/"));
    assert_eq!(result.status, ResolveStatus::Resolved);
    assert_eq!(result.targets[0].owner, "a.Item");
}

#[test]
fn invisible_compilation_import_still_blocks_but_never_leaks_its_declarations() {
    use java_engine::{
        AnalysisSnapshot, CompilationConfig, CompilationId, ProjectConfig, SourceOverlay, WorkspaceConfig,
    };
    use std::{fs, path::PathBuf};
    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let root = std::env::temp_dir().join(format!("java-engine-import-competition-{}", std::process::id()));
    let fixture = Fixture(root);
    for dir in ["visible", "hidden", "app"] {
        fs::create_dir_all(fixture.0.join(dir)).unwrap();
    }
    fs::write(fixture.0.join("visible/Item.java"), "package a; public class Item {public int x;}").unwrap();
    fs::write(fixture.0.join("hidden/Item.java"), "package b; public class Item {public int x;}").unwrap();
    let base = format!("import a.Item; {BODY}");
    fs::write(fixture.0.join("app/Use.java"), &base).unwrap();
    let path = fixture.0.join("app/Use.java").canonicalize().unwrap().to_str().unwrap().to_owned();
    let visible = CompilationId::new("visible", "main");
    let hidden = CompilationId::new("hidden", "main");
    let app = CompilationId::new("app", "main");
    let config = WorkspaceConfig {
        compilations: [
            (visible.clone(), "visible", vec![]),
            (hidden, "hidden", vec![]),
            (app.clone(), "app", vec![visible]),
        ]
        .into_iter()
        .map(|(id, dir, dependencies)| CompilationConfig {
            id,
            inputs: ProjectConfig { source_roots: vec![fixture.0.join(dir)], ..Default::default() },
            dependencies,
        })
        .collect(),
        ..Default::default()
    };
    let original = AnalysisSnapshot::load(&config).unwrap();
    let text = format!("import b.Item; {base}");
    let changed = original
        .with_source_overlays(&[SourceOverlay { owner: app.clone(), path: path.clone(), text: text.clone() }])
        .unwrap();
    for marker in ["/*type*/", "/*field*/"] {
        let old = original.scope(&app).unwrap().resolve_at(&path, offset(&base, marker));
        assert_eq!(old.status, ResolveStatus::Resolved);
        assert_eq!(old.targets[0].owner, "a.Item");
        let new = changed.scope(&app).unwrap().resolve_at(&path, offset(&text, marker));
        assert_ne!(new.status, ResolveStatus::Resolved);
        assert!(new.targets.is_empty());
        assert_eq!(original.scope(&app).unwrap().resolve_at(&path, offset(&base, marker)), old);
    }
}
