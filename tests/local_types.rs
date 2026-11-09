use java_engine::{ParsedFile, Project, ResolveStatus, TargetKind, TextEdit};

fn query(source: &str, marker: &str) -> java_engine::ResolveResult {
    let mut p = Project::new();
    p.insert("C.java", source);
    let byte = source.find(marker).unwrap() as u32;
    let result = p.resolve_at("C.java", byte);
    assert_eq!(result, p.prepare().resolve_at("C.java", byte));
    result
}

#[test]
fn local_type_does_not_escape_method_or_export_members() {
    let s = "class C { void m() { int before = 0; class Dummy<T> { int hidden; void x() { before++; } } int after = 1; after++; before++; } void next() { int other = 0; other++; } }";
    for (marker, name) in [("after++;", "after"), ("before++; } void", "before"), ("other++;", "other")] {
        let r = query(s, marker);
        assert_eq!(r.status, ResolveStatus::Resolved, "{marker}: {r:?}");
        assert_eq!(r.targets[0].kind, TargetKind::Local);
        assert_eq!(r.targets[0].start_byte as usize, s.find(&format!("{name} =")).unwrap());
    }
    for marker in ["hidden;", "before++; } }"] {
        assert_eq!(query(s, marker).status, ResolveStatus::Unresolved);
    }
}

#[test]
fn local_type_shadowing_is_lexical_and_receivers_refuse() {
    let s = "class L { int field; } class C { void m() { L before; { class L {} L inside; inside.field++; L.field++; } L outside; outside.field++; } void n() { L later; later.field++; } }";
    assert_eq!(query(s, "L inside").status, ResolveStatus::Unresolved);
    assert_ne!(query(s, "field++; L.field").status, ResolveStatus::Resolved);
    assert_ne!(query(s, "field++; } L outside").status, ResolveStatus::Resolved);
    for marker in ["L before", "L outside", "L later", "field++; } void", "field++; } }"] {
        assert_eq!(query(s, marker).status, ResolveStatus::Resolved, "{marker}");
    }
}

#[test]
fn local_type_edits_remain_lossless_and_match_full_parse() {
    for declaration in [
        "class L<T> { class Nested { int hidden; } }",
        "record L(int x) { int value() { return x; } }",
        "enum L { A, B; int hidden; }",
        "@A(value = {1, 2}) final class 名 {}",
        "class L extends { int hidden; }",
        "class L;",
        "class L { int hidden;",
    ] {
        let s = format!("class C {{ void m() {{ {declaration} int after = 1; after++; }} void n() {{}} }}");
        let p = ParsedFile::parse(&s);
        assert!(p.tokens_cover_text(), "{}", p.dump());
        let at = s.find("after =").unwrap() as u32;
        let edit = TextEdit { start_byte: at, end_byte: at + 5, insert: "later".into() };
        let changed = p.apply_edit(edit);
        let expected = s.replacen("after =", "later =", 1);
        let full = ParsedFile::parse(&expected);
        assert!(changed.file.tokens_cover_text());
        assert_eq!(changed.file.dump(), full.dump());
    }
}

#[test]
fn unsupported_receiver_never_retries_unrelated_global_type() {
    let s = "class x { static int field; } class L {} class C { void m() { class L {} L x; x.field++; } }";
    let r = query(s, "field++");
    assert_eq!(r.status, ResolveStatus::ReceiverNotBound);
    assert!(r.targets.is_empty());
}

#[test]
fn annotations_unicode_nested_bodies_and_following_methods_stay_contained() {
    for declaration in [
        "@A(value = {1, 2}) final class 名 { class Nested { int hidden; } }",
        "record 名(@A(value = {1, 2}) int x) { int hidden; }",
        "enum 名 { A { int hidden; }, B; }",
        "class 名 extends { int hidden; }",
        "class 名;",
        "class { int hidden; }",
        "record 名( {}",
    ] {
        let s = format!(
            "class C {{ void m() {{ {declaration} int after = 0; after++; }} void n() {{ int later = 0; later++; }} }}"
        );
        let file = ParsedFile::parse(&s);
        assert!(file.tokens_cover_text());
        assert!(file.errors().iter().any(|error| error.message.contains("local type")));
        for marker in ["after++;", "later++;"] {
            let r = query(&s, marker);
            assert_eq!(r.status, ResolveStatus::Resolved, "{declaration}: {r:?}");
            assert_eq!(r.targets[0].kind, TargetKind::Local);
        }
        if s.contains("hidden;") {
            let r = query(&s, "hidden;");
            assert_eq!(r.status, ResolveStatus::Unresolved);
            assert!(r.targets.is_empty());
        }
    }
}

#[test]
fn type_barriers_do_not_hide_values_or_unrelated_qualified_names() {
    let s = "class L { int field; } class C { int L; void m() { L before; class L {} before.field++; L++; p.L qualified; qualified.field++; } }";
    let mut p = Project::new();
    p.insert("C.java", s);
    p.insert("p/L.java", "package p; public class L { public int field; }");
    for marker in ["field++; L++", "L++;", "p.L qualified", "field++; } }"] {
        let r = p.resolve_at("C.java", (s.find(marker).unwrap() + usize::from(marker.starts_with("p.L")) * 2) as u32);
        assert_eq!(r.status, ResolveStatus::Resolved, "{marker}: {r:?}");
    }
}

#[test]
fn same_names_in_other_methods_are_not_exported_as_member_types() {
    let s = "class L { int field; } class C { void a() { class L { int hidden; } L x; } void b() { L y; y.field++; class L {} L z; } }";
    assert_eq!(query(s, "L y").status, ResolveStatus::Resolved);
    assert_eq!(query(s, "field++").status, ResolveStatus::Resolved);
    assert_eq!(query(s, "L z").status, ResolveStatus::Unresolved);
}

#[test]
fn local_types_never_export_across_files() {
    let mut p = Project::new();
    p.insert("C.java", "class C { void a() { @A private class L { int hidden; } } void b() { class L {} } }");
    let source = "class Other { C.L escaped; }";
    p.insert("Other.java", source);
    let r = p.resolve_at("Other.java", source.find("L escaped").unwrap() as u32);
    assert_eq!(r.status, ResolveStatus::Unresolved);
    assert!(r.targets.is_empty());
}

#[test]
fn incremental_type_body_and_header_edits_match_full_targets_and_diagnostics() {
    let source = "class L { int field; } class C { void m() { int before = 0; class 名 { int hidden; } L after; after.field++; before++; } void n() { int later = 0; later++; } }";
    for (needle, replacement) in [
        ("hidden", "秘密"),
        ("class 名", "record 名(int x)"),
        ("class 名", "enum 名"),
        ("class 名 { int hidden; }", "class 名 { class Nested {} }"),
        ("class 名 { int hidden; }", "record 名( {}"),
        ("class 名 { int hidden; }", "class 名 { int hidden;"),
    ] {
        let at = source.find(needle).unwrap();
        let edit = TextEdit { start_byte: at as u32, end_byte: (at + needle.len()) as u32, insert: replacement.into() };
        let expected = source.replacen(needle, replacement, 1);
        let parsed = ParsedFile::parse(source);
        let changed = parsed.apply_edit(edit.clone());
        let verified = parsed.apply_edit_verified(edit.clone());
        let full = ParsedFile::parse(&expected);
        assert!(changed.file.tokens_cover_text());
        assert_eq!(changed.file.dump(), full.dump());
        assert_eq!(verified.file.dump(), full.dump());
        assert_eq!(changed.file.errors(), full.errors());
        let mut edited = Project::new();
        edited.insert("C.java", source);
        edited.edit("C.java", edit).unwrap();
        let mut rebuilt = Project::new();
        rebuilt.insert("C.java", &expected);
        for (byte, ch) in expected.char_indices() {
            if ch.is_alphabetic() {
                assert_eq!(
                    edited.resolve_at("C.java", byte as u32),
                    rebuilt.resolve_at("C.java", byte as u32),
                    "{replacement} at {byte}"
                );
            }
        }
    }
}

#[test]
fn unsupported_diagnostic_spans_the_entire_local_type() {
    let declaration = "@A final class 名 { int hidden; }";
    let source = format!("class C {{ void m() {{ {declaration} int after; }} }}");
    let file = ParsedFile::parse(&source);
    let diagnostic =
        file.errors().into_iter().find(|error| error.message == "local type bindings are unsupported").unwrap();
    assert_eq!(&source[diagnostic.start_byte as usize..diagnostic.end_byte as usize], declaration);
    assert_eq!(diagnostic.end_utf16 - diagnostic.start_utf16, declaration.encode_utf16().count() as u32);
}

#[test]
fn unsupported_class_literals_do_not_become_local_types_during_recovery() {
    for literal in ["int.class", "void.class", "boolean.class", "int[] . /* gap */ class"] {
        let source = format!(
            "class C {{ Object m(Object type) {{ if (type == Integer.class) return (Class<T>) {literal}; int after = 0; after++; return type; }} }}"
        );
        let file = ParsedFile::parse(&source);
        assert!(file.tokens_cover_text());
        assert!(!file.dump().contains("LocalType"), "{literal}: {}", file.dump());
        let result = query(&source, "after++");
        assert_eq!(result.status, ResolveStatus::Resolved);
        assert_eq!(result.targets[0].kind, TargetKind::Local);
    }
}
