use java_engine::{Project, ResolveResult, ResolveStatus, TargetKind};

// Minimal signature-only binary data, assembled without Java tools.
fn binary(name: &str, parent: Option<&str>, methods: &[(&str, &str)], public: bool) -> Vec<u8> {
    fn u(b: &mut Vec<u8>, n: u16) {
        b.extend(n.to_be_bytes());
    }
    fn utf(b: &mut Vec<u8>, s: &str) {
        b.push(1);
        u(b, s.len() as u16);
        b.extend(s.as_bytes());
    }
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61];
    u(&mut b, (5 + 2 * methods.len()) as u16);
    utf(&mut b, name);
    b.push(7);
    u(&mut b, 1);
    utf(&mut b, parent.unwrap_or(name));
    b.push(7);
    u(&mut b, 3);
    for (name, descriptor) in methods {
        utf(&mut b, name);
        utf(&mut b, descriptor);
    }
    u(&mut b, if public { 0x21 } else { 0x20 });
    u(&mut b, 2);
    u(&mut b, if parent.is_some() { 4 } else { 0 });
    u(&mut b, 0);
    u(&mut b, 0);
    u(&mut b, methods.len() as u16);
    for i in 0..methods.len() {
        u(&mut b, 0x101);
        u(&mut b, 5 + 2 * i as u16);
        u(&mut b, 6 + 2 * i as u16);
        u(&mut b, 0);
    }
    u(&mut b, 0);
    b
}
fn project(source: &str) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", source);
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, &[], true)).unwrap();
    p
}
fn query(p: &Project, path: &str, needle: &str) -> ResolveResult {
    let at = p.file(path).unwrap().text().rfind(needle).unwrap() as u32;
    let result = p.resolve_at(path, at);
    assert_eq!(result, p.prepare().resolve_at(path, at));
    result
}
fn selected(p: &Project, needle: &str, path: &str, declaration: &str) {
    let result = query(p, "Calls.java", needle);
    assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
    assert_eq!(result.targets.len(), 1);
    let t = &result.targets[0];
    let at = p.file(path).unwrap().text().find(declaration).unwrap() as u32;
    assert_eq!(t.path, path);
    assert_eq!(t.kind, TargetKind::Method);
    assert_eq!((t.start_byte, t.end_byte), (at, at + t.name.len() as u32));
}
fn refused(p: &Project, needle: &str, declarations: &[(&str, &str)]) {
    let result = query(p, "Calls.java", needle);
    assert_ne!(result.status, ResolveStatus::Resolved, "{result:?}");
    let expected: Vec<_> = declarations
        .iter()
        .map(|(path, decl)| {
            let at = p.file(path).unwrap().text().find(decl).unwrap() as u32;
            (path.to_string(), at)
        })
        .collect();
    let actual: Vec<_> = result.targets.iter().map(|t| (t.path.clone(), t.start_byte)).collect();
    assert_eq!(actual, expected, "{result:?}");
}
#[test]
fn explicit_reference_identity_widening_and_specificity() {
    for methods in ["public void m(A x) {} public void m(B x) {}", "public void m(B x) {} public void m(A x) {}"] {
        let source = format!(
            "class A {{}} class B extends A {{}} class D extends B {{}} class C {{ {methods} }} class Calls {{ void run(C c, D d) {{ c.m(d); }} }}"
        );
        selected(&project(&source), "m(d)", "Calls.java", "m(B x)");
    }
    selected(
        &project("class A {} class C { public void m(A x) {} } class Calls { void run(C c, A a) { c.m((a)); } }"),
        "m((a))",
        "Calls.java",
        "m(A x)",
    );
}
#[test]
fn incomparable_interfaces_preserve_all_candidates() {
    let p = project(
        "interface A {} interface B {} class D implements A, B {} class C { public void m(A x) {} public void m(B x) {} } class Calls { void run(C c, D d) { c.m(d); } }",
    );
    refused(&p, "m(d)", &[("Calls.java", "m(A x)"), ("Calls.java", "m(B x)")]);
}
#[test]
fn independent_qualified_arity_and_type_receiver_guards_need_no_platform() {
    let source = "class C { public void only() {} } class Calls { void run(C c) { c.only(1); C.only(); } }";
    let mut p = Project::new();
    p.insert("Calls.java", source);
    refused(&p, "only(1)", &[("Calls.java", "only()")]);
    refused(&p, "only();", &[("Calls.java", "only()")]);
    p.insert("Calls.java", "class C { public static void only() {} } class Calls { void run() { C.only(1); } }");
    refused(&p, "only(1)", &[("Calls.java", "only()")]);
}
#[test]
fn unique_unknown_arguments_never_become_applicability_evidence() {
    for actual in ["null", "new A()", "give()", "a.value", "1"] {
        let source = format!(
            "class A {{ A value; }} class C {{ public void m(A x) {{}} }} class Calls {{ A give() {{ return null; }} void run(C c, A a) {{ c.m({actual}); }} }}"
        );
        refused(&project(&source), &format!("m({actual})"), &[("Calls.java", "m(A x)")]);
    }
}
#[test]
fn argument_and_formal_names_use_their_own_import_scopes() {
    let mut p =
        project("package app; import api.C; import model.Child; class Calls { void run(C c, Child a) { c.m(a); } }");
    p.insert(
        "C.java",
        "package api; import model.Base; public class C { public void m(Base x) {} public void m(other.Base x) {} }",
    );
    p.insert("Base.java", "package model; public class Base {}");
    p.insert("Child.java", "package model; public class Child extends Base {}");
    p.insert("Other.java", "package other; public class Base {}");
    selected(&p, "m(a)", "C.java", "m(Base x)");
}
#[test]
fn incomplete_hierarchy_and_same_name_ancestors_refuse() {
    for parent in ["Missing", "Base"] {
        let source = format!(
            "class A {{}} class Base {{ public void m(A x) {{}} }} class C extends {parent} {{ public void m(A x) {{}} }} class Calls {{ void run(C c, A a) {{ c.m(a); }} }}"
        );
        let p = project(&source);
        let result = query(&p, "Calls.java", "m(a)");
        assert_ne!(result.status, ResolveStatus::Resolved, "{result:?}");
    }
    let mut p = project("class A {} class C { public void m(A x) {} } class Calls { void run(C c, A a) { c.m(a); } }");
    p.remove_class("Object.class");
    refused(&p, "m(a)", &[("Calls.java", "m(A x)")]);
}
#[test]
fn generic_and_static_competitors_are_not_filtered_out() {
    for competitor in ["public <T> void m(T x) {}", "public static void m(B x) {}", "public void m(A... x) {}"] {
        let source = format!(
            "class A {{}} class B extends A {{}} class C {{ public void m(A x) {{}} {competitor} }} class Calls {{ void run(C c, B b) {{ c.m(b); }} }}"
        );
        assert_ne!(query(&project(&source), "Calls.java", "m(b)").status, ResolveStatus::Resolved);
    }
}
#[test]
fn access_proof_does_not_reuse_legacy_protected_filtering() {
    let mut p =
        project("package app; import api.C; import api.A; class Calls extends C { void run(C c, A a) { c.m(a); } }");
    p.insert("C.java", "package api; public class C { protected void m(A x) {} }");
    p.insert("A.java", "package api; public class A {}");
    refused(&p, "m(a)", &[("C.java", "m(A x)")]);
    p.insert("C.java", "package api; class C { public void m(A x) {} }");
    refused(&p, "m(a)", &[("C.java", "m(A x)")]);
    p.insert("C.java", "package api; public class C { public void m(A x) {} }");
    p.insert("A.java", "package api; class A {}");
    refused(&p, "m(a)", &[("C.java", "m(A x)")]);
}
#[test]
fn generic_array_inferred_and_shadowed_declarations_are_not_erased() {
    for declaration in ["A<String> a", "A[] a", "var a", "Missing a"] {
        let source = format!(
            "class A<T> {{}} class C {{ public void m(A x) {{}} }} class Calls {{ void run(C c) {{ {declaration} = null; c.m(a); }} }}"
        );
        refused(&project(&source), "m(a)", &[("Calls.java", "m(A x)")]);
    }
    let source = "class A {} class C { public void m(A x) {} } class Calls { <A> void run(C c, A a) { c.m(a); } }";
    refused(&project(source), "m(a)", &[("Calls.java", "m(A x)")]);
}
#[test]
fn qualified_nested_call_uses_its_own_argument_list() {
    let source = "class A {} class B extends A {} class C { public C m(A x) { return this; } public C m(B x) { return this; } public void other(A a, A b) {} } class Calls { void run(C c, B b, A a) { c.m(b).other(a, a); } }";
    selected(&project(source), "m(b)", "Calls.java", "m(B x)");
}
#[test]
fn binary_formal_endpoints_do_not_enable_binary_method_selection() {
    let mut p = project(
        "class A {} class C { public void m(A x) {} public void m(java.lang.Number x) {} } class Calls { void run(C c, A a) { c.m(a); } }",
    );
    p.add_class_bytes("Number.class", &binary("java/lang/Number", Some("java/lang/Object"), &[], true)).unwrap();
    selected(&p, "m(a)", "Calls.java", "m(A x)");
}

#[test]
fn interface_widening_has_complete_class_and_interface_edges() {
    let p = project(
        "interface Marker {} class Root {} class Other {} class Branch extends Root implements Marker {} class C { public void m(Marker x) {} public void m(Other x) {} } class Calls { void run(C c, Branch a) { c.m(a); } }",
    );
    selected(&p, "m(a)", "Calls.java", "m(Marker x)");
}

#[test]
fn malformed_or_illegal_inventory_never_proves_a_target() {
    for header in [
        "class Base<>",
        "class Base<T extends>",
        "class Base<T,>",
        "class Base<T, T>",
        "final class Base",
        "interface Base",
    ] {
        let source = format!(
            "{header} {{}} class A {{}} class C extends Base {{ public void m(A a) {{}} }} class Calls {{ void run(C c, A a) {{ c.m(a); }} }}"
        );
        assert_ne!(query(&project(&source), "Calls.java", "m(a)").status, ResolveStatus::Resolved, "{source}");
    }
    for members in [
        "public void m(A a) throws int {}",
        "public void m(A a);",
        "public A<> m(A a) { return null; }",
        "public void m(A a) {} public void m A b;",
        "public abstract void m(A a);",
        "public public void m(A a) {}",
    ] {
        let source = format!("class A {{}} class C {{ {members} }} class Calls {{ void run(C c, A a) {{ c.m(a); }} }}");
        assert_ne!(query(&project(&source), "Calls.java", "m(a)").status, ResolveStatus::Resolved, "{source}");
    }
}
#[test]
fn inherited_candidates_cycles_and_ambiguous_parents_are_preserved() {
    let p = project(
        "class A {} class B extends A {} class Base { public void m(A a) {} } class C extends Base { public void m(B b) {} } class Calls { void run(C c, B b) { c.m(b); } }",
    );
    refused(&p, "m(b)", &[("Calls.java", "m(B b)"), ("Calls.java", "m(A a)")]);
    let p = project(
        "class A {} class Base { public void m(A a) {} } class C extends Base {} class Calls { void run(A a) { C.m(a); } }",
    );
    refused(&p, "m(a)", &[("Calls.java", "m(A a)")]);
    let p = project(
        "class A {} class C extends C { public void m(A a) {} } class Calls { void run(C c, A a) { c.m(a); } }",
    );
    let result = query(&p, "Calls.java", "m(a)");
    assert_eq!(result.status, ResolveStatus::Cycle);
    assert!(result.targets.is_empty());
    let mut p = project(
        "class A {} class C extends Base { public void m(A a) {} } class Calls { void run(C c, A a) { c.m(a); } }",
    );
    p.insert("Base1.java", "class Base {}");
    p.insert("Base2.java", "class Base {}");
    let result = query(&p, "Calls.java", "m(a)");
    assert_eq!(result.status, ResolveStatus::Ambiguous);
    assert_eq!(result.targets.len(), 2);
    assert!(result.targets.iter().all(|t| t.kind == TargetKind::Class));
}
#[test]
fn parentheses_cannot_reinterpret_a_selector_as_an_unrelated_field() {
    let p = project(
        "class A {} class C { public void m(A a) {} } class Calls { C m; void run(C c, A a) { (c).m(a); (c).m(1, 2); } }",
    );
    selected(&p, "m(a);", "Calls.java", "m(A a)");
    refused(&p, "m(1, 2)", &[("Calls.java", "m(A a)")]);
}
#[test]
fn hierarchy_and_formal_access_are_checked_in_the_declaring_package() {
    let mut p = project("package q; import p.C; class Calls { void run(C c, A a) { c.m(a); } }");
    p.insert("A.java", "package q; class A {}");
    p.insert("C.java", "package p; public class C { public void m(q.A a) {} }");
    refused(&p, "m(a)", &[("C.java", "m(q.A a)")]);
    p.insert("A.java", "package q; public class A {}");
    p.insert("C.java", "package p; import r.Hidden; public class C extends Hidden { public void m(q.A a) {} }");
    p.insert("Hidden.java", "package r; class Hidden {}");
    refused(&p, "m(a)", &[("C.java", "m(q.A a)")]);
    p.insert("C.java", "package p; public class C extends Hidden { public void m(q.A a) {} }");
    p.insert("Hidden.java", "package p; class Hidden {}");
    selected(&p, "m(a)", "C.java", "m(q.A a)");
}
#[test]
fn proof_budget_exhaustion_and_local_type_boundaries_refuse() {
    let mut source = "class A {} class C { public void m(A a) {} } class Calls { void run(C c, A a) { c.m(".to_string();
    source.push_str(&"(".repeat(40));
    source.push('a');
    source.push_str(&")".repeat(40));
    source.push_str("); } }");
    refused(&project(&source), "m(", &[("Calls.java", "m(A a)")]);
    let mut p = project(
        "class A {} class C extends P0 { public void m(A a) {} } class Calls { void run(C c, A a) { c.m(a); } }",
    );
    for i in 0..40 {
        p.insert(format!("P{i}.java"), &format!("class P{i} extends P{} {{}}", i + 1));
    }
    p.insert("P40.java", "class P40 {}");
    refused(&p, "m(a)", &[("Calls.java", "m(A a)")]);
    let source =
        "class A {} class C { public void m(A a) {} } class Calls { void run(C c, A a) { class A {} c.m(a); } }";
    refused(&project(source), "m(a)", &[("Calls.java", "m(A a)")]);
    let many = (0..150).map(|i| format!("public void m(T{i} a) {{}} ")).collect::<String>();
    let types = (0..150).map(|i| format!("class T{i} {{}} ")).collect::<String>();
    let source = format!("{types} class C {{ {many} }} class Calls {{ void run(C c, T0 a) {{ c.m(a); }} }}");
    let result = query(&project(&source), "Calls.java", "m(a)");
    assert_ne!(result.status, ResolveStatus::Resolved);
    assert_eq!(result.targets.len(), 150);
}

#[test]
fn source_and_binary_reference_identities_stay_in_the_owning_compilation() {
    use java_engine::{
        ClasspathEntry, CompilationConfig, CompilationId, ProjectConfig, WorkspaceConfig, WorkspaceSnapshot,
    };
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "reference-scopes-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    )));
    for dir in ["lib", "app", "platform/java/lang"] {
        fs::create_dir_all(fixture.0.join(dir)).unwrap();
    }
    let callee =
        "package api; public class C { public void m(Object a) {} public void m(C a) {} public void same(C a) {} }";
    let caller = "package app; import api.C; public class Calls { void run(C c, Object a) { c.m(a); c.same(c); } }";
    fs::write(fixture.0.join("lib/C.java"), callee).unwrap();
    fs::write(fixture.0.join("app/Calls.java"), caller).unwrap();
    fs::write(fixture.0.join("platform/java/lang/Object.class"), binary("java/lang/Object", None, &[], true)).unwrap();
    let lib = CompilationId::new("lib", "main");
    let app = CompilationId::new("app", "main");
    let inputs = |directory: &str| ProjectConfig {
        source_roots: vec![fixture.0.join(directory)],
        classpath: vec![ClasspathEntry::Directory(fixture.0.join("platform"))],
        ..Default::default()
    };
    let config = WorkspaceConfig {
        compilations: vec![
            CompilationConfig { id: lib.clone(), dependencies: vec![], inputs: inputs("lib") },
            CompilationConfig { id: app.clone(), dependencies: vec![lib.clone()], inputs: inputs("app") },
        ],
        ..Default::default()
    };
    let snapshot = WorkspaceSnapshot::load(&config).unwrap();
    let path = fixture.0.join("app/Calls.java").canonicalize().unwrap().to_str().unwrap().to_owned();
    for (needle, expected) in [("m(a)", ResolveStatus::OverloadNotResolved), ("same(c)", ResolveStatus::Resolved)] {
        let at = caller.find(needle).unwrap() as u32;
        let result = snapshot.scope(&app).unwrap().resolve_at(&path, at);
        assert_eq!(result.status, expected, "{result:?}");
        assert_eq!(snapshot.prepare().scope(&app).unwrap().resolve_at(&path, at), result);
        assert!(result.targets.iter().all(|t| t.path.ends_with("lib/C.java")));
    }
}

#[test]
fn source_extension_of_final_binary_is_not_a_reference_proof() {
    let mut p = project(
        "class A {} class C extends Base { public void m(A a) {} } class Calls { void run(C c, A a) { c.m(a); } }",
    );
    let mut bytes = binary("Base", Some("java/lang/Object"), &[], true);
    let access_low = bytes.len() - 13;
    bytes[access_low] |= 0x10; // ACC_FINAL, independently assembled.
    p.add_class_bytes("Base.class", &bytes).unwrap();
    refused(&p, "m(a)", &[("Calls.java", "m(A a)")]);
}
#[test]
fn type_qualifier_unknowns_cannot_escape_through_legacy_lookup() {
    for source in [
        "interface I {} interface J {} class C { public void m() {} } class Calls implements I,J { void run() { C.m(); } }",
        "class C { public void m() {} } class Calls<C> { void run() { C.m(); } }",
        "class C { public static void m() {} } class Calls { <C> void run() { C.m(); } }",
        "class C { public static void m() {} } class Base { public static class C { public static void m() {} } } class Calls extends Base { void run() { C.m(); } }",
        "class C { public void m() {} } class Base { private int C; } class Calls extends Base { void run() { C.m(); } }",
    ] {
        assert_ne!(query(&project(source), "Calls.java", "m();").status, ResolveStatus::Resolved, "{source}");
    }
}

#[test]
fn explicit_local_declarations_not_initializers_supply_reference_evidence() {
    let p = project(
        "class A {} class B extends A {} class C { public void m(A a) {} public void m(B b) {} } class Calls { void run() { C c = null; B b = null; A a = b; c.m(a); } }",
    );
    selected(&p, "m(a)", "Calls.java", "m(A a)");
}

#[test]
fn exhausted_static_namespace_search_cannot_hide_a_known_nested_type() {
    for count in [675, 700, 800] {
        let classes = (0..count).map(|i| format!("class P{i} {{}} ")).collect::<String>();
        let source = format!(
            "{classes} class A {{}} class C {{ public static void m(A a) {{}} }} class Base {{ public static class C {{ public static void m(A a) {{}} }} }} class Calls extends Base {{ void run(A a) {{ C.m(a); }} }}"
        );
        assert_ne!(query(&project(&source), "Calls.java", "m(a)").status, ResolveStatus::Resolved);
    }
}
