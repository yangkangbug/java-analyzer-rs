use java_engine::{GreenNode, ParsedFile, Project, ResolveStatus, SyntaxKind, TextEdit};

fn has_ident(node: &GreenNode, name: &str) -> bool {
    node.children().iter().any(|c| c.as_token().is_some_and(|t| t.kind() == SyntaxKind::Ident && t.text() == name))
        || node.child_nodes().any(|n| has_ident(n, name))
}
fn declaration(node: &GreenNode, kind: SyntaxKind, name: &str) -> bool {
    node.kind() == kind && has_ident(node, name) || node.child_nodes().any(|n| declaration(n, kind, name))
}

#[test]
fn qualified_invocation_types_have_lossless_structure() {
    for args in [
        "String",
        "String, Integer",
        "java.util.List<String>",
        "Map<String, List<?>>",
        "int[]",
        "boolean[][]",
        "List<?>[]",
        "List<? extends Number>",
        "List<? extends String[]>",
        "List<? super String>",
        "@A String",
        "@A(void.class) String",
        "Outer<String>.Inner<Integer>",
    ] {
        let source = format!("class C{{void run(){{api.<{args}>call();int after=1;}}void later(){{}}}}");
        let file = ParsedFile::parse(&source);
        assert!(file.errors().is_empty(), "{args}: {:?}", file.errors());
        assert!(file.tokens_cover_text());
        assert!(file.dump().contains("TypeArgList"));
        assert!(declaration(file.root(), SyntaxKind::LocalVar, "after"));
        assert!(declaration(file.root(), SyntaxKind::Method, "later"));
    }
}

#[test]
fn invocation_type_arguments_reject_invalid_shapes() {
    for args in [
        "",
        "?",
        "? extends String",
        "int",
        "void",
        "void[]",
        "String,",
        "List<int>",
        "List<void[]>",
        "List<>",
        "List<String,>",
        "List<?[]>",
    ] {
        let source = format!("class C{{void run(){{api.<{args}>call();int after=1;}}void later(){{}}}}");
        let file = ParsedFile::parse(&source);
        assert!(!file.errors().is_empty(), "{args}");
        assert!(file.tokens_cover_text());
        assert!(declaration(file.root(), SyntaxKind::LocalVar, "after"), "{}", file.dump());
        assert!(declaration(file.root(), SyntaxKind::Method, "later"));
    }
}

#[test]
fn explicit_invocation_is_not_silently_erased_by_navigation() {
    for expression in [
        "api.<String>make()",
        "api.<String>make().member",
        "api.<String>make.member",
        "api.<String>make.member.member",
        "(api.<String>make()).member",
        "api.<String>make()[0].member",
    ] {
        let source =
            format!("class Api{{Api make(){{return null;}}int member;}}class C{{void run(Api api){{{expression};}}}}");
        let mut project = Project::new();
        project.insert("C.java", &source);
        for needle in ["make", "member"] {
            if let Some(at) = source.rfind(needle).filter(|at| *at > source.find("void run").unwrap()) {
                assert_ne!(
                    project.resolve_at("C.java", at as u32).status,
                    ResolveStatus::Resolved,
                    "{expression} {needle}"
                );
            }
        }
        let receiver_at = source.rfind("api.<").unwrap() as u32;
        assert_eq!(project.resolve_at("C.java", receiver_at).status, ResolveStatus::Resolved);
    }
}

#[test]
fn invocation_type_edits_match_fresh_parse() {
    let source = "class C{void run(){api.<String>call();int after=1;}void later(){}}";
    let mut file = ParsedFile::parse(source);
    for replacement in ["Map<String,List<?>>", "List<", "", "int[]", "String"] {
        let at = file.text().find(".<").unwrap() + 2;
        let end = file.text().find(">call").unwrap();
        let changed =
            file.apply_edit(TextEdit { start_byte: at as u32, end_byte: end as u32, insert: replacement.into() });
        assert!(changed.file.tokens_cover_text());
        assert_eq!(changed.file.root().as_ref(), ParsedFile::parse(changed.file.text()).root().as_ref());
        file = changed.file;
    }
    assert!(file.errors().is_empty());
    assert_eq!(file.root().kind(), SyntaxKind::File);
}
