//! Invocation names use the method namespace, while values still use lexical scopes.
use java_engine::{Project, ResolveResult, ResolveStatus, TargetKind};

fn project(text: &str) -> Project {
    let mut p = Project::new();
    p.insert("Use.java", text);
    assert!(p.file("Use.java").unwrap().errors().is_empty(), "{:?}", p.file("Use.java").unwrap().errors());
    p
}

fn query(p: &Project, marker: &str) -> ResolveResult {
    let text = p.file("Use.java").unwrap().text();
    let marker = format!("/*{marker}*/");
    let byte = (text.find(&marker).unwrap() + marker.len()) as u32;
    let result = p.resolve_at("Use.java", byte);
    assert_eq!(result, p.prepare().resolve_at("Use.java", byte), "{marker}");
    result
}

fn target(p: &Project, marker: &str, kind: TargetKind, declaration: &str) {
    let result = query(p, marker);
    assert_eq!(result.status, ResolveStatus::Resolved, "{marker}: {result:?}");
    assert_eq!(result.targets.len(), 1);
    assert_eq!(result.targets[0].kind, kind, "{marker}: {result:?}");
    assert_eq!(result.targets[0].path, "Use.java");
    assert_eq!(
        result.targets[0].start_byte,
        p.file("Use.java").unwrap().text().find(declaration).unwrap() as u32,
        "{marker}: {result:?}"
    );
}

#[test]
fn locals_parameters_and_nested_scopes_do_not_capture_calls() {
    let p = project(
        r#"class Use {
        void m(int x) {}
        void test(int m) {
            /*parameter_call*/m(/*parameter_value*/m);
            /*after_call*/m(/*after_value*/m);
        }
        void localTest() { { int m = 1; /*local_call*/m(/*local_value*/m); } }
    }"#,
    );
    for marker in ["parameter_call", "local_call", "after_call"] {
        target(&p, marker, TargetKind::Method, "m(int x)");
    }
    for marker in ["parameter_value", "after_value"] {
        target(&p, marker, TargetKind::Parameter, "m) {");
    }
    target(&p, "local_value", TargetKind::Local, "m = 1");
}

#[test]
fn catch_resource_and_lambda_names_do_not_capture_calls() {
    let p = project(
        r#"class Use {
        void m(Object x) {}
        void test() {
            try (Resource m = open()) { /*resource_call*/m(/*resource_value*/m); }
            catch (Exception m) { /*catch_call*/m(/*catch_value*/m); }
            Object f = m -> { /*lambda_call*/m(/*lambda_value*/m); };
            Object g = (Object m) -> { /*typed_call*/m(/*typed_value*/m); };
        }
    }"#,
    );
    for marker in ["resource_call", "catch_call", "lambda_call", "typed_call"] {
        target(&p, marker, TargetKind::Method, "m(Object x)");
    }
    target(&p, "resource_value", TargetKind::Resource, "m = open()");
    target(&p, "catch_value", TargetKind::Parameter, "m) { /*catch");
    target(&p, "lambda_value", TargetKind::Parameter, "m ->");
    target(&p, "typed_value", TargetKind::Parameter, "m) ->");
}

#[test]
fn fields_types_receivers_and_arguments_keep_their_namespaces() {
    let p = project(
        r#"class Use {
        Use m; Use box; int leaf;
        void m(int x) {} void leaf() {} void accept(int x) {}
        class TypeOnly {}
        void test(Use receiver, int index, int[] array) {
            /*call*/m(1); Object x = /*field*/m;
            this./*this_call*/m(1); Object y = this./*this_field*/m;
            /*receiver*/receiver./*qualified_call*/m(1);
            this./*chain_prefix*/box./*chain_call*/m(1);
            accept(/*argument_receiver*/receiver./*argument_field*/leaf);
            accept(/*array*/array[/*index*/index]);
            /*missing_type*/TypeOnly();
            /*missing_field*/box();
        }
    }"#,
    );
    for marker in ["call", "this_call", "qualified_call", "chain_call"] {
        target(&p, marker, TargetKind::Method, "m(int x)");
    }
    for marker in ["field", "this_field"] {
        target(&p, marker, TargetKind::Field, "m; Use");
    }
    target(&p, "chain_prefix", TargetKind::Field, "box; int");
    for marker in ["receiver", "argument_receiver"] {
        target(&p, marker, TargetKind::Parameter, "receiver, int");
    }
    target(&p, "argument_field", TargetKind::Field, "leaf;");
    target(&p, "array", TargetKind::Parameter, "array)");
    target(&p, "index", TargetKind::Parameter, "index, int");
    for marker in ["missing_type", "missing_field"] {
        let r = query(&p, marker);
        assert_eq!(r.status, ResolveStatus::Unresolved, "{marker}: {r:?}");
        assert!(r.targets.is_empty());
    }
}

#[test]
fn no_method_and_static_import_do_not_fall_back_to_values() {
    let p = project(
        r#"import static lib.Tools.imported;
        class Use { void test(int missing, int imported) {
            /*missing*/missing(); /*imported*/imported();
            int x = /*value*/missing;
        } }"#,
    );
    for (marker, status) in
        [("missing", ResolveStatus::Unresolved), ("imported", ResolveStatus::StaticImportNotApplied)]
    {
        let r = query(&p, marker);
        assert_eq!(r.status, status, "{marker}: {r:?}");
        assert!(r.targets.is_empty());
    }
    target(&p, "value", TargetKind::Parameter, "missing, int");
}

#[test]
fn inherited_methods_and_overload_refusals_survive_value_collisions() {
    let p = project(
        r#"class Base { void inherited() {} void overloaded() {} }
        class Use extends Base {
            void overloaded(int x) {}
            void test(int inherited, int overloaded) { /*inherited*/inherited(); /*overloaded*/overloaded(1); }
        }"#,
    );
    target(&p, "inherited", TargetKind::Method, "inherited()");
    let r = query(&p, "overloaded");
    assert_eq!(r.status, ResolveStatus::OverloadNotResolved);
    assert_eq!(r.targets.len(), 2);
    assert!(r.targets.iter().all(|t| t.kind == TargetKind::Method));
}

#[test]
fn classfile_methods_and_mixed_signature_overrides_survive_collisions() {
    let mut p = project("class Use extends lib.Lib { void test(int run) { /*call*/run(); int x = /*value*/run; } }");
    p.add_class_bytes("Lib.class", include_bytes!("fixtures/classes/Lib.class")).unwrap();
    let r = query(&p, "call");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets[0].kind, TargetKind::Method);
    assert_eq!(r.targets[0].owner, "lib.Lib");
    assert_eq!(r.targets[0].path, "Lib.class");
    target(&p, "value", TargetKind::Parameter, "run) {");
    p.insert("Use.java", "class Use extends lib.Lib { void run() {} void test(int run) { /*call*/run(); } }");
    let r = query(&p, "call");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].owner, "Use");
    assert!(r.targets.iter().all(|t| t.kind == TargetKind::Method));
}

#[test]
fn adjacent_method_references_do_not_change_invocation_lookup() {
    // Ordinary method-reference selector binding is a separate unsupported
    // boundary: this invocation change deliberately leaves it alone.
    let p = project(
        r#"class Use {
        void m() {}
        void test(Use m) {
            Object ref = m::m;
            Object ctor = Use::/*constructor*/new;
            /*call*/m();
        }
    }"#,
    );
    target(&p, "call", TargetKind::Method, "m()");
    assert_eq!(query(&p, "constructor").status, ResolveStatus::ConstructorRefNotBound);
}

#[test]
fn workspace_dependency_methods_use_method_namespace_in_both_query_paths() {
    use java_engine::{CompilationConfig, CompilationId, ProjectConfig, WorkspaceConfig, WorkspaceSnapshot};
    let root = std::env::temp_dir().join(format!("java-engine-call-namespace-{}", std::process::id()));
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let base = "package lib; public class Base { public void m() {} }";
    let text =
        "package app; public class Use extends lib.Base { void test(int m) { /*call*/m(); int x = /*value*/m; } }";
    for (path, source) in [("lib/Base.java", base), ("app/Use.java", text)] {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    let lib = CompilationId::new("lib", "main");
    let app = CompilationId::new("app", "main");
    let workspace = WorkspaceSnapshot::load(&WorkspaceConfig {
        compilations: vec![
            CompilationConfig {
                id: lib.clone(),
                inputs: ProjectConfig { source_roots: vec![root.join("lib")], ..Default::default() },
                dependencies: vec![],
            },
            CompilationConfig {
                id: app.clone(),
                inputs: ProjectConfig { source_roots: vec![root.join("app")], ..Default::default() },
                dependencies: vec![lib],
            },
        ],
        ..Default::default()
    })
    .unwrap();
    let path = root.join("app/Use.java").canonicalize().unwrap();
    let base_path = root.join("lib/Base.java").canonicalize().unwrap();
    let prepared = workspace.prepare();
    for (marker, kind, expected_path, declaration, source) in [
        ("/*call*/", TargetKind::Method, base_path.to_str().unwrap(), "m()", base),
        ("/*value*/", TargetKind::Parameter, path.to_str().unwrap(), "m) {", text),
    ] {
        let byte = (text.find(marker).unwrap() + marker.len()) as u32;
        let r = workspace.scope(&app).unwrap().resolve_at(path.to_str().unwrap(), byte);
        assert_eq!(r, prepared.scope(&app).unwrap().resolve_at(path.to_str().unwrap(), byte));
        assert_eq!(r.status, ResolveStatus::Resolved);
        assert_eq!(r.targets.len(), 1);
        assert_eq!(r.targets[0].kind, kind);
        assert_eq!(r.targets[0].path, expected_path);
        assert_eq!(r.targets[0].start_byte, source.find(declaration).unwrap() as u32);
    }
}
