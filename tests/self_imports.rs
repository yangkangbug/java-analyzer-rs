use java_engine::{Project, ResolveStatus, TextEdit};
fn at(source: &str, marker: &str) -> u32 {
    (source.find(marker).unwrap() + marker.len()) as u32
}
fn project(source: &str) -> Project {
    let mut p = Project::new();
    p.insert("p/Use.java", source);
    p
}

#[test]
fn exact_self_imports_contribute_one_declaration() {
    for source in [
        "package p; import p.Item; class Item {/*type*/Item v; int x; int f(){return v./*field*/x;}}",
        "package p; import p.Item; import p.Item; class Item {/*type*/Item v; int x; int f(){return v./*field*/x;}}",
        "package p; import p.Item; class Use {/*type*/Item v; int f(){return v./*field*/x;}} class Item {int x;}",
    ] {
        let p = project(source);
        for marker in ["/*type*/", "/*field*/"] {
            let result = p.resolve_at("p/Use.java", at(source, marker));
            assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
            assert_eq!(result.targets.len(), 1);
            assert_eq!(result.targets[0].path, "p/Use.java");
            assert_eq!(result.targets[0].owner, "p.Item");
            assert_eq!(result, p.prepare().resolve_at("p/Use.java", at(source, marker)));
        }
    }
}

#[test]
fn same_spelling_is_not_declaration_identity() {
    for extra in ["", "class Item {int y;}"] {
        let source = format!("package p; import p.Item; class Item {{/*type*/Item v;}} {extra}");
        let mut p = project(&source);
        if extra.is_empty() {
            p.insert("other/Item.java", "package p; class Item {int y;}");
        }
        let result = p.resolve_at("p/Use.java", at(&source, "/*type*/"));
        assert_eq!(result.status, ResolveStatus::Ambiguous);
        assert_eq!(result.targets.len(), 2, "{result:?}");
        assert_ne!(result.targets[0], result.targets[1]);
        assert!(result.targets.iter().all(|t| t.owner == "p.Item"));
    }
}

#[test]
fn different_imported_type_and_lexical_shadowing_keep_existing_boundaries() {
    let source = "package p; import q.Item; class Item {/*type*/Item v;}";
    let mut p = project(source);
    p.insert("q/Item.java", "package q; public class Item {}");
    let result = p.resolve_at("p/Use.java", at(source, "/*type*/"));
    assert_eq!(result.status, ResolveStatus::Ambiguous);
    assert_eq!(result.targets.len(), 2);
    let source = "package p; import p.Item; class Item {class Inner {class Item {} /*near*/Item v;} void f(){class Item {} /*local*/Item v;}}";
    let p = project(source);
    let near = p.resolve_at("p/Use.java", at(source, "/*near*/"));
    assert_eq!(near.status, ResolveStatus::Resolved);
    assert_eq!(near.targets[0].owner, "p.Item.Inner.Item");
    assert_eq!(p.resolve_at("p/Use.java", at(source, "/*local*/")).status, ResolveStatus::Unresolved);
}

#[test]
fn adding_a_self_import_and_real_duplicate_keeps_old_prepared_version() {
    let source = "package p; class Item {/*type*/Item v; int x; int f(){return v./*field*/x;}}";
    let p = project(source);
    let old = p.prepare();
    let mut edited = p.clone();
    let insert_at = "package p;".len() as u32;
    let insert = " import p.Item;";
    edited.edit("p/Use.java", TextEdit { start_byte: insert_at, end_byte: insert_at, insert: insert.into() }).unwrap();
    let text = edited.file("p/Use.java").unwrap().text().to_owned();
    let fresh = project(&text);
    for marker in ["/*type*/", "/*field*/"] {
        let result = edited.resolve_at("p/Use.java", at(&text, marker));
        assert_eq!(result.status, ResolveStatus::Resolved);
        assert_eq!(result, fresh.resolve_at("p/Use.java", at(&text, marker)));
        assert_eq!(old.resolve_at("p/Use.java", at(source, marker)).status, ResolveStatus::Resolved);
    }
    let before = edited.clone();
    let offset = text.len() as u32;
    edited
        .edit("p/Use.java", TextEdit { start_byte: offset, end_byte: offset, insert: " class Item {}".into() })
        .unwrap();
    let result = edited.resolve_at("p/Use.java", at(&text, "/*type*/"));
    assert_eq!(result.status, ResolveStatus::Ambiguous);
    assert_eq!(result.targets.len(), 2);
    assert_eq!(before.resolve_at("p/Use.java", at(&text, "/*type*/")).status, ResolveStatus::Resolved);
    edited
        .edit(
            "p/Use.java",
            TextEdit { start_byte: offset, end_byte: offset + " class Item {}".len() as u32, insert: String::new() },
        )
        .unwrap();
    assert_eq!(
        edited.resolve_at("p/Use.java", at(&text, "/*type*/")),
        before.resolve_at("p/Use.java", at(&text, "/*type*/"))
    );
}

#[test]
fn source_and_binary_of_same_name_are_distinct_declarations() {
    // Signature-only JVMS fixture, no Java runtime dependency.
    let name = b"p/Item";
    let mut bytes = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61, 0, 3, 1, 0, name.len() as u8];
    bytes.extend(name);
    bytes.extend([7, 0, 1]);
    for value in [0x21u16, 2, 0, 0, 0, 0, 0] {
        bytes.extend(value.to_be_bytes());
    }
    let source = "package p; import p.Item; class Item {/*type*/Item v;}";
    let mut p = project(source);
    p.add_class_bytes("dependency/p/Item.class", &bytes).unwrap();
    let result = p.resolve_at("p/Use.java", at(source, "/*type*/"));
    assert_eq!(result.status, ResolveStatus::Ambiguous);
    assert_eq!(result.targets.len(), 2);
    assert!(result.targets.iter().any(|t| t.path == "dependency/p/Item.class" && t.start_byte == 0));
    assert!(result.targets.iter().any(|t| t.path == "p/Use.java" && t.start_byte > 0));
}

#[test]
fn compilation_identity_and_visibility_survive_self_import_deduplication() {
    use java_engine::{AnalysisSnapshot, CompilationConfig, CompilationId, ProjectConfig, WorkspaceConfig};
    use std::{fs, path::PathBuf};
    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let f = Fixture(std::env::temp_dir().join(format!("java-engine-self-import-{}", std::process::id())));
    let source = "package p; import p.Item; public class Item {/*type*/Item v; int x; int f(){return v./*field*/x;}}";
    for root in ["one", "two", "app"] {
        fs::create_dir_all(f.0.join(root)).unwrap();
    }
    for root in ["one", "two"] {
        fs::write(f.0.join(root).join("Item.java"), source).unwrap();
    }
    let caller = "package p; import p.Item; class Use {/*type*/Item v;}";
    fs::write(f.0.join("app/Use.java"), caller).unwrap();
    let one = CompilationId::new("one", "main");
    let two = CompilationId::new("two", "main");
    let app = CompilationId::new("app", "main");
    let config = WorkspaceConfig {
        compilations: [
            (one.clone(), "one", vec![]),
            (two.clone(), "two", vec![]),
            (app.clone(), "app", vec![one.clone(), two.clone()]),
        ]
        .into_iter()
        .map(|(id, root, dependencies)| CompilationConfig {
            id,
            inputs: ProjectConfig { source_roots: vec![f.0.join(root)], ..Default::default() },
            dependencies,
        })
        .collect(),
        ..Default::default()
    };
    let snapshot = AnalysisSnapshot::load(&config).unwrap();
    for (id, root) in [(one, "one"), (two, "two")] {
        let path = f.0.join(root).join("Item.java").canonicalize().unwrap().to_str().unwrap().to_owned();
        for marker in ["/*type*/", "/*field*/"] {
            let result = snapshot.scope(&id).unwrap().resolve_at(&path, at(source, marker));
            assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
            assert_eq!(result.targets.len(), 1);
            assert_eq!(result.targets[0].path, path);
        }
    }
    let path = f.0.join("app/Use.java").canonicalize().unwrap().to_str().unwrap().to_owned();
    let result = snapshot.scope(&app).unwrap().resolve_at(&path, at(caller, "/*type*/"));
    assert_eq!(result.status, ResolveStatus::Ambiguous);
    assert_eq!(result.targets.len(), 2);
    assert_ne!(result.targets[0].path, result.targets[1].path);
}
