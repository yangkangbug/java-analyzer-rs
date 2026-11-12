use super::*;

fn inventory(result: &str) -> bool {
    let parsed = ParsedFile::parse(&format!("class Host {{ public {result} other() {{ return null; }} }}"));
    index_file("Host.java", &parsed).classes[0].reference_inventory
}

#[test]
fn complete_generic_return_headers_do_not_invalidate_unrelated_declaration_inventory() {
    for result in [
        "Box<?>",
        "Box<? extends Value>",
        "Box<? super Value>",
        "Box<Box<? extends Value>>",
        "Outer<Value>.Inner<Box<?>>",
        "Box<Value[]>",
        "Box<int[]>",
        "Box<? extends Value[]>",
        "Box<? super Value[][]>",
        "Box<Box<?>>[]",
    ] {
        assert!(inventory(result), "{result}");
    }
}

#[test]
fn malformed_return_type_syntax_never_certifies_inventory() {
    for result in [
        "?",
        "? extends Value",
        "?[]",
        "Box<>",
        "Box<Value,>",
        "Box<int>",
        "Box<void>",
        "Box<?[]>",
        "Box<? extends>",
        "Box<? super>",
        "Box<? extends Value super Other>",
        "Box<? extends Value & Other>",
        "Box<<Value>>",
        "Outer<Value>.",
        "void[]",
        "Box<void[]>",
        "Box<? extends int>",
        "Box<? super int>",
        "Box<? extends void[]>",
        "Box<Value><Other>",
    ] {
        assert!(!inventory(result), "{result}");
    }
}

#[test]
fn return_header_inventory_has_a_total_work_bound() {
    let result = format!("Box<{}>", vec!["Value"; REFERENCE_PROOF_WORK].join(","));
    assert!(!inventory(&result));
}

#[test]
fn return_header_inventory_has_a_nesting_bound() {
    let result = format!("{}Value{}", "Box<".repeat(REFERENCE_PROOF_DEPTH + 1), ">".repeat(REFERENCE_PROOF_DEPTH + 1));
    assert!(!inventory(&result));
}

#[test]
fn direct_ancestor_wildcard_boundary_is_not_expanded_by_return_inventory() {
    let parsed = ParsedFile::parse("class Host extends Base<?> { public int call() { return 1; } }");
    assert!(!index_file("Host.java", &parsed).classes[0].reference_inventory);
}

#[test]
fn ordinary_return_header_inventory_is_preserved() {
    for result in [
        "Value",
        "lib.Value",
        "int",
        "boolean",
        "byte",
        "short",
        "char",
        "float",
        "double",
        "long",
        "double[]",
        "void",
        "Value[][]",
        "Box<Value>",
    ] {
        assert!(inventory(result), "{result}");
    }
}

#[test]
fn return_header_depth_limit_accepts_the_boundary_not_the_next_level() {
    let result = format!("{}Value{}", "Box<".repeat(REFERENCE_PROOF_DEPTH), ">".repeat(REFERENCE_PROOF_DEPTH));
    assert!(inventory(&result));
}
