use java_engine::{Project, ResolveStatus, TargetKind, TextEdit};
fn project(text: &str) -> Project {
    let mut p = Project::new();
    p.insert("C.java", text);
    p
}
fn query(p: &Project, marker: &str) -> java_engine::ResolveResult {
    let at = p.file("C.java").unwrap().text().rfind(marker).unwrap() as u32;
    let r = p.resolve_at("C.java", at);
    assert_eq!(r, p.prepare().resolve_at("C.java", at));
    r
}
fn field(p: &Project, marker: &str, decl: &str) {
    let r = query(p, marker);
    assert_eq!(r.status, ResolveStatus::Resolved, "{marker}: {r:?}");
    assert_eq!(r.targets.len(), 1);
    let t = &r.targets[0];
    assert_eq!(t.kind, TargetKind::Field);
    assert_eq!(t.owner, "");
    assert_eq!(t.path, "C.java");
    let at = p.file("C.java").unwrap().text().find(decl).unwrap() as u32;
    assert_eq!((t.start_byte, t.end_byte), (at, at + t.name.len() as u32));
}
fn no(p: &Project, marker: &str) {
    assert_eq!(query(p, marker).status, ResolveStatus::Unresolved, "{marker}");
}
#[test]
fn own_fields_before_after_methods_and_explicit_this_bypass_local() {
    let p = project(
        "class C{int value;Object f(){return new X(){int value;void run(int value){use(value);use(this.value);use(later);}String later;};}}",
    );
    assert_eq!(query(&p, "value);use(this").targets[0].kind, TargetKind::Parameter);
    field(&p, "value);use(later)", "value;void");
    field(&p, "later);", "later;");
}
#[test]
fn receiver_token_only_no_calls_types_or_outer_and_super() {
    let p = project(
        "class C{Object f(){return new X(){Box field;void run(){field.call();this.field.call();use(Outer.this.field);use(super.field);use(other.field);field();}};}}",
    );
    field(&p, "field.call();this", "field;");
    field(&p, "field.call();use", "field;");
    for m in ["call();this", "call();use", "field);use(super", "field);use(other", "field);field", "field();"] {
        no(&p, m);
    }
}
#[test]
fn nested_owners_do_not_share_fields_and_unknown_parent_is_not_needed() {
    let p = project(
        "class C{int outer;Object f(){return new Missing(){int value;void run(){Object n=new Missing(){int inner;void go(){use(inner);use(value);use(outer);}};use(value);use(inner);}};}}",
    );
    field(&p, "inner);use(value)", "inner;");
    field(&p, "value);use(inner)", "value;");
    no(&p, "value);use(outer)");
    no(&p, "outer);");
    no(&p, "inner);}};");
}
#[test]
fn duplicate_fields_and_local_guards_are_terminal() {
    let p =
        project("class C{Object f(){return new X(){int value;long value;void run(){use(value);use(this.value);}};}}");
    for m in ["value);use", "value);}}"] {
        let r = query(&p, m);
        assert_eq!(r.status, ResolveStatus::Ambiguous);
        assert_eq!(r.targets.len(), 2);
        assert_ne!(r.targets[0].start_byte, r.targets[1].start_byte);
    }
    let p = project("class C{Object f(){return new X(){int value;void run(int value,int value){use(value);}};}}");
    assert_eq!(query(&p, "value);").status, ResolveStatus::Ambiguous);
}
#[test]
fn initializers_capture_lambda_enum_and_malformed_remain_closed() {
    let p = project(
        "class C{Object f(int captured){return new X(){int field=1,other=field;{use(field);}void run(){Runnable r=()->use(field);use(captured);}};}enum E{A{int x;void m(){use(x);}};}}",
    );
    for m in ["field;{", "field);}", "field);use", "captured);", "x);"] {
        no(&p, m);
    }
    let p = project("class C{Object f(){return new X(){int field=;void run(){use(field);}};}}");
    no(&p, "field);");
}
#[test]
fn name_navigation_does_not_validate_field_types_or_static_access() {
    let p = project(
        "class C{Object f(){return new X(){Unknown<T> value;static int COUNT;static void run(){use(value);use(COUNT);}};}}",
    );
    field(&p, "value);", "value;");
    field(&p, "COUNT);", "COUNT;");
}
#[test]
fn edit_fresh_and_old_snapshot_keep_anonymous_declaration_identity() {
    let text = "class C{Object f(){return new X(){int value;void run(){use(value);}};}}";
    let original = project(text);
    let old = original.prepare();
    let at = text.find("int value").unwrap() as u32 + 4;
    let mut changed = original.clone();
    changed.edit("C.java", TextEdit { start_byte: at, end_byte: at + 5, insert: "other".into() }).unwrap();
    let fresh = project(changed.file("C.java").unwrap().text());
    assert_eq!(query(&changed, "value);"), query(&fresh, "value);"));
    no(&changed, "value);");
    changed.edit("C.java", TextEdit { start_byte: at, end_byte: at + 5, insert: "value".into() }).unwrap();
    field(&changed, "value);", "value;");
    let pos = text.rfind("value);").unwrap() as u32;
    assert_eq!(old.resolve_at("C.java", pos), query(&original, "value);"));
}

#[test]
fn explicit_this_field_only_binds_identifier_bytes() {
    let text = "class C{Object f(){return new X(){int value;void run(){use(this /*a*/ . /*b*/ value);}};}}";
    let p = project(text);
    let start = text.find("this /*a*/").unwrap();
    let name = text.rfind("value);").unwrap();
    for byte in start..name + 5 {
        let r = p.resolve_at("C.java", byte as u32);
        assert_eq!(r, p.prepare().resolve_at("C.java", byte as u32));
        assert_eq!(
            r.status,
            if byte >= name { ResolveStatus::Resolved } else { ResolveStatus::Unresolved },
            "byte={byte} token={:?}",
            &text[byte..byte + 1]
        );
        if byte >= name {
            assert_eq!(r.targets[0].start_byte, text.find("value;").unwrap() as u32);
        }
    }
}

#[test]
fn bare_field_only_binds_identifier_not_surrounding_trivia() {
    let text = "class C{Object f(){return new X(){int value;void run(){use( /*a*/ value /*b*/ );}};}}";
    let p = project(text);
    let start = text.find("( /*a*/").unwrap() + 1;
    let name = text.rfind("value /*b*/").unwrap();
    let end = text.find("/*b*/ );").unwrap() + 6;
    for byte in start..end {
        let r = p.resolve_at("C.java", byte as u32);
        assert_eq!(r, p.prepare().resolve_at("C.java", byte as u32));
        assert_eq!(
            r.status,
            if (name..name + 5).contains(&byte) { ResolveStatus::Resolved } else { ResolveStatus::Unresolved },
            "byte={byte} token={:?}",
            &text[byte..byte + 1]
        );
    }
}

#[test]
fn same_spelling_anonymous_fields_have_distinct_public_source_identity() {
    let text = "class C{Object first=new X(){int value;void run(){one(value);}};Object second=new X(){int value;void run(){two(value);}};}";
    let p = project(text);
    let first = p.resolve_at("C.java", (text.find("one(value)").unwrap() + 4) as u32);
    let second = p.resolve_at("C.java", (text.find("two(value)").unwrap() + 4) as u32);
    assert_eq!(first.status, ResolveStatus::Resolved);
    assert_eq!(second.status, ResolveStatus::Resolved);
    let a = &first.targets[0];
    let b = &second.targets[0];
    assert_eq!(a.owner, "");
    assert_eq!(b.owner, "");
    assert_eq!(a.name, b.name);
    assert_eq!(a.path, b.path);
    assert_eq!(a.start_byte, text.find("value;").unwrap() as u32);
    assert_eq!(b.start_byte, text.rfind("value;").unwrap() as u32);
    assert_ne!(a.start_byte, b.start_byte);
    assert_ne!(a, b);
}
