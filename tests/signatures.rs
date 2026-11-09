use java_engine::{ParsedFile, Project, ResolveStatus, TargetKind, TextEdit};

fn call(source: &str) -> java_engine::ResolveResult {
    let mut project = Project::new();
    project.insert("Test.java", source);
    let parsed = project.file("Test.java").unwrap();
    assert!(parsed.errors().is_empty(), "{}\n{:?}", parsed.dump(), parsed.errors());
    project.resolve_at("Test.java", source.rfind("m(null)").unwrap() as u32)
}

fn assert_overloads(result: java_engine::ResolveResult, count: usize) {
    assert_eq!(result.status, ResolveStatus::OverloadNotResolved, "{result:?}");
    assert_eq!(result.targets.len(), count);
    assert!(result.targets.iter().all(|target| target.kind == TargetKind::Method));
}

#[test]
fn scalar_and_array_parameters_remain_different_overloads() {
    for (first, second) in [
        ("int", "int[]"),
        ("String", "String[]"),
        ("pkg.Item", "pkg.Item[]"),
        ("int[]", "int[][]"),
        ("String[][]", "String[][][]"),
    ] {
        let source = format!("class C {{ void m({first} a) {{}} void m({second} b) {{}} void test() {{ m(null); }} }}");
        assert_overloads(call(&source), 2);
    }
}

#[test]
fn an_array_method_does_not_hide_a_different_rank_in_a_superclass() {
    for (base, child) in [("int", "int[]"), ("String[]", "String"), ("pkg.Item[]", "pkg.Item[][]")] {
        let source = format!(
            "class A {{ void m({base} a) {{}} }} class B extends A {{ void m({child} b) {{}} void test() {{ m(null); }} }}"
        );
        let result = call(&source);
        assert_overloads(result.clone(), 2);
        assert!(result.targets.iter().any(|target| target.owner == "A"));
        assert!(result.targets.iter().any(|target| target.owner == "B"));
    }
}

#[test]
fn varargs_add_one_array_dimension_instead_of_disappearing() {
    for (first, second) in [("int", "int..."), ("String", "String..."), ("int[]", "int[]...")] {
        let source = format!("class C {{ void m({first} a) {{}} void m({second} b) {{}} void test() {{ m(null); }} }}");
        assert_overloads(call(&source), 2);
    }
}

#[test]
fn equivalent_array_and_varargs_signatures_hide_the_farther_method() {
    for (base, child) in [
        ("int[]", "int..."),
        ("int...", "int[]"),
        ("String[][]", "String[]..."),
        ("pkg.Item[]...", "pkg.Item[][]"),
        ("int[][]", "int[][]"),
    ] {
        let source = format!(
            "class A {{ void m({base} a) {{}} }} class B extends A {{ void m({child} b) {{}} void test() {{ m(null); }} }}"
        );
        let result = call(&source);
        assert_eq!(result.status, ResolveStatus::Resolved, "{base} / {child}: {result:?}");
        assert_eq!(result.targets.len(), 1);
        assert_eq!(result.targets[0].owner, "B");
    }
}

#[test]
fn only_the_matching_array_rank_is_hidden_across_three_levels() {
    let source = "class A { void m(int[] a) {} } class B extends A { void m(int[][] b) {} } class C extends B { void m(int... c) {} void test() { m(null); } }";
    let result = call(source);
    assert_overloads(result.clone(), 2);
    assert!(result.targets.iter().any(|target| target.owner == "B"));
    assert!(result.targets.iter().any(|target| target.owner == "C"));
}

#[test]
fn array_rank_is_not_taken_from_generic_arguments_or_annotations() {
    for (base, child) in [
        ("List<String[]>", "List<String[]>[]"),
        ("List<String[]>", "List<String>[]"),
        ("@Tag({1, 2}) int[]", "int[][]"),
        ("final String /* [] */ []", "String[][]"),
    ] {
        let source = format!(
            "class A {{ void m({base} a) {{}} }} class B extends A {{ void m({child} b) {{}} void test() {{ m(null); }} }}"
        );
        assert_overloads(call(&source), 2);
    }
}

#[test]
fn postfix_parameter_dimensions_are_part_of_the_signature() {
    for (base, child) in
        [("int[] a", "int b[]"), ("String[][] a", "String[] b[]"), ("int[][] a", "int b[][]"), ("int... a", "int b[]")]
    {
        let source = format!(
            "class A {{ void m({base}) {{}} }} class B extends A {{ void m({child}) {{}} void test() {{ m(null); }} }}"
        );
        let result = call(&source);
        assert_eq!(result.status, ResolveStatus::Resolved, "{base} / {child}: {result:?}");
        assert_eq!(result.targets[0].owner, "B");
        assert_eq!(result.targets.len(), 1);
    }
    assert_overloads(call("class C { void m(int a) {} void m(int b[]) {} void test() { m(null); } }"), 2);
}

#[test]
fn array_dimension_edits_change_the_next_signature_lookup() {
    let source = "class A { void m(int[] a) {} } class B extends A { void m(int[] b) {} void test() { m(null); } }";
    let mut project = Project::new();
    project.insert("Test.java", source);
    let before = project.resolve_at("Test.java", source.rfind("m(null)").unwrap() as u32);
    assert_eq!(before.status, ResolveStatus::Resolved);
    assert_eq!(before.targets[0].owner, "B");
    let insert_at = (source.find("int[] b").unwrap() + "int[]".len()) as u32;
    let edited = project
        .edit("Test.java", TextEdit { start_byte: insert_at, end_byte: insert_at, insert: "[]".into() })
        .unwrap();
    assert_eq!(edited.file.root().as_ref(), ParsedFile::parse(edited.file.text()).root().as_ref());
    let at = edited.file.text().rfind("m(null)").unwrap() as u32;
    assert_overloads(project.resolve_at("Test.java", at), 2);
}

#[test]
fn multiple_parameters_keep_their_order_and_individual_dimensions() {
    let source = "class A { void m(int[] a, String b) {} } class B extends A { void m(int a, String[] b) {} void test() { m(null); } }";
    assert_overloads(call(source), 2);
}

#[test]
fn malformed_parameter_lists_do_not_supply_a_confident_signature() {
    for parameter in ["int[]", "int[ value", "int... value[]"] {
        let source = format!(
            "class A {{ void m(int[] a) {{}} }} class B extends A {{ void m({parameter}) {{}} void test() {{ m(null); }} }}"
        );
        let mut project = Project::new();
        project.insert("Test.java", &source);
        assert!(!project.file("Test.java").unwrap().errors().is_empty());
        let result = project.resolve_at("Test.java", source.rfind("m(null)").unwrap() as u32);
        assert_eq!(result.status, ResolveStatus::SignatureNotCompared, "{parameter}: {result:?}");
    }
}

#[test]
fn same_depth_identical_array_signatures_stay_ambiguous() {
    let source = "interface A { void m(int[] a); } interface B { void m(int... b); } abstract class C implements A, B { void test() { m(null); } }";
    let result = call(source);
    assert_eq!(result.status, ResolveStatus::Ambiguous, "{result:?}");
    assert_eq!(result.targets.len(), 2);
}
