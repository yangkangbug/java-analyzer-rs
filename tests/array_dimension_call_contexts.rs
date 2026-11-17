use java_engine::{Project, ResolveResult, ResolveStatus, TextEdit};

fn project(body: &str) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", &format!("class Sizes {{ public int size(int n){{return n;}} public int size(long n){{return 0;}} }} class Calls {{ {body} }}"));
    let mut object = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61, 0, 3, 1, 0, 16];
    object.extend(b"java/lang/Object");
    object.extend([7, 0, 1, 0, 0x21, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    p.add_class_bytes("Object.class", &object).unwrap();
    p
}
fn query(p: &Project) -> ResolveResult {
    let at = p.file("Calls.java").unwrap().text().find("/*Q*/").unwrap() as u32 + 5;
    let r = p.resolve_at("Calls.java", at);
    assert_eq!(r, p.prepare().resolve_at("Calls.java", at));
    r
}
fn check(body: &str, resolved: bool) {
    let p = project(body);
    let r = query(&p);
    assert_eq!(r.status == ResolveStatus::Resolved, resolved, "{body}\n{r:?}");
    if resolved {
        assert!(p.file("Calls.java").unwrap().errors().is_empty());
        assert_eq!(r.targets.len(), 1);
        let t = &r.targets[0];
        assert_eq!((&*t.path, &*t.owner, &*t.name), ("Calls.java", "Sizes", "size"));
        assert_eq!(t.start_byte, p.file("Calls.java").unwrap().text().find("size(int").unwrap() as u32);
    }
}
#[test]
fn dimensions_keep_the_same_caller_and_exact_overload_as_ordinary_expressions() {
    for expr in [
        "s./*Q*/size(b)",
        "new int[s./*Q*/size(b)]",
        "new int[1][s./*Q*/size(b)]",
        "new int[s./*Q*/size(b)][]",
        "new int[/* 数组 🙂 */(s./*Q*/size((b)))]",
        "new int[new int[s./*Q*/size(b)].length]",
    ] {
        let decl = if expr.starts_with("s.") {
            "int"
        } else if expr.ends_with("[]") || expr.starts_with("new int[1]") {
            "int[][]"
        } else {
            "int[]"
        };
        for header in ["void run(Sizes s, byte b)", "static void run(Sizes s, byte b)", "Calls(Sizes s, byte b)"] {
            check(&format!("{header} {{ {decl} value = {expr}; }}"), true);
        }
    }
}
#[test]
fn constructor_arguments_initializers_and_nested_owners_stay_outside() {
    for expr in [
        "new Box(s./*Q*/size(b))",
        "new int[]{s./*Q*/size(b)}",
        "new Box(){int run(){return s./*Q*/size(b);}}",
        "new int[((Sizer)()->s./*Q*/size(b)).size()]",
        "new int[new Sizer(){public int size(){return s./*Q*/size(b);}}.size()]",
        "new Box(new int[s./*Q*/size(b)])",
    ] {
        check(&format!("void run(Sizes s, byte b) {{ Object value = {expr}; }}"), false);
    }
}
#[test]
fn incomplete_or_mixed_array_creation_never_grants_a_caller() {
    for expr in [
        "new [s./*Q*/size(b)]",
        "new .[s./*Q*/size(b)]",
        "new int[s./*Q*/size(b)",
        "new int[][s./*Q*/size(b)]",
        "new int[s./*Q*/size(b)]{1}",
        "new int[s./*Q*/size(b)][@]",
    ] {
        check(&format!("void run(Sizes s, byte b) {{ int[] value = {expr}; }}"), false);
    }
}
#[test]
fn dimensions_preserve_shadowing_static_and_constructor_delegation_limits() {
    for body in [
        "void run(Sizes s, byte b) {{int s=0; int[] x=new int[s./*Q*/size(b)];}}",
        "Sizes s; static void run(byte b) {int[] x=new int[s./*Q*/size(b)];}",
        "Calls(Sizes s, byte b) {this(); int[] x=new int[s./*Q*/size(b)];}",
        "Calls(Sizes s, byte b) {super(new int[s./*Q*/size(b)]);}",
    ] {
        check(body, false);
    }
    check("Sizes s; void run(byte b) {int[] x=new int[this.s./*Q*/size(b)];}", true);
}
#[test]
fn dimension_edits_match_fresh_and_preserve_the_old_snapshot() {
    let body = "void run(Sizes s, byte b) {int[] x=new int[s./*Q*/size(b)];}";
    let p = project(body);
    let old = query(&p);
    assert_eq!(old.status, ResolveStatus::Resolved);
    for (from, to) in [("size(b)]", "size(b)"), ("new int[", "new int[]["), ("size(b)]", "size(b)]{1}")] {
        let mut edited = p.clone();
        let text = p.file("Calls.java").unwrap().text();
        let at = text.find(from).unwrap() as u32;
        edited
            .edit("Calls.java", TextEdit { start_byte: at, end_byte: at + from.len() as u32, insert: to.into() })
            .unwrap();
        assert_eq!(query(&edited), query(&project(&body.replace(from, to))));
        assert_ne!(query(&edited).status, ResolveStatus::Resolved);
        edited
            .edit("Calls.java", TextEdit { start_byte: at, end_byte: at + to.len() as u32, insert: from.into() })
            .unwrap();
        assert_eq!(query(&edited), old);
        assert_eq!(query(&p), old);
    }
}
#[test]
fn excessive_dimension_context_work_fails_closed() {
    let expr = format!("new int[s./*Q*/size(b)]{}", "[]".repeat(2500));
    check(&format!("void run(Sizes s, byte b) {{Object x={expr};}}"), false);
}
#[test]
fn bare_static_selection_shares_dimension_context_work() {
    let locals = (0..3300).map(|i| format!("int v{i}=0;")).collect::<String>();
    for (tails, expected) in [(0, true), (200, false)] {
        let body = format!(
            "static int size(int n){{return n;}} static int size(long n){{return 0;}} static void run(byte b){{ {locals} Object a=new int[/*Q*/size(b)]{}; }}",
            "[]".repeat(tails)
        );
        let p = project(&body);
        let r = query(&p);
        assert_eq!(r.status == ResolveStatus::Resolved, expected, "tails={tails}: {r:?}");
        if expected {
            assert_eq!(r.targets[0].owner, "Calls");
        }
    }
}
#[test]
fn dimension_work_also_bounds_the_primitive_negative_guard_without_platform() {
    let locals = (0..3300).map(|i| format!("int v{i}=0;")).collect::<String>();
    for (tails, expected) in [(0, true), (200, false)] {
        let body = format!(
            "static int size(int n){{return n;}} static void run(byte b){{ {locals} Object a=new int[/*Q*/size(b)]{}; }}",
            "[]".repeat(tails)
        );
        let mut p = project(&body);
        p.remove_class("Object.class");
        let r = query(&p);
        assert_eq!(r.status == ResolveStatus::Resolved, expected, "tails={tails}: {r:?}");
    }
}
