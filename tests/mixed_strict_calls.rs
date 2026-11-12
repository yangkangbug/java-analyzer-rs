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
fn primitive_and_reference_positions_combine_identity_widening_and_specificity() {
    for (methods, call, best) in [
        ("public void pick(int i, B b) {} public void pick(long i, A a) {}", "pick(1, value)", "pick(int i, B b)"),
        (
            "public void pick(long i, A a) {} public void pick(int i, B b) {}",
            "pick(index, (value))",
            "pick(int i, B b)",
        ),
        ("public void pick(A a, long i) {} public void pick(B b, int i) {}", "pick(value, index)", "pick(B b, int i)"),
    ] {
        let source = format!(
            "class A {{}} class B extends A {{}} class D extends B {{}} class Box {{ {methods} }} class Calls {{ void run(Box box, byte index, D value) {{ box.{call}; }} }}"
        );
        winner(&source, call, best);
    }
}

#[test]
fn two_primitives_require_formal_to_formal_widening_not_a_conversion_score() {
    winner(
        "class Box { public void pick(int a, long b) {} public void pick(long a, long b) {} } class Calls { void run(Box box, byte a, int b) { box.pick(a,b); } }",
        "pick(a,b)",
        "pick(int a, long b)",
    );
    refused(
        "class Box { public void pick(int a, long b) {} public void pick(long a, int b) {} } class Calls { void run(Box box) { box.pick(1,1); } }",
        "pick(1,1)",
        2,
    );
}

#[test]
fn crossed_mixed_vectors_remain_ambiguous() {
    refused(
        "class A {} class B extends A {} class Box { public void pick(A a, int i) {} public void pick(B b, long i) {} } class Calls { void run(Box box, B value) { box.pick(value,1); } }",
        "pick(value,1)",
        2,
    );
}

#[test]
fn every_actual_must_have_existing_proof() {
    for actual in
        ["null", "give()", "this.value", "values", "values[0]", "1 + 1", "-1", "int.class", "2147483648", "1_0"]
    {
        let source = format!(
            "class A {{}} class Box {{ public void pick(int i, A a) {{}} }} class Calls {{ int value; int give() {{ return 1; }} void run(Box box, A a, int[] values) {{ box.pick({actual},a); }} }}"
        );
        refused(&source, &format!("pick({actual},a)"), 1);
    }
}

#[test]
fn unknown_or_ineligible_competitors_cannot_be_filtered_by_arity() {
    for competitor in [
        "public void pick(Missing x) {}",
        "public <T> void pick(T x) {}",
        "private void pick(long i, A a) {}",
        "public void pick(long... values) {}",
        "public static void pick(long i, A a) {}",
    ] {
        let source = format!(
            "class A {{}} class Box {{ public void pick(int i, A a) {{}} {competitor} }} class Calls {{ void run(Box box, A a) {{ box.pick(1,a); }} }}"
        );
        refused(&source, "pick(1,a)", 2);
    }
}

#[test]
fn mixed_calls_do_not_add_boxing_narrowing_or_result_receiver_typing() {
    for formal in ["byte", "short", "Integer"] {
        let source = format!(
            "class Integer {{}} class A {{}} class Box {{ public void pick({formal} i, A a) {{}} }} class Calls {{ void run(Box box, A a) {{ box.pick(1,a); }} }}"
        );
        refused(&source, "pick(1,a)", 1);
    }
    let source = "class A { public void later() {} } class Box { public A pick(int i, A a) { return a; } } class Calls { void run(Box box, A a) { box.pick(1,a).later(); } }";
    winner(source, "pick(1,a)", "pick(int i, A a)");
    refused(source, "later();", 0);
}

#[test]
fn shared_depth_and_work_limits_still_refuse_mixed_arguments() {
    for actual in [format!("{}1{}", "(".repeat(40), ")".repeat(40)), "9".repeat(5000)] {
        let source = format!(
            "class A {{}} class Box {{ public void pick(int i, A a) {{}} }} class Calls {{ void run(Box box, A a) {{ box.pick({actual},a); }} }}"
        );
        refused(&source, &format!("pick({actual},a)"), 1);
    }
}

#[test]
fn a_dominating_mixed_vector_is_unique_independently_of_declaration_order() {
    for methods in [
        "public void pick(int i, A a) {} public void pick(long i, B b) {} public void pick(int i, B b) {}",
        "public void pick(int i, B b) {} public void pick(long i, B b) {} public void pick(int i, A a) {}",
    ] {
        let source = format!(
            "// 中😀\r\nclass A {{}} class B extends A {{}} class Box {{ {methods} }} class Calls {{ void run(Box box, B b) {{ box.pick(1,b); }} }}"
        );
        winner(&source, "pick(1,b)", "pick(int i, B b)");
    }
}

#[test]
fn primitive_and_reference_slots_neither_box_nor_unbox_and_keep_unknown_formals() {
    for methods in ["public void pick(int i, Integer a) {}", "public void pick(Integer i, int a) {}"] {
        let source = format!(
            "class Integer {{}} class Box {{ {methods} }} class Calls {{ void run(Box box, Integer value) {{ box.pick(value,1); }} }}"
        );
        if methods.starts_with("public void pick(int") {
            refused(&source, "pick(value,1)", 1);
        } else {
            winner(&source, "pick(value,1)", "pick(Integer i, int a)");
            refused(&source.replace("pick(value,1)", "pick(1,value)"), "pick(1,value)", 1);
        }
    }
    let source = "class A {} class Box { public void pick(int i, A a) {} public void pick(A a, Missing m) {} } class Calls { void run(Box box, A a) { box.pick(1,a); } }";
    refused(source, "pick(1,a)", 2);
}

#[test]
fn mixed_argument_scopes_do_not_borrow_expired_or_overlapping_bindings() {
    for body in
        ["{ int i = 0; } box.pick(i,a);", "int i = 0; { int i = 1; box.pick(i,a); }", "box.pick(i,a); int i = 0;"]
    {
        let source = format!(
            "class A {{}} class Box {{ public void pick(int i, A a) {{}} }} class Calls {{ int i; void run(Box box, A a) {{ {body} }} }}"
        );
        refused(&source, "pick(i,a)", 1);
    }
}
