use java_engine::{Project, TargetKind, TextEdit};
fn p(s: &str) -> Project {
    let mut p = Project::new();
    p.insert("p/C.java", "package p;public class C{public static C[] values;}");
    p.insert("Use.java", s);
    p
}
fn q(p: &Project, s: &str) -> java_engine::ResolveResult {
    let t = p.file("Use.java").unwrap().text();
    let at = t.find(s).unwrap() as u32;
    let r = p.resolve_at("Use.java", at);
    assert_eq!(r, p.prepare().resolve_at("Use.java", at));
    r
}
#[test]
fn legal_compiler_six_targets() {
    let s = "import p.C;class Use{String[] items;void use(Object x){}void f(){for(C C:C.values){use(C);}use(C.values);for(String items:items){use(items);}use(items);}}";
    let p = p(s);
    for (at, kind, decl) in [
        (s.find("C.values").unwrap(), TargetKind::Class, 23),
        (s.find("use(C)").unwrap() + 4, TargetKind::Local, s.find("C:C").unwrap()),
        (s.find("use(C.values)").unwrap() + 4, TargetKind::Class, 23),
        (s.find(":items").unwrap() + 1, TargetKind::Field, s.find("items;").unwrap()),
        (s.find("use(items)").unwrap() + 4, TargetKind::Local, s.find("items:").unwrap()),
        (s.rfind("use(items)").unwrap() + 4, TargetKind::Field, s.find("items;").unwrap()),
    ] {
        let r = p.resolve_at("Use.java", at as u32);
        assert_eq!(r.targets[0].kind, kind);
        assert_eq!(r.targets[0].start_byte, decl as u32);
    }
}
#[test]
fn unbraced_nested_and_nested_header_delimiters() {
    for body in ["use(C);", ";", "label:use(C);", "for(C item:C.values){use(C);}"] {
        let s = format!(
            "import p.C;class Use{{void f(){{for(C C:choose(true?C.values:C.values)) {body} use(C.values);}}}}"
        );
        let p = p(&s);
        assert_eq!(q(&p, "C.values").targets[0].kind, TargetKind::Class);
        let after = s.rfind("C.values").unwrap();
        assert_eq!(p.resolve_at("Use.java", after as u32).targets[0].kind, TargetKind::Class);
    }
}
#[test]
fn basic_for_and_catch_resource_unchanged() {
    let s = "import p.C;class Use{void f(){for(C C=null;test(C);update(C)){use(C);}try(C r=open()){use(r);}catch(Exception e){use(e);}}}";
    let p = p(s);
    assert_eq!(q(&p, "C);").targets[0].kind, TargetKind::Local);
    assert_eq!(q(&p, "r);").targets[0].kind, TargetKind::Resource);
    assert_eq!(q(&p, "e);").targets[0].kind, TargetKind::Parameter);
}
#[test]
fn broken_boundary_does_not_grant_header_variable() {
    for header in ["for(C C:C.values {use(C);}", "for(C C:C.values)", "for(C C:)use(C);"] {
        let s = format!("import p.C;class Use{{void f(){{{header}}}}}");
        let p = p(&s);
        if let Some(at) = s.find("C.values") {
            assert_ne!(p.resolve_at("Use.java", at as u32).targets.first().map(|t| t.kind), Some(TargetKind::Local));
        }
    }
}
#[test]
fn edits_and_old_snapshot() {
    let initial = "import p.C;class Use{void f(){for(C C:C.values){use(C);}}}";
    let mut x = p(initial);
    let old = x.clone();
    for header in ["C C:C.values", "C C=null;test(C);update(C)", "C C:C.values", "C C:C.values)", "C C:C.values"] {
        let t = x.file("Use.java").unwrap().text().to_owned();
        let start = t.find("for(").unwrap() + 4;
        let end = t.find("{use").unwrap() - 1;
        x.edit("Use.java", TextEdit { start_byte: start as u32, end_byte: end as u32, insert: header.into() }).unwrap();
        let text = x.file("Use.java").unwrap().text();
        let fresh = p(text);
        assert_eq!(x.file("Use.java").unwrap().dump(), fresh.file("Use.java").unwrap().dump());
        for at in 0..text.len() {
            assert_eq!(x.resolve_at("Use.java", at as u32), fresh.resolve_at("Use.java", at as u32));
        }
    }
    assert_eq!(q(&old, "C.values"), q(&p(initial), "C.values"));
}

#[test]
fn iterable_lambda_and_anonymous_owners_do_not_borrow_for_local() {
    for iterable in ["supply(()->C.values)", "supply(new X(){void f(){use(C.values);}})"] {
        let text = format!("import p.C;class Use{{void f(){{for(C C:{iterable}){{use(C);}}}}}}");
        let p = p(&text);
        assert!(q(&p, "C.values").targets.iter().all(|t| t.kind != TargetKind::Local));
        let body = text.rfind("use(C)").unwrap() + 4;
        assert_eq!(p.resolve_at("Use.java", body as u32).targets[0].kind, TargetKind::Local);
    }
    let text = "import p.C;class Use{void f(){for(C C:C.values){use(new X(){int C;void f(){use(C);}});}}}";
    let p = p(text);
    let at = text.rfind("use(C)").unwrap() + 4;
    let r = p.resolve_at("Use.java", at as u32);
    assert_eq!(r.targets[0].kind, TargetKind::Field);
    assert_eq!(r.targets[0].start_byte, text.find("C;void").unwrap() as u32);
}

#[test]
fn basic_for_ternary_colon_is_not_enhanced_header() {
    let text = "import p.C;class Use{void f(){for(C C=flag?left:right;test(C);update(C)){use(C);}}}";
    let p = p(text);
    assert_eq!(q(&p, "C);").targets[0].kind, TargetKind::Local);
}

#[test]
fn incomplete_header_keeps_declaration_click_and_following_local() {
    let text = "import p.C;class Use{void f(){for(C C:C.values) {use(C);}int next=1;use(next);}}";
    let project = p(text);
    assert_eq!(q(&project, "C:C").targets[0].kind, TargetKind::Local);
    let at = text.rfind("next").unwrap();
    assert_eq!(project.resolve_at("Use.java", at as u32).targets[0].start_byte, text.find("next=1").unwrap() as u32);
    for text in ["import p.C;class Use{void f(){for(C C:C.values", "import p.C;class Use{void f(){for(C C:C.values)"] {
        let project = p(text);
        assert_eq!(q(&project, "C:C").targets[0].kind, TargetKind::Local);
        assert!(q(&project, "C.values").targets.iter().all(|t| t.kind != TargetKind::Local));
    }
}

#[test]
fn non_header_local_in_invalid_unbraced_body_keeps_legacy_visibility() {
    // This is not valid Java; the independent header fix must not reclassify
    // an existing body LocalVar as the enhanced-for declaration.
    let text = "class Use{void f(){for(;;) int x=x;}}";
    let p = p(text);
    let at = text.rfind("x;").unwrap();
    let r = p.resolve_at("Use.java", at as u32);
    assert_eq!(r.targets[0].kind, TargetKind::Local);
    assert_eq!(r.targets[0].start_byte, text.find("x=x").unwrap() as u32);
}
