use java_engine::{Project, ResolveStatus, TargetKind};

fn project(text: &str) -> Project {
    let mut p = Project::new();
    p.insert("A.java", text);
    assert!(p.file("A.java").unwrap().errors().is_empty(), "{:?}", p.file("A.java").unwrap().errors());
    p
}

fn target(p: &Project, needle: &str, declaration: &str, name: &str, kind: TargetKind) {
    let text = p.file("A.java").unwrap().text();
    let byte = text.rfind(needle).unwrap() as u32;
    let result = p.resolve_at("A.java", byte);
    assert_eq!(p.prepare().resolve_at("A.java", byte), result);
    assert_eq!(result.status, ResolveStatus::Resolved, "{needle}: {result:?}");
    assert_eq!(result.targets.len(), 1);
    let t = &result.targets[0];
    assert_eq!(t.path, "A.java");
    assert_eq!(t.kind, kind);
    assert_eq!(t.name, name);
    assert_eq!(t.start_byte, text.find(declaration).unwrap() as u32);
    assert_eq!(t.end_byte, t.start_byte + name.len() as u32);
}

#[test]
fn every_field_declarator_binds() {
    let p = project("class A { int i, j, k; int m() { return j + k; } }");
    target(&p, "j +", "j,", "j", TargetKind::Field);
    target(&p, "k;", "k;", "k", TargetKind::Field);
}

#[test]
fn every_local_declarator_already_binds() {
    let p = project("class A { int m() { int i = 0, j = i, k = j; return k; } }");
    target(&p, "i,", "i =", "i", TargetKind::Local);
    target(&p, "j;", "j =", "j", TargetKind::Local);
    target(&p, "k;", "k =", "k", TargetKind::Local);
}

#[test]
fn postfix_dimensions_are_parsed_per_declarator() {
    let p = project(
        "class A { A first[], second, third[][]; int value; void m() { A x[], y; second.value; y.value; first.value; third.value; x.value; } }",
    );
    target(&p, "second.value", "second,", "second", TargetKind::Field);
    target(&p, "y.value", "y;", "y", TargetKind::Local);
    for needle in ["second.value", "y.value"] {
        let text = p.file("A.java").unwrap().text();
        let byte = text.find(needle).unwrap() as u32 + needle.find('.').unwrap() as u32 + 1;
        let result = p.resolve_at("A.java", byte);
        assert_eq!(result.status, ResolveStatus::Resolved);
        assert_eq!(result.targets[0].start_byte, text.find("value;").unwrap() as u32);
        assert_eq!(result.targets[0].owner, "A");
    }
    for needle in ["first.value", "third.value", "x.value"] {
        let byte = p.file("A.java").unwrap().text().find(needle).unwrap() as u32 + needle.find('.').unwrap() as u32 + 1;
        assert_eq!(p.resolve_at("A.java", byte).status, ResolveStatus::ReceiverNotBound, "{needle}");
    }
}

#[test]
fn prefix_arrays_are_not_scalar_receivers() {
    let p = project("class A { A[] first, second; int value; void m() { second.value; } }");
    let byte = p.file("A.java").unwrap().text().rfind("value").unwrap() as u32;
    assert_eq!(p.resolve_at("A.java", byte).status, ResolveStatus::ReceiverNotBound);
}

#[test]
fn nested_initializer_commas_and_names_are_not_declarations() {
    let p = project(
        "class A { int a = f(1, 2), b = f(a, 3), c[] = {a, b}; int f(int x, int y) { return x; } int m() { int d = f(a, b), e = f(d, b), g[] = {d, e}; return e + b; } }",
    );
    target(&p, "b;", "b =", "b", TargetKind::Field);
    target(&p, "e +", "e =", "e", TargetKind::Local);
    target(&p, "d, e", "d =", "d", TargetKind::Local);
    target(&p, "b),", "b =", "b", TargetKind::Field);
}

#[test]
fn declaration_order_and_nested_for_scopes_stay_local() {
    let p = project(
        "class A { int j; void m() { int i = j, j = i; { int x, j; j++; } j++; for (int x = 0, y = x; y < 2; y++) { y++; } y++; } }",
    );
    target(&p, "j, j", "j;", "j", TargetKind::Field);
    target(&p, "i;", "i =", "i", TargetKind::Local);
    target(&p, "j++; } j", "j; j", "j", TargetKind::Local);
    target(&p, "j++; for", "j = i", "j", TargetKind::Local);
    target(&p, "y++; } y", "y = x", "y", TargetKind::Local);
    let byte = p.file("A.java").unwrap().text().rfind("y++").unwrap() as u32;
    assert_eq!(p.resolve_at("A.java", byte).status, ResolveStatus::Unresolved);
}

#[test]
fn declarations_include_unicode_names_and_self_initializers() {
    let p = project("class A { int 一, 二; void m() { int α = 1, β = β; 二 = β; } }");
    target(&p, "二 =", "二;", "二", TargetKind::Field);
    target(&p, "β;", "β =", "β", TargetKind::Local);
    target(&p, "β; 二", "β =", "β", TargetKind::Local);
    target(&p, "二;", "二;", "二", TargetKind::Field);
}

#[test]
fn inherited_later_fields_keep_declaration_imports_and_caller_access() {
    let mut p = Project::new();
    let base = "package lib; import model.Value; public class Base { public Value first, second; private Value hidden, secret; }";
    let value = "package model; public class Value { public int first, leaf; private int hidden, secret; }";
    let caller = "package app; import other.Value; class Child extends lib.Base { void m() { second.leaf; this.second.leaf; second.secret; secret.leaf; } }";
    for (path, text) in [
        ("Base.java", base),
        ("Value.java", value),
        ("Other.java", "package other; public class Value { public int leaf; }"),
        ("Child.java", caller),
    ] {
        p.insert(path, text);
        assert!(p.file(path).unwrap().errors().is_empty());
    }
    for (byte, _) in caller.match_indices("leaf;").take(2) {
        let r = p.resolve_at("Child.java", byte as u32);
        assert_eq!(p.prepare().resolve_at("Child.java", byte as u32), r);
        assert_eq!(r.status, ResolveStatus::Resolved);
        assert_eq!(r.targets[0].path, "Value.java");
        assert_eq!(r.targets[0].owner, "model.Value");
        assert_eq!(r.targets[0].start_byte, value.find("leaf;").unwrap() as u32);
        assert_eq!(r.targets[0].end_byte, r.targets[0].start_byte + 4);
    }
    for byte in [caller.find("secret;").unwrap(), caller.find("secret.leaf").unwrap()] {
        assert_eq!(p.resolve_at("Child.java", byte as u32).status, ResolveStatus::Unresolved);
    }
}

#[test]
fn later_field_shadows_inherited_field_and_preserves_access_flags() {
    let p = project("class Base { int j; } class A extends Base { private int i, j; int m() { return j; } }");
    target(&p, "j;", "j; int", "j", TargetKind::Field);
    assert_eq!(
        p.resolve_at("A.java", p.file("A.java").unwrap().text().rfind("j;").unwrap() as u32).targets[0].owner,
        "A"
    );
    let p = project("class A { public private int i, j; int m() { return j; } }");
    let byte = p.file("A.java").unwrap().text().rfind("j;").unwrap() as u32;
    assert_eq!(p.resolve_at("A.java", byte).status, ResolveStatus::NotDetermined);
}

#[test]
fn arrays_in_shared_types_and_other_declaration_forms_are_refused() {
    for text in [
        "class A { int value; A[] first, second; void m() { first.value; second.value; } }",
        "class A { int value; void m(A[] a, A b[], A... c) { a.value; b.value; c.value; } }",
        "record A(A[] a) { int value; void m() { a.value; } }",
        "class A { int value; void m() { try (A[] a = null) { a.value; } catch (A e) {} } }",
    ] {
        let p = project(text);
        for (byte, _) in text.match_indices(".value") {
            let r = p.resolve_at("A.java", byte as u32 + 1);
            assert_eq!(r.status, ResolveStatus::ReceiverNotBound, "{text}: {r:?}");
            assert_eq!(p.prepare().resolve_at("A.java", byte as u32 + 1), r);
        }
    }
}

#[test]
fn unsupported_inferred_and_missing_receiver_types_stay_refused() {
    for text in [
        "class A { int value; void m() { var a = new A(), b = new A(); b.value; } }",
        "class A { Missing a, b; void m() { b.value; } }",
        "class A { int a, b; void m() { b.value; } }",
    ] {
        let p = project(text);
        let byte = text.rfind("value;").unwrap() as u32;
        assert_eq!(p.resolve_at("A.java", byte).status, ResolveStatus::ReceiverNotBound);
    }
}

#[test]
fn malformed_declarators_remain_lossless_without_invented_names() {
    for text in [
        "class A { int a, ; int good; int m() { return good; } }",
        "class A { int a = , b; int good; int m() { return good; } }",
        "class A { int a[, b; int good; int m() { return good; } }",
        "class A { void m() { int a, ; int good; good++; } }",
    ] {
        let mut p = Project::new();
        p.insert("A.java", text);
        assert!(!p.file("A.java").unwrap().errors().is_empty());
        assert_eq!(p.file("A.java").unwrap().root().utf8_len() as usize, text.len());
        let byte = text.rfind("good").unwrap() as u32;
        let r = p.resolve_at("A.java", byte);
        assert_eq!(r.status, ResolveStatus::Resolved, "{text}: {r:?}");
        assert_eq!(r.targets[0].start_byte, text.find("good;").unwrap() as u32);
        assert_eq!(p.prepare().resolve_at("A.java", byte), r);
    }
}

#[test]
fn edits_rebuild_declarators_and_preserve_old_prepared_views() {
    use java_engine::TextEdit;
    let source = "class A { int a, b; int m() { return b; } }";
    let old = project(source);
    let prepared = old.prepare();
    let mut next = old.clone();
    let byte = source.find("b;").unwrap() as u32;
    next.edit("A.java", TextEdit { start_byte: byte, end_byte: byte + 1, insert: "c".into() }).unwrap();
    let use_byte = source.rfind("b;").unwrap() as u32;
    assert_eq!(prepared.resolve_at("A.java", use_byte).status, ResolveStatus::Resolved);
    assert_eq!(next.prepare().resolve_at("A.java", use_byte).status, ResolveStatus::Unresolved);
    let full = project(next.file("A.java").unwrap().text());
    assert_eq!(next.file("A.java").unwrap().root(), full.file("A.java").unwrap().root());
    for byte in 0..source.len() as u32 {
        assert_eq!(next.resolve_at("A.java", byte), full.resolve_at("A.java", byte));
    }
}

#[test]
fn workspace_and_prepared_views_keep_later_field_declaration_context() {
    use java_engine::{CompilationConfig, CompilationId, ProjectConfig, WorkspaceConfig, WorkspaceSnapshot};
    use std::fs;
    let root = std::env::temp_dir().join(format!("java-declarators-{}", std::process::id()));
    fs::create_dir_all(root.join("lib")).unwrap();
    fs::create_dir_all(root.join("app")).unwrap();
    let lib = "package lib; public class Box { public Value a, b; } class Value { public int first, leaf; }";
    let app = "package app; class Value { public int leaf; } class Use { void m(lib.Box box) { box.b.leaf; } }";
    fs::write(root.join("lib/Box.java"), lib).unwrap();
    fs::write(root.join("app/Use.java"), app).unwrap();
    let lib_id = CompilationId::new("lib", "main");
    let app_id = CompilationId::new("app", "main");
    let config = WorkspaceConfig {
        compilations: vec![
            CompilationConfig {
                id: lib_id.clone(),
                inputs: ProjectConfig { source_roots: vec![root.join("lib")], ..Default::default() },
                dependencies: vec![],
            },
            CompilationConfig {
                id: app_id.clone(),
                inputs: ProjectConfig { source_roots: vec![root.join("app")], ..Default::default() },
                dependencies: vec![lib_id.clone()],
            },
        ],
        ..Default::default()
    };
    let snapshot = WorkspaceSnapshot::load(&config).unwrap();
    let prepared = snapshot.prepare();
    let path = root.join("app/Use.java").canonicalize().unwrap().to_str().unwrap().to_owned();
    let byte = app.rfind("leaf").unwrap() as u32;
    let r = snapshot.scope(&app_id).unwrap().resolve_at(&path, byte);
    assert_eq!(prepared.scope(&app_id).unwrap().resolve_at(&path, byte), r);
    assert_eq!(r.status, ResolveStatus::Resolved);
    let target = &r.targets[0];
    assert_eq!(target.owner, "lib.Value");
    assert_eq!(target.path, root.join("lib/Box.java").canonicalize().unwrap().to_str().unwrap());
    assert_eq!(target.start_byte, lib.find("leaf;").unwrap() as u32);
    assert_eq!(target.end_byte, target.start_byte + 4);
    assert_eq!(prepared.scope(&lib_id).unwrap().resolve_at(&path, byte).status, ResolveStatus::Unresolved);
    fs::remove_dir_all(root).unwrap();
}
