use java_engine::{Project, ResolveResult, ResolveStatus, TargetKind};

fn project(source: &str) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", source);
    assert!(p.file("Calls.java").unwrap().errors().is_empty());
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
fn null_selects_reference_specificity_and_never_primitive() {
    for methods in [
        "public void pick(A a) {} public void pick(B b) {} public void pick(int i) {}",
        "public void pick(int i) {} public void pick(B b) {} public void pick(A a) {}",
    ] {
        let source = format!(
            "class A {{}} class B extends A {{}} class Box {{ {methods} }} class Calls {{ void run(Box box) {{ box.pick((null)); }} }}"
        );
        winner(&source, "pick((null))", "pick(B b)");
    }
}

#[test]
fn null_combines_with_primitive_and_reference_slots() {
    for (methods, call, best) in [
        ("public void pick(int i, A a) {} public void pick(long i, B b) {}", "pick(0,null)", None),
        ("public void pick(int i, B b) {} public void pick(long i, A a) {}", "pick(0,null)", Some("pick(int i, B b)")),
        ("public void pick(B b, int i) {} public void pick(A a, long i) {}", "pick(null,0)", Some("pick(B b, int i)")),
        ("public void pick(A a, A b) {} public void pick(B a, B b) {}", "pick(null,value)", Some("pick(B a, B b)")),
    ] {
        let source = format!(
            "class A {{}} class B extends A {{}} class Box {{ {methods} }} class Calls {{ void run(Box box, B value) {{ box.{call}; }} }}"
        );
        if let Some(best) = best {
            winner(&source, call, best);
        } else {
            refused(&source, call, 2);
        }
    }
}

#[test]
fn null_keeps_unrelated_and_crossed_reference_vectors_ambiguous() {
    for methods in [
        "public void pick(A a) {} public void pick(B b) {}",
        "public void pick(A a, B b) {} public void pick(B a, A b) {}",
    ] {
        let call = if methods.contains(",") { "pick(null,null)" } else { "pick(null)" };
        let source = format!(
            "class A {{}} class B {{}} class Box {{ {methods} }} class Calls {{ void run(Box box) {{ box.{call}; }} }}"
        );
        refused(&source, call, 2);
    }
}

#[test]
fn null_does_not_make_unsupported_formals_or_competitors_disappear() {
    for other in [
        "public void pick(Missing x, int i) {}",
        "public <T> void pick(T x) {}",
        "private void pick(B b) {}",
        "public static void pick(B b) {}",
        "public void pick(B... b) {}",
        "public void pick(B[] b) {}",
    ] {
        let source = format!(
            "class A {{}} class B extends A {{}} class Box {{ public void pick(A a) {{}} {other} }} class Calls {{ void run(Box box) {{ box.pick(null); }} }}"
        );
        refused(&source, "pick(null)", 2);
    }
}

#[test]
fn null_never_matches_primitive_or_new_expression_shapes() {
    refused(
        "class Box { public void pick(int i) {} } class Calls { void run(Box box) { box.pick(null); } }",
        "pick(null)",
        1,
    );
    for arg in ["(A)(A)null", "true ? null : null", "give()", "nulls[0]", "this.value", "unknown"] {
        let source = format!(
            "class A {{}} class Box {{ public void pick(A a) {{}} }} class Calls {{ A value; A give() {{ return null; }} void run(Box box, A[] nulls) {{ box.pick({arg}); }} }}"
        );
        refused(&source, &format!("pick({arg})"), 1);
    }
}

#[test]
fn strict_null_inner_call_composes_only_existing_one_layer_receiver() {
    let source = "class A { public void next() {} } class Box { public A pick(A a) { return a; } public A pick(int i) { return null; } } class Calls { void run(Box box) { box.pick(null).next(); } }";
    winner(source, "pick(null)", "pick(A a)");
    let result = query(&project(source), "next();");
    assert_eq!(result.status, ResolveStatus::Resolved);
    assert_eq!(result.targets[0].owner, "A");
    assert_eq!(result.targets[0].start_byte, source.find("next()").unwrap() as u32);
}

#[test]
fn null_still_requires_complete_unique_formals_and_platform() {
    let source =
        "class A {} class Box { public void pick(A a) {} } class Calls { void run(Box box) { box.pick(null); } }";
    let mut p = project(source);
    p.remove_class("Object.class");
    assert_ne!(query(&p, "pick(null)").status, ResolveStatus::Resolved);
    p = project(source);
    p.insert("Duplicate.java", "class A {}");
    assert_ne!(query(&p, "pick(null)").status, ResolveStatus::Resolved);
    for a in ["class A extends Missing {}", "class A<T> {}"] {
        refused(&source.replace("class A {}", a), "pick(null)", 1);
    }
}

#[test]
fn null_parentheses_keep_shared_depth_budget() {
    let arg = format!("{}null{}", "(".repeat(40), ")".repeat(40));
    let source = format!(
        "class A {{}} class Box {{ public void pick(A a) {{}} }} class Calls {{ void run(Box box) {{ box.pick({arg}); }} }}"
    );
    refused(&source, &format!("pick({arg})"), 1);
}

#[test]
fn null_composes_with_inherited_selection_and_proven_override() {
    let source = "class A {} class Base { public void pick(A a) {} } class Box extends Base {} class Calls { void run(Box box) { box.pick(null); } }";
    winner(source, "pick(null)", "pick(A a)");
    let overridden = source.replace("class Box extends Base {}", "class Box extends Base { public void pick(A a) {} }");
    let selected = query(&project(&overridden), "pick(null)");
    assert_eq!(selected.status, ResolveStatus::Resolved);
    assert_eq!(selected.targets[0].owner, "Box");
    assert_eq!(selected.targets[0].start_byte, overridden.rfind("pick(A a)").unwrap() as u32);
    refused(&overridden.replacen("public void pick", "public final void pick", 1), "pick(null)", 2);
}

#[test]
fn null_spelling_is_exact_and_comments_do_not_become_null_evidence() {
    for arg in ["nullValue", "nul", r#""null""#, "/* null */ missing"] {
        let source = format!(
            "class A {{}} class Box {{ public void pick(A a) {{}} }} class Calls {{ void run(Box box) {{ box.pick({arg}); }} }}"
        );
        refused(&source, &format!("pick({arg})"), 1);
    }
}

#[test]
fn null_initializer_does_not_replace_explicit_local_or_parameter_type() {
    for body in ["A x = null; box.pick(x);", "x = null; box.pick(x);"] {
        let parameter = if body.starts_with("A ") { "" } else { ", A x" };
        let source = format!(
            "class A {{}} class B extends A {{}} class Box {{ public void pick(A a) {{}} public void pick(B b) {{}} }} class Calls {{ void run(Box box{parameter}) {{ {body} }} }}"
        );
        winner(&source, "pick(x)", "pick(A a)");
    }
}

#[test]
fn crossed_related_formals_need_one_dominating_vector_in_any_order() {
    let crossed = "public void pick(A a, B b) {} public void pick(B a, A b) {}";
    let source = format!(
        "class A {{}} class B extends A {{}} class Box {{ {crossed} }} class Calls {{ void run(Box box) {{ box.pick(null,null); }} }}"
    );
    refused(&source, "pick(null,null)", 2);
    for methods in
        [format!("{crossed} public void pick(B a, B b) {{}}"), format!("public void pick(B a, B b) {{}} {crossed}")]
    {
        let source = format!(
            "// 中😀\r\nclass A {{}} class B extends A {{}} class Box {{ {methods} }} class Calls {{ void run(Box box) {{ box.pick(/* null */ (null), null); }} }}"
        );
        winner(&source, "pick(/* null */ (null), null)", "pick(B a, B b)");
    }
}
