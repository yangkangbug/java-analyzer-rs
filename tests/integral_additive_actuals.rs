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
fn integral_additive_promotion_selects_int_or_long_without_narrowing() {
    for left in ["byte", "short", "char", "int", "long"] {
        for right in ["byte", "short", "char", "int", "long"] {
            for op in ["+", "-"] {
                let actual = format!("(a {op} (b))");
                let source = format!(
                    "class Box {{ public void pick(byte x) {{}} public void pick(short x) {{}} public void pick(char x) {{}} public void pick(int x) {{}} public void pick(long x) {{}} }} class Calls {{ void run(Box box, {left} a, {right} b) {{ box.pick({actual}); }} }}"
                );
                winner(
                    &project(&source, true),
                    &format!("pick({actual})"),
                    if left == "long" || right == "long" { "pick(long x)" } else { "pick(int x)" },
                );
            }
        }
    }
}
#[test]
fn arithmetic_does_not_fold_overflow_or_negative_results() {
    for actual in ["2147483647 + 1", "0 - 1", "(1 + 2) - (3 + 4)"] {
        let source = format!(
            "class Box {{ public void pick(int x) {{}} public void pick(long x) {{}} }} class Calls {{ void run(Box box) {{ box.pick({actual}); }} }}"
        );
        winner(&project(&source, true), &format!("pick({actual})"), "pick(int x)");
    }
}
#[test]
fn unsupported_operands_and_operators_retain_all_candidates() {
    for actual in [
        "text + i",
        "i + text",
        "boxed + i",
        "f + i",
        "d - i",
        "flag + i",
        "array + i",
        "array[0] + i",
        "unknown + i",
        "holder.field + i",
        "box.value() + i",
        "-1 + i",
        "-2147483648 + i",
        "i + 2147483648",
        "i + 1L",
        "i += 1",
        "i++",
        "i * 1",
        "i / 1",
        "i & 1",
        "i << 1",
    ] {
        let source = format!(
            "class Integer {{}} class String {{}} class Holder {{ int field; }} class Box {{ public void pick(int x) {{}} public void pick(long x) {{}} public int value() {{return 1;}} }} class Calls {{ void run(Box box, int i, String text, Integer boxed, float f, double d, boolean flag, int[] array, Holder holder) {{ box.pick({actual}); }} }}"
        );
        let result = query(&project(&source, true), &format!("pick({actual})"));
        assert_eq!(result.status, ResolveStatus::OverloadNotResolved, "{actual}: {result:?}");
        assert_eq!(result.targets.len(), 2, "{actual}");
    }
}
#[test]
fn long_sum_cannot_narrow_to_int_and_competitors_cannot_disappear() {
    for methods in ["public void pick(int x) {}", "public void pick(int x) {} public <T> void pick(T x) {}"] {
        let source =
            format!("class Box {{ {methods} }} class Calls {{ void run(Box box,long a,int b) {{ box.pick(a+b); }} }}");
        assert_ne!(query(&project(&source, true), "pick(a+b)").status, ResolveStatus::Resolved);
    }
    let source = "class Box { public void pick(int x) {} public <T> void pick(T x) {} } class Calls { void run(Box box,int a,int b) { box.pick(a+b); } }";
    assert_ne!(query(&project(source, true), "pick(a+b)").status, ResolveStatus::Resolved);
}
#[test]
fn malformed_and_expired_local_evidence_refuses() {
    for actual in ["i +", "i + +", "i + 1 2"] {
        let source = format!(
            "class Box {{ public void pick(int x) {{}} public void pick(long x) {{}} }} class Calls {{ void run(Box box,int i) {{ box.pick({actual}); }} }}"
        );
        let mut p = project("class Calls {}", true);
        p.insert("Calls.java", &source);
        assert_ne!(query(&p, &format!("pick({actual})")).status, ResolveStatus::Resolved);
    }
    for declarations in ["{int i=1;}", "int i=1; int i=2;"] {
        let source = format!(
            "class Box {{ public void pick(int x) {{}} public void pick(long x) {{}} }} class Calls {{ void run(Box box) {{ {declarations} box.pick(i+1); }} }}"
        );
        assert_ne!(query(&project(&source, true), "pick(i+1)").status, ResolveStatus::Resolved);
    }
}
