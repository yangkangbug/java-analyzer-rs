use java_engine::{Project, ResolveResult, ResolveStatus};

fn project(source: &str) -> Project {
    let p = project_allow_errors(source);
    assert!(p.file("Calls.java").unwrap().errors().is_empty());
    p
}

fn project_allow_errors(source: &str) -> Project {
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

#[test]
fn composed_source_results_supply_actuals_and_receiver_chains() {
    for body in ["box.accept(box.make());", "box.make().again().next();", "box.accept(box.make().again());"] {
        let s = format!(
            "class Result {{ public Result again() {{ return this; }} public void next() {{}} }} class Box {{ public Result make() {{ return null; }} public void accept(Result r) {{}} }} class Calls {{ void run(Box box) {{ {body} }} }}"
        );
        let needle = if body.contains("accept") { "accept(" } else { "next();" };
        let r = query(&project(&s), needle);
        assert_eq!(r.status, ResolveStatus::Resolved, "{body}: {r:?}");
        assert_eq!(r.targets.len(), 1);
        assert_eq!(r.targets[0].owner, if body.contains("accept") { "Box" } else { "Result" });
        let declaration = if body.contains("accept") { "accept(Result" } else { "next() {}" };
        assert_eq!(r.targets[0].start_byte, s.find(declaration).unwrap() as u32);
    }
}
#[test]
fn primitive_source_results_are_declared_types_not_constant_narrowing() {
    let s = "class Box { public int size() { return 1; } public void pick(int n) {} public void pick(long n) {} } class Calls { void run(Box box) { box.pick(box.size()); } }";
    let r = query(&project(s), "pick(box");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets[0].start_byte, s.find("pick(int").unwrap() as u32);
    let s = s.replace("public void pick(int n) {} public void pick(long n) {}", "public void pick(byte n) {}");
    assert_ne!(query(&project(&s), "pick(box").status, ResolveStatus::Resolved);
}
#[test]
fn unknown_nested_calls_and_unsupported_receivers_remain_terminal() {
    for call in ["box.missing()", "box.make(1)", "box.nothing()", "box.generic()"] {
        let s = format!(
            "class Result {{ public void next() {{}} }} class Box {{ public Result make() {{ return null; }} public void nothing() {{}} public <T> T generic() {{ return null; }} public void accept(Result r) {{}} }} class Calls {{ void run(Box box, int next) {{ {call}.next(); box.accept({call}); }} }}"
        );
        assert_eq!(query(&project(&s), "next();").status, ResolveStatus::ReceiverNotBound);
        assert_ne!(query(&project(&s), "accept(").status, ResolveStatus::Resolved);
    }
}

#[test]
fn multiple_receiver_levels_and_nested_actuals_share_declared_types() {
    let s = "class Value { public Value again() { return this; } public int size() { return 1; } } class Box { public Value make() { return null; } public Value echo(Value v) { return v; } public void take(int n) {} public void take(long n) {} } class Calls { void run(Box b) { b.take(b.echo(b.make().again()).again().size()); } }";
    let r = query(&project(s), "take(b");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets[0].start_byte, s.find("take(int").unwrap() as u32);
}

#[test]
fn composition_uses_static_return_and_declaration_imports() {
    let mut p =
        project("package caller; class Value {} class Calls { void run(api.Box b) { b.make().again().next(); } }");
    p.insert(
        "Value.java",
        "package model; public class Value { public Value again() { return this; } public void next() {} }",
    );
    p.insert("Child.java", "package model; public class Child extends Value { public void next() {} }");
    p.insert("Box.java", "package api; import model.Value; public class Box { public Value make() { return null; } }");
    let old = p.clone();
    let r = query(&p, "next();");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets[0].path, "Value.java");
    p.insert("Box.java", "package api; import caller.Value; public class Box { public Value make() { return null; } }");
    assert_ne!(query(&p, "next();").status, ResolveStatus::Resolved);
    assert_eq!(query(&old, "next();").targets[0].path, "Value.java");
}

#[test]
fn source_result_arguments_preserve_crossed_specificity_and_unknown_competitors() {
    let s = "class A {} class B extends A {} class Box { public B make() { return null; } public int size() { return 1; } public void pick(B v, long n) {} public void pick(A v, int n) {} } class Calls { void run(Box b) { b.pick(b.make(),b.size()); } }";
    assert_ne!(query(&project(s), "pick(b").status, ResolveStatus::Resolved);
    let s = s.replace("public void pick(A v, int n) {}", "public void pick(B v, int n) {}");
    let r = query(&project(&s), "pick(b");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets[0].start_byte, s.find("pick(B v, int").unwrap() as u32);
    let s = s.replace("public B make()", "public void pick(Missing x, int y, int z) {} public B make()");
    assert_ne!(query(&project(&s), "pick(b").status, ResolveStatus::Resolved);
}

#[test]
fn unsupported_bases_and_wrapper_results_do_not_gain_receiver_types() {
    for base in ["new Box()", "Calls.field", "this.field", "((Box)b)", "(true ? b : b)", "Box"] {
        let s = format!(
            "class Value {{ public Value again() {{ return this; }} public void next() {{}} }} class Box {{ public Value make() {{ return null; }} }} class Calls {{ static Box field; void run(Box b, int next) {{ {base}.make().again().next(); }} }}"
        );
        assert_eq!(query(&project(&s), "next();").status, ResolveStatus::ReceiverNotBound, "{base}");
    }
}

#[test]
fn composition_keeps_annotated_generic_array_void_and_static_returns_refused() {
    for method in [
        "@A public Value make() { return null; }",
        "public Value[] make() { return null; }",
        "public <T> T make() { return null; }",
        "public static Value make() { return null; }",
        "public void make() {}",
    ] {
        let s = format!(
            "class Value {{ public Value again() {{ return this; }} public void next() {{}} }} class Box {{ {method} public void take(Value v) {{}} }} class Calls {{ void run(Box b,int next) {{ b.make().again().next(); b.take(b.make()); }} }}"
        );
        assert_eq!(query(&project(&s), "next();").status, ResolveStatus::ReceiverNotBound);
        assert_ne!(query(&project(&s), "take(b").status, ResolveStatus::Resolved, "{method}");
    }
}

#[test]
fn nested_argument_and_receiver_depth_is_one_budget() {
    for (outer, chain) in [(0, 40), (20, 20), (40, 0)] {
        let mut value = format!("b{}", ".again()".repeat(chain));
        for _ in 0..outer {
            value = format!("b.echo({value})");
        }
        let s = format!(
            "class Box {{ public Box again() {{ return this; }} public Box echo(Box v) {{ return v; }} public void next() {{}} }} class Calls {{ void run(Box b,int next) {{ {value}.next(); }} }}"
        );
        assert_eq!(query(&project(&s), "next();").status, ResolveStatus::ReceiverNotBound);
    }
}

#[test]
fn wide_shallow_composition_exhausts_work_without_depth_reset() {
    for count in [2, 200] {
        let params = (0..count).map(|i| format!("int p{i}")).collect::<Vec<_>>().join(",");
        let args = std::iter::repeat_n("b.size()", count).collect::<Vec<_>>().join(",");
        let s = format!(
            "class Box {{ public int size() {{ return 1; }} public void take({params}) {{}} }} class Calls {{ void run(Box b) {{ b.take({args}); }} }}"
        );
        let r = query(&project(&s), "take(b");
        assert_eq!(r.status == ResolveStatus::Resolved, count == 2, "count={count}: {r:?}");
    }
}

#[test]
fn primitive_results_never_become_receivers_or_constant_values() {
    let s = "class Box { public byte small() { return 1; } public int size() { return 1; } public void pick(byte n) {} public void pick(int n) {} } class Calls { void run(Box b,int next) { b.size().next(); b.pick(b.small()); b.pick(b.size()); } }";
    assert_eq!(query(&project(s), "next();").status, ResolveStatus::ReceiverNotBound);
    let byte = query(&project(s), "pick(b.small");
    assert_eq!(byte.status, ResolveStatus::Resolved);
    assert_eq!(byte.targets[0].start_byte, s.find("pick(byte").unwrap() as u32);
    let int = query(&project(s), "pick(b.size");
    assert_eq!(int.status, ResolveStatus::Resolved);
    assert_eq!(int.targets[0].start_byte, s.find("pick(int").unwrap() as u32);
}
