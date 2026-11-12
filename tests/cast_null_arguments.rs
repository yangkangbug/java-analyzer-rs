use java_engine::{Project, ResolveResult, ResolveStatus, TargetKind};

fn project(source: &str) -> Project {
    let p = project_with_errors(source);
    assert!(p.file("Calls.java").unwrap().errors().is_empty());
    p
}

fn project_with_errors(source: &str) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", source);
    let mut object = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61, 0, 3, 1, 0, 16];
    object.extend(b"java/lang/Object");
    object.extend([7, 0, 1, 0, 0x21, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    p.add_class_bytes("Object.class", &object).unwrap();
    p
}

fn query(p: &Project, needle: &str) -> ResolveResult {
    let at = p.file("Calls.java").unwrap().text().rfind(needle).unwrap() as u32;
    let result = p.resolve_at("Calls.java", at);
    assert_eq!(result, p.prepare().resolve_at("Calls.java", at));
    result
}

fn winner(source: &str, call: &str, declaration: &str) {
    let result = query(&project(source), call);
    assert_eq!(result.status, ResolveStatus::Resolved, "{call}: {result:?}");
    assert_eq!(result.targets.len(), 1);
    let target = &result.targets[0];
    let at = source.find(declaration).unwrap() as u32;
    assert_eq!(target.kind, TargetKind::Method);
    assert_eq!(target.path, "Calls.java");
    assert_eq!((target.start_byte, target.end_byte), (at, at + 4));
}

fn refused(source: &str, call: &str, candidates: usize) {
    let result = query(&project(source), call);
    assert_ne!(result.status, ResolveStatus::Resolved, "{call}: {result:?}");
    assert_eq!(result.targets.len(), candidates, "{call}: {result:?}");
}

fn explicit_type(ty: &str) {
    let source = format!(
        "class {ty} {{}} class Child extends {ty} {{}} class Box {{ public void pick({ty} x) {{}} public void pick(Child x) {{}} }} class Calls {{ void run(Box box) {{ box.pick(({ty})null); }} }}"
    );
    winner(&source, &format!("pick(({ty})null)"), &format!("pick({ty} x)"));
}
#[test]
fn cast_null_keeps_token_type() {
    explicit_type("Token");
}
#[test]
fn cast_null_keeps_source_string_type() {
    explicit_type("String");
}
#[test]
fn cast_null_keeps_parent_instead_of_null_specificity() {
    explicit_type("Parent");
}

#[test]
fn cast_null_accepts_bounded_parentheses_and_trivia() {
    let source = "class Token {} class Box { public void pick(Token x) {} public void pick(int x) {} } class Calls { void run(Box box) { box.pick(((Token) /* cast */ ((null)))); } }";
    winner(source, "pick(((Token)", "pick(Token x)");
}

#[test]
fn cast_null_does_not_infer_general_operands_or_casts() {
    for arg in [
        "(Token)unknown",
        "(Token)value",
        "(Token)make()",
        "(Token)(true ? null : null)",
        "(Token)(Token)null",
        "(Token[])null",
        "(Missing)null",
        "(int)null",
        "(Token)nul",
    ] {
        let source = format!(
            "class Token {{}} class Box {{ public void pick(Token x) {{}} }} class Calls {{ Token make() {{ return null; }} void run(Box box, Token value) {{ box.pick({arg}); }} }}"
        );
        refused(&source, &format!("pick({arg})"), 1);
    }
}

#[test]
fn cast_null_requires_nongeneric_complete_type_and_no_typevar_shadow() {
    for declaration in ["class Token<T> {}", "class Token extends Missing {}"] {
        let source = format!(
            "{declaration} class Box {{ public void pick(Token x) {{}} }} class Calls {{ void run(Box box) {{ box.pick((Token)null); }} }}"
        );
        refused(&source, "pick((Token)null)", 1);
    }
    let source = "class Token {} class Box { public void pick(Token x) {} } class Calls<Token> { void run(Box box) { box.pick((Token)null); } }";
    refused(source, "pick((Token)null)", 1);
}

#[test]
fn cast_null_depth_remains_bounded() {
    for arg in [
        format!("(Token){}null{}", "(".repeat(40), ")".repeat(40)),
        format!("{}(Token)null{}", "(".repeat(40), ")".repeat(40)),
    ] {
        let source = format!(
            "class Token {{}} class Box {{ public void pick(Token x) {{}} }} class Calls {{ void run(Box box) {{ box.pick({arg}); }} }}"
        );
        refused(&source, &format!("pick({arg})"), 1);
    }
}

#[test]
fn cast_null_keeps_all_competitors_and_does_not_enable_receivers() {
    for other in
        ["public void pick(Missing x, int i) {}", "public <T> void pick(T x) {}", "public void pick(Token[] x) {}"]
    {
        let source = format!(
            "class Token {{}} class Box {{ public void pick(Token x) {{}} {other} }} class Calls {{ void run(Box box) {{ box.pick((Token)null); }} }}"
        );
        refused(&source, "pick((Token)null)", 2);
    }
    let source = "class Token { public void next() {} } class Calls { void run() { ((Token)null).next(); } }";
    assert_ne!(query(&project(source), "next();").status, ResolveStatus::Resolved);
}

#[test]
fn cast_null_malformed_syntax_cannot_supply_type_evidence() {
    for arg in ["(Token null", "(Token))null", "(Token & Token)null", "(@Missing Token)null", "(Token<Missing>)null"] {
        let source = format!(
            "class Token {{}} class Box {{ public void pick(Token x) {{}} }} class Calls {{ void run(Box box) {{ box.pick({arg}); }} }}"
        );
        let p = project_with_errors(&source);
        assert_ne!(query(&p, &format!("pick({arg})")).status, ResolveStatus::Resolved);
    }
}

#[test]
fn cast_null_outer_and_operand_parentheses_share_depth() {
    let arg = format!("{}(Token){}null{}{}", "(".repeat(20), "(".repeat(20), ")".repeat(20), ")".repeat(20));
    let source = format!(
        "class Token {{}} class Box {{ public void pick(Token x) {{}} }} class Calls {{ void run(Box box) {{ box.pick({arg}); }} }}"
    );
    refused(&source, &format!("pick({arg})"), 1);
}

#[test]
fn cast_null_uses_caller_import_and_requires_access_and_unique_type() {
    let source =
        "package app; import model.Token; import api.Box; class Calls { void run(Box box) { box.pick((Token)null); } }";
    let mut p = project(source);
    p.insert("Token.java", "package model; public class Token {}");
    p.insert("Box.java", "package api; import model.Token; public class Box { public void pick(Token x) {} }");
    let result = query(&p, "pick((Token)null)");
    assert_eq!(result.status, ResolveStatus::Resolved);
    assert_eq!(result.targets[0].path, "Box.java");
    p.insert("Token.java", "package model; class Token {}");
    assert_ne!(query(&p, "pick((Token)null)").status, ResolveStatus::Resolved);
    p.insert("Token.java", "package model; public class Token {}");
    p.insert("Duplicate.java", "package model; public class Token {}");
    assert_ne!(query(&p, "pick((Token)null)").status, ResolveStatus::Resolved);
}

#[test]
fn cast_null_composes_with_existing_inner_call_but_not_call_arguments() {
    let source = "class Token { public void next() {} } class Box { public Token pick(Token x) { return x; } public Token pick(int x) { return null; } } class Calls { void run(Box box) { box.pick((Token)null).next(); box.pick(box.pick((Token)null)); } }";
    let p = project(source);
    assert_eq!(query(&p, "next();").status, ResolveStatus::Resolved);
    assert_ne!(query(&p, "pick(box.pick").status, ResolveStatus::Resolved);
}
