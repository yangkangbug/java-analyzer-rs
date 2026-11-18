use java_engine::{ParsedFile, Project, TargetKind, TextEdit};

#[test]
fn single_statement_containers_do_not_leak_names() {
    for (text, name) in [
        ("class C{int i; int f(){for(@ @ int i=0;i<1;i++){} return i;}}", "i"),
        ("class C{int x; int f(){Object r=(@ @ int x)->x;return x;}}", "x"),
    ] {
        let mut p = Project::new();
        p.insert("C.java", text);
        let at = text.rfind("return ").unwrap() + 7;
        let result = p.resolve_at("C.java", at as u32);
        assert_eq!(result.targets.len(), 1);
        assert_eq!(result.targets[0].kind, TargetKind::Field);
        assert_eq!(result.targets[0].start_byte, text.find(&format!("int {name}")).unwrap() as u32 + 4);
    }
}
#[test]
fn every_unexpected_token_remains_an_independent_lossless_error() {
    for count in [1, 2, 32, 11000] {
        let text = format!("class C{{void f(){{{} }} }}", "@ ".repeat(count));
        let f = ParsedFile::parse(&text);
        assert!(f.tokens_cover_text());
        assert_eq!(f.errors().len(), count);
        for (i, error) in f.errors().iter().enumerate() {
            assert_eq!(error.message, "unexpected token");
            assert_eq!(error.start_byte, text.find('@').unwrap() as u32 + 2 * i as u32);
            assert_eq!(error.end_byte, error.start_byte + 1);
        }
    }
}
#[test]
fn edits_fresh_undo_and_old_snapshot_preserve_complete_trees() {
    let original = "class C{int x;void f(){@ @ ; int next;}}";
    let mut p = Project::new();
    p.insert("C.java", original);
    let old = p.clone();
    for prefix in ["@ @ @", "@A @ @", "@ @ int local", "@ @interface A{}", "@ @"] {
        let text = p.file("C.java").unwrap().text().to_owned();
        let start = text.find('@').unwrap();
        let end = text.find(" ;").unwrap();
        p.edit("C.java", TextEdit { start_byte: start as u32, end_byte: end as u32, insert: prefix.into() }).unwrap();
        let f = p.file("C.java").unwrap();
        let fresh = ParsedFile::parse(f.text());
        assert_eq!(f.dump(), fresh.dump());
        assert_eq!(f.errors(), fresh.errors());
        assert!(f.tokens_cover_text());
        assert_eq!(old.file("C.java").unwrap().dump(), ParsedFile::parse(original).dump());
    }
    assert_eq!(p.file("C.java").unwrap().text(), original);
}
