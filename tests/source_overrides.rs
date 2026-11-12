use java_engine::{Project, ResolveResult, ResolveStatus, TargetKind};

fn project(source: &str) -> Project {
    let p = project_with_errors(source);
    assert!(p.file("Calls.java").unwrap().errors().is_empty());
    p
}

fn project_with_errors(source: &str) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", source);
    let mut object = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61, 0, 3, 1, 0, 16];
    object.extend(b"java/lang/Object");
    object.extend([7, 0, 1, 0, 0x21, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    p.add_class_bytes("Object.class", &object).unwrap();
    p
}

fn query(p: &Project, needle: &str) -> ResolveResult {
    let at = p.file("Calls.java").unwrap().text().rfind(needle).unwrap() as u32;
    let result = p.resolve_at("Calls.java", at);
    assert_eq!(result, p.prepare().resolve_at("Calls.java", at));
    result
}

fn winner(source: &str, call: &str, declaration: &str) {
    let result = query(&project(source), call);
    assert_eq!(result.status, ResolveStatus::Resolved, "{call}: {result:?}");
    assert_eq!(result.targets.len(), 1);
    let target = &result.targets[0];
    let at = source.find(declaration).unwrap() as u32;
    assert_eq!(target.kind, TargetKind::Method);
    assert_eq!(target.path, "Calls.java");
    assert_eq!((target.start_byte, target.end_byte), (at, at + 4));
}

#[test]
fn source_override_uses_declaring_subclass_not_abstract_parent() {
    let s = "abstract class Parent { public abstract Parent pick(); } class Child extends Parent { @Override public Child pick() { return this; } } class Calls { void run(Child c) { c.pick(); } }";
    winner(s, "pick();", "pick() { return this;");
    assert_eq!(query(&project(s), "pick();").targets[0].owner, "Child");
}

#[test]
fn void_and_identical_primitive_returns_allow_source_override() {
    for result in ["void", "int"] {
        let body = if result == "void" { "" } else { "return 1;" };
        let s = format!(
            "class Parent {{ public {result} pick(int x) {{ {body} }} }} class Child extends Parent {{ public {result} pick(int x) {{ {body} }} }} class Calls {{ void run(Child c) {{ c.pick(1); }} }}"
        );
        let r = query(&project(&s), "pick(1)");
        assert_eq!(r.status, ResolveStatus::Resolved);
        assert_eq!(r.targets[0].owner, "Child");
    }
}

#[test]
fn all_override_pairs_are_checked_before_hiding_ancestors() {
    for parent in [
        "public final void pick(int x) {}",
        "public int pick(int x) { return 1; }",
        "public void pick(int x) throws Missing {}",
    ] {
        let s = format!(
            "class Parent {{ {parent} }} class Middle extends Parent {{ public void pick(int x) {{}} }} class Child extends Middle {{ public void pick(int x) {{}} }} class Calls {{ void run(Child c) {{ c.pick(1); }} }}"
        );
        assert_ne!(query(&project(&s), "pick(1)").status, ResolveStatus::Resolved);
    }
}

#[test]
fn duplicate_owner_static_private_generic_and_array_do_not_override() {
    for method in [
        "public static void pick(int x) {}",
        "private void pick(int x) {}",
        "public <T> void pick(int x) {}",
        "public void pick(int x) {} public void pick(int x) {}",
        "public int[] pick(int x) { return null; }",
    ] {
        let s = format!(
            "class Parent {{ public void pick(int x) {{}} }} class Child extends Parent {{ {method} }} class Calls {{ void run(Child c) {{ c.pick(1); }} }}"
        );
        assert_ne!(query(&project(&s), "pick(1)").status, ResolveStatus::Resolved);
    }
}

#[test]
fn annotations_are_not_override_evidence_and_do_not_enable_return_propagation() {
    let s = "abstract class Parent { public abstract Parent pick(); public void next() {} } class Child extends Parent { @Other public Child pick() { return this; } } class Calls { void run(Child c) { c.pick().next(); } }";
    assert_eq!(query(&project(s), "pick().").status, ResolveStatus::Resolved);
    assert_ne!(query(&project(s), "next();").status, ResolveStatus::Resolved);
}

#[test]
fn inherited_overload_and_override_coexist_without_nearest_depth_scoring() {
    let s = "class A {} class B extends A {} class Parent { public void pick(A a) {} public void pick(B b) {} } class Child extends Parent { public void pick(A a) {} } class Calls { void run(Child c, B b) { c.pick(b); } }";
    winner(s, "pick(b)", "pick(B b)");
    assert_eq!(query(&project(s), "pick(b)").targets[0].owner, "Parent");
}

#[test]
fn abstract_intermediate_and_final_leaf_preserve_static_receiver_selection() {
    let s = "class Parent { public Parent pick() { return this; } } abstract class Middle extends Parent { public abstract Middle pick(); } final class Child extends Middle { public final Child pick() { return this; } } class Calls { void run(Parent p, Middle m, Child c) { p.pick(); m.pick(); c.pick(); } }";
    let p = project(s);
    for (call, owner) in [("p.pick", "Parent"), ("m.pick", "Middle"), ("c.pick", "Child")] {
        let at = s.rfind(call).unwrap() as u32 + 2;
        let result = p.resolve_at("Calls.java", at);
        assert_eq!(result.status, ResolveStatus::Resolved);
        assert_eq!(result.targets[0].owner, owner);
        assert_eq!(result, p.prepare().resolve_at("Calls.java", at));
    }
}

#[test]
fn return_widening_native_throws_and_malformed_annotations_cannot_hide_parent() {
    for method in [
        "public long pick() { return 1; }",
        "public native int pick();",
        "public int pick() throws Error { return 1; }",
        "public abstract int pick() { return 1; }",
        "@Bad( public int pick() { return 1; }",
    ] {
        let s = format!(
            "class Error {{}} class Parent {{ public int pick() {{ return 1; }} }} abstract class Child extends Parent {{ {method} }} class Calls {{ void run(Child c) {{ c.pick(); }} }}"
        );
        assert_ne!(query(&project_with_errors(&s), "pick();").status, ResolveStatus::Resolved);
    }
    let s = "class A {} class B {} class Parent { public A pick() { return null; } } class Child extends Parent { public B pick() { return null; } } class Calls { void run(Child c) { c.pick(); } }";
    assert_ne!(query(&project(s), "pick();").status, ResolveStatus::Resolved);
}

#[test]
fn overridden_return_names_use_each_declaring_import_and_refresh_snapshots() {
    let mut p = project("package app; class Calls { void run(api.Child c) { c.pick(); } }");
    p.insert("A.java", "package model; public class A {}");
    p.insert("B.java", "package model; public class B extends A {}");
    p.insert("Other.java", "package other; public class A {}");
    p.insert("Parent.java", "package base; import model.A; public class Parent { public A pick() { return null; } }");
    p.insert(
        "Child.java",
        "package api; import model.B; public class Child extends base.Parent { public B pick() { return null; } }",
    );
    let old = p.clone();
    let result = query(&p, "pick();");
    assert_eq!(result.status, ResolveStatus::Resolved);
    assert_eq!(result.targets[0].path, "Child.java");
    p.insert("Parent.java", "package base; import other.A; public class Parent { public A pick() { return null; } }");
    assert_ne!(query(&p, "pick();").status, ResolveStatus::Resolved);
    assert_eq!(query(&old, "pick();").status, ResolveStatus::Resolved);
}

#[test]
fn malformed_annotation_arguments_cannot_certify_override() {
    for annotation in [r#"@SuppressWarnings("x",)"#, "@A(x=1,2)", "@A(,)", "@A(x=)", "@A(1)"] {
        let s = format!(
            "class Parent {{ public void pick() {{}} }} class Child extends Parent {{ {annotation} public void pick() {{}} }} class Calls {{ void run(Child c) {{ c.pick(); }} }}"
        );
        assert_ne!(query(&project_with_errors(&s), "pick();").status, ResolveStatus::Resolved, "{annotation}");
    }
}

#[test]
fn marker_and_empty_qualified_annotations_are_structural_only() {
    for annotation in ["@A", "@A()", "@pkg.A", "@pkg.A()"] {
        let s = format!(
            "class Parent {{ public void pick() {{}} }} class Child extends Parent {{ {annotation} public void pick() {{}} }} class Calls {{ void run(Child c) {{ c.pick(); }} }}"
        );
        assert_eq!(query(&project(&s), "pick();").status, ResolveStatus::Resolved, "{annotation}");
    }
}
#[test]
fn interface_and_generic_path_are_not_covariance_evidence() {
    for types in [
        "interface A {} class B implements A {}",
        "class A {} class Mid<T> extends A {} class B extends Mid<String> {}",
    ] {
        let s = format!(
            "{types} class Parent {{ public A pick() {{ return null; }} }} class Child extends Parent {{ public B pick() {{ return null; }} }} class Calls {{ void run(Child c) {{ c.pick(); }} }}"
        );
        assert_ne!(query(&project(&s), "pick();").status, ResolveStatus::Resolved);
    }
}
