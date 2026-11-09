use java_engine::{GreenNode, ParsedFile, Project, ResolveStatus, SyntaxKind, TargetKind, TextEdit, Utf16Edit};

fn count(node: &GreenNode, kind: SyntaxKind) -> usize {
    usize::from(node.kind() == kind) + node.child_nodes().map(|n| count(n, kind)).sum::<usize>()
}

fn node_ranges(node: &GreenNode, at: u32, kind: SyntaxKind, result: &mut Vec<(u32, u32)>) {
    if node.kind() == kind {
        result.push((at, at + node.utf8_len()));
    }
    let mut offset = at;
    for child in node.children() {
        if let Some(n) = child.as_node() {
            node_ranges(n, offset, kind, result);
        }
        offset += child.utf8_len();
    }
}

fn check(source: &str, valid: bool) -> ParsedFile {
    let file = ParsedFile::parse(source);
    assert!(file.tokens_cover_text(), "{}", file.dump());
    assert_eq!(file.errors().is_empty(), valid, "{}\n{:?}", file.dump(), file.errors());
    assert_eq!(file.utf16_len() as usize, source.encode_utf16().count());
    file
}

#[test]
fn catch_modifiers_annotations_and_union_types() {
    for header in [
        "E e",
        "E e[]",
        "E<T> e",
        "E<int[]> e",
        "E<? extends int[]> e",
        "E<? super p.@A T @B []> e",
        "E e @A []",
        "p.@A E e",
        "E<List<@A T[]>> e",
        "final E e",
        "@Tag final E e",
        "final @Tag E e",
        "@Tag(value = {1, 2}) final p.E e",
        "final E | F e",
        "@Tag E | @Other F e",
    ] {
        let source = format!("class C {{ void m() {{ try {{}} catch ({header}) {{ use(e); }} }} }}");
        let file = check(&source, true);
        assert_eq!(count(file.root(), SyntaxKind::CatchClause), 1);
    }
}

#[test]
fn malformed_headers_keep_body_and_following_declarations() {
    for header in [
        "final",
        "final E",
        "",
        "public E e",
        "final final E e",
        "int e",
        "E<int> e",
        "E<void[]> e",
        "E[] e",
        "E... e",
        "E e, F f",
        "E | e",
        "E | | F e",
        "E e = x",
        "@A( E e",
        "@A(value = E e",
        "E<@A( value e",
        "E<List<@A( value e",
        "E< e",
    ] {
        let source = format!(
            "class C {{ void m() {{ try {{}} catch ({header}) {{ int inside = 1; }} int after = 2; }} void next() {{}} }}"
        );
        let file = check(&source, false);
        assert_eq!(count(file.root(), SyntaxKind::Method), 2, "{header}\n{}", file.dump());
        assert_eq!(count(file.root(), SyntaxKind::LocalVar), 2, "{header}\n{}", file.dump());
        assert_eq!(count(file.root(), SyntaxKind::Block), 4, "{header}\n{}", file.dump());
        let mut catches = Vec::new();
        node_ranges(file.root(), 0, SyntaxKind::CatchClause, &mut catches);
        assert_eq!(catches.len(), 1);
        let (start, end) = catches[0];
        assert_eq!(&source[start as usize..end as usize], format!("catch ({header}) {{ int inside = 1; }}"));
        for error in file.errors() {
            assert!(error.start_byte >= start && error.end_byte <= end);
            assert_eq!(file.byte_to_utf16(error.start_byte), Some(error.start_utf16));
            assert_eq!(file.byte_to_utf16(error.end_byte), Some(error.end_utf16));
        }
    }
}

#[test]
fn missing_delimiters_do_not_invent_a_body() {
    for clause in ["catch (final E e", "catch final E e)", "catch (E e)", "catch ("] {
        let source = format!("class C {{ void m() {{ try {{}} {clause} }} void next() {{}} }}");
        let file = check(&source, false);
        assert_eq!(count(file.root(), SyntaxKind::Method), 2, "{}", file.dump());
        assert_eq!(count(file.root(), SyntaxKind::Block), 3, "{}", file.dump());
    }
    for clause in ["catch (E e)", "catch (E e"] {
        let source = format!("class C {{ void m() {{ try {{}} {clause} int after = make(); }} void next() {{}} }}");
        let file = check(&source, false);
        assert_eq!(count(file.root(), SyntaxKind::LocalVar), 1);
        assert_eq!(count(file.root(), SyntaxKind::Method), 2);
    }
}

#[test]
fn catch_parameter_binding_stays_in_its_catch() {
    let source = "class C { void m() { try { use(e); } catch (@Tag final E e) { use(e); { use(e); } } catch (F other) { use(e); } finally { use(e); } use(e); } }";
    check(source, true);
    let mut project = Project::new();
    project.insert("C.java", source);
    for (i, (at, _)) in source.match_indices("use(e)").enumerate() {
        let result = project.resolve_at("C.java", (at + 4) as u32);
        assert_eq!(
            result.status,
            if i == 1 || i == 2 { ResolveStatus::Resolved } else { ResolveStatus::Unresolved },
            "{i}: {result:?}"
        );
        if i == 1 || i == 2 {
            assert_eq!(result.targets[0].kind, TargetKind::Parameter);
            let target = &result.targets[0];
            assert_eq!(target.start_byte as usize, source.find("E e)").unwrap() + 2);
            assert_eq!(&source[target.start_byte as usize..target.end_byte as usize], "e");
            assert_eq!(project.prepare().resolve_at("C.java", (at + 4) as u32), result);
        }
    }
}

#[test]
fn catch_header_edits_match_full_parse_and_utf16_offsets() {
    let source = "class C { void m() { /* 😀 */ try {} catch (final E e) { use(e); } } void next() {} }";
    let file = check(source, true);
    let start = source.find("final E e").unwrap() as u32;
    for header in ["E e", "@Tag final E | F e", "final", "@A(", "E e, F f", "final E 问题"] {
        let edit = TextEdit { start_byte: start, end_byte: start + 9, insert: header.into() };
        let fast = file.apply_edit(edit.clone());
        let verified = file.apply_edit_verified(edit);
        assert!(fast.file.tokens_cover_text());
        assert_eq!(fast.file.root(), ParsedFile::parse(fast.file.text()).root());
        assert_eq!(fast.file.root(), verified.file.root());
        assert_eq!(fast.mode, verified.mode);
        let utf = file
            .apply_utf16_edit(Utf16Edit {
                start_utf16: file.byte_to_utf16(start).unwrap(),
                end_utf16: file.byte_to_utf16(start + 9).unwrap(),
                insert: header.into(),
            })
            .unwrap();
        assert_eq!(utf.file.root(), fast.file.root());
    }
}

#[test]
fn nested_broken_annotations_do_not_swallow_following_methods() {
    for prefix in ["@A(", "E<@A(", "E<@A(value=", "E<List<@A("] {
        let source = format!(
            "class C {{ void m() {{ try {{}} catch ({prefix} {{ body(); }} finally {{ done(); }} after(); }} void tail() {{}} }}"
        );
        let file = check(&source, false);
        assert_eq!(count(file.root(), SyntaxKind::Method), 2, "{}", file.dump());
    }
}

#[test]
fn ordinary_formal_parameter_shapes_remain_unchanged() {
    check("class C { C(final E[] a) {} void m(final E... a) {} void n(@Tag E a[]) {} }", true);
}

#[test]
fn catch_parameter_unicode_ranges_are_exact() {
    let source = "class C { void m() { /* 😀 */ try {} catch (final E 问题) { use(问题); } } }";
    let file = check(source, true);
    let mut project = Project::new();
    project.insert("C.java", source);
    let result = project.resolve_at("C.java", (source.find("use(问题)").unwrap() + 4) as u32);
    assert_eq!(result.status, ResolveStatus::Resolved);
    let target = &result.targets[0];
    assert_eq!(target.start_byte as usize, source.find("问题").unwrap());
    assert_eq!(&source[target.start_byte as usize..target.end_byte as usize], "问题");
    assert_eq!(file.byte_to_utf16(target.end_byte).unwrap() - file.byte_to_utf16(target.start_byte).unwrap(), 2);
}
