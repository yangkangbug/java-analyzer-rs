use java_engine::{ParsedFile, Project, ResolveStatus, TargetKind, TextEdit};

fn query(text: &str, marker: &str) -> java_engine::ResolveResult {
    let mut project = Project::new();
    project.insert("C.java", text);
    let at = text.find(marker).unwrap() as u32;
    let result = project.resolve_at("C.java", at);
    assert_eq!(result, project.prepare().resolve_at("C.java", at));
    result
}

#[test]
fn synchronized_is_lossless_and_reuses_block_scope() {
    let text = "class C { C lock; void f(){synchronized /*😀*/ (lock/*monitor*/){int lock=0;lock/*inner*/++;} lock/*after*/=null;}}";
    let parsed = ParsedFile::parse(text);
    assert!(parsed.tokens_cover_text());
    assert!(parsed.errors().is_empty(), "{}", parsed.dump());
    assert!(parsed.dump().contains("SynchronizedStmt"));
    for marker in ["lock/*monitor*/", "lock/*after*/"] {
        let result = query(text, marker);
        assert_eq!(result.status, ResolveStatus::Resolved);
        assert_eq!(result.targets[0].kind, TargetKind::Field);
        assert_eq!(result.targets[0].start_byte, text.find("lock;").unwrap() as u32);
    }
    let result = query(text, "lock/*inner*/");
    assert_eq!(result.status, ResolveStatus::Resolved);
    assert_eq!(result.targets[0].kind, TargetKind::Local);
    assert_eq!(result.targets[0].start_byte, text.find("lock=0").unwrap() as u32);
}

#[test]
fn synchronized_nested_blocks_and_method_modifier_keep_ordinary_calls() {
    let text = "class C {static C lock(){return null;} static void ping(){} synchronized void f(){synchronized(C.lock()){C.ping(); synchronized(this){int n=0;n++;}} n/*outside*/++;}}";
    assert!(ParsedFile::parse(text).errors().is_empty());
    for marker in ["lock()){", "ping();"] {
        let result = query(text, marker);
        assert_eq!(result.status, ResolveStatus::Resolved, "{marker}: {result:?}");
        assert_eq!(result.targets[0].kind, TargetKind::Method);
    }
    assert_eq!(query(text, "n++;}}").status, ResolveStatus::Resolved);
    assert_eq!(query(text, "n++;}}").targets[0].start_byte, text.find("n=0").unwrap() as u32);
    assert_eq!(query(text, "n/*outside*/").status, ResolveStatus::Unresolved);
}

#[test]
fn synchronized_requires_a_real_block_and_does_not_export_recovery_declarations() {
    for bad in [";", "int leaked=0;", "if(true){int leaked=0;}"] {
        let text = format!("class C {{void f(C lock){{synchronized(lock) {bad} leaked++; int after=1;after++;}}}}");
        let parsed = ParsedFile::parse(&text);
        assert!(parsed.tokens_cover_text());
        assert!(!parsed.errors().is_empty(), "{text}");
        assert_eq!(query(&text, "leaked++;").status, ResolveStatus::Unresolved, "{text}");
        assert_eq!(query(&text, "after++;").status, ResolveStatus::Resolved, "{text}");
    }
}

#[test]
fn synchronized_incomplete_delimiters_preserve_text_and_following_block_boundaries() {
    for statement in [
        "synchronized() {}",
        "synchronized(lock {}",
        "synchronized {int hidden=0;}",
        "synchronized(lock)",
        "synchronized(lock) { int hidden=0;",
    ] {
        let text = format!("class C {{void f(C lock){{{statement}}}}}");
        let parsed = ParsedFile::parse(&text);
        assert!(parsed.tokens_cover_text(), "{text}");
        assert!(!parsed.errors().is_empty(), "{text}");
    }
}

#[test]
fn synchronized_edit_sequences_and_undo_match_fresh() {
    let original = "class C {void f(C lock){synchronized(lock){int n=0;n++;} int after=1;after++;}}";
    for (old, replacement) in [
        ("lock){", "this){"),
        ("synchronized", "synchronize"),
        ("(lock)", "()"),
        ("(lock)", "lock"),
        ("(lock)", "(lock"),
        ("{int n=0;n++;}", ";"),
        ("{int n=0;n++;}", "int leaked=0;"),
        ("n=0", "名字=0"),
        ("n++;}", "n++;"),
        ("synchronized(lock){", "synchronized(lock){synchronized(lock){"),
    ] {
        let at = original.find(old).unwrap();
        let changed = ParsedFile::parse(original)
            .apply_edit(TextEdit {
                start_byte: at as u32,
                end_byte: (at + old.len()) as u32,
                insert: replacement.into(),
            })
            .file;
        let text = original.replacen(old, replacement, 1);
        let fresh = ParsedFile::parse(&text);
        assert!(changed.tokens_cover_text());
        assert_eq!(changed.dump(), fresh.dump(), "{old} -> {replacement}");
        assert_eq!(changed.errors(), fresh.errors());
        let mut incremental_project = Project::new();
        incremental_project.insert("C.java", original);
        incremental_project
            .edit(
                "C.java",
                TextEdit { start_byte: at as u32, end_byte: (at + old.len()) as u32, insert: replacement.into() },
            )
            .unwrap();
        let mut fresh_project = Project::new();
        fresh_project.insert("C.java", &text);
        let incremental_index = incremental_project.prepare();
        let fresh_index = fresh_project.prepare();
        for (byte, _) in text.char_indices() {
            assert_eq!(
                incremental_index.resolve_at("C.java", byte as u32),
                fresh_index.resolve_at("C.java", byte as u32),
                "{old} -> {replacement}, byte {byte}"
            );
        }
        let restored = changed
            .apply_edit(TextEdit {
                start_byte: at as u32,
                end_byte: (at + replacement.len()) as u32,
                insert: old.into(),
            })
            .file;
        assert_eq!(restored.dump(), ParsedFile::parse(original).dump(), "undo {old} -> {replacement}");
        assert!(restored.errors().is_empty());
    }
}

#[test]
fn invalid_synchronized_ancestor_blocks_reference_and_call_fallbacks() {
    for head in ["synchronized(lock", "synchronized()", "synchronized lock)"] {
        let text = format!(
            "class C {{int x; static void ping(){{}} void f(C lock){{{head}{{int x=0;x/*bad*/++;ping/*bad*/();}} int after=0;after++;}}}}"
        );
        assert!(!ParsedFile::parse(&text).errors().is_empty());
        for marker in ["x/*bad*/", "ping/*bad*/"] {
            assert_eq!(query(&text, marker).status, ResolveStatus::Unresolved, "{head} {marker}");
        }
        assert_eq!(query(&text, "after++;").status, ResolveStatus::Resolved);
    }
}

#[test]
fn synchronized_recovery_preserves_else_and_following_statements() {
    let text = "class C {void f(C lock){if(true) synchronized(lock) else {int n=0;n++;} n/*outside*/++; int after=0;after++;}}";
    let parsed = ParsedFile::parse(text);
    assert!(parsed.tokens_cover_text());
    assert!(!parsed.errors().is_empty());
    assert_eq!(query(text, "n++;}").status, ResolveStatus::Resolved);
    assert_eq!(query(text, "n/*outside*/").status, ResolveStatus::Unresolved);
    assert_eq!(query(text, "after++;").status, ResolveStatus::Resolved);
}

#[test]
fn synchronized_monitor_nested_blocks_recheck_header_after_damage_and_repair() {
    for original in [
        "class C {void f(){synchronized(new C(){void m(){int a=0;}}){int x=0;x++;}}}",
        "class C {static C use(Runnable r){return null;} void f(){synchronized(use(()->{int a=0;})){int x=0;x++;}}}",
    ] {
        let mut text = original.to_string();
        let mut parsed = ParsedFile::parse(&text);
        assert!(parsed.errors().is_empty(), "{}", parsed.dump());
        // Valid -> damaged -> another edit while damaged -> repaired -> undo edits.
        for (old, replacement) in [("a=0", "a="), ("x=0", "x=1"), ("a=", "a=0"), ("x=1", "x=0")] {
            let at = text.find(old).unwrap();
            parsed = parsed
                .apply_edit(TextEdit {
                    start_byte: at as u32,
                    end_byte: (at + old.len()) as u32,
                    insert: replacement.into(),
                })
                .file;
            text.replace_range(at..at + old.len(), replacement);
            let fresh = ParsedFile::parse(&text);
            assert!(parsed.tokens_cover_text());
            assert_eq!(parsed.dump(), fresh.dump(), "{old} -> {replacement}: {text}");
            assert_eq!(parsed.errors(), fresh.errors());
        }
        assert_eq!(text, original);
        assert!(parsed.errors().is_empty());
    }
}

#[test]
fn synchronized_body_edit_preserves_unchanged_monitor_tree() {
    use java_engine::{GreenNode, SyntaxKind};
    use std::sync::Arc;
    fn find(node: &Arc<GreenNode>, kind: SyntaxKind) -> Option<&Arc<GreenNode>> {
        if node.kind() == kind {
            return Some(node);
        }
        node.child_nodes().find_map(|child| find(child, kind))
    }
    let text = "class C {void f(){synchronized(new C(){void m(){int a=0;}}){int x=0;x++;}}}";
    let parsed = ParsedFile::parse(text);
    let at = text.find("x=0").unwrap() + 2;
    let edit = parsed.apply_edit(TextEdit { start_byte: at as u32, end_byte: at as u32 + 1, insert: "1".into() });
    assert!(edit.file.errors().is_empty());
    assert!(Arc::ptr_eq(
        find(parsed.root(), SyntaxKind::NewExpr).unwrap(),
        find(edit.file.root(), SyntaxKind::NewExpr).unwrap()
    ));
    assert_eq!(edit.file.dump(), ParsedFile::parse(&text.replacen("x=0", "x=1", 1)).dump());
}
