use java_engine::{Project, ResolveStatus};
fn query(s: &str, q: &str) -> java_engine::ResolveResult {
    let mut p = Project::new();
    p.insert("Use.java", s);
    p.resolve_at("Use.java", s.find(q).unwrap() as u32)
}
#[test]
fn local_type_identity_not_global_class() {
    let s = "class T{} class C{void f(){T before; class T{} T after;}}";
    let r = query(s, "T after");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(format!("{:?}", r.targets[0].kind), "LocalType");
    assert_eq!(r.targets[0].start_byte, s.find("T{} T after").unwrap() as u32);
    assert_eq!(format!("{:?}", query(s, "T before").targets[0].kind), "Class");
}
#[test]
fn anonymous_own_formal_beats_outer_local() {
    let s = "class C{void f(){class T{} Object o=new Object(){<T>T id(T x){return x;}};}}";
    let r = query(s, "T id");
    assert_eq!(format!("{:?}", r.targets[0].kind), "TypeParameter");
}
#[test]
fn local_only_anonymous_crossing_refuses() {
    let s = "class T{} class C{void f(){class T{} Object o=new Object(){T id(T x){return x;}};}}";
    assert!(query(s, "T id").targets.is_empty());
}
#[test]
fn duplicate_nested_local_refuses() {
    let s = "class C{void f(){class T{} {class T{} T use;}}}";
    assert!(query(s, "T use").targets.is_empty());
}
#[test]
fn local_nominal_suffix_is_terminal() {
    let s = "class T{static int x=0;} class C{void f(){class T{} T value; int z=value.x;}}";
    assert!(query(s, "x;").targets.is_empty());
}

#[test]
fn record_enum_and_exact_declaration_only() {
    for body in ["record R(int x){}", "enum R{A}", "class R{int x;}"] {
        let s = format!("class C{{void f(){{{body} R value;}}}}");
        let r = query(&s, "R value");
        assert_eq!(format!("{:?}", r.targets[0].kind), "LocalType");
        assert!(r.targets[0].owner.is_empty());
        let name = s.find("R").unwrap();
        let mut p = Project::new();
        p.insert("Use.java", &s);
        assert_eq!(p.resolve_at("Use.java", name as u32).targets, r.targets);
        assert!(p.resolve_at("Use.java", (name - 1) as u32).targets.is_empty());
    }
}
#[test]
fn local_body_remains_opaque() {
    let s = "class C{void f(){class R{int member; R self;} R value;}}";
    for q in ["member", "R self"] {
        assert!(query(s, q).targets.is_empty());
    }
}
#[test]
fn illegal_or_broken_local_name_does_not_fall_back() {
    for local in ["interface T{}", "@interface T{}", "class T;", "class T{"] {
        let s = format!("class T{{}} class C{{void f(){{{local} T value;}}}}");
        assert!(query(&s, "T value").targets.is_empty(), "{s}");
    }
}
#[test]
fn blocks_restore_prior_identity() {
    let s = "class T{} class C{void f(){{class T{} T inner;} T after; {class T{} T other;}}}";
    assert_eq!(format!("{:?}", query(s, "T after").targets[0].kind), "Class");
    let a = query(s, "T inner");
    let b = query(s, "T other");
    assert_eq!(format!("{:?}", a.targets[0].kind), "LocalType");
    assert_ne!(a.targets, b.targets);
}
#[test]
fn local_type_suffix_not_package() {
    let s = "class C{void f(){class T{} T.Inner x;}}";
    assert!(query(s, "Inner x").targets.is_empty());
}
#[test]
fn value_namespace_remains_value() {
    let s = "class C{void f(){class T{} int T=1; int value=T;}}";
    let r = query(s, "T;");
    assert_eq!(format!("{:?}", r.targets[0].kind), "Local");
}
#[test]
fn edit_fresh_undo_old_snapshot() {
    use java_engine::TextEdit;
    let s = "class T{} class C{void f(){class T{} T value;}}";
    let mut p = Project::new();
    p.insert("Use.java", s);
    let old = p.clone();
    let at = s.find("T{} T value").unwrap() as u32;
    p.edit("Use.java", TextEdit { start_byte: at, end_byte: at + 1, insert: "U".into() }).unwrap();
    let edited = p.file("Use.java").unwrap().text().to_owned();
    let mut fresh = Project::new();
    fresh.insert("Use.java", &edited);
    let use_at = edited.find("T value").unwrap() as u32;
    assert_eq!(p.resolve_at("Use.java", use_at), fresh.resolve_at("Use.java", use_at));
    assert_eq!(p.resolve_at("Use.java", use_at), p.prepare().resolve_at("Use.java", use_at));
    assert_eq!(format!("{:?}", old.resolve_at("Use.java", use_at).targets[0].kind), "LocalType");
    p.edit("Use.java", TextEdit { start_byte: at, end_byte: at + 1, insert: "T".into() }).unwrap();
    assert_eq!(p.resolve_at("Use.java", use_at), old.resolve_at("Use.java", use_at));
}

#[test]
fn future_duplicate_in_current_block_is_unknown() {
    for s in [
        "class C{void f(){class T{} T before; class T{} T after;}}",
        "class C{void f(){class T{} {T before; class T{} T after;}}}",
    ] {
        assert!(query(s, "T before").targets.is_empty());
        assert!(query(s, "T after").targets.is_empty());
    }
    let s = "class C{void f(){class T{} T outside; {class T{} T inside;}}}";
    assert_eq!(format!("{:?}", query(s, "T outside").targets[0].kind), "LocalType");
    assert!(query(s, "T inside").targets.is_empty());
}

#[test]
fn missing_name_is_not_invented_or_universal_shadow() {
    for declaration in ["class {}", "class extends T{}", "class ;"] {
        let s = format!("class T{{}} class C{{void f(){{{declaration} T value;}}}}");
        let result = query(&s, "T value");
        assert_eq!(result.status, ResolveStatus::Resolved, "{s}: {result:?}");
        assert_eq!(format!("{:?}", result.targets[0].kind), "Class");
        assert_eq!(result.targets[0].start_byte, 6);
    }
}
#[test]
fn modifiers_annotations_and_unicode_keep_exact_name() {
    let s = "class C{void f(){@A(v={1,2}) final class 名{} 名 value;}}";
    let r = query(s, "名 value");
    assert_eq!(format!("{:?}", r.targets[0].kind), "LocalType");
    assert_eq!(r.targets[0].start_byte, s.find("名{}").unwrap() as u32);
    assert_eq!(r.targets[0].end_byte, r.targets[0].start_byte + 3);
}
