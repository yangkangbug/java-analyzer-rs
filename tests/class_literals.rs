use java_engine::{GreenNode, ParsedFile, Project, ResolveStatus, SyntaxKind, TextEdit};

fn class_literal_count(node: &GreenNode) -> usize {
    if node.kind() == SyntaxKind::ClassLiteralExpr {
        assert!(node.child_nodes().next().is_none(), "class literal must not wrap an ordinary value expression");
        1
    } else {
        node.child_nodes().map(|child| class_literal_count(child)).sum()
    }
}

fn clean(source: &str, literals: usize) -> ParsedFile {
    let file = ParsedFile::parse(source);
    assert!(file.tokens_cover_text(), "{}", file.dump());
    assert!(file.errors().is_empty(), "{:?}\n{}", file.errors(), file.dump());
    assert_eq!(class_literal_count(file.root()), literals, "{}", file.dump());
    file
}

#[test]
fn gson_style_primitive_and_void_class_arguments() {
    clean(
        "class C { void m() { add(int.class, Integer.class); add(void.class, Void.class); int after = 1; after++; } void next() {} }",
        2,
    );
}

#[test]
fn all_primitive_ranks_trivia_and_expression_positions() {
    for primitive in ["byte", "short", "int", "long", "float", "double", "boolean", "char"] {
        for rank in ["", "[]", "[][]", " /*中😀*/ [ /*a*/ ] [ ]"] {
            let literal = format!("{primitive}{rank} /*dot*/ . /*class*/ class");
            clean(
                &format!(
                    "class C {{ Object field = {literal}; Object m() {{ Object x = ({literal}); x = (Object) {literal}; use({literal}); return {literal}; }} }}"
                ),
                5,
            );
        }
    }
    clean("class C { Object m() { return void /*a*/ . class; } }", 1);
    clean("class C { Object f = () -> int.class; Object g = true ? int.class : void.class; }", 3);
    clean("class C { void m() { int.class.getName(); int[][].class.getName(); } }", 2);
}

#[test]
fn malformed_tails_do_not_commit_class_literals() {
    for literal in
        ["void[].class", "int[1].class", "int.classX", "int.", "int[]", "int[.class", "int[] class", "int", "void"]
    {
        let source = format!("class C {{ void m() {{ use({literal}); int after = 1; after++; }} }}");
        let file = ParsedFile::parse(&source);
        assert!(file.tokens_cover_text(), "{literal}");
        assert!(!file.errors().is_empty(), "{literal}");
        assert!(!file.dump().contains("ClassLiteralExpr"), "{literal}: {}", file.dump());
    }
}

#[test]
fn incremental_class_literal_edits_equal_full_parse() {
    for replacement in
        ["boolean[][].class", "void.class", "int[1].class", "void[].class", "int.classX", "int /*中😀*/ [].class"]
    {
        let source = "class C { void m() { use(int.class); int after = 1; after++; } void next() {} }";
        let file = ParsedFile::parse(source);
        let at = source.find("int.class").unwrap() as u32;
        let result = file.apply_edit(TextEdit { start_byte: at, end_byte: at + 9, insert: replacement.into() });
        let full = ParsedFile::parse(&source.replacen("int.class", replacement, 1));
        assert!(result.file.tokens_cover_text());
        assert_eq!(result.file.dump(), full.dump(), "{replacement}");
        assert_eq!(result.file.errors(), full.errors(), "{replacement}");
    }
}

#[test]
fn class_literals_do_not_supply_primitive_overload_proofs() {
    let source = "class C { static void pick(int x) {} static void pick(long x) {} static void m(int value) { pick(value); pick(int.class); pick(int[].class); pick(void.class); int after = 1; after++; } }";
    clean(source, 3);
    let mut p = Project::new();
    p.insert("C.java", source);
    // Minimal public binary Object root, assembled without a JVM.
    let mut object = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61, 0, 3, 1, 0, 16];
    object.extend(b"java/lang/Object");
    object.extend([7, 0, 1, 0, 0x21, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    p.add_class_bytes("Object.class", &object).unwrap();
    let resolved = |marker: &str| {
        let at = source.find(marker).unwrap() as u32;
        let value = p.resolve_at("C.java", at);
        assert_eq!(value, p.prepare().resolve_at("C.java", at));
        value
    };
    assert_eq!(resolved("pick(value)").status, ResolveStatus::Resolved);
    for marker in ["pick(int.class)", "pick(int[].class)", "pick(void.class)"] {
        let result = resolved(marker);
        assert_eq!(result.status, ResolveStatus::OverloadNotResolved, "{marker}");
        assert_eq!(result.targets.len(), 2, "{marker}");
    }
    for marker in ["int.class", "int[].class", "void.class"] {
        let result = resolved(marker);
        assert_eq!(result.status, ResolveStatus::Unresolved, "{marker}");
        assert!(result.targets.is_empty());
    }
    assert_eq!(resolved("after++").status, ResolveStatus::Resolved);
}

#[test]
fn reference_class_syntax_keeps_existing_field_representation() {
    let file = clean("class C { void m() { use(Integer.class, pkg.Type.class); } }", 0);
    assert!(file.dump().contains("FieldExpr"));
}

#[test]
fn class_literal_receivers_never_fall_back_to_same_named_values() {
    for expression in [
        "int.class.getName()",
        "int[].class.getName()",
        "void.class.getName()",
        "(int.class).getName()",
        "((int[][].class)).getName()",
        "((Object) int.class).getName()",
        "int.class.getName().getName()",
        "int.class[0].getName()",
        "int.class::getName",
        "(int[].class)::getName",
    ] {
        let source = format!("class C {{ void m(int getName) {{ use({expression}); }} }}");
        clean(&source, 1);
        let mut p = Project::new();
        p.insert("C.java", &source);
        let start = source.find("use(").unwrap();
        for (offset, _) in source[start..].match_indices("getName") {
            let at = (start + offset) as u32;
            let result = p.resolve_at("C.java", at);
            assert_eq!(result, p.prepare().resolve_at("C.java", at));
            assert_eq!(result.status, ResolveStatus::ReceiverNotBound, "{expression}: {result:?}");
            assert!(result.targets.is_empty());
        }
    }
}

#[test]
fn class_literal_arguments_do_not_poison_normal_receivers() {
    let source = "class Box { void take(Object x) {} } class C { void m(Box box) { box.take(int.class); } }";
    clean(source, 1);
    let mut p = Project::new();
    p.insert("C.java", source);
    for marker in ["box.take", "take(int.class)"] {
        let at = source.find(marker).unwrap() as u32;
        let result = p.resolve_at("C.java", at);
        assert_eq!(result, p.prepare().resolve_at("C.java", at));
        let expected = if marker == "box.take" { ResolveStatus::Resolved } else { ResolveStatus::CallNotResolved };
        assert_eq!(result.status, expected, "{marker}: {result:?}");
        assert_eq!(result.targets.len(), 1);
        assert_eq!(result.targets[0].name, if marker == "box.take" { "box" } else { "take" });
    }
}

#[test]
fn conditional_class_literal_results_do_not_become_same_named_values() {
    for declaration in ["int getName; void m() {", "void m(int getName) {"] {
        for expression in [
            "(true ? int.class : long.class).getName()",
            "(flag ? other : int.class).getName()",
            "(flag ? int.class : other)::getName",
            "(flag ? (otherFlag ? int.class : other) : other).getName()",
        ] {
            let source = format!("class C {{ {declaration} use({expression}); }} }}");
            let file = ParsedFile::parse(&source);
            assert!(file.errors().is_empty(), "{:?}", file.errors());
            let mut p = Project::new();
            p.insert("C.java", &source);
            let at = source.rfind("getName").unwrap() as u32;
            let result = p.resolve_at("C.java", at);
            assert_eq!(result, p.prepare().resolve_at("C.java", at));
            assert_eq!(result.status, ResolveStatus::ReceiverNotBound, "{source}: {result:?}");
            assert!(result.targets.is_empty());
        }
    }
}

#[test]
fn assigned_class_literal_result_does_not_become_same_named_value() {
    for declaration in ["int getName; void m() {", "void m(int getName) {"] {
        let source = format!("class C {{ {declaration} Object clazz; use((clazz = int.class).getName()); }} }}");
        clean(&source, 1);
        let mut p = Project::new();
        p.insert("C.java", &source);
        let at = source.rfind("getName").unwrap() as u32;
        let result = p.resolve_at("C.java", at);
        assert_eq!(result, p.prepare().resolve_at("C.java", at));
        assert_eq!(result.status, ResolveStatus::ReceiverNotBound, "{source}: {result:?}");
        assert!(result.targets.is_empty());
    }
}
