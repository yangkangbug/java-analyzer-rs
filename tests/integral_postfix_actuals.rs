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
fn postfix_preserves_integral_type_for_parameters_and_locals() {
    for ty in ["byte", "short", "char", "int", "long"] {
        for op in ["++", "--"] {
            for local in [false, true] {
                let declaration = if local { format!("{ty} value = input;") } else { String::new() };
                let name = if local { "value" } else { "input" };
                let actual = format!("({name}{op})");
                let source = format!(
                    "class Box {{ public void pick(byte x) {{}} public void pick(short x) {{}} public void pick(char x) {{}} public void pick(int x) {{}} public void pick(long x) {{}} }} class Calls {{ void run(Box box, {ty} input) {{ {declaration} box.pick({actual}); }} }}"
                );
                winner(&project(&source, true), &format!("pick({actual})"), &format!("pick({ty} x)"));
            }
        }
    }
}
#[test]
fn postfix_composition_uses_additive_promotion_without_evaluation() {
    for actual in ["b++ + c--", "s-- - b++", "(i++) + 1"] {
        let source = format!(
            "class Box {{ public void pick(byte x) {{}} public void pick(int x) {{}} public void pick(long x) {{}} }} class Calls {{ void run(Box box, byte b, short s, char c, int i) {{ box.pick({actual}); }} }}"
        );
        winner(&project(&source, true), &format!("pick({actual})"), "pick(int x)");
    }
}
#[test]
fn unsupported_postfix_evidence_stays_conservative() {
    for actual in [
        "++i",
        "--i",
        "holder.field++",
        "array[0]++",
        "boxed++",
        "f++",
        "d--",
        "flag++",
        "box.value()++",
        "1++",
        "(i+1)++",
        "i++++",
        "unknown++",
    ] {
        let source = format!(
            "class Integer {{}} class Holder {{ int field; }} class Box {{ public void pick(int x) {{}} public void pick(long x) {{}} public int value() {{ return 1; }} }} class Calls {{ void run(Box box, int i, int[] array, Holder holder, Integer boxed, float f, double d, boolean flag) {{ box.pick({actual}); }} }}"
        );
        let mut p = project("class Calls {}", true);
        p.insert("Calls.java", &source);
        assert_ne!(query(&p, &format!("pick({actual})")).status, ResolveStatus::Resolved, "{actual}");
    }
}
#[test]
fn final_shared_modifiers_and_scope_cannot_supply_writable_evidence() {
    for declaration in [
        "final int i=1;",
        "final int first=0, i=1;",
        "int i=1; int i=2;",
        "{ int i=1; }",
        "int i;",
        "int i; i=1;",
        "public int i=1;",
        "static int i=1;",
        "@Marker int i=1;",
    ] {
        let source = format!(
            "class Box {{ public void pick(int x) {{}} public void pick(long x) {{}} }} class Calls {{ void run(Box box) {{ {declaration} box.pick(i++); }} }}"
        );
        assert_ne!(query(&project(&source, true), "pick(i++)").status, ResolveStatus::Resolved, "{declaration}");
    }
    for parameter in ["final int i", "@Marker int i"] {
        let source = format!(
            "class Box {{ public void pick(int x) {{}} public void pick(long x) {{}} }} class Calls {{ void run(Box box, {parameter}) {{ box.pick(i++); }} }}"
        );
        assert_ne!(query(&project(&source, true), "pick(i++)").status, ResolveStatus::Resolved, "{parameter}");
    }
}

#[test]
fn initializer_loop_header_and_capture_boundaries_refuse() {
    for body in [
        "int i=box.pick(i++);",
        "int first=0, i=box.pick(first++);",
        "for(int i=0;i<2;i++){ box.pick(i++); }",
        "for(int i:array){ box.pick(i++); }",
        "int i=0; Runnable r=()->{box.pick(i++);};",
        "int i=0; class Inner { void f(){box.pick(i++);} }",
        "try(int i=0){box.pick(i++);}",
        "try {} catch(Exception i){box.pick(i++);}",
    ] {
        let source = format!(
            "class Box {{ public int pick(int x) {{ return x; }} public int pick(long x) {{ return 0; }} }} class Calls {{ void run(Box box,int[] array) {{ {body} }} }}"
        );
        let mut p = project("class Calls {}", true);
        p.insert("Calls.java", &source);
        let marker = if body.contains("first++") { "pick(first++)" } else { "pick(i++)" };
        assert_ne!(query(&p, marker).status, ResolveStatus::Resolved, "{body}");
    }
}
#[test]
fn plain_multiple_declarations_and_loop_body_local_are_supported() {
    for declarations in ["int first=0, i=1;", "int i=1;"] {
        let source = format!(
            "class Box {{ public void pick(int x) {{}} public void pick(long x) {{}} }} class Calls {{ void run(Box box) {{ while(true) {{ {declarations} box.pick(i++); }} }} }}"
        );
        winner(&project(&source, true), "pick(i++)", "pick(int x)");
    }
}
