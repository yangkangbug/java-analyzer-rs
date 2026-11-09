use java_engine::{Project, ResolveStatus, TargetKind, TextEdit};

fn project(files: &[(&str, &str)]) -> Project {
    let mut project = Project::new();
    for (path, text) in files {
        project.insert(*path, text);
        assert!(project.file(path).unwrap().errors().is_empty(), "{path}");
    }
    project
}

fn at(text: &str, needle: &str) -> u32 {
    text.find(needle).unwrap() as u32
}

fn assert_target(project: &Project, path: &str, needle: &str, owner: &str, kind: TargetKind) {
    let text = project.file(path).unwrap().text();
    let result = project.resolve_at(path, at(text, needle));
    assert_eq!(result.status, ResolveStatus::Resolved, "{needle}: {result:?}");
    assert_eq!(result.targets.len(), 1);
    assert_eq!(result.targets[0].owner, owner);
    assert_eq!(result.targets[0].kind, kind);
}

#[test]
fn a_field_receiver_uses_the_declaration_imports_not_the_callers() {
    let sources = project(&[
        ("lib/Box.java", "package lib; import model.Result; public class Box { public Result result; }"),
        ("model/Result.java", "package model; public class Result { public int value; }"),
        ("other/Result.java", "package other; public class Result { public int value; }"),
        (
            "app/Use.java",
            "package app; import other.Result; public class Use { void m(lib.Box box) { int n = box.result.value; } }",
        ),
    ]);
    assert_target(&sources, "app/Use.java", "value;", "model.Result", TargetKind::Field);
}

#[test]
fn a_same_name_and_offset_in_the_caller_cannot_supply_a_fields_type() {
    let declaration = "package lib; public class Box { public Value value; }";
    let caller =
        "package app; public class Use { public Wrong value; void m(lib.Box box) { int n = box.value.leaf; } }";
    assert_eq!(at(declaration, "value;"), at(caller, "value;"));
    let sources = project(&[
        ("lib/Box.java", declaration),
        ("lib/Value.java", "package lib; public class Value { public int leaf; }"),
        ("app/Wrong.java", "package app; public class Wrong { public int leaf; }"),
        ("app/Use.java", caller),
    ]);
    assert_target(&sources, "app/Use.java", "leaf;", "lib.Value", TargetKind::Field);
}

#[test]
fn inherited_field_receivers_keep_their_declaration_context() {
    let sources = project(&[
        ("lib/Base.java", "package lib; import model.Result; public class Base { public Result result; }"),
        ("model/Result.java", "package model; public class Result { public int value; }"),
        (
            "app/Child.java",
            "package app; public class Child extends lib.Base { void m() { int a = result.value; int b = this.result.value; } }",
        ),
    ]);
    let text = sources.file("app/Child.java").unwrap().text();
    for (offset, _) in text.match_indices("value;") {
        let result = sources.resolve_at("app/Child.java", offset as u32);
        assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
        assert_eq!(result.targets[0].owner, "model.Result");
    }
}

#[test]
fn a_chain_can_cross_multiple_source_files_and_end_in_a_method() {
    let sources = project(&[
        ("lib/First.java", "package lib; public class First { public Second second; }"),
        ("lib/Second.java", "package lib; public class Second { public Last last; }"),
        ("lib/Last.java", "package lib; public class Last { public void run() {} }"),
        ("Use.java", "public class Use { void m(lib.First first) { first.second.last.run(); } }"),
    ]);
    assert_target(&sources, "Use.java", "run();", "lib.Last", TargetKind::Method);
}

#[test]
fn a_field_type_can_be_nested_in_its_declaring_class() {
    let sources = project(&[
        (
            "lib/Box.java",
            "package lib; public class Box { public Result result; public class Result { public int value; } }",
        ),
        ("Use.java", "public class Use { void m(lib.Box box) { int n = box.result.value; } }"),
    ]);
    assert_target(&sources, "Use.java", "value;", "lib.Box.Result", TargetKind::Field);
}

#[test]
fn receiver_type_lookup_does_not_move_the_access_check_to_the_declaration() {
    for access in ["public", "private", ""] {
        let result_source =
            format!("package lib; public class Result {{ {access} int value; {access} void run() {{}} }}");
        let sources = project(&[
            ("lib/Box.java", "package lib; public class Box { public Result result; }"),
            ("lib/Result.java", &result_source),
            ("Use.java", "public class Use { void m(lib.Box box) { int n = box.result.value; box.result.run(); } }"),
        ]);
        for needle in ["value;", "run();"] {
            let text = sources.file("Use.java").unwrap().text();
            let result = sources.resolve_at("Use.java", at(text, needle));
            let expected = if access == "public" { ResolveStatus::Resolved } else { ResolveStatus::Unresolved };
            assert_eq!(result.status, expected, "{access} {needle}: {result:?}");
        }
    }
}

#[test]
fn a_missing_declaration_type_does_not_use_the_callers_same_named_type() {
    let sources = project(&[
        ("lib/Box.java", "package lib; import missing.Result; public class Box { public Result result; }"),
        ("app/Result.java", "package app; public class Result { public int value; }"),
        ("app/Use.java", "package app; public class Use { void m(lib.Box box) { int n = box.result.value; } }"),
    ]);
    let text = sources.file("app/Use.java").unwrap().text();
    let result = sources.resolve_at("app/Use.java", at(text, "value;"));
    assert_eq!(result.status, ResolveStatus::ReceiverNotBound);
    assert!(result.targets.is_empty());
}

#[test]
fn an_ambiguous_declaration_type_does_not_pick_a_receiver() {
    let sources = project(&[
        ("lib/Box.java", "package lib; import a.*; import b.*; public class Box { public Result result; }"),
        ("a/Result.java", "package a; public class Result { public int value; }"),
        ("b/Result.java", "package b; public class Result { public int value; }"),
        ("Use.java", "public class Use { void m(lib.Box box) { int n = box.result.value; } }"),
    ]);
    let text = sources.file("Use.java").unwrap().text();
    let result = sources.resolve_at("Use.java", at(text, "value;"));
    assert_eq!(result.status, ResolveStatus::Ambiguous);
    assert!(result.targets.is_empty());
}

#[test]
fn editing_a_declaration_import_changes_the_next_chained_query() {
    let declaration = "package lib; import left.Result; public class Box { public Result result; }";
    let mut sources = project(&[
        ("lib/Box.java", declaration),
        ("left/Result.java", "package left; public class Result { public int value; }"),
        ("right/Result.java", "package right; public class Result { public int value; }"),
        ("Use.java", "public class Use { void m(lib.Box box) { int n = box.result.value; } }"),
    ]);
    assert_target(&sources, "Use.java", "value;", "left.Result", TargetKind::Field);
    let start = at(declaration, "left.Result");
    sources.edit("lib/Box.java", TextEdit { start_byte: start, end_byte: start + 4, insert: "right".into() }).unwrap();
    assert_target(&sources, "Use.java", "value;", "right.Result", TargetKind::Field);
}

#[test]
fn local_and_parameter_receivers_still_resolve_in_their_own_file() {
    let sources = project(&[
        ("lib/Result.java", "package lib; public class Result { public int value; }"),
        (
            "Use.java",
            "import lib.Result; public class Use { void m(Result parameter) { Result local = null; int a = parameter.value; int b = local.value; } }",
        ),
    ]);
    let text = sources.file("Use.java").unwrap().text();
    for (offset, _) in text.match_indices("value;") {
        let result = sources.resolve_at("Use.java", offset as u32);
        assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
        assert_eq!(result.targets[0].owner, "lib.Result");
    }
}
