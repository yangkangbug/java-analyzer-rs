use java_engine::{Project, ResolveStatus, TargetKind, TextEdit};
fn project(expression: &str) -> (Project, String) {
    let text = format!(
        "class C{{int m;int m(){{return 0;}}Object f(Other obj){{return {expression};}}}}class Other{{static int m;static int m(){{return 0;}}}}"
    );
    let mut p = Project::new();
    p.insert("C.java", &text);
    (p, text)
}
fn query(p: &Project, byte: usize) -> java_engine::ResolveResult {
    let result = p.resolve_at("C.java", byte as u32);
    assert_eq!(result, p.prepare().resolve_at("C.java", byte as u32));
    result
}
#[test]
fn terminal_method_names_do_not_enter_field_or_local_namespace() {
    for expression in ["obj::m", "this::m", "Other::m", "(obj)::m", "obj::<String>m"] {
        let (p, text) = project(expression);
        let start = text.find(expression).unwrap();
        let result = query(&p, start + expression.rfind('m').unwrap());
        assert_eq!(result.status, ResolveStatus::ReceiverNotBound, "{expression}: {result:?}");
        assert!(result.targets.is_empty());
    }
    let source = "class C{Object f(Object toString){return StringJoiner::toString;}}";
    let mut p = Project::new();
    p.insert("C.java", source);
    for byte in source.rfind("toString").unwrap()..source.rfind("toString").unwrap() + 8 {
        assert!(query(&p, byte).targets.is_empty());
    }
    let source = "class C{Object f(){int m=1;return C::m;}}";
    let mut p = Project::new();
    p.insert("C.java", source);
    assert!(query(&p, source.rfind("m;").unwrap()).targets.is_empty());
}
#[test]
fn separator_comments_and_whitespace_never_choose_member_targets() {
    for expression in ["obj /*a*/ :: /*b*/ m /*c*/", "(obj) /*a*/ :: /*b*/ m /*c*/", "Other /*a*/ :: /*b*/ m /*c*/"] {
        let (p, text) = project(expression);
        let start = text.find(expression).unwrap() + expression.find("/*a*/").unwrap();
        let end = text.find(expression).unwrap() + expression.len();
        for byte in start..end {
            assert!(query(&p, byte).targets.is_empty(), "{expression} byte {byte}");
        }
    }
}
#[test]
fn existing_prefixes_and_constructor_refusal_are_preserved() {
    let (p, text) = project("(obj)::m");
    let result = query(&p, text.find("obj)::").unwrap());
    assert_eq!(result.status, ResolveStatus::Resolved);
    assert_eq!(result.targets[0].kind, TargetKind::Parameter);
    assert_eq!(result.targets[0].start_byte, text.find("obj){").unwrap() as u32);
    let (p, text) = project("Other::m");
    assert_eq!(query(&p, text.find("Other::").unwrap()).targets[0].kind, TargetKind::Class);
    // The existing parser treats bare obj here as a type/name. This slice does
    // not invent method-reference receiver disambiguation.
    let (p, text) = project("obj::m");
    assert_eq!(query(&p, text.find("obj::").unwrap()).status, ResolveStatus::Unresolved);
    let (p, text) = project("Other::new");
    for byte in text.find("new;}").unwrap()..text.find("new;}").unwrap() + 3 {
        assert_eq!(query(&p, byte).status, ResolveStatus::ConstructorRefNotBound);
    }
}
#[test]
fn local_edit_fresh_undo_and_old_snapshot_preserve_boundary() {
    let (p, text) = project("this.m");
    let dot = text.find("this.m").unwrap() + 4;
    let old = p.prepare();
    let old_result = old.resolve_at("C.java", dot as u32 + 1);
    assert_eq!(old_result.status, ResolveStatus::Resolved);
    let mut edited = p.clone();
    edited.edit("C.java", TextEdit { start_byte: dot as u32, end_byte: dot as u32 + 1, insert: "::".into() }).unwrap();
    let mut fresh = Project::new();
    fresh.insert("C.java", edited.file("C.java").unwrap().text());
    assert_eq!(query(&edited, dot + 2), query(&fresh, dot + 2));
    assert_eq!(query(&edited, dot + 2).status, ResolveStatus::ReceiverNotBound);
    assert_eq!(old.resolve_at("C.java", dot as u32 + 1), old_result);
    edited.edit("C.java", TextEdit { start_byte: dot as u32, end_byte: dot as u32 + 2, insert: ".".into() }).unwrap();
    assert_eq!(query(&edited, dot + 1), old_result);
}
