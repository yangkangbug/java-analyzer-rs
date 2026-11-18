use java_engine::{ParsedFile, Project, ResolveStatus, TargetKind, TextEdit};
#[test]
fn annotation_declaration_elements_and_defaults_have_distinct_structure() {
    let text = "@Deprecated public @interface A { String value(); int count() default 2; String[] names() default {\"x\",\"y\"}; Nested nested() default @Nested(value=\"z\"); @interface Nested {String value();} int CONST=1; }";
    let p = ParsedFile::parse(text);
    assert!(p.errors().is_empty(), "{:?}\n{}", p.errors(), p.dump());
    let dump = p.dump();
    assert!(dump.contains("AnnotationType"));
    assert!(dump.contains("AnnotationElement"));
    assert!(dump.contains("AnnotationDefault"));
    assert_eq!(p.root().utf8_len() as usize, text.len());
}

fn query(p: &Project, marker: &str) -> java_engine::ResolveResult {
    let text = p.file("A.java").unwrap().text();
    let at = text.rfind(marker).unwrap() as u32;
    let r = p.resolve_at("A.java", at);
    assert_eq!(r, p.prepare().resolve_at("A.java", at));
    r
}
#[test]
fn declaration_names_preserved_and_default_descendants_are_opaque() {
    let text = "class Hidden{} @interface A{int value() default new Object(){int hidden;int run(){return hidden;}};int other();} class Use{A annotation;}";
    let mut p = Project::new();
    p.insert("A.java", text);
    let name = query(&p, "value()");
    assert_eq!(name.status, ResolveStatus::Resolved);
    assert_eq!(name.targets[0].kind, TargetKind::Method);
    assert_eq!(name.targets[0].owner, "A");
    assert_eq!(name.targets[0].start_byte, text.find("value()").unwrap() as u32);
    assert_eq!(query(&p, "A annotation").targets[0].kind, TargetKind::Class);
    for marker in ["Object(){", "hidden;int", "run(){", "hidden;}}"] {
        assert_eq!(query(&p, marker).status, ResolveStatus::Unresolved, "{marker}");
    }
}
#[test]
fn annotation_members_are_not_strict_ordinary_methods() {
    let text = "@interface A{int value();String value() default \"x\";} class Use{void m(A a){a.value();}}";
    let mut p = Project::new();
    p.insert("A.java", text);
    let r = query(&p, "value();}}");
    assert_ne!(r.status, ResolveStatus::Resolved);
    assert!(p.file("A.java").unwrap().dump().contains("AnnotationElement"));
}
#[test]
fn nested_annotation_and_anonymous_or_local_boundaries() {
    let text = "class C{ @interface Nested{int value();} Object x=new X(){@interface Hidden{int value();} void run(){Hidden h;}};void m(){@interface Local{int value();} Local local;}}";
    let mut p = Project::new();
    p.insert("A.java", text);
    assert_eq!(query(&p, "Nested{").targets[0].owner, "C.Nested");
    for marker in ["Hidden{", "Hidden h", "Local{", "Local local"] {
        assert_eq!(query(&p, marker).status, ResolveStatus::Unresolved, "{marker}");
    }
}
#[test]
fn invalid_elements_keep_recovery_contained() {
    for text in [
        "@interface A{int value(int x); int next();}",
        "@interface A{int value(){return 1;} int next();}",
        "@interface A{int value() default ; int next();}",
    ] {
        let parsed = ParsedFile::parse(text);
        assert!(!parsed.errors().is_empty());
        let mut p = Project::new();
        p.insert("A.java", text);
        let r = query(&p, "next()");
        assert_eq!(r.status, ResolveStatus::Resolved);
        assert_eq!(r.targets[0].name, "next");
        assert_eq!(r.targets[0].kind, TargetKind::Method);
    }
}
#[test]
fn default_arrays_nested_annotations_and_postfix_dimensions() {
    let text = "@interface N{String value();}@interface A{int scalar() default 1+2; int array()[] default {1,2,};N[] nested() default {@N(\"a\"),@N(value=\"b\")};Class<?> type() default String.class;}";
    let p = ParsedFile::parse(text);
    assert!(p.errors().is_empty(), "{:?}", p.errors());
    assert_eq!(p.dump().matches("AnnotationDefault").count(), 4);
}
#[test]
fn edits_default_and_declaration_marker_match_fresh_and_old_snapshot() {
    let text = "class C{ @interface A{String value() default \"x\";} A a;}";
    let mut p = Project::new();
    p.insert("A.java", text);
    let original = p.clone();
    let old = original.prepare();
    let start = text.find("\"x\"").unwrap() as u32;
    let mut width = 3;
    for insert in ["{\"x\",\"y\"}", "@Missing(value=\"z\")", "new Object(){int x;}", "\"", "\"x\""] {
        p.edit("A.java", TextEdit { start_byte: start, end_byte: start + width, insert: insert.into() }).unwrap();
        width = insert.len() as u32;
        let now = p.file("A.java").unwrap().text();
        let fresh = ParsedFile::parse(now);
        assert_eq!(p.file("A.java").unwrap().dump(), fresh.dump());
        let mut f = Project::new();
        f.insert("A.java", now);
        for (at, _) in now.char_indices() {
            assert_eq!(p.prepare().resolve_at("A.java", at as u32), f.prepare().resolve_at("A.java", at as u32));
        }
    }
    let at = text.find("A a").unwrap() as u32;
    assert_eq!(old.resolve_at("A.java", at), original.resolve_at("A.java", at));
    let marker = p.file("A.java").unwrap().text().find("@interface").unwrap() as u32;
    p.edit("A.java", TextEdit { start_byte: marker, end_byte: marker + 1, insert: String::new() }).unwrap();
    assert_eq!(p.file("A.java").unwrap().dump(), ParsedFile::parse(p.file("A.java").unwrap().text()).dump());
    p.edit("A.java", TextEdit { start_byte: marker, end_byte: marker, insert: "@".into() }).unwrap();
    assert_eq!(p.file("A.java").unwrap().dump(), ParsedFile::parse(text).dump());
}

#[test]
fn new_body_uses_safe_containing_reparse_and_invalid_headers_are_reported() {
    for text in
        ["@interface A<T>{int value();}", "@interface A extends Runnable{int value();}", "@interface A{<T>T value();}"]
    {
        assert!(!ParsedFile::parse(text).errors().is_empty(), "{text}");
    }
    assert!(!java_engine::SyntaxKind::AnnotationBody.is_reparseable());
}

#[test]
fn implicit_annotation_parent_never_makes_elements_strict_unique() {
    // Missing, duplicate, cyclic, and conflicting parent inventories are not
    // permission to treat an annotation element as an ordinary typed method.
    for parents in [
        vec![],
        vec![("Parent.java", "package java.lang.annotation;public interface Annotation {}")],
        vec![
            ("Parent.java", "package java.lang.annotation;public interface Annotation {}"),
            ("Duplicate.java", "package java.lang.annotation;public interface Annotation {}"),
        ],
        vec![("Parent.java", "package java.lang.annotation;public interface Annotation extends Annotation {}")],
        vec![(
            "Parent.java",
            "package java.lang.annotation;public interface Annotation {int value();int inherited();}",
        )],
    ] {
        let mut p = Project::new();
        let text = "@interface A{int value();} class Use{void m(A a){a.value();a.inherited();}}";
        p.insert("A.java", text);
        for (path, source) in parents {
            p.insert(path, source);
        }
        let own = query(&p, "value();a.");
        assert_eq!(own.status, ResolveStatus::SignatureNotCompared);
        assert!(own.targets.iter().any(|t| t.path == "A.java" && t.start_byte == text.find("value()").unwrap() as u32));
        let inherited = query(&p, "inherited();}}");
        assert_ne!(inherited.status, ResolveStatus::Resolved);
    }
}
