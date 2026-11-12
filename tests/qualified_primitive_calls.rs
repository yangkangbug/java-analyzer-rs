use java_engine::{Project, ResolveResult, ResolveStatus, TargetKind};

fn project(source: &str, platform: bool) -> Project {
    let mut project = Project::new();
    project.insert("Calls.java", source);
    assert!(project.file("Calls.java").unwrap().errors().is_empty());
    if platform {
        // Native signature data only; no javac or JVM.
        let mut object = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61, 0, 3, 1, 0, 16];
        object.extend(b"java/lang/Object");
        object.extend([7, 0, 1, 0, 0x21, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        project.add_class_bytes("Object.class", &object).unwrap();
    }
    project
}

fn query(project: &Project, marker: &str) -> ResolveResult {
    let source = project.file("Calls.java").unwrap().text();
    let at = source.rfind(marker).unwrap() as u32;
    let result = project.resolve_at("Calls.java", at);
    assert_eq!(result, project.prepare().resolve_at("Calls.java", at));
    result
}

fn winner(project: &Project, marker: &str, declaration: &str) {
    let result = query(project, marker);
    assert_eq!(result.status, ResolveStatus::Resolved, "{marker}: {result:?}");
    assert_eq!(result.targets.len(), 1);
    let target = &result.targets[0];
    assert_eq!(target.kind, TargetKind::Method);
    assert_eq!(target.path, "Calls.java");
    let at = project.file("Calls.java").unwrap().text().find(declaration).unwrap() as u32;
    assert_eq!((target.start_byte, target.end_byte), (at, at + 4));
}

#[test]
fn explicit_byte_and_int_parameters_and_locals_select_the_unique_widening_winner() {
    for actual_type in ["byte", "int"] {
        for methods in [
            "public void pick(int x) {} public void pick(long x) {}",
            "public void pick(long x) {} public void pick(int x) {}",
        ] {
            let source = format!(
                "class Box {{ {methods} }} class Calls {{ void run(Box box, {actual_type} value) {{ {actual_type} local = value; box.pick(value); (box).pick((local)); }} }}"
            );
            let p = project(&source, true);
            winner(&p, "pick(value)", "pick(int x)");
            winner(&p, "pick((local))", "pick(int x)");
        }
    }
}

#[test]
fn bounded_nonnegative_decimal_int_literals_select_int_not_long() {
    for actual in ["0", "1", "2147483647"] {
        let source = format!(
            "class Box {{ public void pick(int x) {{}} public void pick(long x) {{}} }} class Calls {{ void run(Box box) {{ box.pick({actual}); }} }}"
        );
        winner(&project(&source, true), &format!("pick({actual})"), "pick(int x)");
    }
}

#[test]
fn unsupported_literal_forms_and_class_literals_keep_both_candidates() {
    for actual in [
        "2147483648",
        "999999999999999999999999999999",
        "1L",
        "0x1",
        "01",
        "0b1",
        "1_0",
        "1l",
        "0X1",
        "00",
        "+1",
        "-1",
        "int.class",
        "int[].class",
    ] {
        let source = format!(
            "class Box {{ public void pick(int x) {{}} public void pick(long x) {{}} }} class Calls {{ void run(Box box) {{ box.pick({actual}); }} }}"
        );
        let result = query(&project(&source, true), &format!("pick({actual})"));
        assert_eq!(result.status, ResolveStatus::OverloadNotResolved, "{actual}: {result:?}");
        assert_eq!(result.targets.len(), 2);
        assert!(result.targets.iter().all(|target| target.name == "pick" && target.kind == TargetKind::Method));
    }
}

#[test]
fn unsupported_competitors_and_incomplete_platform_do_not_manufacture_a_winner() {
    for (source, platform) in [
        (
            "class Integer {} class Box { public void pick(Integer x) {} } class Calls { void run(Box box) { box.pick(1); } }",
            true,
        ),
        (
            "class Box { public void pick(int x) {} public <T> void pick(T x) {} } class Calls { void run(Box box) { box.pick(1); } }",
            true,
        ),
        (
            "class Box { public void pick(int x) {} public void pick(long... x) {} } class Calls { void run(Box box) { box.pick(1); } }",
            true,
        ),
        (
            "class Base { public void pick(long x) {} } class Box extends Base { public void pick(int x) {} } class Calls { void run(Box box) { box.pick(1); } }",
            true,
        ),
        (
            "class Box { public void pick(int x) {} public void pick(long x) {} } class Calls { void run(Box box) { box.pick(1); } }",
            false,
        ),
    ] {
        let result = query(&project(source, platform), "pick(1)");
        assert_ne!(result.status, ResolveStatus::Resolved, "{source}: {result:?}");
        assert!(!result.targets.is_empty(), "{source}: preserve source candidates");
    }
}

#[test]
fn mixed_actuals_are_additive_but_outer_call_return_receivers_remain_unproved() {
    let source = "class Value { public void later() {} } class Box { public Value pick(int x) { return null; } public Value pick(long x) { return null; } public void mixed(int x, Value v) {} } class Calls { void run(Box box, Value value) { box.pick(1).later(); box.mixed(0, value); } }";
    let p = project(source, true);
    winner(&p, "pick(1)", "pick(int x)");
    assert_ne!(query(&p, "later();").status, ResolveStatus::Resolved);
    // The mixed-vector slice intentionally advances this former refusal.
    let mixed = query(&p, "mixed(0, value)");
    assert_eq!(mixed.status, ResolveStatus::Resolved);
    assert_eq!(mixed.targets.len(), 1);
    let at = source.find("mixed(int x").unwrap() as u32;
    assert_eq!((mixed.targets[0].start_byte, mixed.targets[0].end_byte), (at, at + 5));
}

#[test]
fn all_explicit_scalar_kinds_use_identity_and_the_widening_partial_order() {
    for (actual, best, other) in [
        ("boolean", "boolean", "int"),
        ("byte", "short", "int"),
        ("short", "int", "long"),
        ("char", "int", "long"),
        ("int", "int", "long"),
        ("long", "long", "float"),
        ("float", "float", "double"),
        ("double", "double", "int"),
    ] {
        let source = format!(
            "class Box {{ public void pick({other} x) {{}} public void pick({best} x) {{}} }} class Calls {{ void run(Box box, {actual} value) {{ box.pick(value); }} }}"
        );
        winner(&project(&source, true), "pick(value)", &format!("pick({best} x)"));
    }
}

#[test]
fn no_field_call_result_array_or_budget_escape_supplies_scalar_proof() {
    for actual in [
        "this.value".to_string(),
        "give()".into(),
        "values".into(),
        "values[0]".into(),
        format!("{}value{}", "(".repeat(40), ")".repeat(40)),
        "9".repeat(5000),
    ] {
        let source = format!(
            "class Box {{ public void pick(int x) {{}} public void pick(long x) {{}} }} class Calls {{ int value; int give() {{ return 1; }} void run(Box box, int value, int[] values) {{ box.pick({actual}); }} }}"
        );
        let result = query(&project(&source, true), &format!("pick({actual})"));
        assert_eq!(result.status, ResolveStatus::OverloadNotResolved, "{actual}: {result:?}");
        assert_eq!(result.targets.len(), 2);
    }
}

#[test]
fn unsupported_scalar_competitors_are_not_removed_before_selection() {
    for competitor in [
        "private void pick(long x) {}",
        "public static void pick(long x) {}",
        "public void pick(Missing x) {}",
        "public void pick(int x) {}",
    ] {
        let source = format!(
            "class Box {{ public void pick(int x) {{}} {competitor} }} class Calls {{ void run(Box box) {{ box.pick(1); }} }}"
        );
        assert_ne!(query(&project(&source, true), "pick(1)").status, ResolveStatus::Resolved, "{competitor}");
    }
}

#[test]
fn singleton_byte_and_short_formals_do_not_narrow_int_constants_or_parameters() {
    for formal in ["byte", "short"] {
        for actual in ["value", "1"] {
            let source = format!(
                "class Box {{ public void pick({formal} x) {{}} }} class Calls {{ void run(Box box, int value) {{ box.pick({actual}); }} }}"
            );
            let result = query(&project(&source, true), &format!("pick({actual})"));
            assert_eq!(result.status, ResolveStatus::CallNotResolved, "{formal} / {actual}: {result:?}");
            assert_eq!(result.targets.len(), 1);
            assert_eq!(result.targets[0].name, "pick");
            assert_eq!(result.targets[0].kind, TargetKind::Method);
        }
    }
}

#[test]
fn missing_expired_or_overlapping_primitive_locals_never_borrow_outer_values() {
    for caller in [
        "void run(Box box) { box.pick(missing); }",
        "int value; void run(Box box) { { int value = 1; } box.pick(value); }",
        "int value; void run(Box box) { box.pick(value); int value = 1; }",
        "void run(Box box, int value) { int value = 1; box.pick(value); }",
        "void run(Box box) { int value = 1; { int value = 2; box.pick(value); } }",
    ] {
        let source = format!(
            "class Box {{ public void pick(int x) {{}} public void pick(long x) {{}} }} class Calls {{ {caller} }}"
        );
        let marker = if caller.contains("missing") { "pick(missing)" } else { "pick(value)" };
        let result = query(&project(&source, true), marker);
        assert_eq!(result.status, ResolveStatus::OverloadNotResolved, "{caller}: {result:?}");
        assert_eq!(result.targets.len(), 2);
        assert!(result.targets.iter().all(|target| target.name == "pick" && target.kind == TargetKind::Method));
    }
}
