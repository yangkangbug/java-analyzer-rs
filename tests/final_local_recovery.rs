use java_engine::{Project, ResolveStatus, TargetKind, TextEdit};
fn project(body: &str) -> Project {
    let mut p = Project::new();
    p.insert("p/C.java", "package p;public class C<T>{public static int x;}");
    p.insert("Use.java", body);
    p
}
fn result(p: &Project, needle: &str) -> java_engine::ResolveResult {
    let text = p.file("Use.java").unwrap().text();
    let at = text.rfind(needle).unwrap() as u32;
    let r = p.resolve_at("Use.java", at);
    assert_eq!(r, p.prepare().resolve_at("Use.java", at));
    r
}
#[test]
fn committed_final_damaged_type_preserves_names_not_type_facts() {
    for decl in ["final p.C<,> C=null", "final p.C<,> first=null,C=null", "final p.C<p.C<,>> C=null"] {
        let text = format!("import p.C;class Use{{int f(){{{decl};return C.x;}}}}");
        let p = project(&text);
        assert!(p.file("Use.java").unwrap().dump().contains("LocalVar"));
        let name = result(&p, "C.x");
        assert_eq!(name.status, ResolveStatus::Resolved);
        assert_eq!(name.targets[0].kind, TargetKind::Local);
        assert_eq!(name.targets[0].start_byte, text.find("C=null").unwrap() as u32);
        assert_ne!(result(&p, "x;").status, ResolveStatus::Resolved);
    }
}
#[test]
fn for_declaration_has_scope_but_no_damaged_type() {
    for tail in ["=null;;", ":values"] {
        let text = format!("import p.C;class Use{{void f(){{for(final p.C<,> C{tail}){{use(C.x);}}}}}}");
        let p = project(&text);
        assert_eq!(result(&p, "C.x").targets[0].kind, TargetKind::Local);
        assert_ne!(result(&p, "x);").status, ResolveStatus::Resolved);
    }
}
#[test]
fn label_and_local_class_do_not_export_local() {
    let text = "import p.C;class Use{void f(){label: final p.C<,> C=null;use(C.x);}}";
    let p = project(text);
    assert!(p.file("Use.java").unwrap().errors().iter().any(|e| e.message.contains("labeled")));
    assert!(result(&p, "C.x").targets.iter().all(|t| t.kind != TargetKind::Local));
    let text = "class Use{void f(){final class C{} }}";
    let p = project(text);
    assert!(p.file("Use.java").unwrap().dump().contains("LocalType"));
    assert!(!p.file("Use.java").unwrap().dump().contains("LocalVar"));
}
#[test]
fn edits_fresh_undo_old_snapshot() {
    let text = "import p.C;class Use{int f(){final p.C<String> C=null;return C.x;}}";
    let mut p = project(text);
    let old = p.clone();
    for args in ["<,>", "<String>", "<p.C<,>>", "<String>"] {
        let current = p.file("Use.java").unwrap().text().to_owned();
        let start = current.find('<').unwrap();
        let end = current.find(" C=null").unwrap();
        p.edit("Use.java", TextEdit { start_byte: start as u32, end_byte: end as u32, insert: args.into() }).unwrap();
        let fresh = project(p.file("Use.java").unwrap().text());
        assert_eq!(p.file("Use.java").unwrap().dump(), fresh.file("Use.java").unwrap().dump());
        assert_eq!(result(&p, "C.x"), result(&fresh, "C.x"));
        assert_eq!(result(&p, "x;"), result(&fresh, "x;"));
    }
    assert_eq!(result(&old, "x;"), result(&project(text), "x;"));
}

#[test]
fn recovery_boundaries_do_not_guess_missing_names() {
    for decl in ["final p.C<String C=null", "final p.C<,>[] C=null", "final final p.C<,> C=null"] {
        let text = format!("import p.C;class Use{{int f(){{{decl};return C.x;}}}}");
        let p = project(&text);
        assert_eq!(result(&p, "C.x").targets[0].kind, TargetKind::Local);
        assert_ne!(result(&p, "x;").status, ResolveStatus::Resolved);
    }
    for decl in ["final C=null", "final p.C<,> =null"] {
        let text = format!("import p.C;class Use{{int f(){{{decl};return C.x;}}}}");
        let p = project(&text);
        assert!(result(&p, "C.x").targets.iter().all(|t| t.kind != TargetKind::Local));
    }
}
#[test]
fn following_declarations_and_catch_recovery_are_not_swallowed() {
    for prefix in ["", "try{}catch(Exception e){}"] {
        let text =
            format!("import p.C;class Use{{int f(){{{prefix}final p.C<,> C=null;int next=1;use(C.x);return next;}}}}");
        let p = project(&text);
        let r = result(&p, "next;");
        assert_eq!(r.targets[0].kind, TargetKind::Local);
        assert_eq!(r.targets[0].start_byte, text.find("next=1").unwrap() as u32);
        assert_ne!(result(&p, "x);").status, ResolveStatus::Resolved);
    }
    // Missing catch ')' still uses its old strict recovery probe. No new commitment there.
    let text = "import p.C;class Use{int f(){try{}catch(Exception e final p.C<,> C=null;return C.x;}}";
    let p = project(text);
    assert!(!p.file("Use.java").unwrap().dump().contains("LocalVar"));
}
#[test]
fn unmodified_expression_and_no_modifier_gap_stay_separate() {
    let text = "import p.C;class Use{int f(){p.C<,> C=null;return C.x;}}";
    let p = project(text);
    assert!(!p.file("Use.java").unwrap().dump().contains("LocalVar"));
    assert_eq!(result(&p, "x;").targets[0].path, "p/C.java");
    for body in ["boolean b=a<c;", "a=b<c;", "for(a=b<c;;){}"] {
        let text = format!("class Use{{void f(){{{body}}}}}");
        let p = project(&text);
        assert!(p.file("Use.java").unwrap().errors().is_empty());
    }
}

#[test]
fn enhanced_for_old_blocker_keeps_iterable_outer_and_body_local() {
    let text = "import p.C;class Use{void f(){for(final p.C<,> C:C.values){use(C.x);}use(C.values);}}";
    let mut project = project(text);
    project.insert("p/C.java", "package p;public class C<T>{public static C[] values;public int x;}");
    let inside = text.find("C.values").unwrap();
    let body = text.find("C.x").unwrap();
    let after = text.rfind("C.values").unwrap();
    for at in [inside, after] {
        let r = project.resolve_at("Use.java", at as u32);
        assert_eq!(r.targets[0].kind, TargetKind::Class);
        assert_eq!(r.targets[0].path, "p/C.java");
    }
    let r = project.resolve_at("Use.java", body as u32);
    assert_eq!(r.targets[0].kind, TargetKind::Local);
    assert_eq!(r.targets[0].start_byte, text.find("C:C.values").unwrap() as u32);
    assert_ne!(project.resolve_at("Use.java", (body + 2) as u32).status, ResolveStatus::Resolved);
}

#[test]
fn incomplete_final_declaration_does_not_steal_next_name() {
    for prefix in ["final C=null;", "final p.C<,> =null;", "final p.C<String C=null;", "final p.C<String "] {
        let text = format!("import p.C;class Use{{int f(){{{prefix}int next=1;return next;}}}}");
        let p = project(&text);
        let r = result(&p, "next;");
        assert_eq!(r.targets[0].kind, TargetKind::Local);
        assert_eq!(r.targets[0].start_byte, text.find("next=1").unwrap() as u32);
    }
}

#[test]
fn annotation_recovery_to_real_final_retains_name_not_type() {
    for prefix in ["@A ", "@A(value=1) ", "@A(final) ", "@A(value=1 "] {
        let text =
            format!("import p.C;class Use{{int f(){{{prefix}final p.C<,> C=null;int next=1;use(C.x);return next;}}}}");
        let p = project(&text);
        assert!(!p.file("Use.java").unwrap().errors().is_empty());
        assert_eq!(p.file("Use.java").unwrap().root().utf8_len() as usize, text.len());
        let r = result(&p, "C.x");
        assert_eq!(r.targets[0].kind, TargetKind::Local);
        assert_eq!(r.targets[0].start_byte, text.find("C=null").unwrap() as u32);
        assert_ne!(result(&p, "x);").status, ResolveStatus::Resolved);
        assert_eq!(result(&p, "next;").targets[0].start_byte, text.find("next=1").unwrap() as u32);
    }
}
#[test]
fn final_inside_annotation_arguments_does_not_export_declaration() {
    for prefix in [
        "@A(final p.C<,> C=null);",
        "@A(value=final p.C<,> C=null);",
        "@A(final p.C<,> C=null;",
        "@A(value=1",
        "@A(final",
    ] {
        let text = format!("import p.C;class Use{{int f(){{{prefix} int next=1;use(C.x);return next;}}}}");
        let p = project(&text);
        assert!(result(&p, "C.x").targets.iter().all(|t| t.kind != TargetKind::Local));
        assert_eq!(result(&p, "next;").targets[0].start_byte, text.find("next=1").unwrap() as u32);
    }
}

#[test]
fn enhanced_broken_boundary_and_basic_for_remain_distinct() {
    for tail in [":C.values", ":C.values)", ":C.values {use(C);}"] {
        let text = format!("import p.C;class Use{{void f(){{for(final p.C<,> C{tail}}}}}");
        let p = project(&text);
        assert!(result(&p, "C.values").targets.iter().all(|t| t.kind != TargetKind::Local));
        let decl = text.find("C:C.values").unwrap();
        assert_eq!(p.resolve_at("Use.java", decl as u32).targets[0].kind, TargetKind::Local);
    }
    let text = "import p.C;class Use{void f(){for(final p.C<,> C=null;test(C);update(C)){use(C.x);}}}";
    let p = project(text);
    assert_eq!(result(&p, "C);").targets[0].kind, TargetKind::Local);
    assert_ne!(result(&p, "x);").status, ResolveStatus::Resolved);
}

#[test]
fn malformed_array_type_retains_only_real_declaration_name() {
    let text = "import p.C;class Use{int f(){final p.C<,>[ C=null;return C.x;}}";
    let p = project(text);
    assert_eq!(result(&p, "C.x").targets[0].kind, TargetKind::Local);
    assert_ne!(result(&p, "x;").status, ResolveStatus::Resolved);
}
