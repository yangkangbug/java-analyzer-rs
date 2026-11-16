use java_engine::{ParsedFile, Project, ResolveStatus, TextEdit};
type Method<'a> = (&'a str, &'a str, u16, Option<&'a str>);
fn binary(name: &str, parent: Option<&str>, flags: u16, methods: &[Method<'_>]) -> Vec<u8> {
    fn u(b: &mut Vec<u8>, v: u16) {
        b.extend(v.to_be_bytes());
    }
    fn utf(b: &mut Vec<u8>, n: &mut u16, s: &str) -> u16 {
        let at = *n;
        *n += 1;
        b.push(1);
        u(b, s.len() as u16);
        b.extend(s.as_bytes());
        at
    }
    fn cls(b: &mut Vec<u8>, n: &mut u16, s: &str) -> u16 {
        let t = utf(b, n, s);
        let at = *n;
        *n += 1;
        b.push(7);
        u(b, t);
        at
    }
    let mut cp = Vec::new();
    let mut n = 1;
    let this = cls(&mut cp, &mut n, name);
    let parent = parent.map(|p| cls(&mut cp, &mut n, p)).unwrap_or(0);
    let signame = utf(&mut cp, &mut n, "Signature");
    let records: Vec<_> = methods
        .iter()
        .map(|(name, desc, flags, sig)| {
            (utf(&mut cp, &mut n, name), utf(&mut cp, &mut n, desc), *flags, sig.map(|s| utf(&mut cp, &mut n, s)))
        })
        .collect();
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61];
    u(&mut b, n);
    b.extend(cp);
    for v in [flags, this, parent, 0, 0, records.len() as u16] {
        u(&mut b, v);
    }
    for (name, desc, flags, sig) in records {
        for v in [flags, name, desc, if sig.is_some() { 1 } else { 0 }] {
            u(&mut b, v);
        }
        if let Some(sig) = sig {
            u(&mut b, signame);
            b.extend(2u32.to_be_bytes());
            u(&mut b, sig);
        }
    }
    u(&mut b, 0);
    b
}
fn query(s: &str, marker: &str) -> java_engine::ResolveResult {
    let mut p = Project::new();
    p.insert("C.java", s);
    let at = s.find(marker).unwrap() as u32;
    let r = p.resolve_at("C.java", at);
    assert_eq!(r, p.prepare().resolve_at("C.java", at));
    r
}
#[test]
fn lossless_utf8_label_tokens_do_not_enter_value_namespace() {
    let s = "class C {void m(int 名){名 /* 注释 */ : while(true){名++; break 名;} 名++;}}";
    let p = ParsedFile::parse(s);
    assert!(p.errors().is_empty(), "{}", p.dump());
    assert!(p.tokens_cover_text());
    assert!(p.dump().contains("LabeledStmt"));
    assert_eq!(query(s, "名 /*").status, ResolveStatus::Unresolved);
    assert_eq!(query(s, "名;}").status, ResolveStatus::Unresolved);
    let r = query(s, "名++;");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets[0].start_byte, s.find("名)").unwrap() as u32);
    let s = "class C {void m(int label){label: for(;;){continue label;}}}";
    assert_eq!(query(s, "label;").status, ResolveStatus::Unresolved);
}
#[test]
fn labeled_for_and_block_preserve_local_boundaries() {
    let s = "class C {void m(){loop: for(int i=0;i<2;i++){i++;} i++; block: {int hidden=0;hidden++;} hidden++; int after=0;after++;}}";
    assert!(ParsedFile::parse(s).errors().is_empty());
    assert_eq!(query(s, "i++;}").status, ResolveStatus::Resolved);
    assert_eq!(query(s, "i++; block").status, ResolveStatus::Unresolved);
    assert_eq!(query(s, "hidden++;}").status, ResolveStatus::Resolved);
    assert_eq!(query(s, "hidden++; int").status, ResolveStatus::Unresolved);
    assert_eq!(query(s, "after++;").status, ResolveStatus::Resolved);
}
#[test]
fn continuous_labels_are_bounded_without_losing_tokens() {
    for count in [1, 2, 32, 33, 1000] {
        let labels = (0..count).map(|i| format!("l{i}:")).collect::<String>();
        let s = format!("class C {{void m(){{{labels} ; int after=0;after++;}}}}");
        let p = ParsedFile::parse(&s);
        assert!(p.tokens_cover_text());
        assert_eq!(p.errors().is_empty(), count <= 32, "{count}");
        assert_eq!(query(&s, "after++;").status, ResolveStatus::Resolved);
    }
}
#[test]
fn dangling_else_belongs_to_inner_if_through_label() {
    let s = "class C {void m(boolean a,boolean b){if(a) label: if(b) ; else ;}}";
    let p = ParsedFile::parse(s);
    assert!(p.errors().is_empty(), "{}", p.dump());
    fn walk(n: &java_engine::GreenNode, found: &mut Vec<bool>) {
        if format!("{:?}", n.kind()) == "IfStmt" {
            found.push(n.children().iter().filter_map(|c| c.as_token()).any(|t| t.text() == "else"));
        }
        for c in n.child_nodes() {
            walk(c, found);
        }
    }
    let mut found = Vec::new();
    walk(p.root(), &mut found);
    assert_eq!(found, vec![false, true]);
}
#[test]
fn illegal_direct_declarations_do_not_export_bindings() {
    for body in ["int hidden=0;", "final int hidden=0;", "class Hidden {int secret;}"] {
        let s = format!("class C {{void m(){{bad: {body} hidden++; int after=0;after++;}}}}");
        let p = ParsedFile::parse(&s);
        assert!(!p.errors().is_empty(), "{body}");
        assert!(p.tokens_cover_text());
        assert_eq!(query(&s, "hidden++;").status, ResolveStatus::Unresolved, "{body}");
        assert_eq!(query(&s, "after++;").status, ResolveStatus::Resolved);
    }
}
#[test]
fn missing_body_preserves_outer_delimiters_and_following_members() {
    for s in [
        "class C {void m(){label:}}",
        "class C {void m(){label:",
        "class C {void m(boolean b){if(b) label: else ;} void next(){int after=0;after++;}}",
    ] {
        let p = ParsedFile::parse(s);
        assert!(!p.errors().is_empty(), "{s}");
        assert!(p.tokens_cover_text());
        if s.contains("after++") {
            assert_eq!(query(s, "after++;").status, ResolveStatus::Resolved);
        }
    }
}
#[test]
fn edits_match_fresh_parse_across_label_boundaries() {
    let s = "class C {void m(boolean a, boolean b){if(a) 标签: if(b) {int v=0;v++;} else ;}}";
    for (old, new) in [
        ("标签:", "newLabel:"),
        ("标签:", "标签"),
        ("标签:", "a:b:"),
        ("{int v=0;v++;}", ";"),
        ("if(b) {int v=0;v++;}", ""),
        ("v=0", "名字=0"),
        ("else ;", "else {int x=1;}"),
    ] {
        let at = s.find(old).unwrap();
        let p = ParsedFile::parse(s);
        let edit =
            p.apply_edit(TextEdit { start_byte: at as u32, end_byte: (at + old.len()) as u32, insert: new.into() });
        let fresh = ParsedFile::parse(&s.replacen(old, new, 1));
        assert!(edit.file.tokens_cover_text());
        assert_eq!(edit.file.dump(), fresh.dump(), "{old} => {new}");
        assert_eq!(edit.file.errors(), fresh.errors());
    }
}
#[test]
fn legal_labels_preserve_static_call_proof() {
    let api = "package lib; public class Api {public static int pick(int i){done: {i++;} return i;}}";
    for labeled_caller in [false, true] {
        let body = if labeled_caller { "here: {box.take(Api.pick(1));}" } else { "box.take(Api.pick(1));" };
        let s = format!(
            "import lib.Api; class Box {{public void take(int i){{}} public void take(long i){{}}}} class C {{void m(Box box){{{body}}}}}"
        );
        let mut p = Project::new();
        p.insert("Api.java", api);
        p.insert("C.java", &s);
        p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, &[])).unwrap();
        let r = p.resolve_at("C.java", s.find("take(Api.pick").unwrap() as u32);
        assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
        if r.status == ResolveStatus::Resolved {
            assert_eq!(r.targets[0].start_byte, s.find("take(int i)").unwrap() as u32);
        }
    }
}

#[test]
fn invalid_labeled_declaration_blocks_return_proof_and_edit_recovers() {
    let api = "package lib; public class Api {public static int pick(int i){bad: int hidden=0; return i;}}";
    let parsed = ParsedFile::parse(api);
    assert!(!parsed.errors().is_empty());
    let body = "int hidden=0;";
    let at = api.find(body).unwrap();
    let changed = parsed.apply_edit(TextEdit {
        start_byte: at as u32,
        end_byte: (at + body.len()) as u32,
        insert: "{int hidden=0;}".into(),
    });
    let repaired = api.replacen(body, "{int hidden=0;}", 1);
    let fresh = ParsedFile::parse(&repaired);
    assert!(fresh.errors().is_empty(), "{}", fresh.dump());
    assert!(changed.file.tokens_cover_text());
    assert_eq!(changed.file.dump(), fresh.dump());
    assert_eq!(changed.file.errors(), fresh.errors());
    let caller = "import lib.Api; class Box {public void take(int i){} public void take(long i){}} class C {void m(Box box){box.take(Api.pick(1));}}";
    for (source, expected) in [(api, false), (repaired.as_str(), true)] {
        let mut project = Project::new();
        project.insert("Api.java", source);
        project.insert("C.java", caller);
        project.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, &[])).unwrap();
        let result = project.resolve_at("C.java", caller.find("take(Api.pick").unwrap() as u32);
        assert_eq!(result.status == ResolveStatus::Resolved, expected, "{result:?}");
        if expected {
            assert_eq!(result.targets.len(), 1);
            assert_eq!(result.targets[0].start_byte, caller.find("take(int i)").unwrap() as u32);
        }
    }
}

#[test]
fn legal_labels_preserve_instance_overload_and_return_chain_proofs() {
    for has_label in [false, true] {
        let marker = if has_label { "unused: ;" } else { "" };
        let text = format!(
            "class Product {{public int value(int i){{return i;}} public int value(long i){{return 0;}}}} class Box {{public void take(int i){{}} public void take(long i){{}} public Product make(){{return null;}}}} class C {{void m(Box box){{{marker} box.take(1); box.make().value(1);}} void other(Box box){{box.take(2);}}}}"
        );
        let mut project = Project::new();
        project.insert("C.java", &text);
        project.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, &[])).unwrap();
        for call in ["take(1)", "value(1)"] {
            let at = text.find(call).unwrap() as u32;
            let result = project.resolve_at("C.java", at);
            assert_eq!(result, project.prepare().resolve_at("C.java", at));
            assert_eq!(result.status, ResolveStatus::Resolved, "{call} label={has_label}: {result:?}");
            assert_eq!(result.targets.len(), 1);
            let declaration = if call == "take(1)" { "take(int i)" } else { "value(int i)" };
            assert_eq!(result.targets[0].start_byte, text.find(declaration).unwrap() as u32);
        }
        // A separate unlabeled method retains the same exact selection.
        let result = project.resolve_at("C.java", text.find("take(2)").unwrap() as u32);
        assert_eq!(result.status, ResolveStatus::Resolved, "other method: {result:?}");
        assert_eq!(result.targets.len(), 1);
        assert_eq!(result.targets[0].start_byte, text.find("take(int i)").unwrap() as u32);
    }
}
