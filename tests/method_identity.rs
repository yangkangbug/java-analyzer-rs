use java_engine::{Project, ResolveResult, ResolveStatus};

// Signature-only JVMS bytes. No compiler or Java executable is needed.
fn class(name: &str, parent: Option<&str>, methods: &[(&str, &str)]) -> Vec<u8> {
    class_metadata(name, parent, 0x21, methods, None)
}
fn class_metadata(
    name: &str,
    parent: Option<&str>,
    flags: u16,
    methods: &[(&str, &str)],
    signature: Option<&str>,
) -> Vec<u8> {
    fn u(b: &mut Vec<u8>, value: u16) {
        b.extend(value.to_be_bytes());
    }
    fn utf(b: &mut Vec<u8>, value: &str) {
        b.push(1);
        u(b, value.len() as u16);
        b.extend(value.as_bytes());
    }
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61];
    u(&mut b, 5 + 2 * methods.len() as u16 + if signature.is_some() { 2 } else { 0 });
    utf(&mut b, name);
    b.push(7);
    u(&mut b, 1);
    utf(&mut b, parent.unwrap_or("java/lang/Object"));
    b.push(7);
    u(&mut b, 3);
    for (name, descriptor) in methods {
        utf(&mut b, name);
        utf(&mut b, descriptor);
    }
    if let Some(signature) = signature {
        utf(&mut b, "Signature");
        utf(&mut b, signature);
    }
    u(&mut b, flags);
    u(&mut b, 2);
    u(&mut b, if parent.is_some() { 4 } else { 0 });
    u(&mut b, 0);
    u(&mut b, 0);
    u(&mut b, methods.len() as u16);
    for i in 0..methods.len() {
        u(&mut b, if flags & 0x200 != 0 { 0x401 } else { 0x101 });
        u(&mut b, 5 + 2 * i as u16);
        u(&mut b, 6 + 2 * i as u16);
        if i == 0 && signature.is_some() {
            u(&mut b, 1);
            u(&mut b, 5 + 2 * methods.len() as u16);
            b.extend(2u32.to_be_bytes());
            u(&mut b, 6 + 2 * methods.len() as u16);
        } else {
            u(&mut b, 0);
        }
    }
    u(&mut b, 0);
    b
}
fn binary(p: &mut Project, name: &str, parent: Option<&str>, methods: &[(&str, &str)]) {
    p.add_class_bytes(format!("{name}.class"), &class(name, parent, methods)).unwrap();
}
fn query(p: &Project, path: &str) -> ResolveResult {
    let at = p.file(path).unwrap().text().rfind("m(null)").unwrap() as u32;
    let rebuilt = p.resolve_at(path, at);
    assert_eq!(rebuilt, p.prepare().resolve_at(path, at));
    rebuilt
}
fn expect(p: &Project, path: &str, status: ResolveStatus, owners: &[&str]) {
    let r = query(p, path);
    assert_eq!(r.status, status, "{r:?}");
    let mut actual: Vec<_> = r.targets.iter().map(|t| t.owner.as_str()).collect();
    actual.sort();
    let mut expected = owners.to_vec();
    expected.sort();
    assert_eq!(actual, expected, "{r:?}");
}
#[test]
fn mixed_primitive_array_varargs_signatures_hide_only_the_matching_parent() {
    for (parameter, descriptor) in
        [("int i", "(I)V"), ("int[] i", "([I)V"), ("int... i", "([I)V"), ("int i[][]", "([[I)V"), ("", "()I")]
    {
        let mut p = Project::new();
        binary(&mut p, "Base", None, &[("m", descriptor)]);
        p.insert(
            "Child.java",
            &format!("class Child extends Base {{ public void m({parameter}) {{}} void test() {{ m(null); }} }}"),
        );
        expect(&p, "Child.java", ResolveStatus::Resolved, &["Child"]);
    }
}
#[test]
fn source_names_resolve_at_the_declaration_not_the_call_site() {
    let mut p = Project::new();
    p.insert("a/Item.java", "package a; public class Item {}");
    p.insert("b/Item.java", "package b; public class Item {}");
    p.insert("Base.java", "import a.Item; class Base { public void m(Item i) {} }");
    p.insert(
        "Child.java",
        "import b.Item; class Child extends Base { public void m(Item i) {} void test() { m(null); } }",
    );
    expect(&p, "Child.java", ResolveStatus::OverloadNotResolved, &["Base", "Child"]);
    p.insert(
        "Child.java",
        "import a.Item; class Child extends Base { public void m(a.Item i) {} void test() { m(null); } }",
    );
    expect(&p, "Child.java", ResolveStatus::Resolved, &["Child"]);
}
#[test]
fn mixed_declared_types_use_imports_arrays_and_nested_identity() {
    for (source, descriptor) in
        [("Item i", "(Lp/Item;)V"), ("p.Item[] i", "([Lp/Item;)V"), ("Outer.Inner i", "(Lp/Outer$Inner;)V")]
    {
        let mut p = Project::new();
        binary(&mut p, "p/Item", None, &[]);
        binary(&mut p, "p/Outer", None, &[]);
        binary(&mut p, "p/Outer$Inner", None, &[]);
        binary(&mut p, "Base", None, &[("m", descriptor)]);
        p.insert("Child.java", &format!("import p.Item; import p.Outer; class Child extends Base {{ public void m({source}) {{}} void test() {{ m(null); }} }}"));
        expect(&p, "Child.java", ResolveStatus::Resolved, &["Child"]);
    }
}
#[test]
fn missing_ambiguous_and_shadowing_type_parameters_are_not_spelling_identity() {
    for header in ["class Child extends Base", "class Child<Item> extends Base"] {
        let mut p = Project::new();
        p.insert("Base.java", "class Base { public void m(Item i) {} }");
        p.insert("Child.java", &format!("{header} {{ public void m(Item i) {{}} void test() {{ m(null); }} }}"));
        expect(&p, "Child.java", ResolveStatus::SignatureNotCompared, &["Base", "Child"]);
    }
    for method in ["public <Item> void m(Item i)", "public void m(Item<String> i)"] {
        let mut p = Project::new();
        p.insert("Item.java", "class Item<T> {}");
        p.insert("Base.java", "class Base { public void m(Item i) {} }");
        p.insert("Child.java", &format!("class Child extends Base {{ {method} {{}} void test() {{ m(null); }} }}"));
        expect(&p, "Child.java", ResolveStatus::SignatureNotCompared, &["Base", "Child"]);
    }
}
#[test]
fn generic_parameter_and_zero_arguments_are_provably_distinct() {
    let mut p = Project::new();
    binary(&mut p, "Base", None, &[("m", "()Ljava/lang/Object;")]);
    p.insert(
        "Child.java",
        "class Child extends Base { public <T> T m(T value) { return value; } void test() { m(null); } }",
    );
    expect(&p, "Child.java", ResolveStatus::OverloadNotResolved, &["Base", "Child"]);
}
#[test]
fn return_descriptors_do_not_create_overloads() {
    let mut p = Project::new();
    binary(&mut p, "Base", None, &[("m", "()Ljava/lang/Object;")]);
    binary(&mut p, "Child", Some("Base"), &[("m", "()Ljava/lang/String;")]);
    p.insert("Use.java", "class Use extends Child { void test() { m(null); } }");
    expect(&p, "Use.java", ResolveStatus::Resolved, &["Child"]);
}
#[test]
fn same_owner_covariant_or_bridge_duplicates_stay_explicit() {
    let mut p = Project::new();
    binary(&mut p, "Base", None, &[("m", "()Ljava/lang/Object;"), ("m", "()Ljava/lang/String;")]);
    p.insert("Use.java", "class Use extends Base { void test() { m(null); } }");
    expect(&p, "Use.java", ResolveStatus::Ambiguous, &["Base", "Base"]);
}

#[test]
fn malformed_descriptors_cannot_supply_method_identity() {
    for descriptor in ["(V)V", "([V)V", "()Vjunk", "(I)", "(L;)V", "(Lx//Y;)V", "()Q", "(I)V;", "(Lx.Y;)V"] {
        let mut p = Project::new();
        if p.add_class_bytes("Base.class", &class("Base", None, &[("m", descriptor)])).is_err() {
            continue;
        }
        p.insert("Use.java", "class Use extends Base { void test() { m(null); } }");
        expect(&p, "Use.java", ResolveStatus::SignatureNotCompared, &["Base"]);
    }
}
#[test]
fn unrelated_hierarchy_branches_do_not_hide_by_distance() {
    let mut p = Project::new();
    p.insert("Use.java", "interface A { void m(int x); } interface B { void m(int x); } interface C extends B {} abstract class Use implements A, C { void test() { m(null); } }");
    expect(&p, "Use.java", ResolveStatus::Ambiguous, &["A", "B"]);
}

#[test]
fn qualified_type_variable_must_not_be_class_identity() {
    let mut p = Project::new();
    p.insert("Outer.java", "class Outer { static class Inner {} }");
    p.insert("Base.java", "class Base { public void m(Outer.Inner x) {} }");
    p.insert(
        "Child.java",
        "class Child<Outer> extends Base { public void m(Outer.Inner x) {} void test() { m(null); } }",
    );
    expect(&p, "Child.java", ResolveStatus::SignatureNotCompared, &["Base", "Child"]);
}

#[test]
fn outer_type_parameter_shadow_is_conservative() {
    let mut p = Project::new();
    p.insert("T.java", "class T {}");
    p.insert("Base.java", "class Base { public void m(T x) {} }");
    p.insert(
        "Child.java",
        "class Outer<T> { class Child extends Base { public void m(T x) {} void test() { m(null); } } }",
    );
    expect(&p, "Child.java", ResolveStatus::SignatureNotCompared, &["Base", "Outer.Child"]);
}

#[test]
fn imported_nested_alias_is_identity() {
    let mut p = Project::new();
    p.insert("p/Outer.java", "package p; public class Outer { public static class Inner {} }");
    p.insert("Base.java", "class Base { public void m(p.Outer.Inner x) {} }");
    p.insert(
        "Child.java",
        "import p.Outer.Inner; class Child extends Base { public void m(Inner x) {} void test() { m(null); } }",
    );
    expect(&p, "Child.java", ResolveStatus::Resolved, &["Child"]);
}

#[test]
fn transitive_override_is_not_sensitive_to_interface_order() {
    for edges in ["A, B", "B, A"] {
        let mut p = Project::new();
        p.insert("Child.java",&format!("interface A {{ void m(int x); }} interface B extends A {{ void m(int x); }} abstract class Child implements {edges} {{ void test() {{ m(null); }} }}"));
        expect(&p, "Child.java", ResolveStatus::Resolved, &["B"]);
    }
}

#[test]
fn conflicting_single_imports_cannot_supply_identity() {
    let mut p = Project::new();
    p.insert("a/Item.java", "package a; public class Item {}");
    p.insert("b/Item.java", "package b; public class Item {}");
    p.insert("Base.java", "class Base { public void m(a.Item x) {} }");
    p.insert(
        "Child.java",
        "import a.Item; import b.Item; class Child extends Base { public void m(Item x) {} void test() { m(null); } }",
    );
    expect(&p, "Child.java", ResolveStatus::SignatureNotCompared, &["Base", "Child"]);
}

#[test]
fn stars_ambiguous_identity_stays_unknown() {
    let mut p = Project::new();
    p.insert("a/Item.java", "package a; public class Item {}");
    p.insert("b/Item.java", "package b; public class Item {}");
    p.insert("Base.java", "class Base { public void m(a.Item x) {} }");
    p.insert(
        "Child.java",
        "import a.*; import b.*; class Child extends Base { public void m(Item x) {} void test() { m(null); } }",
    );
    expect(&p, "Child.java", ResolveStatus::SignatureNotCompared, &["Base", "Child"]);
}

#[test]
fn lone_valid_unknown_name_is_navigable_not_applicability() {
    for parameter in ["Missing x", "Missing<Other> x", "T x"] {
        let mut p = Project::new();
        p.insert(
            "Child.java",
            &format!("class Child<T> {{ public void m({parameter}) {{}} void test() {{ m(null); }} }}"),
        );
        expect(&p, "Child.java", ResolveStatus::Resolved, &["Child"]);
    }
}

#[test]
fn lone_malformed_source_is_not_navigable() {
    for parameter in ["int[]", "int[ x", "int... x[]"] {
        let mut p = Project::new();
        p.insert(
            "Child.java",
            &format!("class Child {{ public void m({parameter}) {{}} void test() {{ m(null); }} }}"),
        );
        expect(&p, "Child.java", ResolveStatus::SignatureNotCompared, &["Child"]);
    }
}

#[test]
fn binary_primitive_looking_declared_name_is_not_primitive() {
    let mut p = Project::new();
    binary(&mut p, "int", None, &[]);
    binary(&mut p, "Base", None, &[("m", "(I)V")]);
    binary(&mut p, "Child", Some("Base"), &[("m", "(Lint;)V")]);
    p.insert("Use.java", "class Use extends Child { void test() { m(null); } }");
    expect(&p, "Use.java", ResolveStatus::OverloadNotResolved, &["Base", "Child"]);
}

#[test]
fn inherited_member_type_is_not_unrelated_package_type() {
    let mut p = Project::new();
    p.insert("Item.java", "class Item {}");
    p.insert("Base.java", "class Base { public static class Item {} public void m(Item x) {} }");
    p.insert("Child.java", "class Child extends Base { public void m(Item x) {} void test() { m(null); } }");
    let r = query(&p, "Child.java");
    assert!(
        r.status == ResolveStatus::SignatureNotCompared
            || (r.status == ResolveStatus::Resolved && r.targets[0].owner == "Child"),
        "{r:?}"
    );
}

#[test]
fn generic_zero_arg_duplicate_stays_uncompared() {
    let mut p = Project::new();
    p.insert("Base.java", "class Base { public void m() {} }");
    p.insert("Child.java", "class Child extends Base { public <T> void m() {} void test() { m(null); } }");
    expect(&p, "Child.java", ResolveStatus::SignatureNotCompared, &["Base", "Child"]);
}

#[test]
fn single_candidate_order_is_stable_across_insertion_order() {
    let files = [
        ("a/Item.java", "package a; public class Item {}"),
        ("b/Item.java", "package b; public class Item {}"),
        ("Base.java", "import a.Item; class Base { public void m(Item i) {} }"),
        ("Child.java", "import b.Item; class Child extends Base { public void m(Item i) {} void test() { m(null); } }"),
    ];
    let mut a = Project::new();
    let mut b = Project::new();
    for (p, s) in files {
        a.insert(p, s);
    }
    for (p, s) in files.into_iter().rev() {
        b.insert(p, s);
    }
    assert_eq!(query(&a, "Child.java"), query(&b, "Child.java"));
}

#[test]
fn interfaces_do_not_override_object_via_synthetic_superclass() {
    for source_interface in [false, true] {
        let mut p = Project::new();
        binary(&mut p, "java/lang/Object", None, &[("m", "()Ljava/lang/String;")]);
        if source_interface {
            p.insert("I.java", "interface I { public String m(); }");
        } else {
            p.add_class_bytes(
                "I.class",
                &class_metadata("I", Some("java/lang/Object"), 0x601, &[("m", "()Ljava/lang/String;")], None),
            )
            .unwrap();
            assert!(p.class_signature("I.class").unwrap().is_interface);
        }
        p.insert("Use.java", "class Use implements I { void test() { m(null); } }");
        expect(&p, "Use.java", ResolveStatus::Ambiguous, &["java.lang.Object", "I"]);
    }
    let mut p = Project::new();
    binary(&mut p, "java/lang/Object", None, &[("m", "()Ljava/lang/String;")]);
    binary(&mut p, "Child", Some("java/lang/Object"), &[("m", "()Ljava/lang/String;")]);
    assert!(!p.class_signature("Child.class").unwrap().is_interface);
    p.insert("Use.java", "class Use extends Child { void test() { m(null); } }");
    expect(&p, "Use.java", ResolveStatus::Resolved, &["Child"]);
}
#[test]
fn binary_generic_metadata_stays_unsupported_even_without_parameters() {
    for (descriptor, signature, parameter) in [
        ("(Ljava/lang/Object;)V", "<T:Ljava/lang/Object;>(TT;)V", "Object x"),
        ("()V", "<T:Ljava/lang/Object;>()V", ""),
    ] {
        let mut p = Project::new();
        binary(&mut p, "java/lang/Object", None, &[]);
        p.add_class_bytes("Base.class", &class_metadata("Base", None, 0x21, &[("m", descriptor)], Some(signature)))
            .unwrap();
        p.insert(
            "Use.java",
            &format!("class Use extends Base {{ public void m({parameter}) {{}} void test() {{ m(null); }} }}"),
        );
        expect(&p, "Use.java", ResolveStatus::SignatureNotCompared, &["Base", "Use"]);
    }
}
#[test]
fn diamonds_share_one_declaration_and_same_depth_overrides_use_subtyping() {
    for edges in ["Left, Right", "Right, Left"] {
        let mut p = Project::new();
        p.insert("Use.java", &format!("interface Base {{ void m(int x); }} interface Left extends Base {{}} interface Right extends Base {{}} abstract class Use implements {edges} {{ void test() {{ m(null); }} }}"));
        expect(&p, "Use.java", ResolveStatus::Resolved, &["Base"]);
    }
}

struct WorkspaceFixture(std::path::PathBuf);
impl WorkspaceFixture {
    fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "java-engine-method-identity-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) -> std::path::PathBuf {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for WorkspaceFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn workspace_parameter_identity_uses_declaration_compilation_and_provenance() {
    use java_engine::{
        ClasspathEntry, CompilationConfig, CompilationId, ProjectConfig, WorkspaceConfig, WorkspaceSnapshot,
    };
    // 0: the consumer cannot repair a missing dependency parameter type.
    // 1: two source declarations of the same FQN remain ambiguous in the consumer.
    // 2: even the same physical binary belongs to separate linking contexts.
    for mode in 0..3 {
        let fixture = WorkspaceFixture::new();
        fixture.write("lib/p/Base.java", "package p; public class Base { public void m(Item x) {} }");
        if mode < 2 {
            fixture.write("app/p/Item.java", "package p; public class Item {}");
        }
        if mode == 1 {
            fixture.write("lib/p/Item.java", "package p; public class Item {}");
        }
        if mode == 2 {
            fixture.write("classes/p/Item.class", class("p/Item", None, &[]));
        }
        let source = "package p; public class Child extends Base { public void m(Item x) {} void test() { m(null); } }";
        let path = fixture.write("app/p/Child.java", source).canonicalize().unwrap().to_str().unwrap().to_string();
        let lib = CompilationId::new("lib", "main");
        let app = CompilationId::new("app", "main");
        let inputs = |root: &str| ProjectConfig {
            source_roots: vec![fixture.0.join(root)],
            classpath: if mode == 2 { vec![ClasspathEntry::Directory(fixture.0.join("classes"))] } else { vec![] },
            ..Default::default()
        };
        let workspace = WorkspaceSnapshot::load(&WorkspaceConfig {
            compilations: vec![
                CompilationConfig { id: lib.clone(), inputs: inputs("lib"), dependencies: vec![] },
                CompilationConfig { id: app.clone(), inputs: inputs("app"), dependencies: vec![lib] },
            ],
            ..Default::default()
        })
        .unwrap();
        let at = source.rfind("m(null)").unwrap() as u32;
        let result = workspace.scope(&app).unwrap().resolve_at(&path, at);
        assert_eq!(result, workspace.prepare().scope(&app).unwrap().resolve_at(&path, at));
        let expected = if mode == 2 { ResolveStatus::OverloadNotResolved } else { ResolveStatus::SignatureNotCompared };
        assert_eq!(result.status, expected, "mode {mode}: {result:?}");
        assert_eq!(result.targets.len(), 2);
        assert!(result.targets.iter().any(|target| target.owner == "p.Base"));
        assert!(result.targets.iter().any(|target| target.owner == "p.Child"));
    }
}
