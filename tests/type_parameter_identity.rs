use java_engine::{Project, ResolveStatus};
#[test]
fn formal_type_use_is_not_same_named_class() {
    let s = "class T { int x; } class Box<T> { T value; <T> T id(T v){return v;} }";
    let mut p = Project::new();
    p.insert("Use.java", s);
    for (needle, delta) in [("T value", 0), ("T id", 0), ("T v)", 0)] {
        let r = p.resolve_at("Use.java", (s.find(needle).unwrap() + delta) as u32);
        assert_eq!(r.status, ResolveStatus::Resolved);
        assert_eq!(format!("{:?}", r.targets[0].kind), "TypeParameter");
    }
}
#[test]
fn formal_declared_receiver_never_borrows_class_field() {
    let s = "class T { int x; } class Box<T> { T value; int f(){return value.x;} }";
    let mut p = Project::new();
    p.insert("Use.java", s);
    let r = p.resolve_at("Use.java", s.find("value.x").unwrap() as u32 + 6);
    assert!(r.targets.is_empty(), "{r:?}");
}

fn check(s: &str, query: &str, declaration: &str, kind: &str) {
    let mut p = Project::new();
    p.insert("Use.java", s);
    let at = s.find(query).unwrap() as u32;
    let r = p.resolve_at("Use.java", at);
    assert_eq!(r.status, ResolveStatus::Resolved, "{query}: {r:?}");
    assert_eq!(format!("{:?}", r.targets[0].kind), kind);
    assert_eq!(r.targets[0].start_byte, s.find(declaration).unwrap() as u32, "{query}: {r:?}");
}
#[test]
fn own_class_formal_beats_inherited_member() {
    check("class Base{class T{}} class Box<T> extends Base{T value;}", "T value", "T>", "TypeParameter");
}
#[test]
fn body_member_but_not_bound_shadows_formal() {
    let s = "class Box<T extends Comparable<T>>{class T{} T value;<T> T id(T v){return v;}}";
    check(s, "T>>", "T extends", "TypeParameter");
    check(s, "T value", "T{}", "Class");
    check(s, "T id", "T> T", "TypeParameter");
}
#[test]
fn header_and_mutual_bounds() {
    let s = "@A(T.class) class Box<T extends Comparable<U>, U extends Comparable<T>>{T value;}";
    let mut p = Project::new();
    p.insert("Use.java", s);
    assert!(p.resolve_at("Use.java", s.find("T.class").unwrap() as u32).targets.is_empty());
    check(s, "U>,", "U extends", "TypeParameter");
    check(s, "T>>", "T extends", "TypeParameter");
}
#[test]
fn method_and_constructor_annotation_scope() {
    for s in ["class C{@A(T.class)<T> T id(T v){return v;}}", "class C{@A(T.class)<T> C(T v){}}"] {
        let mut p = Project::new();
        p.insert("Use.java", s);
        assert!(p.resolve_at("Use.java", s.find("T.class").unwrap() as u32).targets.is_empty());
        check(s, "T v", "T>", "TypeParameter");
    }
}
#[test]
fn static_context_cannot_fall_back_but_own_formal_can() {
    let s = "class T{} class C<T>{static T field; static <T> T id(T v){return v;}}";
    let mut p = Project::new();
    p.insert("Use.java", s);
    assert!(p.resolve_at("Use.java", s.find("T field").unwrap() as u32).targets.is_empty());
    check(s, "T id", "T> T", "TypeParameter");
    check("class C<T>{class T{} static T value;}", "T value", "T{}", "Class");
}
#[test]
fn crossing_owner_is_targeted_and_nearer_member_is_preserved() {
    let s = "class T{} class X{} class C<T>{class N{T outer; X ordinary; class T{} T own;}}";
    check(s, "X ordinary", "X{}", "Class");
    check(s, "T own", "T{} T own", "Class");
    let s = "class T{} class C<T>{class N{T outer;}}";
    let mut p = Project::new();
    p.insert("Use.java", s);
    assert!(p.resolve_at("Use.java", s.find("T outer").unwrap() as u32).targets.is_empty());
}
#[test]
fn expression_namespace_not_formal() {
    let s = "class C<T>{int T; int f(){return T;} T field;}";
    check(s, "T;}", "T; int", "Field");
    check(s, "T field", "T>", "TypeParameter");
}
#[test]
fn formal_prefix_cannot_be_package_or_receiver() {
    let s = "class T{class Inner{}} class C<T>{T.Inner value;}";
    let mut p = Project::new();
    p.insert("Use.java", s);
    let at = s.find("T.Inner").unwrap() as u32;
    assert_eq!(format!("{:?}", p.resolve_at("Use.java", at).targets[0].kind), "TypeParameter");
    assert!(p.resolve_at("Use.java", at + 2).targets.is_empty());
}
#[test]
fn duplicate_and_damaged_formals_terminal() {
    for s in ["class T{} class C<T,T>{T value;}", "class T{} class C<T extends >{T value;}"] {
        let mut p = Project::new();
        p.insert("Use.java", s);
        assert!(p.resolve_at("Use.java", s.find("T value").unwrap() as u32).targets.is_empty());
    }
}
#[test]
fn declaration_identity_is_range_not_empty_owner() {
    let s = "class A<T>{T a;} class B<T>{T b;}";
    let mut p = Project::new();
    p.insert("Use.java", s);
    let prep = p.prepare();
    let a = prep.resolve_at("Use.java", s.find("T a").unwrap() as u32).targets.remove(0);
    let b = prep.resolve_at("Use.java", s.find("T b").unwrap() as u32).targets.remove(0);
    assert_eq!(a.owner, "");
    assert_eq!(b.owner, "");
    assert_ne!(a, b);
}

#[test]
fn type_and_value_namespaces_and_package_are_independent() {
    let mut p = Project::new();
    p.insert("T/A.java", "package T; public class A{}");
    let s = "import T.A; class C<T>{ A x; T t; }";
    p.insert("Use.java", s);
    assert_eq!(format!("{:?}", p.resolve_at("Use.java", s.find("A x").unwrap() as u32).targets[0].kind), "Class");
    assert_eq!(
        format!("{:?}", p.resolve_at("Use.java", s.find("T t").unwrap() as u32).targets[0].kind),
        "TypeParameter"
    );
}
#[test]
fn incremental_formal_rename_fresh_undo_and_snapshot() {
    use java_engine::TextEdit;
    let s = "class T{int x;} class C<T>{T value;int f(){return value.x;}}";
    let mut p = Project::new();
    p.insert("Use.java", s);
    let old = p.clone();
    let at = s.find("T>").unwrap() as u32;
    p.edit("Use.java", TextEdit { start_byte: at, end_byte: at + 1, insert: "U".into() }).unwrap();
    let edited = p.file("Use.java").unwrap().text().to_owned();
    let mut fresh = Project::new();
    fresh.insert("Use.java", &edited);
    for query in ["T value", "value.x"] {
        let n = edited.find(query).unwrap() as u32;
        assert_eq!(p.resolve_at("Use.java", n), fresh.resolve_at("Use.java", n));
        assert_eq!(p.resolve_at("Use.java", n), p.prepare().resolve_at("Use.java", n));
    }
    assert_eq!(
        format!("{:?}", old.resolve_at("Use.java", s.find("T value").unwrap() as u32).targets[0].kind),
        "TypeParameter"
    );
    p.edit("Use.java", TextEdit { start_byte: at, end_byte: at + 1, insert: "T".into() }).unwrap();
    assert_eq!(
        p.resolve_at("Use.java", s.find("T value").unwrap() as u32),
        old.resolve_at("Use.java", s.find("T value").unwrap() as u32)
    );
}

#[test]
fn nearer_class_self_and_formal_named_like_class() {
    check("class C<T>{class T{T value;}}", "T value", "T{T", "Class");
    check("class Box<Box>{Box value;}", "Box value", "Box>", "TypeParameter");
}
#[test]
fn static_initializer_and_anonymous_boundaries() {
    let s = "class T{} class C<T>{static{T value;}Object a=new Object(){T field; <U> U id(U v){return v;}};}";
    let mut p = Project::new();
    p.insert("Use.java", s);
    for needle in ["T value", "T field"] {
        assert!(p.resolve_at("Use.java", s.find(needle).unwrap() as u32).targets.is_empty());
    }
    check(s, "U id", "U>", "TypeParameter");
    check(s, "U v", "U>", "TypeParameter");
}

#[test]
fn local_type_scope_is_terminal_before_formal() {
    for s in ["class C<T>{void f(){class T{} T x;}}", "class C{<T> void f(){class T{} T x;}}"] {
        let mut p = Project::new();
        p.insert("Use.java", s);
        assert!(p.resolve_at("Use.java", s.find("T x").unwrap() as u32).targets.is_empty());
    }
}

#[test]
fn implicit_static_interface_field_cannot_borrow_formal_or_class() {
    let s = "class T{} interface C<T>{T value=null;}";
    let mut p = Project::new();
    p.insert("Use.java", s);
    assert!(p.resolve_at("Use.java", s.find("T value").unwrap() as u32).targets.is_empty());
}
