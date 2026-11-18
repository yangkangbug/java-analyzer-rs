use java_engine::{ParsedFile, Project, ResolveStatus};
#[test]
fn compiler_valid_roles_parse_without_errors() {
    for text in [
        "class C { int var; int var(int var){ var: while(var>0){break var;} return this.var+var;} enum E {var} }",
        "class C { java.util.function.Function<String,String> a=var->var,b=(var)->var,c=(var var)->var; }",
        "class C { void f(){var var=1; System.out.println(var);} }",
        "package var.pkg; class C {}",
        "import var.pkg.C; class Use {var.pkg.C value; C other;}",
        "@interface A {int var();} @A(var=1) class C {}",
        "class C {void var(){} Runnable f(){return this::var;}}",
        "class C {void f(Object x){try(java.io.StringReader var=new java.io.StringReader(\"\")){var.read();}catch(Exception var){var.toString();} if(x instanceof String var){var.length();}}}",
    ] {
        let p = ParsedFile::parse(text);
        assert!(
            p.errors().iter().all(|e| e.message == "type pattern flow scope is unsupported"),
            "{text}: {:?}\\n{}",
            p.errors(),
            p.dump()
        );
    }
}
#[test]
fn restricted_type_roles_stay_rejected() {
    for text in [
        "class var {}",
        "class C<var>{}",
        "class C{pkg.var value;}",
        "class C<T>{C<T>.var.X value;}",
        "import static p.var.x; class C{}",
        "import static p.var.*; class C{}",
        "class C{p.@A C.var.D value;}",
        "class C{void f(){try{}catch(p.@A C.var.D e){}}}",
    ] {
        assert!(!ParsedFile::parse(text).errors().is_empty(), "{text}");
    }
}
#[test]
fn ordinary_var_exact_local_navigation() {
    for text in [
        "class C{void f(int var){use(var);}}",
        "class C{void f(){int var=1;use(var);}}",
        "class C{void f(){for(int var=0;var<1;var++){use(var);}}",
        "class C{void f(int[] xs){for(int var:xs){use(var);}}}",
    ] {
        let mut p = Project::new();
        p.insert("C.java", text);
        let at = text.rfind("var").unwrap() as u32;
        let r = p.resolve_at("C.java", at);
        assert_eq!(r.status, ResolveStatus::Resolved, "{text}: {r:?}");
        assert_eq!(r.targets[0].start_byte, text.find("var").unwrap() as u32);
        assert_eq!(r, p.prepare().resolve_at("C.java", at));
    }
}

#[test]
fn inference_marker_and_namespace_roles_do_not_create_type_facts() {
    for text in [
        "class C{void f(){var var=new Missing();var.member();}}",
        "class C{void f(){var var;var.member();}}",
        "class C{void f(){var var=1,x=2;var.member();}}",
    ] {
        let p = ParsedFile::parse(text);
        assert!(p.dump().contains("Var[var]"));
        let mut project = Project::new();
        project.insert("C.java", text);
        assert_ne!(project.resolve_at("C.java", text.find("member").unwrap() as u32).status, ResolveStatus::Resolved);
    }
    for text in [
        "import static p.C.var; class C{}",
        "import p.var.*;class C{}",
        "package p.var;class C{}",
        "class C{void f(){var.pkg.C value;}}",
        "class C{void f(){var.getName();}}",
    ] {
        assert!(ParsedFile::parse(text).errors().is_empty(), "{text}");
    }
}

#[test]
fn contextual_names_edit_and_undo_match_fresh_preserving_snapshot() {
    let first = "class C{int var;void f(int var){use(var);}}";
    let mut p = Project::new();
    p.insert("C.java", first);
    let old = p.clone();
    let old_result = old.resolve_at("C.java", first.rfind("var").unwrap() as u32);
    for next in [
        "class C{int var;void f(int val){use(val);}}",
        "class C{int var;void f(){var var=1;use(var);}}",
        "class C{int var;void f(){var.pkg.C value;use(var);}}",
        "class C{int var;void f(int var){use(var",
        first,
    ] {
        let len = p.file("C.java").unwrap().text().len() as u32;
        p.edit("C.java", java_engine::TextEdit { start_byte: 0, end_byte: len, insert: next.into() }).unwrap();
        let mut fresh = Project::new();
        fresh.insert("C.java", next);
        assert_eq!(p.file("C.java").unwrap().dump(), fresh.file("C.java").unwrap().dump());
        assert_eq!(p.file("C.java").unwrap().root().utf8_len() as usize, next.len());
        let prepared = p.prepare();
        for at in 0..next.len() as u32 {
            assert_eq!(prepared.resolve_at("C.java", at), fresh.resolve_at("C.java", at), "at={at} text={next}");
        }
        assert_eq!(old.resolve_at("C.java", first.rfind("var").unwrap() as u32), old_result);
    }
}

#[test]
fn normalized_var_uses_existing_field_catch_resource_and_lambda_scopes() {
    for text in [
        "class C{int var;void f(){use(this.var);}}",
        "class C{void f(){try{}catch(Exception var){use(var);}}}",
        "class C{void f(){try(R var=open()){use(var);}}}",
        "class C{Object f(){return var->var;}}",
    ] {
        let mut p = Project::new();
        p.insert("C.java", text);
        let at = text.rfind("var").unwrap() as u32;
        let r = p.resolve_at("C.java", at);
        assert_eq!(r.status, ResolveStatus::Resolved, "{text}: {r:?}");
        assert_eq!(r.targets[0].start_byte, text.find("var").unwrap() as u32);
    }
}

#[test]
fn local_byte_edits_reparse_roles_and_preserve_old_snapshot() {
    let first = "class C{void f(){int var=1;use(var);}}";
    let mut p = Project::new();
    p.insert("C.java", first);
    let old = p.clone();
    for (needle, replacement) in [
        ("int var", "int name"),
        ("use(var)", "use(name)"),
        ("int name", "var name"),
        ("var name", "var.pkg.C name"),
        ("var.pkg.C name", "int var"),
        ("use(name)", "use(var)"),
    ] {
        let source = p.file("C.java").unwrap().text();
        let at = source.find(needle).unwrap();
        let mut next = source.to_owned();
        next.replace_range(at..at + needle.len(), replacement);
        p.edit(
            "C.java",
            java_engine::TextEdit {
                start_byte: at as u32,
                end_byte: (at + needle.len()) as u32,
                insert: replacement.into(),
            },
        )
        .unwrap();
        let mut fresh = Project::new();
        fresh.insert("C.java", &next);
        assert_eq!(p.file("C.java").unwrap().dump(), fresh.file("C.java").unwrap().dump());
        let ready = p.prepare();
        for offset in 0..next.len() as u32 {
            assert_eq!(
                ready.resolve_at("C.java", offset),
                fresh.resolve_at("C.java", offset),
                "offset={offset} {next}"
            );
        }
        assert_eq!(old.file("C.java").unwrap().text(), first);
        assert_eq!(old.resolve_at("C.java", first.rfind("var").unwrap() as u32).status, ResolveStatus::Resolved);
    }
    assert_eq!(p.file("C.java").unwrap().text(), first);
}
