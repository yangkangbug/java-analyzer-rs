use java_engine::{ParsedFile, Project, ResolveStatus, TextEdit};

fn query(s: &str, marker: &str) -> java_engine::ResolveResult {
    let mut p = Project::new();
    p.insert("C.java", s);
    let at = s.find(marker).unwrap() as u32;
    let r = p.resolve_at("C.java", at);
    assert_eq!(r, p.prepare().resolve_at("C.java", at));
    r
}
#[test]
fn traditional_switch_structure_and_shared_scope() {
    let s = "class C { void m(int k) { switch(k) { case 0: case 1: int 名 = 1; break; case 2: 名++; { int nested = 0; nested++; } break; default: break; } 名++; } }";
    let p = ParsedFile::parse(s);
    assert!(p.errors().is_empty(), "{}", p.dump());
    assert!(p.dump().contains("SwitchStmt"));
    assert!(p.dump().contains("SwitchBody"));
    assert_eq!(query(s, "名++; {").status, ResolveStatus::Resolved);
    assert_eq!(query(s, "名++; } }").status, ResolveStatus::Unresolved);
    assert_eq!(query(s, "nested++;").status, ResolveStatus::Resolved);
    assert!(p.tokens_cover_text());
}
#[test]
fn labels_do_not_bind_as_ordinary_values_but_selector_and_body_do() {
    let s = "class C { void m(int k, int VALUE) { switch(k) { case VALUE: k++; break; default: k++; } } }";
    assert_eq!(query(s, "VALUE:").status, ResolveStatus::Unresolved);
    assert_eq!(query(s, "k) {").status, ResolveStatus::Resolved);
    assert_eq!(query(s, "k++; break").status, ResolveStatus::Resolved);
}
#[test]
fn unsupported_switches_are_opaque_and_do_not_export_locals() {
    for label in ["case 1 ->", "case 1, 2:", "case String value:", "case null:", "case String value when true:"] {
        let s = format!(
            "class C {{ void m(int k) {{ switch(k) {{ {label} {{ int hidden=0; hidden++; }} }} int after=1; after++; }} }}"
        );
        let p = ParsedFile::parse(&s);
        assert!(!p.errors().is_empty(), "{label}");
        assert!(p.tokens_cover_text(), "{label}");
        assert_eq!(query(&s, "hidden++;").status, ResolveStatus::Unresolved, "{label}");
        assert_eq!(query(&s, "after++;").status, ResolveStatus::Resolved, "{label}");
    }
    for expr in [
        "return switch(k) { case 0: int hidden=0; yield hidden; default: yield 1; };",
        "int x=switch(k) { case 0 -> { int hidden=0; yield hidden; } default -> 1; };",
        "use(switch(k) { case 0: int hidden=0; yield hidden; });",
        "int x=(switch(k) { case 0: int hidden=0; yield hidden; });",
    ] {
        let s = format!("class C {{ int m(int k) {{ {expr} int after=1; after++; }} }}");
        let p = ParsedFile::parse(&s);
        assert!(!p.errors().is_empty(), "{expr}");
        assert!(p.tokens_cover_text(), "{expr}");
        assert_eq!(query(&s, "hidden;").status, ResolveStatus::Unresolved, "{expr}");
        assert_eq!(query(&s, "after++;").status, ResolveStatus::Resolved, "{expr}");
        assert!(!p.dump().contains("SwitchStmt"), "{expr}: {}", p.dump());
    }
}
#[test]
fn nested_switch_local_types_and_recovery_stay_contained() {
    let s = "class C { void m(int k) { switch(k) { case 0: switch(k) { case 1: k++; break; default: break; } break; default: { class Local { int secret; } int after=0; after++; } } int outside=0; outside++; } }";
    let p = ParsedFile::parse(s);
    assert!(p.tokens_cover_text());
    assert_eq!(p.dump().matches("SwitchStmt").count(), 2);
    assert_eq!(query(s, "secret;").status, ResolveStatus::Unresolved);
    assert_eq!(query(s, "after++;").status, ResolveStatus::Resolved);
    assert_eq!(query(s, "outside++;").status, ResolveStatus::Resolved);
    for body in [
        "case : k++;",
        "case 0 k++;",
        "default k++;",
        "k++; case 0: break;",
        "case 0: switch(k) { case String s: int hidden=0; } break;",
    ] {
        let s = format!("class C {{ void m(int k) {{ switch(k) {{ {body} }} int after=0; after++; }} }}");
        let p = ParsedFile::parse(&s);
        assert!(!p.errors().is_empty(), "{body}");
        assert!(p.tokens_cover_text());
        assert_eq!(query(&s, "after++;").status, ResolveStatus::Resolved, "{body}");
    }
}
#[test]
fn switch_edits_match_full_parse_including_utf8_and_unsupported_transitions() {
    let s = "class C { void m(int k) { switch(k) { case 0: int 名=0; 名++; break; default: { k++; } } } }";
    for (needle, replacement) in [
        ("case 0:", "case 1:"),
        ("case 0:", "case 0 ->"),
        ("名=0", "名字=0"),
        ("k++;", "switch(k) { default: break; }"),
        ("case 0:", "case String value:"),
        ("break;", "break label;"),
        ("default:", "case true ? 1 : 2:"),
    ] {
        let at = s.find(needle).unwrap();
        let p = ParsedFile::parse(s);
        let changed = p.apply_edit(TextEdit {
            start_byte: at as u32,
            end_byte: (at + needle.len()) as u32,
            insert: replacement.into(),
        });
        let expected = s.replacen(needle, replacement, 1);
        let full = ParsedFile::parse(&expected);
        assert!(changed.file.tokens_cover_text());
        assert_eq!(changed.file.dump(), full.dump(), "{needle} => {replacement}");
    }
}

#[test]
fn opaque_switch_expression_skips_selector_braces_and_preserves_next_statement() {
    for selector in ["new C() { int member; }.hashCode()", "call(new int[] {1, 2})"] {
        let s = format!(
            "class C {{ void m() {{ int x=switch({selector}) {{ case 0: int hidden=0; yield hidden; default: yield 1; }}; int after=0; after++; }} }}"
        );
        let p = ParsedFile::parse(&s);
        assert!(p.tokens_cover_text());
        assert!(!p.errors().is_empty());
        assert!(!p.dump().contains("SwitchStmt"));
        assert_eq!(query(&s, "hidden;").status, ResolveStatus::Unresolved);
        assert_eq!(query(&s, "after++;").status, ResolveStatus::Resolved);
    }
}

#[test]
fn switch_local_type_shadowing_ends_at_switch_body() {
    let s =
        "class L {} class C { void m(int k) { switch(k) { case 0: class L {} case 1: L inside; break; } L outside; } }";
    assert_eq!(query(s, "L inside").status, ResolveStatus::Unresolved);
    assert_eq!(query(s, "L outside").status, ResolveStatus::Resolved);
}

#[test]
fn switch_body_bounded_edit_matrix_matches_full_parse() {
    let s = "class C { void m(int k) { switch(k) { case 0: { int 名=0; 名++; } break; case 1: switch(k) { default: break; } default: break; } int after=0; } }";
    let p = ParsedFile::parse(s);
    let begin = s.find("case 0").unwrap();
    let end = s.find(" int after").unwrap();
    for (at, ch) in s.char_indices().filter(|(at, _)| *at >= begin && *at < end) {
        for replacement in ["", ":", "}", "->", "/*文*/"] {
            let edit =
                TextEdit { start_byte: at as u32, end_byte: (at + ch.len_utf8()) as u32, insert: replacement.into() };
            let changed = p.apply_edit(edit);
            let expected = format!("{}{}{}", &s[..at], replacement, &s[at + ch.len_utf8()..]);
            let full = ParsedFile::parse(&expected);
            assert!(changed.file.tokens_cover_text(), "at={at} replacement={replacement}");
            assert_eq!(changed.file.dump(), full.dump(), "at={at} replacement={replacement}");
        }
    }
}

#[test]
fn selector_block_edits_recheck_the_switch_commit_boundary() {
    let s = "class C { void m() { switch(call(() -> { return 1; })) { case 0: break; } } }";
    let p = ParsedFile::parse(s);
    assert!(p.errors().is_empty(), "{}", p.dump());
    for replacement in ["int = ;", "return 2;", "switch(k) { case String s: break; }"] {
        let needle = "return 1;";
        let at = s.find(needle).unwrap();
        let changed = p.apply_edit(TextEdit {
            start_byte: at as u32,
            end_byte: (at + needle.len()) as u32,
            insert: replacement.into(),
        });
        let full = ParsedFile::parse(&s.replacen(needle, replacement, 1));
        assert!(changed.file.tokens_cover_text());
        assert_eq!(changed.file.dump(), full.dump(), "{replacement}");
    }
}
