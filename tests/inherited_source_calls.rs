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

fn refused(source: &str, call: &str, candidates: usize) {
    let result = query(&project(source), call);
    assert_ne!(result.status, ResolveStatus::Resolved, "{call}: {result:?}");
    assert_eq!(result.targets.len(), candidates, "{call}: {result:?}");
}

#[test]
fn unique_inherited_method_uses_parent_target() {
    let s = "class Parent { public void pick(int x) {} } class Child extends Parent {} class Calls { void run(Child c) { c.pick(1); } }";
    winner(s, "pick(1)", "pick(int x)");
    assert_eq!(query(&project(s), "pick(1)").targets[0].owner, "Parent");
}

#[test]
fn inherited_different_arity_and_type_overloads_use_strict_selection() {
    for (parent, child, call, expected) in [
        ("public void pick() {}", "public void pick(int x) {}", "pick()", "pick()"),
        ("public void pick(int x) {}", "public void pick(long x) {}", "pick(1)", "pick(int x)"),
        ("public void pick(long x) {}", "public void pick(int x) {}", "pick(1)", "pick(int x)"),
    ] {
        let s = format!(
            "class Parent {{ {parent} }} class Child extends Parent {{ {child} }} class Calls {{ void run(Child c) {{ c.{call}; }} }}"
        );
        winner(&s, call, expected);
    }
}

#[test]
fn inherited_formals_and_return_use_declaring_imports() {
    let mut p = project(
        "package app; import api.Child; import model.Value; class Calls { void run(Child c, Value v) { c.pick(v).next(); } }",
    );
    p.insert("Value.java", "package model; public class Value { public void next() {} }");
    p.insert(
        "Parent.java",
        "package base; import model.Value; public class Parent { public Value pick(Value x) { return x; } }",
    );
    p.insert("Child.java", "package api; public class Child extends base.Parent {}");
    let inner = query(&p, "pick(v)");
    assert_eq!(inner.status, ResolveStatus::Resolved);
    assert_eq!(inner.targets[0].path, "Parent.java");
    assert_eq!(query(&p, "next();").status, ResolveStatus::Resolved);
}

#[test]
fn inherited_same_signature_and_unknown_competitors_stay_refused() {
    for other in [
        "public void pick(int x) {}",
        "private void pick(long x) {}",
        "public static void pick(long x) {}",
        "public void pick(Missing x, int y) {}",
        "public <T> void pick(T x) {}",
        "public void pick(int... x) {}",
    ] {
        let s = format!(
            "class Parent {{ public void pick(int x) {{}} }} class Child extends Parent {{ {other} }} class Calls {{ void run(Child c) {{ c.pick(1); }} }}"
        );
        refused(&s, "pick(1)", 2);
    }
}

#[test]
fn inherited_generic_and_interface_paths_stay_refused() {
    for defs in [
        "class Parent<T> { public void pick(int x) {} } class Child extends Parent<String> {}",
        "interface Parent { void pick(int x); } class Child implements Parent {}",
    ] {
        let s = format!("{defs} class Calls {{ void run(Child c) {{ c.pick(1); }} }}");
        assert_ne!(query(&project(&s), "pick(1)").status, ResolveStatus::Resolved);
    }
}

#[test]
fn parent_method_update_does_not_change_old_project_snapshot() {
    let s = "class Calls { void run(Child c) { c.pick(1); } }";
    let mut p = project(s);
    p.insert("Parent.java", "class Parent { public void pick(int x) {} }");
    p.insert("Child.java", "class Child extends Parent {}");
    let old = p.clone();
    assert_eq!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    p.insert("Parent.java", "class Parent { public void pick(boolean x) {} }");
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    p.insert("Parent.java", "class Parent {}");
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    assert_eq!(query(&old, "pick(1)").status, ResolveStatus::Resolved);
}

#[test]
fn inherited_chain_has_shared_depth_limit() {
    for count in [3, 40] {
        let mut s = "class Base { public void pick(int x) {} }".to_owned();
        let mut parent = "Base".to_owned();
        for i in 0..count {
            s += &format!(" class C{i} extends {parent} {{}}");
            parent = format!("C{i}");
        }
        s += &format!(" class Calls {{ void run({parent} c) {{ c.pick(1); }} }}");
        let result = query(&project(&s), "pick(1)");
        if count == 3 {
            assert_eq!(result.status, ResolveStatus::Resolved);
        } else {
            assert_ne!(result.status, ResolveStatus::Resolved);
        }
    }
}

#[test]
fn crossed_inherited_vectors_need_one_dominating_candidate() {
    let base = "class A {} class B extends A {} class Parent { public void pick(A a, B b) {} } class Child extends Parent { public void pick(B a, A b) {} EXTRA } class Calls { void run(Child c, B b) { c.pick(b,b); } }";
    refused(&base.replace("EXTRA", ""), "pick(b,b)", 2);
    winner(&base.replace("EXTRA", "public void pick(B a, B b) {}"), "pick(b,b)", "pick(B a, B b)");
}

#[test]
fn same_simple_formal_names_use_each_declaring_import() {
    let mut p = project(
        "package app; import api.Child; import p.Token; class Calls { void run(Child c, Token x) { c.pick(x); } }",
    );
    p.insert("P.java", "package p; public class Token {}");
    p.insert("Q.java", "package q; public class Token {}");
    p.insert("Parent.java", "package base; import p.Token; public class Parent { public void pick(Token x) {} }");
    p.insert(
        "Child.java",
        "package api; import q.Token; public class Child extends base.Parent { public void pick(Token x) {} }",
    );
    let result = query(&p, "pick(x)");
    assert_eq!(result.status, ResolveStatus::Resolved);
    assert_eq!(result.targets[0].path, "Parent.java");
}

#[test]
fn unknown_or_inaccessible_parent_candidates_are_not_filtered() {
    for method in
        ["private void pick(long x) {}", "public static void pick(long x) {}", "public void pick(Missing x, int y) {}"]
    {
        let s = format!(
            "class Parent {{ {method} }} class Child extends Parent {{ public void pick(int x) {{}} }} class Calls {{ void run(Child c) {{ c.pick(1); }} }}"
        );
        refused(&s, "pick(1)", 2);
    }
}

#[test]
fn outer_return_receiver_navigation_preserves_inherited_declaring_owner() {
    let s = "class Parent { public void pick(int x) {} } class Child extends Parent {} class Factory { public Child make() { return null; } } class Calls { void run(Factory f) { f.make().pick(1); } }";
    winner(s, "pick(1)", "pick(int x)");
    assert_eq!(query(&project(s), "pick(1)").targets[0].owner, "Parent");
}

#[test]
fn noncontributing_generic_interfaces_do_not_replace_the_source_parent_chain() {
    let s = "interface Marker<T> {} class Parent { public void pick(int x) {} } class Child extends Parent implements Marker<String> {} class Calls { void run(Child c) { c.pick(1); } }";
    winner(s, "pick(1)", "pick(int x)");
}

#[test]
fn transitive_interface_name_or_incomplete_graph_remains_a_barrier() {
    for interfaces in [
        "interface Root<T> { void pick(int x); } interface Marker<T> extends Root<T> {}",
        "interface Marker<T> extends Missing<T> {}",
        "interface Marker<T> extends Marker<T> {}",
    ] {
        let s = format!(
            "{interfaces} class Parent {{ public void pick(int x) {{}} }} class Child extends Parent implements Marker<String> {{}} class Calls {{ void run(Child c) {{ c.pick(1); }} }}"
        );
        assert_ne!(query(&project(&s), "pick(1)").status, ResolveStatus::Resolved);
    }
}
