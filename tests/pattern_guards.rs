use java_engine::{ParsedFile, Project, ResolveStatus, TargetKind, TextEdit};

fn query(source: &str, marker: &str) -> java_engine::ResolveResult {
    let mut p = Project::new();
    p.insert("C.java", source);
    let at = source.find(marker).unwrap() as u32;
    let result = p.resolve_at("C.java", at);
    assert_eq!(result, p.prepare().resolve_at("C.java", at));
    result
}

#[test]
fn unsupported_pattern_cannot_fall_back_to_field_but_other_names_still_navigate() {
    let s = "class C {Object value; Object other; Object f(Object input){if(input instanceof String value){Object keep=other/*other*/; this.value/*explicit*/=keep; return value/*pattern*/;} return null;} Object g(){return value/*neighbor*/;}}";
    let parsed = ParsedFile::parse(s);
    assert!(parsed.tokens_cover_text());
    assert!(parsed.errors().iter().any(|e| e.message.contains("pattern")));
    assert_eq!(query(s, "value/*pattern*/").status, ResolveStatus::Unresolved);
    assert_eq!(query(s, "other/*other*/").targets[0].kind, TargetKind::Field);
    assert_eq!(query(s, "keep;").targets[0].kind, TargetKind::Local);
    assert_eq!(query(s, "value/*explicit*/").targets[0].kind, TargetKind::Field);
    assert_eq!(query(s, "value/*neighbor*/").targets[0].kind, TargetKind::Field);
    assert_eq!(query(s, "value){").status, ResolveStatus::Unresolved);
}

#[test]
fn pattern_flow_forms_and_lambda_capture_never_rebind_same_name() {
    for body in [
        "if(!(input instanceof String value)) return null; return value/*use*/;",
        "if(input instanceof String value && value/*use*/.isEmpty()) return null; return null;",
        "if(!(input instanceof String value) || value/*use*/.isEmpty()) return null; return null;",
        "if(!(input instanceof String value))return null;else return value/*use*/;",
        "while(input instanceof String value){return value/*use*/;}return null;",
        "return input instanceof String value ? value/*use*/ : null;",
        "if(input instanceof String value){return (java.util.function.Supplier<Object>)()->value/*use*/;}return null;",
    ] {
        let s = format!("class C {{Object value; Object f(Object input){{{body}}}}}");
        assert!(ParsedFile::parse(&s).tokens_cover_text());
        assert_eq!(query(&s, "value/*use*/").status, ResolveStatus::Unresolved, "{body}");
    }
}

#[test]
fn pattern_receivers_and_type_name_fallback_are_blocked() {
    for s in [
        "class Other{int length(){return 1;}} class C{Other value; int f(Object input){if(input instanceof String value){return value.length/*call*/();}return 0;}}",
        "class P{void hit(){}} class value{static void hit(){}} class C{void f(Object input){if(input instanceof P value){value.hit/*call*/();}}}",
    ] {
        let result = query(s, if s.contains("length/*call*/") { "length/*call*/" } else { "hit/*call*/" });
        assert_ne!(result.status, ResolveStatus::Resolved, "{result:?}");
        assert!(result.targets.is_empty(), "{result:?}");
    }
}

#[test]
fn final_annotated_generic_array_patterns_are_explicitly_unsupported() {
    for ty in ["final String", "@Mark String", "java.util.List<?>", "Object[]", "final @Mark Object[]"] {
        let s = format!(
            "class C {{Object value; Object f(Object input){{if(input instanceof {ty} value){{return value/*use*/;}}return null;}}}}"
        );
        let parsed = ParsedFile::parse(&s);
        assert!(parsed.tokens_cover_text());
        assert!(parsed.errors().iter().any(|e| e.message.contains("pattern")), "{ty}: {}", parsed.dump());
        assert_eq!(query(&s, "value/*use*/").status, ResolveStatus::Unresolved, "{ty}");
    }
    let s =
        "class C{Object value; Object f(Object input){if(input instanceof String){return value/*old*/;}return null;}}";
    assert!(ParsedFile::parse(s).errors().is_empty());
    assert_eq!(query(s, "value/*old*/").targets[0].kind, TargetKind::Field);
}

#[test]
fn guards_are_bounded_to_execution_unit_and_only_the_affected_name() {
    for unit in [
        "C(Object input){if(input instanceof String value){Object used=value/*use*/;}}",
        "{Object input=null;if(input instanceof String value){Object used=value/*use*/;}}",
        "boolean ok = input instanceof String value && value/*use*/.isEmpty();",
    ] {
        let s = format!("class C{{Object input; Object value; {unit} Object g(){{return value/*neighbor*/;}}}}");
        assert_eq!(query(&s, "value/*use*/").status, ResolveStatus::Unresolved, "{unit}");
        assert_eq!(query(&s, "value/*neighbor*/").targets[0].kind, TargetKind::Field, "{unit}");
    }
    // Deliberate conservative boundary: the later bare field name is also refused.
    let s = "class C{Object value; Object f(Object input){if(input instanceof String value){}return value/*after*/;}}";
    assert_eq!(query(s, "value/*after*/").status, ResolveStatus::Unresolved);
}

#[test]
fn pattern_guard_edits_reindex_and_leave_old_prepared_versions_unchanged() {
    let text = "class C{Object value; Object f(Object input){if(input instanceof String value){return value/*use*/;}return null;}}";
    let mut p = Project::new();
    p.insert("C.java", text);
    let old = p.clone();
    let prepared = old.prepare();
    assert_eq!(
        prepared.resolve_at("C.java", text.find("value/*use*/").unwrap() as u32).status,
        ResolveStatus::Unresolved
    );
    let at = text.find("String value").unwrap() + "String".len();
    p.edit("C.java", TextEdit { start_byte: at as u32, end_byte: (at + 6) as u32, insert: String::new() }).unwrap();
    let changed = p.file("C.java").unwrap().text().to_owned();
    assert_eq!(p.resolve_at("C.java", changed.find("value/*use*/").unwrap() as u32).targets[0].kind, TargetKind::Field);
    assert_eq!(
        prepared.resolve_at("C.java", text.find("value/*use*/").unwrap() as u32).status,
        ResolveStatus::Unresolved
    );
    p.edit("C.java", TextEdit { start_byte: at as u32, end_byte: at as u32, insert: " value".into() }).unwrap();
    let mut fresh = Project::new();
    fresh.insert("C.java", text);
    assert_eq!(p.file("C.java").unwrap().dump(), fresh.file("C.java").unwrap().dump());
    let a = p.prepare();
    let b = fresh.prepare();
    for (at, _) in text.char_indices() {
        assert_eq!(a.resolve_at("C.java", at as u32), b.resolve_at("C.java", at as u32));
    }
}

#[test]
fn enum_constant_guard_does_not_escape_to_neighbor_constant_or_method() {
    let s = "class Inputs{static Object input;} class value{static boolean ok(){return true;}} enum E{A(Inputs.input instanceof String value && value/*pattern*/.isEmpty()), B(value/*neighbor*/.ok()); E(boolean flag){} boolean m(){return value/*method*/.ok();}}";
    assert!(ParsedFile::parse(s).tokens_cover_text());
    assert_eq!(query(s, "value/*pattern*/").status, ResolveStatus::Unresolved);
    for marker in ["value/*neighbor*/", "value/*method*/"] {
        let r = query(s, marker);
        assert_eq!(r.status, ResolveStatus::Resolved, "{marker}: {r:?}");
        assert_eq!(r.targets[0].kind, TargetKind::Class);
    }
}

#[test]
fn pattern_value_guard_does_not_hide_the_method_namespace() {
    let s = "class C{Object value(){return null;} Object f(Object input){if(input instanceof String value){return value/*method*/();}return null;}}";
    let r = query(s, "value/*method*/");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets[0].kind, TargetKind::Method);
}
