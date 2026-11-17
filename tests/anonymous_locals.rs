use java_engine::{Project, ResolveResult, ResolveStatus, TextEdit};
fn project(text: &str) -> Project {
    let mut p = Project::new();
    p.insert("C.java", text);
    p
}
fn query(p: &Project, marker: &str) -> ResolveResult {
    let at = p.file("C.java").unwrap().text().rfind(marker).unwrap() as u32;
    let result = p.resolve_at("C.java", at);
    assert_eq!(result, p.prepare().resolve_at("C.java", at));
    result
}
fn target(p: &Project, marker: &str, declaration: &str) {
    let r = query(p, marker);
    assert_eq!(r.status, ResolveStatus::Resolved, "{marker}: {r:?}");
    assert_eq!(r.targets.len(), 1);
    let text = p.file("C.java").unwrap().text();
    let at = text.find(declaration).unwrap() as u32;
    assert_eq!(r.targets[0].start_byte, at, "{marker}");
    assert_eq!(r.targets[0].end_byte, at + r.targets[0].name.len() as u32);
    assert_eq!(r.targets[0].owner, "");
}
fn no(p: &Project, marker: &str) {
    assert_eq!(query(p, marker).status, ResolveStatus::Unresolved, "{marker}");
}
#[test]
fn own_parameters_locals_and_declarations_shadow_without_capturing() {
    let p = project(
        "class C{int value; Object f(int outer){return new X(){int field; void run(int value){int local=value; use(local); use(value); use(outer); use(field);}};}}",
    );
    target(&p, "value); use(outer)", "value){");
    target(&p, "local);", "local=value");
    target(&p, "value){", "value){");
    target(&p, "local=value", "local=value");
    no(&p, "outer);");
    target(&p, "field);", "field;");
    no(&p, "use(local)");
}
#[test]
fn nested_owner_and_constructor_argument_boundaries() {
    let p = project(
        "class C{Object f(int outer){return new X(outer){void run(int own){Object x=new X(own){void run(int inner){use(inner);use(own);use(outer);}};use(own);}};}}",
    );
    target(&p, "outer){void", "outer){return");
    target(&p, "own){void run(int inner)", "own){Object");
    target(&p, "inner);", "inner){use");
    target(&p, "own);}};}}", "own){Object");
    no(&p, "own);use(outer)");
    no(&p, "outer);}}");
}
#[test]
fn initializer_lambda_local_class_enum_and_fields_remain_opaque() {
    let p = project(
        "class C{Object f(){return new X(){int field=1;{int init=1;use(init);} void run(int own){Runnable r=()->{int lam=1;use(lam);use(own);}; use(own);use(field);}};} enum E{A{void run(int en){use(en);}};}}",
    );
    target(&p, "own);use(field)", "own){Runnable");
    no(&p, "init);");
    no(&p, "lam);");
    no(&p, "own);};");

    target(&p, "field);", "field=1");
    no(&p, "en);");
}
#[test]
fn catch_resource_scope_and_decl_before_use_are_shared() {
    let p = project(
        "class C{Object f(){return new X(){void run(){use(later); int later=1;use(later);try(R resource=open()){use(resource);}catch(E caught){use(caught);use(resource);}finally{use(caught);} }};}}",
    );
    target(&p, "later);try", "later=1");
    no(&p, "later); int");
    target(&p, "resource);}catch", "resource=open");
    target(&p, "caught);use", "caught){use");
    no(&p, "resource);}finally");
    no(&p, "caught);} }");
}
#[test]
fn duplicates_and_pattern_refusals_survive_owner_filter() {
    let p = project("class C{Object f(){return new X(){void run(int x,int x){use(x);} };}}");
    assert_eq!(query(&p, "x);").status, ResolveStatus::Ambiguous);
    let guarded = project(
        "class C{Object f(){return new X(){void run(Object input){int value=1;if(input instanceof String value){use(value);}}};}}",
    );
    no(&guarded, "value);");
    let local =
        project("class C{Object f(){return new X(){void run(int own){class Local{void m(){use(own);}}use(own);}};}}");
    no(&local, "own);}}use");
    no(&local, "own);}};");
}
#[test]
fn receivers_resolve_only_the_local_not_members_types_or_calls() {
    let p = project(
        "class C{Object f(){return new X(){void run(Box own){own.call();use(own);Box other;this.field=own;}};}}",
    );
    target(&p, "own.call", "own){");
    target(&p, "own;}}", "own){");
    no(&p, "call();");
    no(&p, "Box other");
    no(&p, "field=own");
}
#[test]
fn edit_recovery_fresh_and_old_snapshot_have_no_stale_owner() {
    let text = "class C{Object f(){return new X(){void run(int own){use(own);}};}}";
    let original = project(text);
    let snapshot = original.prepare();
    let at = text.find("int own").unwrap() as u32;
    let mut changed = original.clone();
    for insert in ["int ", "int renamed", "int own"] {
        let current = changed.file("C.java").unwrap().text().to_string();
        let end = current.find("){use").unwrap() as u32;
        changed.edit("C.java", TextEdit { start_byte: at, end_byte: end, insert: insert.into() }).unwrap();
        let now = changed.file("C.java").unwrap().text().to_string();
        let fresh = project(&now);
        assert_eq!(query(&changed, "own);"), query(&fresh, "own);"));
    }
    target(&changed, "own);", "own){use");
    let pos = text.rfind("own);").unwrap() as u32;
    assert_eq!(snapshot.resolve_at("C.java", pos), query(&original, "own);"));
}

#[test]
fn sibling_methods_nested_blocks_and_invalid_statement_boundaries() {
    let p = project(
        "class C{Object f(){return new X(){void one(int a){use(a);}void two(int b){use(a);{int inner=b;use(inner);}use(inner);use(b);}};}}",
    );
    target(&p, "a);}void", "a){use");
    target(&p, "inner);}use", "inner=b");
    target(&p, "b);}};", "b){use");
    no(&p, "a);{");
    no(&p, "inner);use");
    for body in ["label: int value=1;use(value);", "synchronized int value=1;use(value);", "int value=;use(value);"] {
        let p = project(&format!("class C{{Object f(){{return new X(){{void run(){{{body}}}}};}}}}"));
        no(&p, "value);");
    }
}
#[test]
fn brace_damage_and_undo_match_fresh_without_member_leakage() {
    let text = "class C{Object f(){return new X(){void run(int own){use(own);}};}}";
    let original = project(text);
    let old = original.prepare();
    let at = text.find("}};").unwrap() as u32;
    let mut changed = original.clone();
    changed.edit("C.java", TextEdit { start_byte: at, end_byte: text.len() as u32, insert: String::new() }).unwrap();
    let fresh = project(changed.file("C.java").unwrap().text());
    assert_eq!(query(&changed, "own);"), query(&fresh, "own);"));
    no(&changed, "own);");
    changed.edit("C.java", TextEdit { start_byte: at, end_byte: at, insert: text[at as usize..].into() }).unwrap();
    target(&changed, "own);", "own){use");
    let pos = text.rfind("own);").unwrap() as u32;
    assert_eq!(old.resolve_at("C.java", pos), query(&original, "own);"));
}
