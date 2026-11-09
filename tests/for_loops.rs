use java_engine::{GreenNode, ParsedFile, Project, ResolveStatus, SyntaxKind, TargetKind, TextEdit};

fn count(node: &GreenNode, kind: SyntaxKind) -> usize {
    usize::from(node.kind() == kind) + node.child_nodes().map(|n| count(n, kind)).sum::<usize>()
}

fn check(header: &str, valid: bool) -> ParsedFile {
    let source = format!("class C {{ void m() {{ for ({header}) {{ use(i); }} int after = 2; }} void next() {{}} }}");
    let file = ParsedFile::parse(&source);
    assert!(file.tokens_cover_text(), "{}", file.dump());
    assert_eq!(file.errors().is_empty(), valid, "{header}\n{}\n{:?}", file.dump(), file.errors());
    assert_eq!(count(file.root(), SyntaxKind::Method), 2, "{}", file.dump());
    file
}

#[test]
fn basic_for_expression_lists_and_declarations() {
    for header in [
        ";;",
        "i=0;;",
        ";;i++",
        "int i=0;i<len;i++,offset1++,offset2++",
        "int i=1,j=len2-1;i<=j;i++,j--",
        "i=0,j=1;i<10;i++,j--",
        "start(a,b),j=f(c,d);ready();step(a,b),j+=2",
        "a=new int[]{1,2},b=3;;a[0]++,b--",
        "new C(),new C(a,b){};;++i,--j",
        "final int i=0,j=1;i<j;i++,j--",
        "String value : values",
        "final String value : values",
        "i = (x,y) -> x+y,j=0;;i++,j--",
    ] {
        check(header, true);
    }
}

#[test]
fn list_items_must_be_statement_expressions_and_cannot_be_empty() {
    for header in [
        "i=0,;;",
        "i=0,,j=1;;",
        ",i=0;;",
        ";;i++,",
        ";;,i++",
        ";;i++,,j--",
        "1,2;;",
        "i+j;;",
        ";;i+j",
        "(i++);;",
        "new int[2];;",
        "i::run;;",
        "i=0;i<1,j<2;i++",
        "i=(1,2);;",
        "i=0;;i++ j++",
        "i=;;j++",
    ] {
        check(header, false);
    }
}

#[test]
fn comma_does_not_become_a_general_expression_operator() {
    for source in ["class C { void m() { i++,j++; } }", "class C { int m() { return 1,2; } }"] {
        let file = ParsedFile::parse(source);
        assert!(file.tokens_cover_text());
        assert!(!file.errors().is_empty());
    }
}

#[test]
fn loop_locals_bind_in_each_update_and_end_at_loop() {
    let source = "class C { void m() { for (int i=0,j=1;i<j;i++,j--) { use(i,j); } use(i,j); } }";
    let mut project = Project::new();
    project.insert("C.java", source);
    for needle in ["i<j", "j;i++", "i++", "j--", "i,j);"] {
        let at = source.find(needle).unwrap() as u32;
        let result = project.resolve_at("C.java", at);
        assert_eq!(result.status, ResolveStatus::Resolved, "{needle}: {result:?}");
        assert_eq!(result.targets[0].kind, TargetKind::Local);
    }
    for offset in [0, 2] {
        let at = source.rfind("i,j").unwrap() as u32 + offset;
        assert_eq!(project.resolve_at("C.java", at).status, ResolveStatus::Unresolved);
    }
}

#[test]
fn expression_list_edits_match_full_parse_even_when_incomplete() {
    let source = "class C { void m() { /* 中文😀 */ for (int i=0,j=1;i<j;i++,j--) { use(i,j); } } void next() {} }";
    for (needle, insert) in [
        ("i++,j--", "i++,j--,k++"),
        ("i++,j--", "i++,"),
        ("i++,j--", ""),
        ("i++,j--", "i++,,j--"),
        ("int i=0,j=1", "i=0,j=f(1,2)"),
        ("i<j", "i<j,j>0"),
    ] {
        let file = ParsedFile::parse(source);
        let start = source.find(needle).unwrap() as u32;
        let edit = TextEdit { start_byte: start, end_byte: start + needle.len() as u32, insert: insert.into() };
        let fast = file.apply_edit(edit.clone());
        let verified = file.apply_edit_verified(edit);
        assert!(fast.file.tokens_cover_text());
        assert_eq!(fast.mode, verified.mode);
        assert_eq!(fast.file.root(), verified.file.root());
        assert_eq!(fast.file.root(), ParsedFile::parse(fast.file.text()).root());
    }
}

#[test]
fn list_nodes_keep_exact_utf8_ranges_and_nested_commas() {
    fn find_for(node: &GreenNode, at: u32) -> Option<(&GreenNode, u32)> {
        if node.kind() == SyntaxKind::ForStmt {
            return Some((node, at));
        }
        let mut offset = at;
        for child in node.children() {
            if let Some(node) = child.as_node()
                && let Some(found) = find_for(node, offset)
            {
                return Some(found);
            }
            offset += child.utf8_len();
        }
        None
    }
    let source = "class C { void m() { /* 中文😀 */ for (i=f(1,2), /* α */ j=0; i<j; i++, /* β */ j--) {} } }";
    let file = ParsedFile::parse(source);
    assert!(file.errors().is_empty(), "{}", file.dump());
    assert!(file.tokens_cover_text());
    let (node, mut offset) = find_for(file.root(), 0).unwrap();
    let mut expressions = Vec::new();
    let mut commas = 0;
    for child in node.children() {
        if let Some(n) = child.as_node() {
            if matches!(n.kind(), SyntaxKind::BinaryExpr | SyntaxKind::UnaryExpr) {
                expressions.push(&source[offset as usize..(offset + n.utf8_len()) as usize]);
            }
        } else if child.as_token().unwrap().kind() == SyntaxKind::Comma {
            commas += 1;
        }
        offset += child.utf8_len();
    }
    assert_eq!(expressions, ["i=f(1,2)", "j=0", "i<j", "i++", "j--"]);
    assert_eq!(commas, 2);
    assert_eq!(file.utf16_len() as usize, source.encode_utf16().count());
}

#[test]
fn all_assignment_forms_and_statement_expression_boundaries() {
    for op in ["=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>=", ">>>="] {
        check(&format!("i {op} 1,j {op} 2;;i {op} 3,j {op} 4"), true);
    }
    for expr in ["(x=1)", "(f())", "(new C())", "new C[f()]", "x?f():g()", "x->x", "!x", "+x", "x", "null"] {
        check(&format!("{expr};;"), false);
        check(&format!(";;{expr}"), false);
    }
}

#[test]
fn unfinished_headers_are_lossless_and_keep_following_method() {
    for header in ["i=0,j=", ";;i++,j=", ";;i++,", ";;i++,f(", "i=f(1,2", "i=0,,j=1;;i++"] {
        check(header, false);
    }
    for source in ["class C { void m() { for (", "class C { void m() { for (;;i++,", "class C { void m() { for (i=0,j="]
    {
        let file = ParsedFile::parse(source);
        assert!(file.tokens_cover_text());
        assert!(!file.errors().is_empty());
    }
}

#[test]
fn partial_fragment_consumption_falls_back_without_losing_text() {
    for source in [
        "class C { void m() { for (;;x=new int[]{1,2}) {} } void sibling() {} }",
        "class C { void m() { int[] x={1,2}; } void sibling() {} }",
    ] {
        let start = source.find('1').unwrap() as u32;
        let file = ParsedFile::parse(source);
        assert!(file.errors().is_empty());
        let edit = TextEdit { start_byte: start, end_byte: start, insert: ";".into() };
        let fast = file.apply_edit(edit.clone());
        let verified = file.apply_edit_verified(edit);
        assert!(fast.file.tokens_cover_text(), "{}", fast.file.dump());
        assert_eq!(fast.file.root(), verified.file.root());
        assert_eq!(fast.file.root(), ParsedFile::parse(fast.file.text()).root());
    }
}
