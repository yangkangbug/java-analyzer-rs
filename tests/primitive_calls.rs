use java_engine::{Project, ResolveResult, ResolveStatus, TargetKind};

// Minimal JVMS signature data, assembled without javac or a Java runtime.
fn object(parent: Option<&str>, method: Option<&str>) -> Vec<u8> {
    fn u(b: &mut Vec<u8>, n: u16) {
        b.extend(n.to_be_bytes());
    }
    fn utf(b: &mut Vec<u8>, s: &str) {
        b.push(1);
        u(b, s.len() as u16);
        b.extend(s.as_bytes());
    }
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61];
    u(&mut b, 7);
    utf(&mut b, "java/lang/Object");
    b.push(7);
    u(&mut b, 1);
    utf(&mut b, parent.unwrap_or("java/lang/Object"));
    b.push(7);
    u(&mut b, 3);
    utf(&mut b, method.unwrap_or("unrelated"));
    utf(&mut b, "()V");
    u(&mut b, 0x21);
    u(&mut b, 2);
    u(&mut b, if parent.is_some() { 4 } else { 0 });
    u(&mut b, 0);
    u(&mut b, 0);
    u(&mut b, 1);
    u(&mut b, 0x101);
    u(&mut b, 5);
    u(&mut b, 6);
    u(&mut b, 0);
    u(&mut b, 0);
    b
}

fn project(source: &str) -> Project {
    let mut project = Project::new();
    project.insert("Calls.java", source);
    project.add_class_bytes("Object.class", &object(None, None)).unwrap();
    project
}

fn query(project: &Project, needle: &str) -> ResolveResult {
    let source = project.file("Calls.java").unwrap().text();
    let at = source.rfind(needle).unwrap() as u32;
    let rebuilt = project.resolve_at("Calls.java", at);
    assert_eq!(rebuilt, project.prepare().resolve_at("Calls.java", at));
    rebuilt
}

fn selected(source: &str, needle: &str, declaration: &str) {
    let result = query(&project(source), needle);
    assert_eq!(result.status, ResolveStatus::Resolved, "{source}\n{result:?}");
    assert_eq!(result.targets.len(), 1, "{result:?}");
    let target = &result.targets[0];
    assert_eq!(target.kind, TargetKind::Method);
    assert_eq!(target.path, "Calls.java");
    assert_eq!(target.owner, "Calls");
    let start = source.find(declaration).unwrap() as u32;
    assert_eq!((target.start_byte, target.end_byte), (start, start + 1), "{result:?}");
}

fn refused(source: &str, needle: &str, status: ResolveStatus, count: usize) {
    let result = query(&project(source), needle);
    assert_eq!(result.status, status, "{source}\n{result:?}");
    assert_eq!(result.targets.len(), count, "{result:?}");
    let expected: Vec<_> = source
        .match_indices("m(")
        .filter_map(|(at, _)| {
            matches!(source[..at].split_whitespace().last(), Some("void" | "int" | "float" | "double"))
                .then_some((at as u32, at as u32 + 1))
        })
        .collect();
    let actual: Vec<_> = result.targets.iter().map(|t| (t.start_byte, t.end_byte)).collect();
    assert_eq!(actual, expected, "complete original declaration candidates: {result:?}");
    for target in &result.targets {
        assert_eq!(target.path, "Calls.java");
        assert_eq!(target.kind, TargetKind::Method);
        assert_eq!(&source[target.start_byte as usize..target.end_byte as usize], "m");
    }
}

#[test]
fn primitive_identity_and_widening_matrix() {
    let kinds = ["boolean", "byte", "short", "char", "int", "long", "float", "double"];
    let destinations: [&[&str]; 8] = [
        &["boolean"],
        &["byte", "short", "int", "long", "float", "double"],
        &["short", "int", "long", "float", "double"],
        &["char", "int", "long", "float", "double"],
        &["int", "long", "float", "double"],
        &["long", "float", "double"],
        &["float", "double"],
        &["double"],
    ];
    for (i, actual) in kinds.iter().enumerate() {
        for formal in kinds {
            let source = format!(
                "class Calls {{ static void m({formal} x) {{}} static void m() {{}} static void test({actual} value) {{ m(value); }} }}"
            );
            if destinations[i].contains(&formal) {
                selected(&source, "m(value)", &format!("m({formal} x)"));
            } else {
                refused(&source, "m(value)", ResolveStatus::OverloadNotResolved, 2);
            }
        }
    }
}

#[test]
fn most_specific_is_a_partial_order_not_declaration_order_or_arity() {
    for methods in
        ["static void m(float a) {} static void m(long a) {}", "static void m(long a) {} static void m(float a) {}"]
    {
        let source = format!("class Calls {{ {methods} static void test(byte a) {{ m(a); }} }}");
        selected(&source, "m(a)", "m(long a)");
    }
    refused(
        "class Calls { static void m(int a, long b) {} static void m(long a, int b) {} static void test(int a, int b) { m(a, b); } }",
        "m(a, b)",
        ResolveStatus::OverloadNotResolved,
        2,
    );
    refused(
        "class Calls { static void m(byte a) {} static void m(short a) {} static void test(int a) { m(a); } }",
        "m(a)",
        ResolveStatus::OverloadNotResolved,
        2,
    );
    refused(
        "class Calls { static void m(int a) {} static int m(int b) { return b; } static void test(int a) { m(a); } }",
        "m(a)",
        ResolveStatus::Ambiguous,
        2,
    );
}

#[test]
fn explicit_locals_parentheses_and_array_indexing_select_source_ranges() {
    for ty in ["float", "double"] {
        let other = if ty == "float" { "double" } else { "float" };
        let source = format!(
            "class Calls {{ static {ty} m({ty} a, {ty} b) {{ return a; }} static {other} m({other} a, {other} b) {{ return a; }} static {ty} test({ty}[] array) {{ {ty} best = array[0]; for (int j = 1; j < array.length; j++) {{ best = m((array[j]), (best)); }} return best; }} }}"
        );
        selected(&source, "m((array[j]), (best))", &format!("m({ty} a"));
    }
    for index in ["byte", "short", "char", "int"] {
        let source = format!(
            "class Calls {{ static void m(double x) {{}} static void m(float x) {{}} static void test(double[][] arrays, {index} index) {{ double first, value = arrays[index][index]; m(value); }} }}"
        );
        selected(&source, "m(value)", "m(double x)");
        let direct = format!(
            "class Calls {{ static void m(double x) {{}} static void m(float x) {{}} static void test(double[][] arrays, {index} index) {{ m((arrays[index])[index]); }} }}"
        );
        selected(&direct, "m((arrays[index])[index])", "m(double x)");
    }
    selected(
        "class Calls { static void m(float[] a) {} static void m(double[] a) {} static void test(float values[]) { m((values)); } }",
        "m((values))",
        "m(float[] a)",
    );
    selected(
        "class Calls { static void m(float[] a) {} static void m(float[][] a) {} static void test(float values[][]) { m(values); } }",
        "m(values)",
        "m(float[][] a)",
    );
    selected(
        "class Calls { static void m(float... a) {} static void m(double[] a) {} static void test(float[] values) { m(values); } }",
        "m(values)",
        "m(float... a)",
    );
}

#[test]
fn array_rank_element_invariance_and_index_conversion_are_required() {
    for actual in ["int[]", "float[][]", "float"] {
        let source = format!(
            "class Calls {{ static void m(float[] a) {{}} static void m(double[] a) {{}} static void test({actual} values) {{ m(values); }} }}"
        );
        refused(&source, "m(values)", ResolveStatus::OverloadNotResolved, 2);
    }
    for index in ["long", "float", "double", "boolean", "int[]"] {
        let source = format!(
            "class Calls {{ static void m(float a) {{}} static void m(double a) {{}} static void test(float[] values, {index} i) {{ m(values[i]); }} }}"
        );
        refused(&source, "m(values[i])", ResolveStatus::OverloadNotResolved, 2);
    }
    refused(
        "class Calls { static void m(float... a) {} static void m(double[] a) {} static void test(float value) { m(value); } }",
        "m(value)",
        ResolveStatus::OverloadNotResolved,
        2,
    );
}

#[test]
fn nested_call_argument_uses_a_strictly_proved_return_type() {
    let source = "class Calls { static float m(float a, float b) { return a; } static double m(double a, double b) { return a; } static void test(float a, float b, float c) { m(m(a, b), c); } }";
    selected(source, "m(a, b)", "m(float a");
    selected(source, "m(m(a, b), c)", "m(float a");
}

#[test]
fn receiver_call_does_not_borrow_the_outer_calls_arguments() {
    selected(
        "class Calls { static Calls m(float a) { return null; } static Calls m(double a) { return null; } void other(double d) {} static void test(float f, double d) { m(f).other(d); } }",
        "m(f)",
        "m(float a)",
    );
}

#[test]
fn singleton_unknown_applicability_is_not_a_java_error_or_a_selected_target() {
    for (formal, actual, value) in [
        ("Long x", "long a", "a"),
        ("float... x", "float a", "a, a"),
        ("float... x", "float a", ""),
        ("float x", "float a", "1"),
        ("Object x", "float a", "null"),
    ] {
        let source = format!(
            "class Calls {{ static void m({formal}) {{}} static float give() {{ return 0; }} static void test({actual}) {{ m({value}); }} }}"
        );
        refused(&source, &format!("m({value})"), ResolveStatus::CallNotResolved, 1);
    }
    refused(
        "class Calls { static <T> void m(T a) {} static void test(float a) { m(a); } }",
        "m(a)",
        ResolveStatus::CallNotResolved,
        1,
    );
    selected(
        "class Calls { static void m(float a) {} static float give() { return 0; } static void test() { m(give()); } }",
        "m(give())",
        "m(float a)",
    );
    selected("class Calls { static void m(float a) {} static void test(float a) { m(a); } }", "m(a)", "m(float a)");
    selected("class Calls { static void m() {} static void test() { m(); } }", "m();", "m()");
    // Outside the new closed/static scope, unique names remain legacy navigation.
    let p = project("class Calls { void m(Object a) {} void test(Object a) { m(a); } }");
    assert_eq!(query(&p, "m(a)").status, ResolveStatus::Resolved);
}

#[test]
fn known_singleton_contradictions_do_not_require_an_object_inventory() {
    for source in [
        "class Calls { void m() {} void test() { m(1); } }",
        "class Calls { static void m(byte a) {} static void test(int a) { m(a); } }",
    ] {
        let mut p = project(source);
        p.remove_class("Object.class");
        let needle = if source.contains("m(1)") { "m(1)" } else { "m(a)" };
        let result = query(&p, needle);
        assert_eq!(result.status, ResolveStatus::CallNotResolved, "{result:?}");
        assert_eq!(result.targets.len(), 1);
    }
}

#[test]
fn raw_syntax_and_inapplicable_duplicate_declarations_block_selection() {
    refused(
        "class Calls { static void m(float a) {} static void m(double a) {} static void test(float b) { try(float a=b) { m(a); } } }",
        "m(a)",
        ResolveStatus::OverloadNotResolved,
        2,
    );
    for body in ["m(a[])", "m(a[0])", "m(a,)"] {
        let source = format!(
            "class Calls {{ static void m(float a) {{}} static void m(double a) {{}} static void test(float[] a) {{ {body}; }} }}"
        );
        refused(&source, body, ResolveStatus::OverloadNotResolved, 2);
    }
    for extra in [
        "static static void bad() {}",
        "abstract static void bad() {}",
        "static void bad(float a);",
        "static void bad(float a,) {}",
    ] {
        let source = format!(
            "class Calls {{ static void m(float a) {{}} static void m(double a) {{}} {extra} static void test(float a) {{ m(a); }} }}"
        );
        refused(&source, "m(a)", ResolveStatus::OverloadNotResolved, 2);
    }
    refused(
        "class Calls { static void m(float a) {} static void m(double a) {} static void test(float a,) { m(a); } }",
        "m(a)",
        ResolveStatus::OverloadNotResolved,
        2,
    );
    refused(
        "class Calls { static void m(float a) {} static void m(boolean a) {} static void m(boolean b) {} static void test(float a) { m(a); } }",
        "m(a)",
        ResolveStatus::OverloadNotResolved,
        3,
    );
}

#[test]
fn constructor_records_cannot_become_ordinary_static_methods() {
    let source =
        "class Calls { static Calls(float a) {} static Calls(double a) {} static void test(float a){ Calls(a); } }";
    let result = query(&project(source), "Calls(a)");
    assert_eq!(result.status, ResolveStatus::OverloadNotResolved, "{result:?}");
    assert_eq!(result.targets.len(), 2);
    assert_eq!(result.targets[0].start_byte, source.find("Calls(float a)").unwrap() as u32);
    assert_eq!(result.targets[1].start_byte, source.find("Calls(double a)").unwrap() as u32);
}

#[test]
fn declaration_annotations_cannot_borrow_method_body_variable_types() {
    refused(
        "class Calls { static void m(float a) {} static void m(double a) {} static void test(float a, @Anno(m(a)) int x) {} }",
        "m(a)",
        ResolveStatus::OverloadNotResolved,
        2,
    );
    refused(
        "class Calls { static void m(float a) {} static void m(double a) {} static void test(float a) { @Anno(m(a)) int x; } }",
        "m(a)",
        ResolveStatus::OverloadNotResolved,
        2,
    );
}

#[test]
fn a_known_primitive_contradiction_does_not_depend_on_unknown_other_formals() {
    let source =
        "class Calls { static void m(int x, Object other) {} static void test(boolean a, Object b) { m(a, b); } }";
    let mut p = project(source);
    p.remove_class("Object.class");
    let result = query(&p, "m(a, b)");
    assert_eq!(result.status, ResolveStatus::CallNotResolved, "{result:?}");
    assert_eq!(result.targets.len(), 1);
}

#[test]
fn enhanced_for_variable_is_not_visible_in_its_iterable_expression() {
    let source = "class Calls { static double x; static float[] m(float a){return null;} static float[] m(double a){return null;} static void test(){for(float x: m(x)){} } }";
    let result = query(&project(source), "m(x)");
    assert_eq!(result.status, ResolveStatus::OverloadNotResolved, "{result:?}");
    assert_eq!(result.targets.len(), 2);
    selected(
        "class Calls { static void m(float a) {} static void m(double a) {} static void test(float[] values) { for (float x : values) { m(x); } } }",
        "m(x)",
        "m(float a)",
    );
}

#[test]
fn complete_owner_and_explicit_unambiguous_object_are_required() {
    let source =
        "class Calls { static void m(float a) {} static void m(double a) {} static void test(float a) { m(a); } }";
    let mut p = project(source);
    assert!(p.remove_class("Object.class"));
    assert_eq!(query(&p, "m(a)").status, ResolveStatus::OverloadNotResolved);
    p.add_class_bytes("Object.class", &object(Some("Missing"), None)).unwrap();
    assert_eq!(query(&p, "m(a)").status, ResolveStatus::OverloadNotResolved);
    p.add_class_bytes("Object.class", &object(None, Some("m"))).unwrap();
    let result = query(&p, "m(a)");
    assert_eq!(result.status, ResolveStatus::OverloadNotResolved);
    assert_eq!(result.targets.len(), 3);
    p.add_class_bytes("Object.class", &object(None, None)).unwrap();
    p.add_class_bytes("other/Object.class", &object(None, None)).unwrap();
    assert_eq!(query(&p, "m(a)").status, ResolveStatus::Ambiguous);
    for header in [
        "class Calls extends Missing",
        "class Calls extends java.lang.Object",
        "class Calls implements Missing",
        "class Calls<T>",
        "interface Calls",
        "enum Calls",
        "record Calls()",
    ] {
        let s = source.replacen("class Calls", header, 1);
        assert_ne!(query(&project(&s), "m(a)").status, ResolveStatus::Resolved, "{s}");
    }
    let mut p = project(source);
    p.insert("other.java", source);
    assert_ne!(query(&p, "m(a)").status, ResolveStatus::Resolved);
}

#[test]
fn unsupported_competitors_and_contexts_do_not_supply_proof() {
    for extra in [
        "static void m(Missing x) {}",
        "static <T> void m(T x) {}",
        "void m(long x) {}",
        "static public private void m(long x) {}",
    ] {
        let source = format!(
            "class Calls {{ static void m(float a) {{}} static void m(double a) {{}} {extra} static void test(float a) {{ m(a); }} }}"
        );
        let r = query(&project(&source), "m(a)");
        assert_ne!(r.status, ResolveStatus::Resolved, "{source}\n{r:?}");
    }
    for caller in [
        "void test(float a) { m(a); }",
        "static <T> void test(float a) { m(a); }",
        "static void test(float a) { Runnable r = () -> m(a); }",
        "static void test(float a) { Object o = new Object() { void run() { m(a); } }; }",
        "static class Nested { static void test(float a) { m(a); } }",
        "static void test(float a) { class Local { void run() { m(a); } } }",
    ] {
        let source = format!("class Calls {{ static void m(float a) {{}} static void m(double a) {{}} {caller} }}");
        if caller.contains("class Local") {
            // Local-type internals are an opaque binding boundary.
            let result = query(&project(&source), "m(a)");
            assert_eq!(result.status, ResolveStatus::Unresolved);
            assert!(result.targets.is_empty());
        } else {
            refused(&source, "m(a)", ResolveStatus::OverloadNotResolved, 2);
        }
    }
    for body in ["m(1)", "m(1.0)", "m((float) a)", "m(a + a)", "m(unknown)", "m(null)", "m(a,)"] {
        let source = format!(
            "class Calls {{ static void m(float a) {{}} static void m(double a) {{}} static void test(float a) {{ {body}; }} }}"
        );
        refused(&source, body, ResolveStatus::OverloadNotResolved, 2);
    }
    for bad in [
        "static void broken(void q) {}",
        "static void broken(float q,) {}",
        "static void broken(float... q, int x) {}",
        "static void broken(float... q[]) {}",
        "static void broken(float q) { @; }",
    ] {
        let source = format!(
            "class Calls {{ static void m(float a) {{}} static void m(double a) {{}} {bad} static void test(float a) {{ m(a); }} }}"
        );
        refused(&source, "m(a)", ResolveStatus::OverloadNotResolved, 2);
    }
    refused(
        "class Calls { static void m(float a) {} static void m(double a) {} static void test(float a) { double a = 0; m(a); } }",
        "m(a)",
        ResolveStatus::OverloadNotResolved,
        2,
    );
}

#[test]
fn unique_call_known_arity_and_primitive_contradictions_are_not_resolved() {
    for (formal, actual, value) in [
        ("", "int a", "1"),
        ("int x", "int a", ""),
        ("byte x", "int a", "a"),
        ("int x", "boolean a", "a"),
        ("float[] x", "double[] a", "a"),
    ] {
        let source =
            format!("class Calls {{ static void m({formal}) {{}} static void test({actual}) {{ m({value}); }} }}");
        refused(&source, &format!("m({value})"), ResolveStatus::CallNotResolved, 1);
    }
}

#[test]
fn workspace_platform_proof_stays_in_the_declaration_compilation() {
    use java_engine::{
        ClasspathEntry, CompilationConfig, CompilationId, ProjectConfig, WorkspaceConfig, WorkspaceSnapshot,
    };
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "primitive-calls-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    let source =
        "class Calls { static void m(float a) {} static void m(double a) {} static void test(float a) { m(a); } }";
    for directory in ["lib", "app", "platform/java/lang"] {
        fs::create_dir_all(fixture.0.join(directory)).unwrap();
    }
    fs::write(fixture.0.join("lib/Calls.java"), source).unwrap();
    fs::write(fixture.0.join("app/Other.java"), "class Other {}").unwrap();
    fs::write(fixture.0.join("platform/java/lang/Object.class"), object(None, None)).unwrap();
    let lib = CompilationId::new("lib", "main");
    let app = CompilationId::new("app", "main");
    let mut config = WorkspaceConfig {
        compilations: vec![
            CompilationConfig {
                id: lib.clone(),
                dependencies: vec![],
                inputs: ProjectConfig { source_roots: vec![fixture.0.join("lib")], ..Default::default() },
            },
            CompilationConfig {
                id: app.clone(),
                dependencies: vec![lib.clone()],
                inputs: ProjectConfig {
                    source_roots: vec![fixture.0.join("app")],
                    classpath: vec![ClasspathEntry::Directory(fixture.0.join("platform"))],
                    ..Default::default()
                },
            },
        ],
        ..Default::default()
    };
    let path = fixture.0.join("lib/Calls.java").canonicalize().unwrap().to_str().unwrap().to_owned();
    let at = source.rfind("m(a)").unwrap() as u32;
    let before = WorkspaceSnapshot::load(&config).unwrap();
    for id in [&lib, &app] {
        let result = before.scope(id).unwrap().resolve_at(&path, at);
        assert_eq!(result.status, ResolveStatus::OverloadNotResolved);
        assert_eq!(before.prepare().scope(id).unwrap().resolve_at(&path, at), result);
    }
    config.compilations[0].inputs.classpath.push(ClasspathEntry::Directory(fixture.0.join("platform")));
    let after = WorkspaceSnapshot::load(&config).unwrap();
    for id in [&lib, &app] {
        let result = after.scope(id).unwrap().resolve_at(&path, at);
        assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
        assert_eq!(result.targets[0].start_byte, source.find("m(float a)").unwrap() as u32);
        assert_eq!(after.prepare().scope(id).unwrap().resolve_at(&path, at), result);
        assert_eq!(before.scope(id).unwrap().resolve_at(&path, at).status, ResolveStatus::OverloadNotResolved);
    }
}
