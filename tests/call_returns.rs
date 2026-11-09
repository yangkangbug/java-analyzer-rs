use java_engine::{Project, ResolveResult, ResolveStatus, TargetKind};

// Minimal root class signature assembled in Rust, not by javac.
fn object(method: &str) -> Vec<u8> {
    fn u(out: &mut Vec<u8>, value: u16) {
        out.extend(value.to_be_bytes());
    }
    fn utf(out: &mut Vec<u8>, value: &str) {
        out.push(1);
        u(out, value.len() as u16);
        out.extend(value.as_bytes());
    }
    let mut out = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61];
    u(&mut out, 5);
    utf(&mut out, "java/lang/Object");
    out.push(7);
    u(&mut out, 1);
    utf(&mut out, method);
    utf(&mut out, "()F");
    for value in [0x21, 2, 0, 0, 0, 1, 0x109, 3, 4, 0, 0] {
        u(&mut out, value);
    }
    out
}

fn project(source: &str) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", source);
    p.add_class_bytes("Object.class", &object("unrelated")).unwrap();
    p
}

fn query(p: &Project, needle: &str) -> ResolveResult {
    let source = p.file("Calls.java").unwrap().text();
    let at = source.rfind(needle).unwrap() as u32;
    let result = p.resolve_at("Calls.java", at);
    assert_eq!(result, p.prepare().resolve_at("Calls.java", at));
    result
}

fn selected(p: &Project, needle: &str, declaration: &str) {
    let result = query(p, needle);
    assert_eq!(result.status, ResolveStatus::Resolved, "{needle}: {result:?}");
    assert_eq!(result.targets.len(), 1, "{result:?}");
    let target = &result.targets[0];
    assert_eq!(target.path, "Calls.java");
    assert_eq!(target.kind, TargetKind::Method);
    assert_eq!(target.owner, "Calls");
    let start = p.file("Calls.java").unwrap().text().find(declaration).unwrap() as u32;
    assert_eq!((target.start_byte, target.end_byte), (start, start + target.name.len() as u32));
}

fn refused(p: &Project, needle: &str, status: ResolveStatus, declarations: &[&str]) {
    let result = query(p, needle);
    assert_eq!(result.status, status, "{needle}: {result:?}");
    let source = p.file("Calls.java").unwrap().text();
    assert_eq!(result.targets.len(), declarations.len(), "{result:?}");
    for (target, declaration) in result.targets.iter().zip(declarations) {
        assert_eq!(target.path, "Calls.java");
        assert_eq!(target.kind, TargetKind::Method);
        assert_eq!(target.owner, "Calls");
        let start = source.find(declaration).unwrap() as u32;
        assert_eq!((target.start_byte, target.end_byte), (start, start + target.name.len() as u32));
    }
}

#[test]
fn proven_nested_primitive_calls_select_exact_overloads() {
    let p = project(
        "class Calls { static float max(float a, float b) { return a; } static double max(double a, double b) { return a; } static void test(float a, float b, float c) { max(max(a, b), c); } }",
    );
    selected(&p, "max(a, b)", "max(float a");
    selected(&p, "max(max(a, b), c)", "max(float a");
}

#[test]
fn selected_declarations_explicit_return_type_drives_outer_widening() {
    let p = project(
        "class Calls { static double inner(float a) { return a; } static float inner(double a) { return 0; } static void outer(float a) {} static void outer(double a) {} static void test(float a) { outer(inner(a)); } }",
    );
    selected(&p, "inner(a)", "inner(float a");
    selected(&p, "outer(inner(a))", "outer(double a");
}

#[test]
fn legacy_resolved_inner_is_not_a_proof_of_its_return_type() {
    let p = project(
        "class Calls { float inner(float a) { return a; } static void outer(float a) {} static void outer(double a) {} static void test(float a) { outer(inner(a)); } }",
    );
    refused(&p, "inner(a)", ResolveStatus::CallNotResolved, &["inner(float a"]);
    refused(&p, "outer(inner(a))", ResolveStatus::OverloadNotResolved, &["outer(float a", "outer(double a"]);
    let mut without_object = p;
    without_object.remove_class("Object.class");
    selected(&without_object, "inner(a)", "inner(float a");
    refused(
        &without_object,
        "outer(inner(a))",
        ResolveStatus::OverloadNotResolved,
        &["outer(float a", "outer(double a"],
    );
}

#[test]
fn scalar_results_cover_all_primitives_and_zero_argument_calls() {
    for primitive in ["boolean", "byte", "short", "char", "int", "long", "float", "double"] {
        let p = project(&format!(
            "class Calls {{ static native {primitive} inner(); static void outer({primitive} a) {{}} static void outer() {{}} static void test() {{ outer(inner()); }} }}"
        ));
        selected(&p, "inner()", "inner()");
        selected(&p, "outer(inner())", &format!("outer({primitive} a"));
    }
}

#[test]
fn widening_is_applied_at_each_call_without_target_typing() {
    for methods in [
        "static byte inner(long a) { return 0; } static double inner(float a) { return a; }",
        "static double inner(float a) { return a; } static byte inner(long a) { return 0; }",
    ] {
        let p = project(&format!(
            "class Calls {{ {methods} static void outer(short a) {{}} static void outer(double a) {{}} static void test(int a) {{ outer(inner(a)); }} }}"
        ));
        selected(&p, "inner(a)", "inner(long a");
        selected(&p, "outer(inner(a))", "outer(short a");
    }
    let p = project(
        "class Calls { static double inner(float a) { return a; } static float inner(double a) { return 0; } static void outer(float a) {} static void test(float a) { outer(inner(a)); } }",
    );
    selected(&p, "inner(a)", "inner(float a");
    refused(&p, "outer(inner(a))", ResolveStatus::CallNotResolved, &["outer(float a"]);
}

#[test]
fn nested_results_compose_with_locals_parentheses_and_array_indices() {
    let p = project(
        "class Calls { static int index(int i) { return i; } static float inner(float a) { return a; } static double inner(double a) { return a; } static void outer(float a) {} static void outer(double a) {} static void test(float[][] values, int i) { float a = values[i][i]; outer((inner(inner((values[index(i)])[index(i)])))); outer(inner(a)); } }",
    );
    selected(&p, "outer((inner(inner((values[index(i)])[index(i)]))))", "outer(float a");
    selected(&p, "outer(inner(a))", "outer(float a");
    let bad = project(
        "class Calls { static long index(int i) { return i; } static void outer(float a) {} static void outer(double a) {} static void test(float[] values, int i) { outer(values[index(i)]); } }",
    );
    refused(&bad, "outer(values[index(i)])", ResolveStatus::OverloadNotResolved, &["outer(float a", "outer(double a"]);
}

#[test]
fn only_complete_unannotated_scalar_return_headers_supply_values() {
    for declaration in [
        "static void inner() {}",
        "static float[] inner() { return null; }",
        "static float inner()[] { return null; }",
        "static float[][] inner() { return null; }",
        "static void[] inner() { return null; }",
        "static ? inner() { return null; }",
        "static Object inner() { return null; }",
        "static Float inner() { return null; }",
        "static <T> float inner() { return 0; }",
        "@Anno static float inner() { return 0; }",
        "static @Anno float inner() { return 0; }",
        "static float @Anno [] inner() { return null; }",
        "static float inner() @Anno [] { return null; }",
        "static float inner() [ { return 0; }",
    ] {
        let p = project(&format!(
            "class Calls {{ {declaration} static void outer(float a) {{}} static void outer(double a) {{}} static void test() {{ outer(inner()); }} }}"
        ));
        refused(&p, "outer(inner())", ResolveStatus::OverloadNotResolved, &["outer(float a", "outer(double a"]);
    }
    let p = project(
        "class Calls { static float inner(float a) throws Exception { return a; } static void outer(float a) {} static void outer(double a) {} static void test(float a) { outer(inner(a)); } }",
    );
    selected(&p, "outer(inner(a))", "outer(float a");
    let p = project(
        "class Calls { static float[] inner() { return null; } static void outer(float a) {} static void outer(double a) {} static void test(int i) { outer(inner()[i]); } }",
    );
    refused(&p, "outer(inner()[i])", ResolveStatus::OverloadNotResolved, &["outer(float a", "outer(double a"]);
}

#[test]
fn inner_applicability_unknowns_and_competitors_are_not_discarded() {
    for (inner, call) in [
        ("static float inner(float a) { return a; }", "inner()"),
        ("static float inner(float a) { return a; }", "inner(1)"),
        ("static float inner(float a) { return a; }", "inner(field)"),
        ("static float inner(float a) { return a; }", "inner(missing)"),
        ("static float inner(float a) { return a; }", "missing(a)"),
        ("static float inner(float a) { return a; }", "inner(a, a)"),
        ("static float inner(boolean a) { return 0; }", "inner(a)"),
        ("static float inner(Float a) { return 0; }", "inner(a)"),
        ("static float inner(float... a) { return 0; }", "inner(a)"),
        ("static float inner(float a) { return a; } static float inner(Missing a) { return 0; }", "inner(a)"),
        ("static float inner(float a) { return a; } static <T> float inner(T a) { return 0; }", "inner(a)"),
        ("static float inner(float a) { return a; } float inner(long a) { return 0; }", "inner(a)"),
        (
            "static float inner(float a) { return a; } static float inner(boolean a) { return 0; } static double inner(boolean a) { return 0; }",
            "inner(a)",
        ),
    ] {
        let call = format!("outer({call})");
        let p = project(&format!(
            "class Calls {{ static float field; {inner} static void outer(float a) {{}} static void outer(double a) {{}} static void test(float a) {{ {call}; }} }}"
        ));
        refused(&p, &call, ResolveStatus::OverloadNotResolved, &["outer(float a", "outer(double a"]);
    }
}

#[test]
fn return_types_cannot_break_signature_or_specificity_ties() {
    let p = project(
        "class Calls { static float inner(int a) { return a; } static double inner(int b) { return b; } static void outer(float a) {} static void outer(double a) {} static void test(int a) { outer(inner(a)); } }",
    );
    refused(&p, "inner(a)", ResolveStatus::Ambiguous, &["inner(int a", "inner(int b"]);
    refused(&p, "outer(inner(a))", ResolveStatus::OverloadNotResolved, &["outer(float a", "outer(double a"]);
    let p = project(
        "class Calls { static float inner(int a, long b) { return a; } static double inner(long a, int b) { return b; } static void outer(float a) {} static void outer(double a) {} static void test(int a, int b) { outer(inner(a, b)); } }",
    );
    refused(&p, "inner(a, b)", ResolveStatus::OverloadNotResolved, &["inner(int a", "inner(long a"]);
    refused(&p, "outer(inner(a, b))", ResolveStatus::OverloadNotResolved, &["outer(float a", "outer(double a"]);
}

#[test]
fn calls_on_qualified_receivers_and_constructors_are_not_result_proofs() {
    for expression in [
        "Calls.inner(a)",
        "other.inner(a)",
        "this.inner(a)",
        "super.inner(a)",
        "(inner)(a)",
        "new Calls(a)",
        "Calls(a)",
    ] {
        let call = format!("outer({expression})");
        let p = project(&format!(
            "class Calls {{ Calls(float a) {{}} static float inner(float a) {{ return a; }} static void outer(float a) {{}} static void outer(double a) {{}} static void test(float a, Calls other) {{ {call}; }} }}"
        ));
        refused(&p, &call, ResolveStatus::OverloadNotResolved, &["outer(float a", "outer(double a"]);
    }
    let p = project(
        "class Parent { static float inner(float a) { return a; } } class Calls extends Parent { static void outer(float a) {} static void outer(double a) {} static void test(float a) { outer(inner(a)); } }",
    );
    refused(&p, "outer(inner(a))", ResolveStatus::OverloadNotResolved, &["outer(float a", "outer(double a"]);
    let p = project(
        "import static Other.inner; class Other { static float inner() { return 0; } } class Calls { static void outer(float a) {} static void outer(double a) {} static void test() { outer(inner()); } }",
    );
    refused(&p, "outer(inner())", ResolveStatus::OverloadNotResolved, &["outer(float a", "outer(double a"]);
}

#[test]
fn each_nested_name_needs_its_own_closed_object_inventory() {
    let source = "class Calls { static float inner() { return 0; } static void outer(float a) {} static void outer(double a) {} static void test() { outer(inner()); } }";
    let mut p = project(source);
    selected(&p, "outer(inner())", "outer(float a");
    p.add_class_bytes("Object.class", &object("inner")).unwrap();
    refused(&p, "outer(inner())", ResolveStatus::OverloadNotResolved, &["outer(float a", "outer(double a"]);
    p.insert("Calls.java", &source.replace("static float inner() { return 0; }", ""));
    let inner = query(&p, "inner()");
    assert_eq!(inner.status, ResolveStatus::Resolved);
    assert_eq!(inner.targets[0].path, "Object.class");
    assert_eq!(inner.targets[0].start_byte, 0);
    assert_eq!(inner.targets[0].end_byte, 0);
    refused(&p, "outer(inner())", ResolveStatus::OverloadNotResolved, &["outer(float a", "outer(double a"]);
}

#[test]
fn nested_expression_proof_respects_scope_boundaries() {
    for caller in [
        "void test(float a) { outer(inner(a)); }",
        "static <T> void test(float a) { outer(inner(a)); }",
        "static void test(float a) { Runnable r = () -> outer(inner(a)); }",
        "static void test(float a) { Object r = new Object() { void run() { outer(inner(a)); } }; }",
        "static class Nested { static void test(float a) { outer(inner(a)); } }",
        "static void test(float a) { class Local { void run() { outer(inner(a)); } } }",
        "static void test(float a, @Anno(outer(inner(a))) int b) {}",
        "static void test(float a) { @Anno(outer(inner(a))) int b; }",
        "static void test(float a) { try(float b = a) { outer(inner(b)); } }",
        "static void test(float a) { for(float b : outer(inner(b))) {} }",
    ] {
        let call = if caller.contains("inner(b)") { "outer(inner(b))" } else { "outer(inner(a))" };
        let p = project(&format!(
            "class Calls {{ static float inner(float a) {{ return a; }} static void outer(float a) {{}} static void outer(double a) {{}} {caller} }}"
        ));
        if caller.contains("class Local") {
            // Opaque local-type bodies cannot borrow enclosing method candidates.
            refused(&p, call, ResolveStatus::Unresolved, &[]);
        } else {
            refused(&p, call, ResolveStatus::OverloadNotResolved, &["outer(float a", "outer(double a"]);
        }
    }
    let p = project(
        "class Calls { static float inner(float a) { return a; } static void outer(float a) {} static void outer(double a) {} static void test(float[] values) { for(float a : values) { outer(inner(a)); } } }",
    );
    selected(&p, "outer(inner(a))", "outer(float a");
}

fn nested(depth: usize) -> String {
    format!("{}a{}", "inner(".repeat(depth), ")".repeat(depth))
}

#[test]
fn depth_and_total_work_exhaustion_refuse_without_singleton_fallback() {
    for depth in [1, 8, 31] {
        let call = format!("outer({})", nested(depth));
        let p = project(&format!(
            "class Calls {{ static float inner(float a) {{ return a; }} static void outer(float a) {{}} static void test(float a) {{ {call}; }} }}"
        ));
        selected(&p, &call, "outer(float a");
    }
    for depth in [32, 40] {
        let call = format!("outer({})", nested(depth));
        let p = project(&format!(
            "class Calls {{ static float inner(float a) {{ return a; }} static void outer(float a) {{}} static void test(float a) {{ {call}; }} }}"
        ));
        assert!(p.file("Calls.java").unwrap().errors().is_empty());
        refused(&p, &call, ResolveStatus::CallNotResolved, &["outer(float a"]);
        selected(&p, "inner(a)", "inner(float a");
    }
    let count = 64;
    let parameters = (0..count).map(|i| format!("float a{i}")).collect::<Vec<_>>().join(", ");
    let call = format!("outer({})", vec![nested(8); count].join(", "));
    let p = project(&format!(
        "class Calls {{ static float inner(float a) {{ return a; }} static void outer({parameters}) {{}} static void test(float a) {{ {call}; }} }}"
    ));
    assert!(p.file("Calls.java").unwrap().errors().is_empty());
    refused(&p, &call, ResolveStatus::CallNotResolved, &["outer(float a0"]);
    selected(&p, "inner(a)", "inner(float a");
}

#[test]
fn source_edits_change_result_types_and_removal_drops_the_proof() {
    use java_engine::TextEdit;
    let source = "class Calls { static float inner(float a) { return a; } static void outer(float a) {} static void outer(double a) {} static void test(float a) { outer(inner(a)); } }";
    let mut p = project(source);
    selected(&p, "outer(inner(a))", "outer(float a");
    let at = source.find("float inner").unwrap() as u32;
    p.edit("Calls.java", TextEdit { start_byte: at, end_byte: at + 5, insert: "double".into() }).unwrap();
    selected(&p, "outer(inner(a))", "outer(double a");
    let source = p.file("Calls.java").unwrap().text();
    let declaration = "static double inner(float a) { return a; }";
    let at = source.find(declaration).unwrap() as u32;
    p.edit("Calls.java", TextEdit { start_byte: at, end_byte: at + declaration.len() as u32, insert: String::new() })
        .unwrap();
    refused(&p, "outer(inner(a))", ResolveStatus::OverloadNotResolved, &["outer(float a", "outer(double a"]);
}

#[test]
fn malformed_throws_headers_never_supply_scalar_results() {
    for throws in ["int", "void", "?", "int[]", "Exception[]", "E<int>", "E<String>", "@Anno Exception"] {
        let p = project(&format!(
            "class Calls {{ static float inner(float a) throws {throws} {{ return a; }} static void outer(float a) {{}} static void outer(double a) {{}} static void test(float a) {{ outer(inner(a)); }} }}"
        ));
        refused(&p, "outer(inner(a))", ResolveStatus::OverloadNotResolved, &["outer(float a", "outer(double a"]);
    }
    let p = project(
        "class Calls { static float inner(float a) throws java.io.IOException, java.lang.Exception { return a; } static void outer(float a) {} static void outer(double a) {} static void test(float a) { outer(inner(a)); } }",
    );
    selected(&p, "outer(inner(a))", "outer(float a");
}

#[test]
fn reloaded_return_types_and_object_proofs_stay_in_the_owning_snapshot() {
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
        "call-returns-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    for directory in ["lib", "app", "platform/java/lang"] {
        fs::create_dir_all(fixture.0.join(directory)).unwrap();
    }
    let source = "class Calls { static float inner(float a) { return a; } static void outer(float a) {} static void outer(double a) {} static void test(float a) { outer(inner(a)); } }";
    let source_path = fixture.0.join("lib/Calls.java");
    fs::write(&source_path, source).unwrap();
    fs::write(fixture.0.join("app/Other.java"), "class Other {}").unwrap();
    fs::write(fixture.0.join("platform/java/lang/Object.class"), object("unrelated")).unwrap();
    let path = source_path.canonicalize().unwrap().to_str().unwrap().to_owned();
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
    let check = |snapshot: &WorkspaceSnapshot, source: &str, declaration: Option<&str>| {
        let at = source.rfind("outer(inner(a))").unwrap() as u32;
        for id in [&lib, &app] {
            let result = snapshot.scope(id).unwrap().resolve_at(&path, at);
            assert_eq!(result, snapshot.prepare().scope(id).unwrap().resolve_at(&path, at));
            if let Some(declaration) = declaration {
                assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
                assert_eq!(result.targets.len(), 1);
                assert_eq!(result.targets[0].path, path);
                assert_eq!(result.targets[0].owner, "Calls");
                assert_eq!(result.targets[0].kind, TargetKind::Method);
                let start = source.find(declaration).unwrap() as u32;
                assert_eq!((result.targets[0].start_byte, result.targets[0].end_byte), (start, start + 5));
            } else {
                assert_eq!(result.status, ResolveStatus::OverloadNotResolved, "{result:?}");
                assert_eq!(result.targets.len(), 2);
                for (target, declaration) in result.targets.iter().zip(["outer(float a", "outer(double a"]) {
                    assert_eq!(target.path, path);
                    let start = source.find(declaration).unwrap() as u32;
                    assert_eq!((target.start_byte, target.end_byte), (start, start + 5));
                }
            }
        }
    };
    let wrong_platform = WorkspaceSnapshot::load(&config).unwrap();
    check(&wrong_platform, source, None);
    config.compilations[0].inputs.classpath.push(ClasspathEntry::Directory(fixture.0.join("platform")));
    let old = WorkspaceSnapshot::load(&config).unwrap();
    check(&old, source, Some("outer(float a"));
    let changed = source.replace("float inner", "double inner");
    fs::write(&source_path, &changed).unwrap();
    let current = WorkspaceSnapshot::load(&config).unwrap();
    check(&current, &changed, Some("outer(double a"));
    let removed = changed.replace("static double inner(float a) { return a; }", "");
    fs::write(&source_path, &removed).unwrap();
    let latest = WorkspaceSnapshot::load(&config).unwrap();
    check(&latest, &removed, None);
    check(&current, &changed, Some("outer(double a"));
    check(&old, source, Some("outer(float a"));
    check(&wrong_platform, source, None);
}

#[test]
fn negative_guard_budget_exhaustion_cannot_restore_legacy_singleton_success() {
    let deeply_parenthesized = format!("{}a{}", "(".repeat(40), ")".repeat(40));
    let many_locals = (0..4200).map(|i| format!("int x{i};")).collect::<String>();
    for (expression, locals) in [(deeply_parenthesized.as_str(), ""), ("a", many_locals.as_str())] {
        let call = format!("outer({expression})");
        let mut p = project(&format!(
            "class Calls {{ static void outer(byte a) {{}} static void test(int a) {{ {locals} {call}; }} }}"
        ));
        p.remove_class("Object.class");
        assert!(p.file("Calls.java").unwrap().errors().is_empty());
        refused(&p, &call, ResolveStatus::CallNotResolved, &["outer(byte a"]);
    }
}
