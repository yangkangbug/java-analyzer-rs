use java_engine::{GreenNode, ParsedFile, SyntaxKind, TextEdit, UpdateMode};
use std::sync::Arc;

fn compare(original: &ParsedFile, edit: TextEdit) -> ParsedFile {
    let old_root = Arc::clone(original.root());
    let old_text = original.text().to_owned();
    let changed = original.apply_edit(edit);
    let full = ParsedFile::parse(changed.file.text());
    assert!(changed.file.tokens_cover_text());
    assert_eq!(changed.file.root(), full.root(), "{}", changed.file.text());
    assert_eq!(changed.file.errors(), full.errors());
    assert!(Arc::ptr_eq(original.root(), &old_root));
    assert_eq!(original.text(), old_text);
    changed.file
}

#[test]
fn recovering_fragment_must_own_its_boundary_braces() {
    let source = "class C {void m(){try{int n=0;}catch(Exception e){int m=1;}finally{int z=0;} int after=0;}}";
    let original = ParsedFile::parse(source);
    assert!(original.errors().is_empty());
    let at = source.find("finally{").unwrap() + "finally".len();
    // These edits make recovery consume the candidate's ending brace in a
    // descendant. Lexical balance and full fragment length alone are insufficient.
    for inserted in ["/", "*", "\\", ";", ")", "->"] {
        let changed =
            compare(&original, TextEdit { start_byte: at as u32, end_byte: at as u32, insert: inserted.into() });
        assert!(!changed.errors().is_empty());
        let restored = compare(
            &changed,
            TextEdit { start_byte: at as u32, end_byte: (at + inserted.len()) as u32, insert: String::new() },
        );
        assert_eq!(restored.root(), original.root());
    }
}

#[test]
fn annotation_blocks_recheck_declaration_and_catch_context() {
    // Annotation element validity is not established by these syntax fixtures.
    // The production edit path must match fresh recovery for malformed input too.
    for source in [
        "@A(x=()->{int z=0;}) class C {void m(){int n=0;}}",
        "class C {@A(x=()->{int z=0;}) void m(){int n=0;}}",
        "class C {void m(){@A(x=()->{int z=0;}) int n=0;}}",
        "class C {void m(){try{}catch(@A(x=()->{int z=0;}) Exception e){int n=0;}}}",
    ] {
        let original = ParsedFile::parse(source);
        let at = source.find("int z").unwrap();
        for inserted in ["(", ")", "/*中😀*/", "int = ;"] {
            let changed =
                compare(&original, TextEdit { start_byte: at as u32, end_byte: at as u32, insert: inserted.into() });
            let restored = compare(
                &changed,
                TextEdit { start_byte: at as u32, end_byte: (at + inserted.len()) as u32, insert: String::new() },
            );
            assert_eq!(restored.root(), original.root());
        }
    }
}

fn nodes(node: &Arc<GreenNode>, kind: SyntaxKind, out: &mut Vec<Arc<GreenNode>>) {
    if node.kind() == kind {
        out.push(Arc::clone(node));
    }
    for child in node.child_nodes() {
        nodes(child, kind, out);
    }
}

#[test]
fn ordinary_method_body_keeps_incremental_mode_and_exact_sibling_sharing() {
    let source = "@A class C {void first(){int n=1;} void second(){int z=0;}}";
    let original = ParsedFile::parse(source);
    let at = source.find("n=1").unwrap() + 2;
    let changed = original.apply_edit(TextEdit { start_byte: at as u32, end_byte: at as u32 + 1, insert: "2".into() });
    assert_eq!(changed.mode, UpdateMode::Incremental { kind: SyntaxKind::Block });
    assert_eq!(changed.file.root(), ParsedFile::parse(changed.file.text()).root());
    let (mut before, mut after) = (vec![], vec![]);
    nodes(original.root(), SyntaxKind::Method, &mut before);
    nodes(changed.file.root(), SyntaxKind::Method, &mut after);
    assert_eq!(before.len(), 2);
    assert_eq!(after.len(), 2);
    assert!(Arc::ptr_eq(&before[1], &after[1]));
}

#[test]
fn bounded_edit_and_undo_matrix_matches_fresh_without_verified_fallback() {
    let seeds = [
        "class C {void m(){int n=0; {n++;} int after=0;}}",
        "class C {void m(){if(ok){int n=0;}else{int m=1;} int after=0;}}",
        "class C {void m(){try{int n=0;}catch(Exception e){int m=1;}finally{int z=0;} int after=0;}}",
        "class C {void m(){for(int i=0;i<3;i++){int n=0;} int after=0;}}",
        "class C {void m(){while(ok){int n=0;}do{int m=1;}while(ok); int after=0;}}",
        "class C {void m(){synchronized(lock){int n=0;} int after=0;}}",
        "class C {void m(){Runnable r=()->{int n=0;}; int after=0;}}",
        "class C {void m(){Object x=new C(){int n=0;void f(){int z=0;}}; int after=0;}}",
        "class C {void m(){class L {int n=0;void f(){int z=0;}} int after=0;}}",
        "class C {void m(){try{int n=0;}catch(@A(x=()->{int z=0;}) Exception e){int m=1;} int after=0;}}",
        "@A(x=()->{int z=0;}) class C {void m(){int n=0;}}",
        "enum C {A{void m(){int n=0;}}, B; void f(){int z=0;}}",
        "record C(int x){C {int n=0;} void f(){int z=0;}}",
        "class C {int[] a={1,2}; static {int n=0;} {int m=1;} void f(){String s=\"hello\";}}",
        "interface C {default void f(){int n=0;} class N {void f(){int z=0;}}}",
        "class C {void m(){switch(k){case 0:{int n=0;}break;default:break;} int after=0;}}",
    ];
    for source in seeds {
        let original = ParsedFile::parse(source);
        for (at, ch) in source.char_indices() {
            for inserted in
                ["", "/", "*", "\"", "'", "\\", "\n", "\r\n", "null", ";", "}", "{", "(", ")", "->", "/*中😀*/"]
            {
                for len in [0, ch.len_utf8()] {
                    let changed = compare(
                        &original,
                        TextEdit { start_byte: at as u32, end_byte: (at + len) as u32, insert: inserted.into() },
                    );
                    let restored = compare(
                        &changed,
                        TextEdit {
                            start_byte: at as u32,
                            end_byte: (at + inserted.len()) as u32,
                            insert: source[at..at + len].into(),
                        },
                    );
                    assert_eq!(restored.root(), original.root());
                }
            }
        }
    }
}

#[test]
fn damaged_trees_can_take_further_edits_before_context_is_repaired() {
    for (source, marker, damage) in [
        ("class C {void m(){@A(x=()->{int z=0;}) int n=0;}}", "int z", "("),
        ("class C {void m(){try{}finally{int z=0;} int n=0;}}", "{int z", ";"),
    ] {
        let original = ParsedFile::parse(source);
        let at = source.find(marker).unwrap();
        let broken = compare(&original, TextEdit { start_byte: at as u32, end_byte: at as u32, insert: damage.into() });
        assert!(!broken.errors().is_empty());
        let value = broken.text().find("z=0").unwrap() + 2;
        let changed = compare(
            &broken,
            TextEdit { start_byte: value as u32, end_byte: value as u32 + 1, insert: "2; int extra=3".into() },
        );
        assert!(!changed.errors().is_empty());
        let repaired = compare(
            &changed,
            TextEdit { start_byte: at as u32, end_byte: (at + damage.len()) as u32, insert: String::new() },
        );
        let expected = source.replacen("z=0", "z=2; int extra=3", 1);
        assert_eq!(repaired.text(), expected);
        assert_eq!(repaired.root(), ParsedFile::parse(&expected).root());
        assert!(repaired.errors().is_empty());
        assert_eq!(original.text(), source);
    }
}
