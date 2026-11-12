use java_engine::{Project, ResolveResult, ResolveStatus, TargetKind};

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

fn selected(source: &str, expression: &str) {
    let result = query(&project(source), expression);
    assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
    assert_eq!(result.targets.len(), 1);
    let target = &result.targets[0];
    assert_eq!(target.kind, TargetKind::Method);
    assert_eq!(target.owner, "Result");
    let at = source.find("next()").unwrap() as u32;
    assert_eq!((target.start_byte, target.end_byte), (at, at + 4));
}

#[test]
fn one_selected_call_propagates_declared_reference_not_runtime_override() {
    for receiver in ["box.take(1)", "(box.take(1))", "((box)).take((1))"] {
        let source = format!(
            "abstract class Result {{ public void next() {{}} }} class Sub extends Result {{ public void next() {{}} }} class Box {{ public Result take(int i) {{ return null; }} public Result take(Result r) {{ return r; }} }} class Calls {{ void run(Box box) {{ {receiver}.next(); }} }}"
        );
        selected(&source, "next();");
    }
}

#[test]
fn unsupported_call_receivers_never_fall_back_to_same_named_values() {
    for receiver in ["box.missing()", "box.take(null)", "box.take(1).again()", "make()"] {
        let source = format!(
            "class Result {{ public Result again() {{ return null; }} public void next() {{}} }} class Box {{ public Result take(int i) {{ return null; }} }} class Calls {{ Result make() {{ return null; }} void run(Box box, int next) {{ {receiver}.next(); }} }}"
        );
        let result = query(&project(&source), "next();");
        assert_eq!(result.status, ResolveStatus::ReceiverNotBound, "{receiver}: {result:?}");
        assert!(result.targets.is_empty());
    }
}

#[test]
fn wrapped_unsupported_receivers_never_discard_their_call() {
    for receiver in ["((Result) box.take(1))", "(true ? box.take(1) : r)", "(r = box.take(1))", "box.take(1)[0]"] {
        let source = format!(
            "class Result {{ public void next() {{}} }} class Box {{ public Result take(int i) {{ return null; }} }} class Calls {{ void run(Box box, Result r, int next) {{ {receiver}.next(); }} }}"
        );
        let result = query(&project(&source), "next();");
        assert_eq!(result.status, ResolveStatus::ReceiverNotBound, "{receiver}: {result:?}");
    }
}

#[test]
fn unsupported_returns_and_unproven_inner_calls_remain_refused() {
    for method in [
        "public Result[] take(int i) { return null; }",
        "public Result take(int i)[] { return null; }",
        "public Result<Result> take(int i) { return null; }",
        "public Missing take(int i) { return null; }",
        "public int take(int i) { return 0; }",
        "public void take(int i) {}",
        "public static Result take(int i) { return null; }",
        "public <T> Result take(int i) { return null; }",
        "public Result take(byte i) { return null; }",
        "public Result take(int... i) { return null; }",
    ] {
        let source = format!(
            "class Result {{ public void next() {{}} }} class Box {{ {method} }} class Calls {{ void run(Box box, int next) {{ box.take(1).next(); }} }}"
        );
        assert_eq!(
            query(&project_allow_errors(&source), "next();").status,
            ResolveStatus::ReceiverNotBound,
            "{method}"
        );
    }
}

#[test]
fn return_typing_composes_with_inherited_outer_selection_but_not_call_arguments() {
    let source = "class Result { public void next() {} } class Sub extends Result {} class Box { public Sub take(int i) { return null; } public void accept(Result r) {} } class Calls { void run(Box box) { box.take(1).next(); box.accept(box.take(1)); } }";
    let inherited = query(&project(source), "next();");
    assert_eq!(inherited.status, ResolveStatus::Resolved);
    assert_eq!(inherited.targets.len(), 1);
    assert_eq!(inherited.targets[0].owner, "Result");
    assert_eq!(inherited.targets[0].start_byte, source.find("next()").unwrap() as u32);
    assert_ne!(query(&project(source), "accept(box").status, ResolveStatus::Resolved);
    let override_source =
        source.replace("class Sub extends Result {}", "class Sub extends Result { public void next() {} }");
    assert_ne!(query(&project(&override_source), "next();").status, ResolveStatus::Resolved);
}

#[test]
fn return_names_use_declaration_imports_not_callers_same_named_type() {
    let mut p = project(
        "package use; class Result { public void next() {} } class Calls { void run(api.Box box) { box.take(1).next(); } }",
    );
    p.insert(
        "Box.java",
        "package api; import result.Result; public class Box { public Result take(int i) { return null; } }",
    );
    p.insert("Result.java", "package result; public abstract class Result { public void next() {} }");
    let result = query(&p, "next();");
    assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
    assert_eq!(result.targets[0].path, "Result.java");
}

#[test]
fn return_receiver_requires_caller_access_and_non_generic_complete_source_type() {
    for result_decl in [
        "class Result { public void next() {} }",
        "public class Result<T> { public void next() {} }",
        "public class Result extends Missing { public void next() {} }",
    ] {
        let mut p = project("package use; class Calls { void run(api.Box box, int next) { box.take(1).next(); } }");
        p.insert("Box.java", "package api; public class Box { public Result take(int i) { return null; } }");
        p.insert("Result.java", &format!("package api; {result_decl}"));
        assert_eq!(query(&p, "next();").status, ResolveStatus::ReceiverNotBound, "{result_decl}");
    }
}

#[test]
fn inner_complete_competitors_and_explicit_local_scope_are_required() {
    for competitor in [
        "public Result take(Missing x) { return null; }",
        "public <T> Result take(T x) { return null; }",
        "private Result take(long x) { return null; }",
    ] {
        let source = format!(
            "class Result {{ public void next() {{}} }} class Box {{ public Result take(int i) {{ return null; }} {competitor} }} class Calls {{ void run(Box box) {{ box.take(1).next(); }} }}"
        );
        assert_eq!(query(&project(&source), "next();").status, ResolveStatus::ReceiverNotBound);
    }
    for body in [
        "{ Box box = null; } box.take(1).next();",
        "Box box = null; { Box box = null; box.take(1).next(); }",
        "Box.take(1).next();",
        "this.box.take(1).next();",
    ] {
        let source = format!(
            "class Result {{ public void next() {{}} }} class Box {{ public Result take(int i) {{ return null; }} }} class Calls {{ Box box; void run() {{ {body} }} }}"
        );
        assert_eq!(query(&project(&source), "next();").status, ResolveStatus::ReceiverNotBound, "{body}");
    }
}

#[test]
fn method_references_and_deep_chains_remain_refused_without_breaking_inner_queries() {
    let source = "class Result { public Result next() { return null; } } class Box { public Result take(int i) { return null; } } class Calls { void run(Box box, int next) { box.take(1).next().next(); box.take(1)::next; } }";
    let p = project(source);
    assert_eq!(query(&p, "next;").status, ResolveStatus::ReceiverNotBound);
    assert_eq!(query(&p, "next();").status, ResolveStatus::ReceiverNotBound);
    let at = source.find("next().next").unwrap() as u32;
    let result = p.resolve_at("Calls.java", at);
    assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
    assert_eq!(result.targets[0].owner, "Result");
}

#[test]
fn a_call_in_arguments_does_not_activate_receiver_guard() {
    let source = "class Result {} class Box { public Result take(int i) { return null; } public void accept(Result r) {} } class Calls { void run(Box box) { box.accept(box.take(1)); } }";
    let result = query(&project(source), "accept(box");
    assert_eq!(result.status, ResolveStatus::CallNotResolved);
    assert_eq!(result.targets.len(), 1);
    assert_eq!(result.targets[0].name, "accept");
}

#[test]
fn return_receiver_parens_and_shared_budget_are_bounded() {
    for receiver in [
        format!("{}box.take(1){}", "(".repeat(40), ")".repeat(40)),
        format!("{}box{}.take(1)", "(".repeat(40), ")".repeat(40)),
        format!("box.take({})", "9".repeat(5000)),
    ] {
        let source = format!(
            "class Result {{ public void next() {{}} }} class Box {{ public Result take(int i) {{ return null; }} }} class Calls {{ void run(Box box) {{ {receiver}.next(); }} }}"
        );
        assert_eq!(query(&project(&source), "next();").status, ResolveStatus::ReceiverNotBound);
    }
}

#[test]
fn return_header_throws_does_not_replace_actual_return_type() {
    let source = "class Failure {} class Result { public void next() {} } class Box { public Result take(int i) throws Failure { return null; } } class Calls { void run(Box box) { box.take(1).next(); } }";
    selected(source, "next();");
}

#[test]
fn return_proof_retains_declaration_compilation_scope() {
    use java_engine::{
        ClasspathEntry, CompilationConfig, CompilationId, ProjectConfig, WorkspaceConfig, WorkspaceSnapshot,
    };
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "return-scopes-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    )));
    for directory in ["lib", "app", "platform/java/lang"] {
        fs::create_dir_all(fixture.0.join(directory)).unwrap();
    }
    let caller = "package app; class Result {} class Calls { void run(api.Box box) { box.take(1).next(); } }";
    fs::write(fixture.0.join("app/Calls.java"), caller).unwrap();
    fs::write(
        fixture.0.join("lib/Box.java"),
        "package api; import model.Result; public class Box { public Result take(int i) { return null; } }",
    )
    .unwrap();
    fs::write(
        fixture.0.join("lib/Result.java"),
        "package model; public abstract class Result { public void next() {} }",
    )
    .unwrap();
    let mut object = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61, 0, 3, 1, 0, 16];
    object.extend(b"java/lang/Object");
    object.extend([7, 0, 1, 0, 0x21, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    fs::write(fixture.0.join("platform/java/lang/Object.class"), object).unwrap();
    let lib = CompilationId::new("lib", "main");
    let app = CompilationId::new("app", "main");
    let inputs = |name: &str| ProjectConfig {
        source_roots: vec![fixture.0.join(name)],
        classpath: vec![ClasspathEntry::Directory(fixture.0.join("platform"))],
        ..Default::default()
    };
    let snapshot = WorkspaceSnapshot::load(&WorkspaceConfig {
        compilations: vec![
            CompilationConfig { id: lib.clone(), dependencies: vec![], inputs: inputs("lib") },
            CompilationConfig { id: app.clone(), dependencies: vec![lib], inputs: inputs("app") },
        ],
        ..Default::default()
    })
    .unwrap();
    let path = fixture.0.join("app/Calls.java").canonicalize().unwrap().to_str().unwrap().to_owned();
    let at = caller.find("next();").unwrap() as u32;
    let result = snapshot.scope(&app).unwrap().resolve_at(&path, at);
    assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
    assert_eq!(result, snapshot.prepare().scope(&app).unwrap().resolve_at(&path, at));
    assert!(result.targets[0].path.ends_with("lib/Result.java"));
    assert_eq!(result.targets[0].owner, "model.Result");
}

#[test]
fn binary_result_with_call_origin_never_falls_back_to_selector() {
    for receiver in [r#"("x" + box.take(1))"#, "(s += box.take(1))"] {
        let source = format!(
            "class Result {{}} class Box {{ public Result take(int i) {{ return null; }} }} class Calls {{ void run(Box box, String s, int next) {{ {receiver}.next(); }} }}"
        );
        assert_eq!(query(&project(&source), "next();").status, ResolveStatus::ReceiverNotBound, "{receiver}");
    }
}

#[test]
fn prepared_refresh_tracks_return_declaration_and_utf8_ranges() {
    let mut p = project("class Calls { void run(Box box) { box.take(1).next(); } }");
    for prefix in ["", "// 中😀\r\n"] {
        let result = format!("{prefix}class Result {{ public void next() {{}} }}");
        p.insert("Result.java", &result);
        p.insert("Box.java", "class Box { public Result take(int i) { return null; } }");
        let good = query(&p, "next();");
        assert_eq!(good.status, ResolveStatus::Resolved);
        let at = result.find("next()").unwrap() as u32;
        assert_eq!((good.targets[0].start_byte, good.targets[0].end_byte), (at, at + 4));
        p.insert("Box.java", "class Box { public Result[] take(int i) { return null; } }");
        assert_eq!(query(&p, "next();").status, ResolveStatus::ReceiverNotBound);
    }
}

#[test]
fn unary_call_result_cannot_become_a_simple_selector() {
    for receiver in ["(-box.take(1))", "(!box.take(1))", "(~box.take(1))", "(++box.take(1))", "(box.take(1)++)"] {
        let source = format!(
            "class Box {{ public int take(int i) {{ return 0; }} }} class Calls {{ void run(Box box, int next) {{ {receiver}.next(); }} }}"
        );
        assert_eq!(query(&project(&source), "next();").status, ResolveStatus::ReceiverNotBound, "{receiver}");
    }
}

#[test]
fn compound_assignment_result_tracks_call_in_left_value_origin() {
    let source = r#"class Result { String text; } class Box { public Result take(int i) { return null; } } class Calls { void run(Box box, int next) { (box.take(1).text += "x").next(); } }"#;
    assert_eq!(query(&project(source), "next();").status, ResolveStatus::ReceiverNotBound);
}
