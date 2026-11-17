// JLS 9.7.1/9.7.3 list syntax, not annotation value/type/target semantics.
// https://docs.oracle.com/javase/specs/jls/se21/html/jls-9.html#jls-9.7.1
use java_engine::{ParsedFile, Project, ResolveStatus, TargetKind, TextEdit};

#[test]
fn annotation_argument_lists_preserve_named_single_and_array_forms() {
    for annotation in [
        "@A",
        "@A()",
        "@A(1)",
        "@A(value=1)",
        "@A(value=1, other=2)",
        "@A({1,2,})",
        "@A(value={1,2,}, other=@B(3))",
        "@pkg.A(x=Object.class,y=E.VALUE)",
    ] {
        let s = format!("{annotation} class C {{ int sentinel; }}");
        let p = ParsedFile::parse(&s);
        assert!(p.tokens_cover_text());
        assert!(p.errors().is_empty(), "{annotation}: {:?}", p.errors());
    }
}

#[test]
fn annotation_argument_lists_reject_trailing_or_mixed_entries_losslessly() {
    for annotation in [
        "@A(1,)",
        "@A(value=1,)",
        "@A(1,2)",
        "@A(1,other=2)",
        "@A(value=1,2)",
        "@A(value=1,,other=2)",
        "@A(value=)",
        "@A(,)",
    ] {
        let s = format!("{annotation} class C {{ int sentinel; }}");
        let p = ParsedFile::parse(&s);
        assert!(p.tokens_cover_text());
        assert!(!p.errors().is_empty(), "{annotation} was accepted");
        let mut project = Project::new();
        project.insert("C.java", &s);
        let result = project.resolve_at("C.java", s.find("sentinel").unwrap() as u32);
        assert_eq!(result.status, ResolveStatus::Resolved, "{annotation}: {result:?}");
        assert_eq!(result.targets.len(), 1);
        assert_eq!(result.targets[0].kind, TargetKind::Field);
        assert_eq!(result.targets[0].owner, "C");
    }
}

#[test]
fn annotation_list_edits_match_fresh_after_damage_and_repair() {
    let original = "class C{ void f(){ @A(value={1,2,}, other=@B(3)) Object local=null; } }";
    for (needle, replacement) in [("value=", ""), (", other=@B(3)", ","), ("@B(3)", "@B(3,)")] {
        let at = original.find(needle).unwrap();
        let mut p = Project::new();
        p.insert("C.java", original);
        p.edit(
            "C.java",
            TextEdit { start_byte: at as u32, end_byte: (at + needle.len()) as u32, insert: replacement.into() },
        )
        .unwrap();
        let text = p.file("C.java").unwrap().text().to_owned();
        let fresh = ParsedFile::parse(&text);
        assert_eq!(p.file("C.java").unwrap().dump(), fresh.dump());
        assert_eq!(p.file("C.java").unwrap().errors(), fresh.errors());
        assert!(!fresh.errors().is_empty());
        p.edit(
            "C.java",
            TextEdit { start_byte: at as u32, end_byte: (at + replacement.len()) as u32, insert: needle.into() },
        )
        .unwrap();
        assert_eq!(p.file("C.java").unwrap().dump(), ParsedFile::parse(original).dump());
        assert!(p.file("C.java").unwrap().errors().is_empty());
    }
}
