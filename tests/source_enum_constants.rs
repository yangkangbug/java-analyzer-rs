use java_engine::{ParsedFile, Project, ResolveStatus, TargetKind, TextEdit};

fn query(p: &Project, path: &str, text: &str, marker: &str) -> java_engine::ResolveResult {
    let at = text.find(marker).unwrap() as u32;
    let result = p.resolve_at(path, at);
    assert_eq!(result, p.prepare().resolve_at(path, at));
    result
}

#[test]
fn constants_have_exact_public_field_identity_and_enum_type() {
    let e = "package p; public enum E { A, 名; public int field; E get() { return A; } }";
    let c = "package q; import p.E; class C { void m() { E a=E.A; E b=E.名; E.A.field++; } }";
    let mut p = Project::new();
    p.insert("p/E.java", e);
    p.insert("q/C.java", c);
    for (path, s, marker, name) in [
        ("p/E.java", e, "A,", "A"),
        ("p/E.java", e, "A; }", "A"),
        ("q/C.java", c, "A; E", "A"),
        ("q/C.java", c, "名;", "名"),
    ] {
        let r = query(&p, path, s, marker);
        assert_eq!(r.status, ResolveStatus::Resolved, "{marker}: {r:?}");
        assert_eq!(r.targets.len(), 1);
        assert_eq!(r.targets[0].kind, TargetKind::Field);
        assert_eq!(r.targets[0].owner, "p.E");
        assert_eq!(r.targets[0].start_byte as usize, e.find(name).unwrap());
        assert_eq!(r.targets[0].end_byte - r.targets[0].start_byte, name.len() as u32);
    }
    assert_eq!(query(&p, "q/C.java", c, "field++").status, ResolveStatus::Resolved);
}

#[test]
fn constant_bodies_never_export_members_types_or_borrow_outer_bindings() {
    let e = "enum E { A(outer) { int hidden; void ghost() { outer++; } class Nested {} }, B { int hidden; void ghost() {} }; static int outer; int real; E(int n) {} E() {} void m(E e) { e.hidden++; e.ghost(); E.Nested x; e.real++; } }";
    let mut p = Project::new();
    p.insert("E.java", e);
    for marker in ["hidden;", "ghost()", "outer++;", "Nested {}", "hidden++;", "ghost();", "Nested x"] {
        assert_ne!(query(&p, "E.java", e, marker).status, ResolveStatus::Resolved, "{marker}");
    }
    assert_eq!(query(&p, "E.java", e, "outer)").status, ResolveStatus::Resolved);
    assert_eq!(query(&p, "E.java", e, "real++").status, ResolveStatus::Resolved);
    for marker in ["A(outer)", "B {"] {
        assert_eq!(query(&p, "E.java", e, marker).status, ResolveStatus::Resolved);
    }
}

#[test]
fn nested_enum_identity_and_duplicate_constants_are_not_collapsed() {
    let s = "package p; class Outer { enum E { A; } void m() { E x=E.A; } }";
    let mut p = Project::new();
    p.insert("O.java", s);
    let r = query(&p, "O.java", s, "A; } }");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets[0].owner, "p.Outer.E");
    let duplicate = "enum E { A, A; void m() { E x=E.A; } }";
    p.insert("D.java", duplicate);
    let r = query(&p, "D.java", duplicate, "A; } }");
    assert_ne!(r.status, ResolveStatus::Resolved);
}

#[test]
fn malformed_constants_do_not_supply_navigation_and_keep_later_members() {
    for declaration in ["A(,)", "A(1", "@Broken( A"] {
        let s = format!("enum E {{ {declaration}, B; int real; void m() {{ E x=E.A; }} }}");
        let mut p = Project::new();
        p.insert("E.java", &s);
        assert_ne!(query(&p, "E.java", &s, "A; }").status, ResolveStatus::Resolved, "{declaration}");
    }
}

#[test]
fn body_edits_and_prepared_snapshots_keep_containment() {
    let s = "enum E { A { int hidden; }; int real; void m(E e) { e.hidden++; e.real++; E x=E.A; } }";
    let at = s.find("hidden;").unwrap();
    let original = ParsedFile::parse(s);
    let edited =
        original.apply_edit(TextEdit { start_byte: at as u32, end_byte: (at + 6) as u32, insert: "other".into() });
    let changed = s.replacen("hidden;", "other;", 1);
    assert_eq!(edited.file.dump(), ParsedFile::parse(&changed).dump());
    let mut p = Project::new();
    p.insert("E.java", s);
    let snapshot = p.clone();
    let old = snapshot.prepare();
    let old_at = s.find("A; }").unwrap() as u32;
    let old_result = old.resolve_at("E.java", old_at);
    assert_eq!(old_result.status, ResolveStatus::Resolved);
    let update = p
        .edit("E.java", TextEdit { start_byte: at as u32, end_byte: (at + 6) as u32, insert: "other".into() })
        .unwrap();
    assert_eq!(update.file.dump(), ParsedFile::parse(&changed).dump());
    let mut rebuilt = Project::new();
    rebuilt.insert("E.java", &changed);
    assert_eq!(query(&p, "E.java", &changed, "A; }"), query(&rebuilt, "E.java", &changed, "A; }"));
    assert_eq!(old.resolve_at("E.java", old_at), old_result);
    assert_ne!(query(&p, "E.java", &changed, "hidden++").status, ResolveStatus::Resolved);
    assert_eq!(query(&p, "E.java", &changed, "real++").status, ResolveStatus::Resolved);
    assert_eq!(query(&p, "E.java", &changed, "A; }").status, ResolveStatus::Resolved);
}

#[test]
fn annotated_constants_and_nested_public_owner_cross_package() {
    let s = "package p; public class Outer { public enum E { @Deprecated A(Seed.n) { int hidden; }, B; public int real; E(int n) {} E() {} } } class Seed { static int n; }";
    let c = "package q; import p.Outer.E; class C { void m() { E e=E.A; E.B.real++; } }";
    let mut p = Project::new();
    p.insert("p/Outer.java", s);
    p.insert("q/C.java", c);
    assert!(ParsedFile::parse(s).errors().is_empty());
    let r = query(&p, "q/C.java", c, "A;");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets[0].owner, "p.Outer.E");
    assert_eq!(r.targets[0].start_byte as usize, s.find("A(Seed").unwrap());
    assert_eq!(query(&p, "p/Outer.java", s, "n) {").status, ResolveStatus::Resolved);
    assert_eq!(query(&p, "q/C.java", c, "real++").status, ResolveStatus::Resolved);
}

#[test]
fn body_ranges_refresh_when_added_removed_or_edited() {
    let original = "enum E { A; int real; void m(E e) { e.hidden++; e.real++; } }";
    for replacement in ["A { int hidden; void ghost() {} }", "A { class Nested {} }", "A { int hidden;"] {
        let at = original.find("A;").unwrap();
        let p = ParsedFile::parse(original);
        let edited =
            p.apply_edit(TextEdit { start_byte: at as u32, end_byte: (at + 1) as u32, insert: replacement.into() });
        let expected = original.replacen("A;", &format!("{replacement};"), 1);
        assert_eq!(edited.file.dump(), ParsedFile::parse(&expected).dump());
        assert!(edited.file.tokens_cover_text());
    }
}

#[test]
fn anonymous_argument_bodies_do_not_pollute_enum_but_arguments_still_bind() {
    let s = "class X { X(int n) {} } enum E { A(new X(outer) { int hidden; void ghost() { outer++; } class Nested {} }), B(new X(outer) { int hidden; void ghost() {} }); static int outer; int real; E(X x) {} void m(E e) { e.hidden++; e.ghost(); E.Nested n; e.real++; } }";
    let mut p = Project::new();
    p.insert("E.java", s);
    assert!(ParsedFile::parse(s).errors().is_empty());
    for marker in ["hidden;", "ghost()", "outer++;", "Nested {}", "hidden++;", "ghost();", "Nested n"] {
        assert_ne!(query(&p, "E.java", s, marker).status, ResolveStatus::Resolved, "{marker}");
    }
    assert_eq!(query(&p, "E.java", s, "outer) {").status, ResolveStatus::Resolved);
    assert_eq!(query(&p, "E.java", s, "real++").status, ResolveStatus::Resolved);
    assert_eq!(query(&p, "E.java", s, "A(new").status, ResolveStatus::Resolved);
}

#[test]
fn constants_do_not_bypass_owner_access_and_body_members_do_not_shadow_enum_members() {
    let mut p = Project::new();
    p.insert("p/E.java", "package p; enum E { A; }");
    let c = "package q; import p.E; class C { void m() { E e=E.A; } }";
    p.insert("q/C.java", c);
    assert_ne!(query(&p, "q/C.java", c, "A;").status, ResolveStatus::Resolved);
    let s = "enum E { A { int shared; }; int shared; void m(E e) { e.shared++; } }";
    p.insert("E.java", s);
    let r = query(&p, "E.java", s, "shared++");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].start_byte as usize, s.rfind("shared;").unwrap());
}

#[test]
fn enclosing_enum_access_handles_private_nests_and_implicit_interface_members() {
    for owner in
        ["package p; public interface Outer { enum E { A } }", "package p; public class Outer { public enum E { A } }"]
    {
        let mut p = Project::new();
        p.insert("p/Outer.java", owner);
        let c = "package q; import p.Outer.E; class C { E e=E.A; }";
        p.insert("q/C.java", c);
        assert_eq!(query(&p, "q/C.java", c, "A;").status, ResolveStatus::Resolved, "{owner}");
    }
    for owner in [
        "package p; class Outer { public enum E { A } }",
        "package p; public class Outer { private enum E { A } }",
        "package p; public class Outer { protected enum E { A } }",
    ] {
        let mut p = Project::new();
        p.insert("p/Outer.java", owner);
        let c = "package q; import p.Outer.E; class C { E e=E.A; }";
        p.insert("q/C.java", c);
        assert_ne!(query(&p, "q/C.java", c, "A;").status, ResolveStatus::Resolved, "{owner}");
    }
    let s = "class Outer { private enum E { A } E e=E.A; }";
    let mut p = Project::new();
    p.insert("Outer.java", s);
    assert_eq!(query(&p, "Outer.java", s, "A;").status, ResolveStatus::Resolved);
}

#[test]
fn multi_level_interface_member_types_are_implicitly_public() {
    let s = "package p; public interface Outer { interface Inner { enum E { A } } }";
    let c = "package q; import p.Outer.Inner.E; class C { E e=E.A; }";
    let mut p = Project::new();
    p.insert("p/Outer.java", s);
    p.insert("q/C.java", c);
    let r = query(&p, "q/C.java", c, "A;");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].owner, "p.Outer.Inner.E");
    assert_eq!(r.targets[0].start_byte as usize, s.find("A }").unwrap());
}
