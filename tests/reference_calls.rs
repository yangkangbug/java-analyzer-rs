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
    for actual in ["(A)(A)null", "new A()", "give()", "a.value", "1"] {
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
fn source_identity_and_exact_binary_compatibility_preserve_declaring_scope() {
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
    let caller = "package app; import api.C; public class Calls { void run(C c, Object a) { c.m(a); c.same(c); } }";
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
    // Preserve the original identity fixture alongside its scalar-competitor extension.
    for callee in [
        "package api; public class C { public void m(Object a) {} public void m(C a) {} public void same(C a) {} }",
        "package api; public class C { public void m(Object a) {} public void m(C a) {} public void m(int a) {} public void same(C a) {} public void same(int a) {} }",
    ] {
        fs::write(fixture.0.join("lib/C.java"), callee).unwrap();
        let snapshot = WorkspaceSnapshot::load(&config).unwrap();
        let path = fixture.0.join("app/Calls.java").canonicalize().unwrap().to_str().unwrap().to_owned();
        // The exact Object root now has cross-scope argument compatibility;
        // source C still uses its existing visible declaration identity.
        for (needle, declaration) in [("m(a)", "m(Object a)"), ("same(c)", "same(C a)")] {
            let at = caller.find(needle).unwrap() as u32;
            let result = snapshot.scope(&app).unwrap().resolve_at(&path, at);
            assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
            assert_eq!(result.targets.len(), 1);
            assert_eq!(result.targets[0].start_byte, callee.find(declaration).unwrap() as u32);
            assert_eq!(snapshot.prepare().scope(&app).unwrap().resolve_at(&path, at), result);
            assert!(result.targets.iter().all(|t| t.path.ends_with("lib/C.java")));
        }
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

#[test]
fn explicit_zero_argument_construction_proves_reference_arguments() {
    for methods in [
        "public void m(Base x) {} public void m(Other x) {} public void m(java.lang.Object x) {}",
        "public void m(java.lang.Object x) {} public void m(Other x) {} public void m(Base x) {}",
    ] {
        let source = format!(
            "class Base {{}} class Other {{}} final class Leaf extends Base {{ @SuppressWarnings(\"unused\") public Leaf() {{}} }} class C {{ {methods} }} class Calls {{ void run(C c) {{ c.m((new Leaf())); }} }}"
        );
        selected(&project(&source), "m((new Leaf()))", "Calls.java", "m(Base x)");
    }
}

#[test]
fn construction_arguments_require_explicit_supported_constructor_inventory() {
    for declaration in [
        "class Leaf extends Base {}",
        "class Leaf extends Base { public void Leaf() {} }",
        "class Leaf extends Base { public Wrong() {} }",
        "class Leaf extends Base { private Leaf() {} }",
        "class Leaf extends Base { protected Leaf() {} }",
        "class Leaf extends Base { Leaf() {} }",
        "abstract class Leaf extends Base { public Leaf() {} }",
        "interface Leaf {}",
        "enum Leaf { ONE }",
        "record Leaf() {}",
        "class Leaf<T> extends Base { public Leaf() {} }",
        "class Leaf extends Base { public <T> Leaf() {} }",
        "class Leaf extends Base { public Leaf(int x) {} }",
        "class Leaf extends Base { public Leaf(Base... x) {} }",
        "class Leaf extends Base { public Leaf() {} public Leaf(int x) {} }",
        "class Leaf extends Base { public Leaf() {} public Leaf() {} }",
        "class Leaf extends Base { public Leaf() throws Missing {} }",
        "class Leaf extends Base { public Leaf(); }",
        "class Leaf extends Base { public Leaf() { int =; } }",
    ] {
        let source = format!(
            "class Base {{}} {declaration} class C {{ public void m(Base x) {{}} }} class Calls {{ void run(C c) {{ c.m(new Leaf()); }} }}"
        );
        refused(&project(&source), "m(new Leaf())", &[("Calls.java", "m(Base x)")]);
    }
}

#[test]
fn construction_argument_syntax_is_exact_and_missing_evidence_refuses() {
    for actual in [
        "new Leaf(1)",
        "new Leaf() {}",
        "new Leaf[1]",
        "new Leaf[] {}",
        "new Leaf<>()",
        "new <Base> Leaf()",
        "missing",
        "new Missing()",
    ] {
        let source = format!(
            "class Base {{}} class Leaf extends Base {{ public Leaf() {{}} }} class C {{ public void m(Base x) {{}} }} class Calls {{ void run(C c) {{ c.m({actual}); }} }}"
        );
        refused(&project(&source), &format!("m({actual})"), &[("Calls.java", "m(Base x)")]);
    }
    let source = "class Base {} class Leaf extends Base { public Leaf() {} } class C { public void m(Base x) {} } class Calls { void run(C c) { c.m(new Leaf()); } }";
    let mut p = project(source);
    p.remove_class("Object.class");
    refused(&p, "m(new Leaf())", &[("Calls.java", "m(Base x)")]);
}

#[test]
fn construction_arguments_preserve_identity_access_and_complete_hierarchies() {
    let call = "class C { public void m(java.lang.Object x) {} } class Calls { void run(C c) { c.m(new Leaf()); } }";
    for declaration in [
        "class Leaf extends Missing { public Leaf() {} }",
        "class Leaf extends Leaf { public Leaf() {} }",
        "final class Base {} class Leaf extends Base { public Leaf() {} }",
        "class Leaf { public Leaf() {} public void broken(; }",
        "class Leaf { public Leaf() {} } class Leaf { public Leaf() {} }",
    ] {
        let p = project(&format!("{declaration} {call}"));
        assert_ne!(query(&p, "Calls.java", "m(new Leaf())").status, ResolveStatus::Resolved, "{declaration}");
    }
    let mut p = project(&format!("import model.Leaf; {call}"));
    p.insert("Leaf.java", "package model; class Leaf { public Leaf() {} }");
    refused(&p, "m(new Leaf())", &[("Calls.java", "m(java.lang.Object x)")]);
    p.insert("Leaf.java", "package model; public class Leaf { public Leaf() {} }");
    selected(&p, "m(new Leaf())", "Calls.java", "m(java.lang.Object x)");
    p.insert("Duplicate.java", "package model; public class Leaf { public Leaf() {} }");
    assert_ne!(query(&p, "Calls.java", "m(new Leaf())").status, ResolveStatus::Resolved);
    for caller in [
        "class Calls<Leaf> { void run(C c) { c.m(new Leaf()); } }",
        "class Calls { <Leaf> void run(C c) { c.m(new Leaf()); } }",
        "class Calls { void run(C c) { class Leaf {} c.m(new Leaf()); } }",
        "class Calls { void run(C c) { int =; c.m(new Leaf()); } }",
        "class Calls extends Missing { void run(C c) { c.m(new Leaf()); } }",
        "class Calls { static class Leaf { public Leaf() {} } void run(C c) { c.m(new Leaf()); } }",
    ] {
        let p = project(&format!(
            "class Leaf {{ public Leaf() {{}} }} class C {{ public void m(java.lang.Object x) {{}} }} {caller}"
        ));
        refused(&p, "m(new Leaf())", &[("Calls.java", "m(java.lang.Object x)")]);
    }
}

#[test]
fn construction_arguments_keep_all_method_competitors_and_shared_limits() {
    for competitor in [
        "public <T> void m(T x) {}",
        "private void m(Leaf x) {}",
        "public static void m(Leaf x) {}",
        "public void m(Leaf... x) {}",
    ] {
        let p = project(&format!(
            "class Leaf {{ public Leaf() {{}} }} class C {{ public void m(java.lang.Object x) {{}} {competitor} }} class Calls {{ void run(C c) {{ c.m(new Leaf()); }} }}"
        ));
        assert_ne!(query(&p, "Calls.java", "m(new Leaf())").status, ResolveStatus::Resolved);
    }
    let p = project(
        "class Leaf { public Leaf() {} } class Parent { public void m(Leaf x) {} } class C extends Parent { public void m(java.lang.Object x) {} } class Calls { void run(C c) { c.m(new Leaf()); } }",
    );
    refused(&p, "m(new Leaf())", &[("Calls.java", "m(java.lang.Object x)"), ("Calls.java", "m(Leaf x)")]);
    for (body, argument) in [
        (format!("/*{}*/", "x".repeat(5000)), "new Leaf()".to_string()),
        (String::new(), format!("{}new Leaf(){}", "(".repeat(40), ")".repeat(40))),
    ] {
        let p = project(&format!(
            "class Leaf {{ public Leaf() {{ {body} }} }} class C {{ public void m(java.lang.Object x) {{}} }} class Calls {{ void run(C c) {{ c.m({argument}); }} }}"
        ));
        refused(&p, "m(", &[("Calls.java", "m(java.lang.Object x)")]);
    }
}

#[test]
fn construction_argument_support_does_not_enable_new_receivers_or_call_results() {
    for statement in ["new C().m(new Leaf());", "(new C()).m(new Leaf());", "c.m(new Leaf().self());"] {
        let p = project(&format!(
            "class Leaf {{ public Leaf() {{}} public Leaf self() {{ return this; }} }} class C {{ public C() {{}} public void m(Leaf x) {{}} public void m(java.lang.Object x) {{}} }} class Calls {{ void run(C c) {{ {statement} }} }}"
        ));
        assert_ne!(query(&p, "Calls.java", "m(new Leaf()").status, ResolveStatus::Resolved, "{statement}");
    }
}

#[test]
fn construction_type_annotations_and_binary_classes_remain_unsupported() {
    let mut p =
        project("class C { public void m(java.lang.Object x) {} } class Calls { void run(C c) { c.m(new Leaf()); } }");
    p.add_class_bytes("Leaf.class", &binary("Leaf", Some("java/lang/Object"), &[("<init>", "()V")], true)).unwrap();
    refused(&p, "m(new Leaf())", &[("Calls.java", "m(java.lang.Object x)")]);
    p.remove_class("Leaf.class");
    p.insert("Leaf.java", "class Leaf { public Leaf() {} public void Leaf() {} }");
    selected(&p, "m(new Leaf())", "Calls.java", "m(java.lang.Object x)");
    p.insert(
        "Calls.java",
        "class C { public void m(java.lang.Object x) {} } class Calls { void run(C c) { c.m(new @Marker Leaf()); } }",
    );
    refused(&p, "m(new @Marker Leaf())", &[("Calls.java", "m(java.lang.Object x)")]);
}

// Signature-only String fixtures, never executable Java. The optional Signature
// and Comparable edge model the conservative behavior of real platform String.
fn literal_string_binary(flags: u16, signature: Option<&str>, interfaces: &[&str]) -> Vec<u8> {
    fn u(out: &mut Vec<u8>, n: u16) {
        out.extend(n.to_be_bytes());
    }
    fn utf(out: &mut Vec<u8>, s: &str) {
        out.push(1);
        u(out, s.len() as u16);
        out.extend(s.as_bytes());
    }
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61];
    u(&mut b, (5 + interfaces.len() * 2 + if signature.is_some() { 2 } else { 0 }) as u16);
    utf(&mut b, "java/lang/String");
    b.push(7);
    u(&mut b, 1);
    utf(&mut b, "java/lang/Object");
    b.push(7);
    u(&mut b, 3);
    for (i, interface) in interfaces.iter().enumerate() {
        utf(&mut b, interface);
        b.push(7);
        u(&mut b, (5 + i * 2) as u16);
    }
    if let Some(s) = signature {
        utf(&mut b, "Signature");
        utf(&mut b, s);
    }
    u(&mut b, flags);
    u(&mut b, 2);
    u(&mut b, 4);
    u(&mut b, interfaces.len() as u16);
    for i in 0..interfaces.len() {
        u(&mut b, (6 + i * 2) as u16);
    }
    u(&mut b, 0);
    u(&mut b, 0);
    u(&mut b, u16::from(signature.is_some()));
    if signature.is_some() {
        u(&mut b, (5 + interfaces.len() * 2) as u16);
        b.extend(2u32.to_be_bytes());
        u(&mut b, (6 + interfaces.len() * 2) as u16);
    }
    b
}
fn literal_project(source: &str) -> Project {
    let mut p = project(source);
    p.add_class_bytes("String.class", &literal_string_binary(0x31, None, &[])).unwrap();
    p
}
#[test]
fn string_literals_validate_ordinary_spelling_and_preserve_ranges() {
    for literal in [r#""""#, r#""hello""#, r#""中文😀""#, r#""\b\t\n\f\r\s\"\'\\""#, r#""\0\07\377\400\777""#] {
        let source = format!(
            "/* 😀 */ class C {{ public void m(java.lang.String x) {{}} }} class Calls {{ void run(C c) {{ c.m(({literal})); }} }}"
        );
        selected(&literal_project(&source), &format!("m(({literal}))"), "Calls.java", "m(java.lang.String x)");
    }
}
#[test]
fn string_literals_reject_other_expressions_and_unaudited_spelling() {
    let null_source = "class C { public void m(java.lang.String x) {} } class Calls { void run(C c) { c.m(null); } }";
    selected(&literal_project(null_source), "m(null)", "Calls.java", "m(java.lang.String x)");
    let cast_source = null_source.replace("m(null)", "m((java.lang.String)null)");
    selected(&literal_project(&cast_source), "m((java.lang.String)null)", "Calls.java", "m(java.lang.String x)");

    for literal in [
        r#""\q""#,
        r#""\8""#,
        r#""\u0041""#,
        r#""\uuuu0041""#,
        r#""\u0022""#,
        r#""\u000a""#,
        r#""\u005c""#,
        r#""\\u0041""#,
        "\"a\\\nb\"",
        "\"a\\\rb\"",
        "\"\"\"\nhello\n\"\"\"",
        "(java.lang.String)(java.lang.String)null",
        "1",
        "true",
        "'a'",
        r#""a" + "b""#,
        r#"(java.lang.String) "a""#,
    ] {
        let source = format!(
            "class C {{ public void m(java.lang.String x) {{}} }} class Calls {{ void run(C c) {{ c.m({literal}); }} }}"
        );
        refused(&literal_project(&source), &format!("m({literal})"), &[("Calls.java", "m(java.lang.String x)")]);
    }
}
#[test]
fn string_literals_do_not_follow_lexical_string_shadows() {
    let source = r#"class String {} class C { public void m(java.lang.String x) {} public void m(String x) {} } class Calls { void run(C c) { c.m("x"); } }"#;
    selected(&literal_project(source), r#"m("x")"#, "Calls.java", "m(java.lang.String x)");
    let source =
        r#"class String {} class C { public void m(String x) {} } class Calls { void run(C c) { c.m("x"); } }"#;
    refused(&literal_project(source), r#"m("x")"#, &[("Calls.java", "m(String x)")]);
    let source = r#"class java {} class C { public void m(String x) {} } class Calls { class String {} void run(C c) { c.m("x"); } }"#;
    selected(&literal_project(source), r#"m("x")"#, "Calls.java", "m(String x)");
}
#[test]
fn string_literals_require_unique_complete_binary_platform_identity() {
    let source = r#"class C { public void m(java.lang.String x) {} } class Calls { void run(C c) { c.m("x"); } }"#;
    for missing in ["String.class", "Object.class"] {
        let mut p = literal_project(source);
        p.remove_class(missing);
        refused(&p, r#"m("x")"#, &[("Calls.java", "m(java.lang.String x)")]);
    }
    for flags in [0x21, 0x30, 0x231] {
        let mut p = project(source);
        p.add_class_bytes("String.class", &literal_string_binary(flags, None, &[])).unwrap();
        assert_ne!(query(&p, "Calls.java", r#"m("x")"#).status, ResolveStatus::Resolved);
    }
    let mut p = project(source);
    p.insert("String.java", "package java.lang; public final class String {}");
    refused(&p, r#"m("x")"#, &[("Calls.java", "m(java.lang.String x)")]);
    let mut p = literal_project(source);
    p.insert("String.java", "package java.lang; public final class String {}");
    assert_ne!(query(&p, "Calls.java", r#"m("x")"#).status, ResolveStatus::Resolved);
    let mut p = literal_project(source);
    p.add_class_bytes("OtherString.class", &literal_string_binary(0x31, None, &[])).unwrap();
    assert_ne!(query(&p, "Calls.java", r#"m("x")"#).status, ResolveStatus::Resolved);
    let mut p = project(source);
    p.add_class_bytes("String.class", &literal_string_binary(0x31, None, &["missing/Interface"])).unwrap();
    refused(&p, r#"m("x")"#, &[("Calls.java", "m(java.lang.String x)")]);
}
#[test]
fn string_literals_preserve_competitor_and_budget_refusals() {
    for competitor in [
        "public void m(int x) {}",
        "public static void m(Object x) {}",
        "public <T> void m(T x) {}",
        "public void m(Object... x) {}",
        "private void m(Object x) {}",
    ] {
        let source = format!(
            "class C {{ public void m(String x) {{}} {competitor} }} class Calls {{ void run(C c) {{ c.m(\"x\"); }} }}"
        );
        if competitor == "public void m(int x) {}" {
            // Intentional additive change: exact String beats a strict-inapplicable scalar.
            selected(&literal_project(&source), "m(\"x\")", "Calls.java", "m(String x)");
        } else {
            assert_ne!(query(&literal_project(&source), "Calls.java", "m(\"x\")").status, ResolveStatus::Resolved);
        }
    }
    for literal in [format!("\"{}\"", "x".repeat(5000)), format!("{}\"x\"{}", "(".repeat(40), ")".repeat(40))] {
        let source =
            format!("class C {{ public void m(String x) {{}} }} class Calls {{ void run(C c) {{ c.m({literal}); }} }}");
        refused(&literal_project(&source), &format!("m({literal})"), &[("Calls.java", "m(String x)")]);
    }
}
#[test]
fn string_literal_specificity_requires_proved_binary_edges() {
    for methods in [
        "public void m(String x) {} public void m(Object x) {}",
        "public void m(Object x) {} public void m(String x) {}",
    ] {
        let source = format!("class C {{ {methods} }} class Calls {{ void run(C c) {{ c.m(\"x\"); }} }}");
        selected(&literal_project(&source), "m(\"x\")", "Calls.java", "m(String x)");
    }
    // Real String has a generic Signature. Existing conversion deliberately
    // declines even its Object edge, rather than treating erasure as proof.
    let source = r#"class C { public void m(String x) {} public void m(Object x) {} } class Calls { void run(C c) { c.m("x"); } }"#;
    let mut p = project(source);
    p.add_class_bytes(
        "String.class",
        &literal_string_binary(
            0x31,
            Some("Ljava/lang/Object;Ljava/lang/Comparable<Ljava/lang/String;>;"),
            &["java/lang/Comparable"],
        ),
    )
    .unwrap();
    let mut comparable = binary("java/lang/Comparable", Some("java/lang/Object"), &[], true);
    // The class access follows this fixture's four constant-pool entries.
    let access = 10 + 3 + "java/lang/Comparable".len() + 3 + 3 + "java/lang/Object".len() + 3;
    comparable[access..access + 2].copy_from_slice(&0x601u16.to_be_bytes());
    p.add_class_bytes("Comparable.class", &comparable).unwrap();
    refused(&p, r#"m("x")"#, &[("Calls.java", "m(String x)"), ("Calls.java", "m(Object x)")]);
    p.insert("Calls.java", r#"class C { public void m(String x) {} } class Calls { void run(C c) { c.m("x"); } }"#);
    selected(&p, r#"m("x")"#, "Calls.java", "m(String x)");
}

#[test]
fn string_literals_preserve_arity_and_declaration_site_imports() {
    let source = r#"class C { public void m(String x) {} public void m(String x, String y) {} } class Calls { void run(C c) { c.m("x", ("😀")); c.m("x"); c.m(); } }"#;
    let p = literal_project(source);
    selected(&p, r#"m("x", ("😀"))"#, "Calls.java", "m(String x, String y)");
    selected(&p, r#"m("x")"#, "Calls.java", "m(String x)");
    refused(&p, "m();", &[("Calls.java", "m(String x)"), ("Calls.java", "m(String x, String y)")]);
    let mut p = literal_project(
        r#"package app; import other.String; import api.C; class Calls { void run(C c) { c.m("x"); } }"#,
    );
    p.insert("Other.java", "package other; public class String {}");
    p.insert("C.java", "package api; public class C { public void m(String x) {} }");
    selected(&p, r#"m("x")"#, "C.java", "m(String x)");
    p.insert("C.java", "package api; import other.String; public class C { public void m(String x) {} }");
    refused(&p, r#"m("x")"#, &[("C.java", "m(String x)")]);
}

#[test]
fn string_literals_retain_inherited_and_incomparable_candidates() {
    let source = r#"class Base { public void m(Object x) {} } class C extends Base { public void m(String x) {} } class Calls { void run(C c) { c.m("x"); } }"#;
    refused(&literal_project(source), r#"m("x")"#, &[("Calls.java", "m(String x)"), ("Calls.java", "m(Object x)")]);
    let source = r#"interface A {} interface B {} class C { public void m(A x) {} public void m(B x) {} } class Calls { void run(C c) { c.m("x"); } }"#;
    let mut p = project(source);
    p.add_class_bytes("String.class", &literal_string_binary(0x31, None, &["A", "B"])).unwrap();
    refused(&p, r#"m("x")"#, &[("Calls.java", "m(A x)"), ("Calls.java", "m(B x)")]);
}

#[test]
fn string_literals_reject_generic_platform_and_malformed_syntax() {
    let source = r#"class C { public void m(Object x) {} } class Calls { void run(C c) { c.m("x"); } }"#;
    let mut p = project(source);
    p.add_class_bytes(
        "String.class",
        &literal_string_binary(0x31, Some("<T:Ljava/lang/Object;>Ljava/lang/Object;"), &[]),
    )
    .unwrap();
    refused(&p, r#"m("x")"#, &[("Calls.java", "m(Object x)")]);
    for argument in ["\"unterminated", "\"dangling\\", "\"a\nb\"", "\"a\rb\""] {
        let source = format!(
            "class C {{ public void m(String x) {{}} }} class Calls {{ void run(C c) {{ c.m({argument}); }} }}"
        );
        assert_ne!(
            query(&literal_project(&source), "Calls.java", &format!("m({argument}")).status,
            ResolveStatus::Resolved
        );
    }
}

#[test]
fn string_literal_identity_across_compilations_requires_matching_binary_closure() {
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
        "literal-scopes-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    )));
    for directory in ["lib", "app", "platform/java/lang", "other/java/lang"] {
        fs::create_dir_all(fixture.0.join(directory)).unwrap();
    }
    let callee = "package api; public class C { public void m(String a) {} public void mixed(int i, String a) {} }";
    let caller = r#"package app; import api.C; public class Calls { void run(C c, String s) { c.m("😀"); c.m(s); c.mixed(1,"😀"); c.mixed(1,s); } }"#;
    fs::write(fixture.0.join("lib/C.java"), callee).unwrap();
    fs::write(fixture.0.join("app/Calls.java"), caller).unwrap();
    let string = literal_string_binary(0x31, Some("Ljava/lang/Object;"), &[]);
    for platform in ["platform", "other"] {
        fs::write(fixture.0.join(platform).join("java/lang/String.class"), &string).unwrap();
        fs::write(fixture.0.join(platform).join("java/lang/Object.class"), binary("java/lang/Object", None, &[], true))
            .unwrap();
    }
    let lib = CompilationId::new("lib", "main");
    let app = CompilationId::new("app", "main");
    let inputs = |source: &str, platform: &str| ProjectConfig {
        source_roots: vec![fixture.0.join(source)],
        classpath: vec![ClasspathEntry::Directory(fixture.0.join(platform))],
        ..Default::default()
    };
    let path = fixture.0.join("app/Calls.java").canonicalize().unwrap().to_str().unwrap().to_owned();
    for (platform, expected) in [("platform", ResolveStatus::Resolved), ("other", ResolveStatus::CallNotResolved)] {
        let snapshot = WorkspaceSnapshot::load(&WorkspaceConfig {
            compilations: vec![
                CompilationConfig { id: lib.clone(), dependencies: vec![], inputs: inputs("lib", "platform") },
                CompilationConfig { id: app.clone(), dependencies: vec![lib.clone()], inputs: inputs("app", platform) },
            ],
            ..Default::default()
        })
        .unwrap();
        for (needle, expected) in [
            (r#"m("😀")"#, expected),
            // Exact ordinary binary roots now share the validated closure bridge.
            ("m(s)", expected),
            (r#"mixed(1,"😀")"#, expected),
            ("mixed(1,s)", expected),
        ] {
            let at = caller.find(needle).unwrap() as u32;
            let result = snapshot.scope(&app).unwrap().resolve_at(&path, at);
            assert_eq!(result.status, expected, "{platform}: {result:?}");
            assert_eq!(result, snapshot.prepare().scope(&app).unwrap().resolve_at(&path, at));
            assert_eq!(result.targets.len(), 1);
            assert!(result.targets[0].path.ends_with("lib/C.java"));
        }
    }
    // The exact same String artifact is present in both scopes, but app's
    // Object is selected from a different dependency. Root-name equality is
    // insufficient even when that dependency has byte-identical contents.
    fs::remove_file(fixture.0.join("other/java/lang/String.class")).unwrap();
    let check = |app_inputs: ProjectConfig| {
        let snapshot = WorkspaceSnapshot::load(&WorkspaceConfig {
            compilations: vec![
                CompilationConfig { id: lib.clone(), dependencies: vec![], inputs: inputs("lib", "platform") },
                CompilationConfig { id: app.clone(), dependencies: vec![lib.clone()], inputs: app_inputs },
            ],
            ..Default::default()
        })
        .unwrap();
        let at = caller.find(r#"m("😀")"#).unwrap() as u32;
        let result = snapshot.scope(&app).unwrap().resolve_at(&path, at);
        assert_ne!(result.status, ResolveStatus::Resolved, "{result:?}");
        assert_eq!(result, snapshot.prepare().scope(&app).unwrap().resolve_at(&path, at));
    };
    let mut different_ancestor = inputs("app", "other");
    different_ancestor.classpath.push(ClasspathEntry::Directory(fixture.0.join("platform")));
    check(different_ancestor);
    // A source duplicate in either owning compilation cannot certify the
    // platform identity, even though both scopes still have identical binaries.
    for source in ["app", "lib"] {
        let shadow = fixture.0.join(source).join("String.java");
        fs::write(&shadow, "package java.lang; public final class String {}").unwrap();
        check(inputs("app", "platform"));
        fs::remove_file(shadow).unwrap();
    }
    // Oversized signature metadata must not be compared for free in the
    // separately loaded directory fallback (shared archive Arcs avoid the scan).
    fs::write(
        fixture.0.join("platform/java/lang/String.class"),
        literal_string_binary(0x31, Some(&format!("Ljava/lang/Object;{}", "x".repeat(5000))), &[]),
    )
    .unwrap();
    check(inputs("app", "platform"));
}

#[test]
fn strict_reference_winner_precedes_scalar_primitive_competitors() {
    for methods in [
        "public void m(Parent x) {} public void m(int x) {}",
        "public void m(int x) {} public void m(Parent x) {}",
        "public void m(Object x) {} public void m(int x) {} public void m(Parent x) {}",
        "public void m(Parent x) {} public void m(Object x) {} public void m(int x) {}",
        "public void m(int x) {} public void m(Parent x) {} public void m(Object x) {}",
        "public void m(Parent x) {} public void m(int x) {} public void m(long x) {} public void m(boolean x) {}",
    ] {
        let source = format!(
            "class Parent {{}} class Child extends Parent {{}} class C {{ {methods} }} class Calls {{ void run(C c, Child a) {{ c.m(a); }} }}"
        );
        selected(&project(&source), "m(a)", "Calls.java", "m(Parent x)");
    }
    let source = "class C { public void m(Object x) {} public void m(int x) {} } class Calls { void run(C c, Integer a) { c.m(a); } }";
    let mut p = project(source);
    // Signature-only nongeneric hierarchy tests phase ordering, not real JDK Integer support.
    p.add_class_bytes("Integer.class", &binary("java/lang/Integer", Some("java/lang/Object"), &[], true)).unwrap();
    selected(&p, "m(a)", "Calls.java", "m(Object x)");
}

#[test]
fn strict_reference_mixed_vectors_preserve_primitive_identity() {
    let source = "class A {} class B extends A {} class C { public void m(A x, A y) {} public void m(int x, A y) {} public void m(A x, int y) {} public void m(long x, A y) {} } class Calls { void run(C c, B a) { c.m(a, a); } }";
    selected(&project(source), "m(a, a)", "Calls.java", "m(A x, A y)");
    for duplicate in ["m(int x, A y)", "m(A x, int y)"] {
        let source = format!(
            "class A {{}} class C {{ public void m(A x, A y) {{}} public void {duplicate} {{}} public void {duplicate} {{}} }} class Calls {{ void run(C c, A a) {{ c.m(a, a); }} }}"
        );
        let result = query(&project(&source), "Calls.java", "m(a, a)");
        assert_ne!(result.status, ResolveStatus::Resolved);
        assert_eq!(result.targets.len(), 3);
    }
}

#[test]
fn strict_reference_no_winner_keeps_complete_candidates() {
    for methods in [
        vec!["m(int x)"],
        vec!["m(int x)", "m(long x)"],
        vec!["m(Other x)", "m(int x)"],
        vec!["m(Left x)", "m(Right x)", "m(int x)"],
    ] {
        let declarations = methods.iter().map(|m| format!("public void {m} {{}} ")).collect::<String>();
        let source = format!(
            "interface Left {{}} interface Right {{}} class A implements Left, Right {{}} class Other {{}} class C {{ {declarations} }} class Calls {{ void run(C c, A a) {{ c.m(a); }} }}"
        );
        let expected = methods.iter().map(|m| ("Calls.java", *m)).collect::<Vec<_>>();
        refused(&project(&source), "m(a)", &expected);
    }
}

#[test]
fn strict_reference_does_not_discard_unsupported_primitive_competitors() {
    for competitor in [
        "public static void m(int x) {}",
        "private void m(int x) {}",
        "public <T> void m(int x) {}",
        "public void m(int... x) {}",
        "public void m(int[] x) {}",
        "public void m(int x[]) {}",
        "public void m(A[] x) {}",
        "public void m(int x) throws int {}",
        "public void m(int x);",
        "public public void m(int x) {}",
        "public void m(int x) {} public void m(int y) {}",
        "public void m(A x) {}",
        "public void m(int x, Missing y) {}",
        "public void m(int x, Hidden<A> y) {}",
    ] {
        let source = format!(
            "class A {{}} class Hidden<T> {{}} class C {{ public void m(A x) {{}} {competitor} }} class Calls {{ void run(C c, A a) {{ c.m(a); }} }}"
        );
        let result = query(&project(&source), "Calls.java", "m(a)");
        assert_ne!(result.status, ResolveStatus::Resolved, "{competitor}: {result:?}");
        assert!(result.targets.len() >= 2, "{competitor}: {result:?}");
    }
}

#[test]
fn strict_reference_competitors_preserve_hierarchy_and_argument_guards() {
    for ancestry in ["extends Missing", "extends Base"] {
        let source = format!(
            "class A {{}} class Base {{ public void m(int x) {{}} }} class C {ancestry} {{ public void m(A x) {{}} public void m(long x) {{}} }} class Calls {{ void run(C c, A a) {{ c.m(a); }} }}"
        );
        let result = query(&project(&source), "Calls.java", "m(a)");
        assert_ne!(result.status, ResolveStatus::Resolved);
        assert_eq!(result.targets.len(), if ancestry == "extends Base" { 3 } else { 2 });
    }
    let source = "class A {} class C { public void m(A x) {} public void m(int x) {} } class Calls { void run(C c, A a) { c.m(a); } }";
    let mut p = project(source);
    p.remove_class("Object.class");
    refused(&p, "m(a)", &[("Calls.java", "m(A x)"), ("Calls.java", "m(int x)")]);
    p = project(source);
    p.insert("Duplicate.java", "class A {}");
    refused(&p, "m(a)", &[("Calls.java", "m(A x)"), ("Calls.java", "m(int x)")]);
    for expression in [
        "(A)(A)null".to_owned(),
        "give()".to_owned(),
        "1L".to_owned(),
        format!("{}a{}", "(".repeat(40), ")".repeat(40)),
    ] {
        let text = source.replace("c.m(a)", &format!("c.m({expression})"));
        refused(&project(&text), &format!("m({expression})"), &[("Calls.java", "m(A x)"), ("Calls.java", "m(int x)")]);
    }
    // Plain null and a single reference cast of null are strict actuals; nested casts stay unsupported.
    selected(&project(&source.replace("c.m(a)", "c.m((A)null)")), "m((A)null)", "Calls.java", "m(A x)");
    selected(&project(&source.replace("c.m(a)", "c.m(null)")), "m(null)", "Calls.java", "m(A x)");
    // The additive scalar-primitive slice now proves a decimal int argument.
    selected(&project(&source.replace("c.m(a)", "c.m(1)")), "m(1)", "Calls.java", "m(int x)");
    // Formal evidence must be complete even for a primitive-inapplicable vector.
    for extra in ["class B extends Missing {}", "class B {} class B {}"] {
        let source = format!(
            "class A {{}} {extra} class C {{ public void m(A x, A y) {{}} public void m(int x, B y) {{}} }} class Calls {{ void run(C c, A a) {{ c.m(a, a); }} }}"
        );
        refused(&project(&source), "m(a, a)", &[("Calls.java", "m(A x, A y)"), ("Calls.java", "m(int x, B y)")]);
    }
}

#[test]
fn strict_reference_refresh_preserves_exact_target_ranges_and_access() {
    let call = "class A {} class Calls { void run(C c, A a) { c.m(a); } }";
    let mut p = project(call);
    for prefix in ["", "// 😀\r\n"] {
        let callee = format!("{prefix}class C {{ public void m(A x) {{}} public void m(int x) {{}} }}");
        p.insert("C.java", &callee);
        selected(&p, "m(a)", "C.java", "m(A x)");
        p.insert("C.java", &callee.replace("public void m(int", "private void m(int"));
        refused(&p, "m(a)", &[("C.java", "m(A x)"), ("C.java", "m(int x)")]);
        p.insert("C.java", &callee);
        selected(&p, "m(a)", "C.java", "m(A x)");
    }
}
