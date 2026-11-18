use java_engine::{ParsedFile, Project, ResolveStatus, SyntaxKind, TextEdit};

#[test]
fn non_pattern_switch_syntax_preserves_structure() {
    for body in [
        "return switch(n){case 0 -> n; default -> {yield n;}};",
        "return switch(n){case 0: yield n; default: yield 1;};",
        "switch(n){case 0, 1 -> hit(); default -> {int x=n; hit();}}",
        "return switch(n){case 0, 1 -> throw new E(); default -> n;};",
    ] {
        let source = format!("class C {{ int f(int n){{{body}}} }}");
        let file = ParsedFile::parse(&source);
        assert!(file.tokens_cover_text());
        assert!(file.errors().is_empty(), "{source}\n{}", file.dump());
        assert!(file.dump().contains("SwitchBody"));
    }
}

#[test]
fn yield_context_changes_reparse_its_enclosing_expression() {
    let source = "class C {int f(int n){return switch(n){default -> {int 中=1; yield 中;}};}}";
    let file = ParsedFile::parse(source);
    assert!(file.errors().is_empty(), "{}", file.dump());
    let at = source.find("yield").unwrap() as u32;
    let edit = TextEdit { start_byte: at, end_byte: at + 5, insert: "return".into() };
    let changed = file.apply_edit(edit);
    assert_eq!(changed.file.root(), ParsedFile::parse(changed.file.text()).root());
}

fn query(source: &str, marker: &str) -> java_engine::ResolveResult {
    let mut project = Project::new();
    project.insert("C.java", source);
    let at = source.find(marker).unwrap() as u32;
    let found = project.resolve_at("C.java", at);
    assert_eq!(found, project.prepare().resolve_at("C.java", at));
    found
}

#[test]
fn constant_label_names_are_not_lambdas_or_value_references() {
    for label in ["VALUE", "(VALUE)", "1 + VALUE", "VALUE, OTHER", "0, 1"] {
        for delimiter in [":", "->"] {
            let body = if delimiter == ":" { "yield n;" } else { "n;" };
            let source = format!(
                "class C {{int f(int n,int VALUE,int OTHER){{return switch(n){{case {label} {delimiter} {body} default {delimiter} {body}}};}}}}"
            );
            let file = ParsedFile::parse(&source);
            assert!(file.errors().is_empty(), "{source}\n{}", file.dump());
            assert!(!file.dump().contains("LambdaExpr"));
            assert_eq!(query(&source, "n){").status, ResolveStatus::Resolved);
            if let Some(at) = source.find("case VALUE") {
                let mut project = Project::new();
                project.insert("C.java", &source);
                assert_eq!(project.resolve_at("C.java", (at + 5) as u32).status, ResolveStatus::Unresolved);
            }
        }
    }
}

#[test]
fn arrow_blocks_have_separate_scopes_and_colon_groups_share_one() {
    let source = "class C {int f(int n){int r=switch(n){case 0 -> {int same=n; yield same;} default -> {int same=n; yield same+1;}}; same++; return r;}}";
    let file = ParsedFile::parse(source);
    assert!(file.errors().is_empty(), "{}", file.dump());
    let first = query(source, "same;}");
    let second = query(source, "same+1");
    assert_eq!(first.status, ResolveStatus::Resolved);
    assert_eq!(second.status, ResolveStatus::Resolved);
    assert_eq!(first.targets[0].start_byte, source.find("same=n").unwrap() as u32);
    assert_eq!(second.targets[0].start_byte, source.rfind("same=n").unwrap() as u32);
    assert_ne!(first.targets, second.targets);
    assert_eq!(query(source, "same++;").status, ResolveStatus::Unresolved);
    assert_eq!(query(source, "yield same;").status, ResolveStatus::Unresolved);
    let colon = "class C {int f(int n){return switch(n){case 0: int value=n; case 1: value++; yield value; default: yield n;};}}";
    assert!(ParsedFile::parse(colon).errors().is_empty());
    assert_eq!(query(colon, "value++;").targets[0].start_byte, colon.find("value=n").unwrap() as u32);
}

#[test]
fn malformed_rules_and_unsupported_patterns_stay_opaque() {
    for body in [
        "case String s -> {int hidden=0;}",
        "case String s when true -> {int hidden=0;}",
        "case null -> {int hidden=0;}",
        "case null, default -> {int hidden=0;}",
        "case 0, -> {int hidden=0;}",
        "case 0 -> -> {int hidden=0;}",
        "case 0: -> {int hidden=0;}",
        "case 0 -> {int hidden=0;} default: break;",
        "case 0: break; default -> {int hidden=0;}",
        "case 0 -> int hidden=0;",
        "case 0 -> 1;",
        "case 0 -> throw;",
        "case 0 -> call() default -> {}",
    ] {
        let source = format!("class C {{void f(int n){{switch(n){{{body}}} int after=0; after++;}}}}");
        let file = ParsedFile::parse(&source);
        assert!(file.tokens_cover_text(), "{body}");
        assert!(!file.errors().is_empty(), "{body}\n{}", file.dump());
        assert!(!file.dump().contains("SwitchRule"), "{body}\n{}", file.dump());
        assert_eq!(query(&source, "after++;").status, ResolveStatus::Resolved, "{body}");
    }
}

fn count(node: &java_engine::GreenNode, kind: SyntaxKind) -> usize {
    usize::from(node.kind() == kind) + node.child_nodes().map(|child| count(child, kind)).sum::<usize>()
}

#[test]
fn nearest_switch_expression_owns_yield_through_statement_switches() {
    let source = "class C {int f(int n){return switch(n){default -> {switch(n){default: yield n;} yield switch(n){default -> {yield n;}};}};}}";
    let file = ParsedFile::parse(source);
    assert!(file.errors().is_empty(), "{}", file.dump());
    assert_eq!(count(file.root(), SyntaxKind::SwitchExpr), 2);
    assert_eq!(count(file.root(), SyntaxKind::SwitchStmt), 1);
    assert_eq!(count(file.root(), SyntaxKind::YieldStmt), 3);
}

#[test]
fn lambda_anonymous_and_local_type_bodies_do_not_inherit_yield_context() {
    for nested in ["Runnable r=()->{yield n;};", "Object r=new C(){int g(){yield n;}};", "class L {int g(){yield n;}}"]
    {
        let source = format!("class C {{int f(int n){{return switch(n){{default -> {{{nested} yield n;}}}};}}}}");
        let file = ParsedFile::parse(&source);
        assert!(file.tokens_cover_text(), "{nested}");
        assert_eq!(count(file.root(), SyntaxKind::YieldStmt), 1, "{nested}\n{}", file.dump());
    }
    let own = "class C {int f(int n){return switch(n){default -> {Object r=()->{return switch(n){default -> {yield n;}};}; yield n;}};}}";
    let file = ParsedFile::parse(own);
    assert!(file.errors().is_empty(), "{}", file.dump());
    assert_eq!(count(file.root(), SyntaxKind::YieldStmt), 2);
}

#[test]
fn contextual_yield_identifiers_and_keywords_have_distinct_roles() {
    for statements in ["yield = n; yield++; yield += n;", "yield--; yield = yield + n;"] {
        let source = format!(
            "class C {{int f(int n){{int yield=0; {statements} return switch(n){{default -> {{{statements} yield(yield);}}}};}}}}"
        );
        let file = ParsedFile::parse(&source);
        assert!(file.errors().is_empty(), "{source}\n{}", file.dump());
        assert_eq!(count(file.root(), SyntaxKind::YieldStmt), 1);
        assert_eq!(query(&source, "yield=0").status, ResolveStatus::Resolved);
        assert_eq!(query(&source, statements).status, ResolveStatus::Resolved);
        assert_eq!(query(&source, "yield(yield)").status, ResolveStatus::Unresolved);
    }
    let source = "class C {int yield(int n){return n;} int f(int n){return this.yield(n);}}";
    assert!(ParsedFile::parse(source).errors().is_empty());
    assert!(!ParsedFile::parse("class C {int f(int n){yield(n);}}").errors().is_empty());
}

#[test]
fn switch_result_receivers_never_borrow_lexical_members() {
    let sw = "switch(n){default -> x;}";
    for base in [
        format!("({sw})"),
        format!("(({sw}))"),
        format!("((C)({sw}))"),
        format!("({sw})[0]"),
        format!("({sw})()"),
        format!("(ok?x:({sw}))"),
        format!("(ok?({sw}):x)"),
        format!("(x=({sw}))"),
    ] {
        for suffix in ["x", "m()"] {
            let source =
                format!("class C {{C x; C m(){{return x;}} void f(int n,boolean ok){{ ({base}).{suffix}; }} }}");
            let marker = format!("{suffix};");
            // Query the final receiver member, not lexical declarations.
            let at = source.rfind(&marker).unwrap() as u32;
            let mut project = Project::new();
            project.insert("C.java", &source);
            assert!(ParsedFile::parse(&source).tokens_cover_text());
            let result = project.resolve_at("C.java", at);
            assert_eq!(result.status, ResolveStatus::ReceiverNotBound, "{source}");
            assert!(result.targets.is_empty(), "{source}");
        }
    }
}

#[test]
fn switch_expression_edit_matrix_matches_fresh_and_undo() {
    for source in [
        "class C {int f(int n){return switch(n){case 0 -> {int 名=1; yield 名;} default -> n;};} void sibling(){}}",
        "class C {void f(int n){switch(n){case 0 -> {int 名=1; 名++;} default -> call();}}}",
        "class C {int f(int n){return switch(new C(){int m(){return 1;}}.m()){default -> {yield n;}};}}",
    ] {
        let original = ParsedFile::parse(source);
        assert!(original.errors().is_empty(), "{}", original.dump());
        let old = original.dump();
        let begin = source.find("switch").unwrap();
        for (at, ch) in source.char_indices().filter(|(at, _)| *at >= begin) {
            for replacement in ["", ":", "->", "yield", "/*中😀*/", "{", "}"] {
                let changed = original.apply_edit(TextEdit {
                    start_byte: at as u32,
                    end_byte: (at + ch.len_utf8()) as u32,
                    insert: replacement.into(),
                });
                let full = ParsedFile::parse(changed.file.text());
                assert!(changed.file.tokens_cover_text());
                assert_eq!(changed.file.root(), full.root(), "{source}: at={at} {replacement}");
                assert_eq!(changed.file.errors(), full.errors());
                let restored = changed.file.apply_edit(TextEdit {
                    start_byte: at as u32,
                    end_byte: (at + replacement.len()) as u32,
                    insert: ch.to_string(),
                });
                assert_eq!(restored.file.root(), original.root(), "undo at={at} {replacement}");
                assert_eq!(original.dump(), old);
            }
        }
    }
}

#[test]
fn missing_yield_operands_and_semicolons_keep_diagnostics() {
    for stmt in ["yield;", "yield n", "yield ) ;"] {
        let source = format!("class C {{int f(int n){{return switch(n){{default -> {{{stmt}}}}};}}}}");
        let file = ParsedFile::parse(&source);
        assert!(file.tokens_cover_text());
        assert!(!file.errors().is_empty(), "{source}\n{}", file.dump());
    }
}

#[test]
fn contextual_yield_never_becomes_a_restricted_type_name() {
    for source in [
        "import p.yield; class C{}",
        "import static p.yield.x; class C{}",
        "import static p.yield.*; class C{}",
        "class yield {}",
        "class C<yield>{}",
        "class C{p.yield x;}",
    ] {
        assert!(!ParsedFile::parse(source).errors().is_empty(), "{source}");
    }
    for source in [
        "package yield.pkg; class C{}",
        "import yield.pkg.C; class Use {yield.pkg.C value;}",
        "import static p.C.yield; class C{}",
        "class C{int yield(int x){return x;} int f(){return this.yield(0);}}",
    ] {
        assert!(ParsedFile::parse(source).errors().is_empty(), "{source}");
    }
}

#[test]
fn yield_like_statements_outside_switch_keep_diagnostics() {
    for statement in ["yield(x);", "yield +x;", "yield -x;", "yield x;"] {
        let source = format!("class C {{void f(int x){{{statement}}}}}");
        let file = ParsedFile::parse(&source);
        assert!(file.tokens_cover_text());
        assert!(!file.errors().is_empty(), "{source}");
        assert_eq!(count(file.root(), SyntaxKind::YieldStmt), 0);
    }
}

#[test]
fn strict_switch_actuals_preserve_the_complete_overload_candidates() {
    let source = "class C {static void choose(int x){} static void choose(long x){} static void f(int n){choose(switch(n){default -> 1;});}}";
    let result = query(source, "choose(switch");
    assert_eq!(result.status, ResolveStatus::OverloadNotResolved, "{result:?}");
    assert_eq!(result.targets.len(), 2);
    assert_eq!(
        result.targets.iter().map(|t| t.start_byte).collect::<Vec<_>>(),
        vec![source.find("choose(int").unwrap() as u32, source.find("choose(long").unwrap() as u32]
    );
}

#[test]
fn argument_and_index_switches_do_not_taint_the_receiver_spine() {
    // These are name-level controls, not newly inferred call/array result types.
    // Compare existing outer lookup against an equivalent ordinary operand.
    for template in
        ["class C {int x; void f(C c,int n){c.call(ARG).x;}}", "class C {int x; void f(C c,int n){c[ARG].x;}}"]
    {
        let plain = template.replace("ARG", "n");
        let switched = template.replace("ARG", "switch(n){default -> n;}");
        let baseline = query(&plain, "x;}}");
        let candidate = query(&switched, "x;}}");
        assert_eq!(candidate.status, baseline.status);
        assert_eq!(candidate.targets, baseline.targets);
        // An unrelated unsupported call result can already be refused; this
        // equality must not be mistaken for a new result-typing guarantee.
        // The private provenance unit test checks argument exclusion directly.
    }
}

#[test]
fn edits_inside_expression_rule_blocks_do_not_splice_context_free_fragments() {
    fn blocks(node: &std::sync::Arc<java_engine::GreenNode>, out: &mut Vec<std::sync::Arc<java_engine::GreenNode>>) {
        if node.kind() == SyntaxKind::Block {
            out.push(node.clone());
        }
        for child in node.child_nodes() {
            blocks(child, out);
        }
    }
    let source = "class C {int f(int n){return switch(n){default -> {int x=1; yield x;}};}}";
    let file = ParsedFile::parse(source);
    let at = source.find("x=1").unwrap() + 2;
    let changed = file.apply_edit(TextEdit { start_byte: at as u32, end_byte: at as u32 + 1, insert: "2".into() });
    assert_eq!(changed.file.root(), ParsedFile::parse(changed.file.text()).root());
    let (mut before, mut after) = (vec![], vec![]);
    blocks(file.root(), &mut before);
    blocks(changed.file.root(), &mut after);
    assert_eq!(before.len(), 2);
    assert_eq!(after.len(), 2);
    assert!(!std::sync::Arc::ptr_eq(&before[0], &after[0])); // method rechecked
    assert_eq!(count(changed.file.root(), SyntaxKind::YieldStmt), 1);
}
