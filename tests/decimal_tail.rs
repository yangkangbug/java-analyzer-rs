use java_engine::{ParsedFile, Project, ResolveStatus, SyntaxKind, TextEdit, lex};
fn tokens(text: &str) -> Vec<(SyntaxKind, &str)> {
    lex(text).into_iter().map(|t| (t.kind, &text[t.start as usize..t.end as usize])).collect()
}
#[test]
fn decimal_tail_and_separator_boundaries_are_lossless() {
    for text in
        ["1.", "1.d", "1.e2", "1.e+2", "1.e-2", "1_2.", "1__2.", "1.2__3", "1.e1__2", "1.2_3", "1.e1_2", ".5", "1.2"]
    {
        assert_eq!(tokens(text), vec![(SyntaxKind::DoubleLiteral, text)], "{text}");
    }
    assert_eq!(tokens("1.f"), vec![(SyntaxKind::FloatLiteral, "1.f")]);
    for text in ["1._2", "1_.", "1.e_2", "1.e2_", "1.2_", "1_f"] {
        assert_eq!(tokens(text), vec![(SyntaxKind::ErrorToken, text)], "{text}");
    }
    assert_eq!(
        tokens("1..x"),
        vec![(SyntaxKind::DoubleLiteral, "1."), (SyntaxKind::Dot, "."), (SyntaxKind::Ident, "x")]
    );
    assert_eq!(tokens("1.foo"), vec![(SyntaxKind::FloatLiteral, "1.f"), (SyntaxKind::Ident, "oo")]);
    assert_eq!(tokens("1..."), vec![(SyntaxKind::DoubleLiteral, "1."), (SyntaxKind::Dot, "."), (SyntaxKind::Dot, ".")]);
    for text in ["1./*c*/+2", "1. +2", "1.e+", "1.l"] {
        assert_eq!(tokens(text).iter().map(|(_, t)| *t).collect::<String>(), text);
    }
    assert_eq!(
        tokens("0x1.p2"),
        vec![(SyntaxKind::IntLiteral, "0x1"), (SyntaxKind::Dot, "."), (SyntaxKind::Ident, "p2")]
    );
    assert_eq!(tokens("0b1"), vec![(SyntaxKind::IntLiteral, "0b1")]);
}
#[test]
fn trailing_dot_is_arithmetic_not_a_field_selector() {
    let p = ParsedFile::parse("class C{void f(float v){if(v-1.<1.){}}}");
    assert!(p.errors().is_empty(), "{:?}", p.errors());
    assert!(!p.dump().contains("FieldExpr"));
    assert!(p.dump().contains("DoubleLiteral[1.]"));
    // 1.x is now a double token followed by an identifier, not an invented
    // field selector. The pre-existing 1..x binder gap is recorded separately.
    let source = "class C{int x;double value=1.x;}";
    let mut project = Project::new();
    project.insert("C.java", source);
    assert_ne!(project.resolve_at("C.java", source.rfind("x;").unwrap() as u32).status, ResolveStatus::Resolved);

    for text in ["class C{double x=1.foo;}", "class C{double x=1.e+;}", "class C{double x=1.l;}"] {
        assert!(!ParsedFile::parse(text).errors().is_empty(), "{text}");
    }
}
#[test]
fn local_numeric_edits_match_fresh_and_preserve_old_snapshot() {
    let start = "class C{/*界*/double x=1.;double f(){return x;}}";
    let mut p = Project::new();
    p.insert("C.java", start);
    let old = p.clone();
    let at = start.find("1.").unwrap() as u32;
    let mut width = 2;
    for replacement in ["1", "1.", "1.f", "1._2", "1.e+", "1.e+2", "1.0", "1."] {
        p.edit("C.java", TextEdit { start_byte: at, end_byte: at + width, insert: replacement.into() }).unwrap();
        width = replacement.len() as u32;
        let text = p.file("C.java").unwrap().text();
        let fresh = ParsedFile::parse(text);
        assert_eq!(p.file("C.java").unwrap().dump(), fresh.dump());
        assert_eq!(p.file("C.java").unwrap().errors(), fresh.errors());
        let mut q = Project::new();
        q.insert("C.java", text);
        let pos = text.rfind("x;").unwrap() as u32;
        assert_eq!(p.prepare().resolve_at("C.java", pos), q.resolve_at("C.java", pos));
        assert_eq!(old.file("C.java").unwrap().text(), start);
    }
}
#[test]
fn floating_tail_never_becomes_a_strict_int_actual() {
    for literal in ["1.", "1.f", "1.d", "1.e2"] {
        let text = format!(
            "class C{{static void pick(int x){{}} static void pick(double x){{}} static void run(){{pick({literal});}}}}"
        );
        let mut p = Project::new();
        let mut object = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61, 0, 3, 1, 0, 16];
        object.extend(b"java/lang/Object");
        object.extend([7, 0, 1, 0, 0x21, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        p.add_class_bytes("Object.class", &object).unwrap();
        p.insert("C.java", &text);
        let r = p.resolve_at("C.java", text.rfind("pick(").unwrap() as u32);
        assert_ne!(r.status, ResolveStatus::Resolved, "{literal}: {r:?}");
    }
}
