use java_engine::{Project, ResolveStatus, TextEdit};
fn project(text: &str) -> Project {
    let mut p = Project::new();
    p.insert("p/C.java", "package p;public class C<T>{public static int x;}");
    p.insert("Use.java", text);
    p
}
#[test]
fn malformed_type_names_do_not_supply_receiver_facts() {
    let cases = [
        "class Use{p.C.var field;int f(){return field.x;}}",
        "class Use{int f(p.C.var param){return param.x;}}",
        "class Use{int f(){p.C.var local=null;return local.x;}}",
        "class Use{p..C field;int f(){return field.x;}}",
        "class Use{p.C<,> field;int f(){return field.x;}}",
        "class Use{int f(p.C<,> param){return param.x;}}",
        "class Use{int f(){p.C<,> local=null;return local.x;}}",
        "class Use{void f(){try(p.C<,> resource=open()){use(resource.x);}}}",
        "class Use{void f(){try{}catch(p.C<,> caught){use(caught.x);}}}",
        "import p.C;class Use{p.C<,> C;int f(){return C.x;}}",
        "import p.C;class Use{int f(p.C<,> C){return C.x;}}",
    ];
    let mut failures = Vec::new();
    for text in cases {
        let p = project(text);
        let at = text.rfind(".x").unwrap() as u32 + 1;
        let r = p.resolve_at("Use.java", at);
        assert_eq!(r, p.prepare().resolve_at("Use.java", at));
        if r.status == ResolveStatus::Resolved {
            failures.push((text, r));
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}
#[test]
fn valid_erased_names_and_prefix_navigation_keep_existing_contract() {
    for text in [
        "class Use{p.C<Missing> field;int f(){return field.x;}}",
        "class Use{p.C<@A String> field;int f(){return field.x;}}",
        "class Use{p.C<> field;int f(){return field.x;}}",
        "class Use{p.C field;int f(){return field.x;}}",
    ] {
        let p = project(text);
        let r = p.resolve_at("Use.java", text.rfind(".x").unwrap() as u32 + 1);
        assert_eq!(r.status, ResolveStatus::Resolved);
        assert_eq!(r.targets[0].path, "p/C.java");
    }
    let text = "class Use{p.C. field;}";
    let p = project(text);
    assert_eq!(p.resolve_at("Use.java", text.find("C.").unwrap() as u32).status, ResolveStatus::Resolved);
    let text = "class Use{p.C<,> field;}";
    let p = project(text);
    let r = p.resolve_at("Use.java", text.find("field").unwrap() as u32);
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets[0].name, "field");
}
#[test]
fn damaged_type_local_edits_match_fresh_and_old_snapshot() {
    let initial = "class Use{p.C field;int f(){return field.x;}}";
    let mut p = project(initial);
    let old = p.clone();
    let mut width = 3;
    for replacement in ["p..C", "p.C<,>", "p.C.var", "p.C<Missing>", "p.C"] {
        let at = p.file("Use.java").unwrap().text().find("p.").unwrap() as u32;
        p.edit("Use.java", TextEdit { start_byte: at, end_byte: at + width, insert: replacement.into() }).unwrap();
        width = replacement.len() as u32;
        let text = p.file("Use.java").unwrap().text();
        let fresh = project(text);
        assert_eq!(p.file("Use.java").unwrap().dump(), fresh.file("Use.java").unwrap().dump());
        let ready = p.prepare();
        for byte in 0..text.len() as u32 {
            assert_eq!(ready.resolve_at("Use.java", byte), fresh.resolve_at("Use.java", byte));
        }
        assert_eq!(old.resolve_at("Use.java", initial.rfind(".x").unwrap() as u32 + 1).status, ResolveStatus::Resolved);
    }
    assert_eq!(p.file("Use.java").unwrap().text(), initial);
}

#[test]
fn existing_local_declaration_keeps_name_barrier_when_type_is_damaged() {
    // Malformed annotation value is skipped structurally by declaration lookahead,
    // so this case actually creates a LocalVar, unlike the separately recorded
    // p.C<,> C case which is outside this declaration-fact contract.
    for text in [
        "import p.C;class Use{int f(){p.C<@A(value=) String> C=null;return C.x;}}",
        "import p.C;class Use{int f(){p.C<Missing> C=null;return C.x;}}",
    ] {
        let p = project(text);
        let dump = p.file("Use.java").unwrap().dump();
        assert!(dump.contains("LocalVar"), "{dump}");
        let name = text.find("> C=").unwrap() as u32 + 2;
        let target = p.resolve_at("Use.java", name);
        assert_eq!(target.status, ResolveStatus::Resolved);
        assert_eq!(target.targets[0].start_byte, name);
        let use_at = text.rfind("C.x").unwrap() as u32;
        let name_use = p.resolve_at("Use.java", use_at);
        assert_eq!(name_use.status, ResolveStatus::Resolved);
        assert_eq!(name_use.targets[0].start_byte, name);
        let member = p.resolve_at("Use.java", use_at + 2);
        if text.contains("value=") {
            assert_ne!(member.status, ResolveStatus::Resolved);
        } else {
            assert_eq!(member.status, ResolveStatus::Resolved);
        }
    }
}
#[test]
fn anonymous_own_field_keeps_existing_body_validity_boundary() {
    for (ty, expected) in [("p.C<,>", ResolveStatus::Unresolved), ("p.C<Missing>", ResolveStatus::Resolved)] {
        let text = format!("class Use{{Object a=new X(){{{ty} field;void f(){{use(field);use(this.field);}}}};}}");
        let p = project(&text);
        for mark in ["field);", "this.field"] {
            let at = text.rfind(mark).unwrap() as u32 + if mark.starts_with("this.") { 5 } else { 0 };
            let r = p.resolve_at("Use.java", at);
            assert_eq!(r.status, expected);
            if expected == ResolveStatus::Resolved {
                assert_eq!(r.targets[0].name, "field");
                assert!(r.targets[0].owner.is_empty());
            }
        }
    }
}

#[test]
fn supported_annotated_catch_type_keeps_receiver_fact() {
    let text = "class Use{void f(){try{}catch(p.@A @B C caught){use(caught.x);}}}";
    let mut p = Project::new();
    p.insert("p/C.java", "package p;public class C extends RuntimeException{public int x;}");
    p.insert("Use.java", text);
    assert!(p.file("Use.java").unwrap().errors().is_empty());
    let r = p.resolve_at("Use.java", text.rfind(".x").unwrap() as u32 + 1);
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets[0].path, "p/C.java");
}
