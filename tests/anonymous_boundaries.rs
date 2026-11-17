use java_engine::{ParsedFile, Project, ResolveStatus, TargetKind, TextEdit};

fn query(s: &str, marker: &str) -> java_engine::ResolveResult {
    let mut p = Project::new();
    p.insert("C.java", s);
    let at = s.find(marker).unwrap() as u32;
    let result = p.resolve_at("C.java", at);
    assert_eq!(result, p.prepare().resolve_at("C.java", at));
    result
}
#[test]
fn anonymous_field_shadowing_is_not_bound_to_outer_local_or_exported() {
    let s = "class C{Object value; Object f(){Object value=null;return new Object(){Object value;Object g(){return value/*inner*/;}};} Object h(){return value/*outer*/;}}";
    assert!(ParsedFile::parse(s).errors().is_empty());
    assert_eq!(query(s, "value/*inner*/").status, ResolveStatus::Unresolved);
    let r = query(s, "value/*outer*/");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].kind, TargetKind::Field);
    assert_eq!(r.targets[0].start_byte, s.find("value;").unwrap() as u32);
}
#[test]
fn anonymous_methods_and_named_members_do_not_become_enclosing_members() {
    let s = "class C{Object x=new Object(){int hidden; void ghost(){} class Nested{}}; void m(C c){c.hidden/*field*/++; c.ghost/*method*/(); C.Nested/*type*/ n;} class Real{} Real/*real*/ r;}";
    for marker in ["hidden;", "ghost()", "Nested{", "hidden/*field*/", "ghost/*method*/", "Nested/*type*/"] {
        let r = query(s, marker);
        assert_eq!(r.status, ResolveStatus::Unresolved, "{marker}: {r:?}");
        assert!(r.targets.is_empty());
    }
    assert_eq!(query(s, "Real/*real*/").status, ResolveStatus::Resolved);
}
#[test]
fn constructor_arguments_remain_outside_the_body_boundary() {
    let s = "class X{X(Object a,Object b){}} class C{Object f(Object value){return new X(value/*arg*/,new Object(){Object value;Object g(){return value/*nested*/;}}){Object value;Object g(){return value/*body*/;}};}}";
    assert!(ParsedFile::parse(s).errors().is_empty());
    assert_eq!(query(s, "value/*arg*/").targets[0].kind, TargetKind::Parameter);
    assert_eq!(query(s, "value/*nested*/").status, ResolveStatus::Unresolved);
    assert_eq!(query(s, "value/*body*/").status, ResolveStatus::Unresolved);
    assert_eq!(query(s, "X(value").targets[0].kind, TargetKind::Class);
}
#[test]
fn anonymous_pattern_guards_do_not_leak_but_argument_guards_remain() {
    let s = "class X{X(boolean b){}} class C{Object value; Object f(Object input){Object x=new Object(){boolean b=input instanceof String value;};return value/*outer*/;} Object g(Object input){Object x=new X(input instanceof String value){};return value/*guard*/;}}";
    assert_eq!(query(s, "value/*outer*/").targets[0].kind, TargetKind::Field);
    assert_eq!(query(s, "value/*guard*/").status, ResolveStatus::Unresolved);
}
#[test]
fn anonymous_body_edits_match_fresh_and_preserve_old_prepared_index() {
    let original = "class C{Object value;Object f(){return new Object();}Object h(){return value/*use*/;}}";
    let at = original.find("new Object()").unwrap() + "new Object()".len();
    let mut p = Project::new();
    p.insert("C.java", original);
    let old = p.clone();
    let prepared = old.prepare();
    for body in ["{Object value;}", "{Object value;Object g(){return value;}}", "{class Nested{}}"] {
        p.edit("C.java", TextEdit { start_byte: at as u32, end_byte: at as u32, insert: body.into() }).unwrap();
        let text = p.file("C.java").unwrap().text().to_owned();
        let mut fresh = Project::new();
        fresh.insert("C.java", &text);
        assert_eq!(p.file("C.java").unwrap().dump(), fresh.file("C.java").unwrap().dump());
        let a = p.prepare();
        let b = fresh.prepare();
        for (byte, _) in text.char_indices() {
            assert_eq!(a.resolve_at("C.java", byte as u32), b.resolve_at("C.java", byte as u32));
        }
        assert_eq!(prepared.resolve_at("C.java", original.find("value/*use*/").unwrap() as u32).targets.len(), 1);
        p.edit("C.java", TextEdit { start_byte: at as u32, end_byte: (at + body.len()) as u32, insert: String::new() })
            .unwrap();
        assert_eq!(p.file("C.java").unwrap().dump(), ParsedFile::parse(original).dump());
    }
}
