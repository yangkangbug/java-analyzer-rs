//! Small lexical-binding oracle, not a javac or whole-Java oracle.
//!
//! Expected declarations come from Java lexical scope rules and manually chosen
//! source locations below, never from either resolver. A basic-for declaration
//! shadows a field until the end of the loop. An expression-list initializer
//! declares nothing and therefore uses the enclosing block's locals.

use java_engine::{ParsedFile, Project, ResolveResult, ResolveStatus, Target, TargetKind};

#[test]
fn basic_for_targets_match_independent_lexical_scope_goldens() {
    // The later block is disjoint from the first loop, avoiding illegal Java
    // redeclaration of a local inside the scope of a same-named outer local.
    let source = "class C { int i; int j; void m() { for (int i=0,j=1; i<j; i++,j--) { loop(i,j); } after(i,j); { int i=4,j=5; for (i=0,j=1; i<j; i++,j--) outer(i,j); } } void loop(int a,int b) {} void after(int a,int b) {} void outer(int a,int b) {} }";
    let parsed = ParsedFile::parse(source);
    assert!(parsed.errors().is_empty(), "{}", parsed.dump());
    assert!(parsed.tokens_cover_text());
    let mut project = Project::new();
    project.insert("C.java", source);
    let prepared = project.prepare();

    // These anchors are manual source positions, independent of parser trees
    // and binding output. Assert exact identifier slices to catch fixture drift.
    let field_i = source.find("int i;").unwrap() + 4;
    let field_j = source.find("int j;").unwrap() + 4;
    let loop_i = source.find("int i=0,j=1").unwrap() + 4;
    let loop_j = loop_i + 4;
    let outer_i = source.find("int i=4,j=5").unwrap() + 4;
    let outer_j = outer_i + 4;
    let cases = [
        ("loop(i,j)", 5, "i", TargetKind::Local, loop_i),
        ("loop(i,j)", 7, "j", TargetKind::Local, loop_j),
        // A same-spelling loop local must not leak beyond the loop body.
        ("after(i,j)", 6, "i", TargetKind::Field, field_i),
        ("after(i,j)", 8, "j", TargetKind::Field, field_j),
        ("outer(i,j)", 6, "i", TargetKind::Local, outer_i),
        ("outer(i,j)", 8, "j", TargetKind::Local, outer_j),
    ];
    for (anchor, within, name, kind, declaration) in cases {
        let query = source.find(anchor).unwrap() + within;
        assert_eq!(&source[query..query + 1], name);
        assert_eq!(&source[declaration..declaration + 1], name);
        let expected = ResolveResult {
            status: ResolveStatus::Resolved,
            targets: vec![Target {
                binary: None,
                path: "C.java".into(),
                kind,
                name: name.into(),
                owner: if kind == TargetKind::Field { "C".into() } else { String::new() },
                start_byte: declaration as u32,
                end_byte: declaration as u32 + 1,
            }],
        };
        assert_eq!(project.resolve_at("C.java", query as u32), expected, "fresh: {anchor} {name}");
        assert_eq!(prepared.resolve_at("C.java", query as u32), expected, "prepared: {anchor} {name}");
    }
}
