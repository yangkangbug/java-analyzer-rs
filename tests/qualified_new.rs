use java_engine::{ParsedFile, Project, ResolveStatus, TargetKind, TextEdit};
fn p(text: &str) -> Project {
    let mut p = Project::new();
    p.insert("C.java", text);
    p
}
fn query(p: &Project, marker: &str) -> java_engine::ResolveResult {
    let at = p.file("C.java").unwrap().text().rfind(marker).unwrap() as u32;
    let r = p.resolve_at("C.java", at);
    assert_eq!(r, p.prepare().resolve_at("C.java", at));
    r
}
#[test]
fn legal_qualified_creation_preserves_text_and_has_distinct_structure() {
    for expression in [
        "outer.new Inner()",
        "(outer).new Inner(1)",
        "outer.new Inner<String>()",
        "outer.new Inner<>()",
        "outer.new <String> Inner(1)",
        "outer.new @A Inner()",
        "outer.new Inner(){int x;void f(){use(x);}}",
    ] {
        let text = format!("class C{{void f(Outer outer){{Object value={expression};}}}}");
        let parsed = ParsedFile::parse(&text);
        assert!(parsed.errors().is_empty(), "{expression}: {:?}", parsed.errors());
        assert_eq!(parsed.root().utf8_len() as usize, text.len());
        fn count(n: &java_engine::GreenNode) -> usize {
            usize::from(format!("{:?}", n.kind()) == "QualifiedNewExpr")
                + n.child_nodes().map(|x| count(x)).sum::<usize>()
        }
        assert_eq!(count(parsed.root()), 1);
    }
    for header in ["outer.new Inner();true;outer.new Inner()", "outer.new Inner(),outer.new Inner();;"] {
        let text = format!("class C{{void f(Outer outer){{for({header}){{}}}}}}");
        assert!(ParsedFile::parse(&text).errors().is_empty(), "{text}");
    }
}
#[test]
fn qualifier_and_arguments_bind_but_creator_name_does_not_fall_back() {
    let text = "class Inner{} class C{int VALUE;void f(Outer outer){Object x=outer.new Inner(VALUE);}}";
    let project = p(text);
    assert_eq!(query(&project, "outer.new").targets[0].kind, TargetKind::Parameter);
    assert_eq!(query(&project, "VALUE);").targets[0].kind, TargetKind::Field);
    assert!(query(&project, "Inner(VALUE)").targets.is_empty());
}
#[test]
fn qualified_result_never_becomes_a_same_named_lexical_field() {
    for expression in [
        "o.new Inner().x",
        "(o.new Inner()).x",
        "((C)o.new Inner()).x",
        "(flag?o.new Inner():other).x",
        "(other=o.new Inner()).x",
        "o.new Inner()[0].x",
        "o.new Inner().m()",
        "((o.new Inner())).m()",
        "o.new Inner().m().x",
        "(o.new Inner())[0].x",
        "o.new Inner().new Deep().x",
    ] {
        let text = format!("class C{{int x;int m;void f(Outer o,boolean flag,C other){{use({expression});}}}}");
        let project = p(&text);
        let name = if expression.ends_with("m()") { "m()" } else { "x);" };
        let result = query(&project, name);
        assert_ne!(result.status, ResolveStatus::Resolved, "{expression}: {result:?}");
        assert!(result.targets.is_empty());
    }
    // Creation in an argument/index operand must not taint the unrelated receiver.
    let text = "class C{int x;void f(Outer o,C c){c.call(o.new Inner());use(c.x);}}";
    let project = p(text);
    assert_eq!(query(&project, "x);").targets[0].kind, TargetKind::Field);
}
#[test]
fn anonymous_body_inventory_stays_separate_from_enclosing_owner() {
    let text =
        "class C{int value;void f(Outer o){Object x=o.new Inner(){int value;void f(int arg){use(value);use(arg);}};}}";
    let project = p(text);
    let field = query(&project, "value);");
    assert_eq!(field.status, ResolveStatus::Resolved);
    assert_eq!(field.targets[0].owner, "");
    assert_eq!(field.targets[0].start_byte, text.rfind("value;void").unwrap() as u32);
    assert_eq!(query(&project, "arg);").targets[0].kind, TargetKind::Parameter);
}
#[test]
fn illegal_creators_keep_diagnostics_and_never_bind_colliding_type() {
    for expression in
        ["o.new int()", "o.new Inner[1]", "o.new ()", "o.new Inner.Other()", "o.new var()", "o.new Inner<,>()"]
    {
        let text = format!("class Inner{{}} class Other{{}} class C{{void f(Outer o){{Object x={expression};}}}}");
        let project = p(&text);
        assert!(!project.file("C.java").unwrap().errors().is_empty(), "{expression}");
    }
}
#[test]
fn local_edit_fresh_undo_and_snapshot() {
    let text = "class C{void f(Outer o,int arg){Object x=o.new Inner(arg);}}";
    let original = p(text);
    let old = original.prepare();
    let at = text.find("Inner(arg)").unwrap();
    let mut edited = original.clone();
    edited.edit("C.java", TextEdit { start_byte: at as u32, end_byte: at as u32 + 5, insert: "".into() }).unwrap();
    let fresh = p(edited.file("C.java").unwrap().text());
    assert_eq!(query(&edited, "arg);"), query(&fresh, "arg);"));
    edited.edit("C.java", TextEdit { start_byte: at as u32, end_byte: at as u32, insert: "Inner".into() }).unwrap();
    assert_eq!(edited.file("C.java").unwrap().root(), original.file("C.java").unwrap().root());
    let byte = text.rfind("arg);").unwrap() as u32;
    assert_eq!(old.resolve_at("C.java", byte), edited.resolve_at("C.java", byte));
}

#[test]
fn independent_type_arguments_keep_type_navigation() {
    let text = "class T{} class Inner{} class C{void f(Outer o){Object x=o.new <T> Inner<T>();}}";
    let project = p(text);
    let start = text.find("o.new").unwrap();
    for (offset, _) in text[start..].match_indices('T') {
        let at = start + offset;
        let result = project.resolve_at("C.java", at as u32);
        assert_eq!(result.status, ResolveStatus::Resolved, "byte {at}: {result:?}");
        assert_eq!(result.targets[0].kind, TargetKind::Class);
        assert_eq!(result.targets[0].start_byte, text.find("T{}").unwrap() as u32);
    }
    assert!(query(&project, "Inner<T>").targets.is_empty());
}
#[test]
fn malformed_creation_recovers_following_local_declaration() {
    for creation in ["o.new", "o.new Inner(", "o.new Inner<,>()"] {
        let text = format!("class C{{void f(Outer o){{Object x={creation};int after=1;use(after);}}}}");
        let project = p(&text);
        assert!(!project.file("C.java").unwrap().errors().is_empty());
        let result = query(&project, "after);");
        assert_eq!(result.status, ResolveStatus::Resolved, "{creation}: {result:?}");
        assert_eq!(result.targets[0].kind, TargetKind::Local);
        assert_eq!(result.targets[0].start_byte, text.find("after=1").unwrap() as u32);
    }
}

#[test]
fn provenance_scan_is_bounded_and_unknown_does_not_become_absent() {
    // An intentionally unsupported, large result expression must not exhaust
    // the host stack or fall back to the lexical field after its scan budget.
    let expression = format!("({}+o.new Inner()).x", vec!["1"; 700].join("+"));
    let text = format!("class C{{int x;void f(Outer o){{use({expression});}}}}");
    let project = p(&text);
    assert!(query(&project, "x);").targets.is_empty());
}
#[test]
fn constructor_arguments_and_index_operands_are_not_result_provenance() {
    // Compare the same unsupported outer receiver to its pre-existing form;
    // no success is claimed for this independent call-result field lookup.
    for (plain, qualified) in
        [("c.call(null).x", "c.call(o.new Inner()).x"), ("arr[0].x", "arr[o.new Inner().index()].x")]
    {
        let wrap = |expression: &str| format!("class C{{int x;void f(Outer o,C c,C[] arr){{use({expression});}}}}");
        let a = p(&wrap(plain));
        let b = p(&wrap(qualified));
        let before = query(&a, "x);");
        let after = query(&b, "x);");
        assert_eq!(after, before, "{qualified}");
    }
}

#[test]
fn forbidden_creator_var_cannot_become_argument_local() {
    let s = "class C{void f(Outer o,int var){Object x=o.new var();}}";
    let mut p = Project::new();
    p.insert("C.java", s);
    let r = p.resolve_at("C.java", s.rfind("var()").unwrap() as u32);
    assert_ne!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert!(r.targets.is_empty());
}
