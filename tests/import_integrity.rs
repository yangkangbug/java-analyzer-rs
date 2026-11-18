use java_engine::{Project, ResolveStatus, TextEdit};
fn project(text: &str) -> Project {
    let mut p = Project::new();
    p.insert("p/C.java", "package p; public class C { public static int x; }");
    p.insert("q/C.java", "package q; public class C { public static int x; }");
    p.insert("C.java", "class C { public static int x; }");
    p.insert("Use.java", text);
    p
}
#[test]
fn damaged_import_cannot_promote_a_prefix_or_fallback() {
    for import in ["import p..C;", "import p.C.var;", "import p.C", "import p.;", "import p..*;"] {
        let text = format!("{import} class Use {{ C value; }}");
        let p = project(&text);
        let r = p.resolve_at("Use.java", text.rfind("C value").unwrap() as u32);
        // A malformed demand import does not outrank a known same-package type.
        if import == "import p..*;" {
            assert_eq!(r.status, ResolveStatus::Resolved);
        } else {
            assert_ne!(r.status, ResolveStatus::Resolved, "{text}: {r:?}");
        }
        assert_eq!(r, p.prepare().resolve_at("Use.java", text.rfind("C value").unwrap() as u32));
    }
}
#[test]
fn invalid_and_valid_single_competitors_do_not_disappear() {
    for imports in ["import p..C;import q.C;", "import q.C;import p..C;", "import p.;import q.C;"] {
        let text = format!("{imports} class Use {{ C value; }}");
        assert_ne!(
            project(&text).resolve_at("Use.java", text.rfind("C value").unwrap() as u32).status,
            ResolveStatus::Resolved
        );
    }
    let text = "import p.C;import p.C;class Use { C value; }";
    let r = project(text).resolve_at("Use.java", text.rfind("C value").unwrap() as u32);
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets[0].path, "p/C.java");
}
#[test]
fn malformed_import_does_not_suppress_local_or_own_field() {
    for text in [
        "import static p.;class Use { int x; int f(){return x;} }",
        "import static p.;class Use { int f(int x){return x;} }",
        "import p.;class Use { class C{} C value; }",
    ] {
        let name = if text.contains("C value") { "C value" } else { "x;}" };
        let r = project(text).resolve_at("Use.java", text.rfind(name).unwrap() as u32);
        assert_eq!(r.status, ResolveStatus::Resolved, "{text}: {r:?}");
        assert_eq!(r.targets[0].path, "Use.java");
    }
}
#[test]
fn import_damage_edits_match_fresh_and_keep_old_snapshot() {
    let text = "import p.C; class Use { C value; }";
    let mut p = project(text);
    let old = p.clone();
    let mut width = 3;
    for name in ["p..C", "p.", "q.C", "p.C"] {
        p.edit("Use.java", TextEdit { start_byte: 7, end_byte: 7 + width, insert: name.into() }).unwrap();
        width = name.len() as u32;
        let text = p.file("Use.java").unwrap().text();
        let fresh = project(text);
        let at = text.rfind("C value").unwrap() as u32;
        assert_eq!(p.resolve_at("Use.java", at), fresh.resolve_at("Use.java", at));
        assert_eq!(p.resolve_at("Use.java", at), p.prepare().resolve_at("Use.java", at));
    }
    assert_eq!(old.resolve_at("Use.java", text.rfind("C value").unwrap() as u32).targets[0].path, "p/C.java");
}

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

#[test]
fn complete_graph_static_imports_never_shorten_damaged_names() {
    for import in [
        "import static p.C..x;",
        "import static p.C.var.x;",
        "import static p.C.;",
        "import static p.C.x",
        "import static p.C..*;",
        "import static p.C.x;import static p.C..x;",
    ] {
        let text = format!("{import} class Use {{ int f(){{return x;}} }}");
        let mut p = project(&text);
        p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, &[])).unwrap();
        let r = p.resolve_at("Use.java", text.rfind("x;").unwrap() as u32);
        assert_ne!(r.status, ResolveStatus::Resolved, "{text}: {r:?}");
    }
    for import in ["import static p.C.x;", "import static p.C.x;import static p.C.x;"] {
        let text = format!("{import} class Use {{ int f(){{return x;}} }}");
        let mut p = project(&text);
        p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, &[])).unwrap();
        let r = p.resolve_at("Use.java", text.rfind("x;").unwrap() as u32);
        assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
        assert_eq!(r.targets[0].path, "p/C.java");
        assert_eq!(r.targets[0].name, "x");
    }
}
#[test]
fn malformed_demand_blocks_only_demand_and_not_same_package() {
    let text = "import p..*;class Use { C value; }";
    let mut p = Project::new();
    p.insert("p/C.java", "package p; public class C{}");
    p.insert("Use.java", text);
    assert_ne!(p.resolve_at("Use.java", text.rfind("C value").unwrap() as u32).status, ResolveStatus::Resolved);
    p.insert("C.java", "class C{}");
    assert_eq!(p.resolve_at("Use.java", text.rfind("C value").unwrap() as u32).targets[0].path, "C.java");
}

#[test]
fn malformed_single_is_unknown_not_java_ambiguity() {
    let text = "import p..C;class C{} class Use{C value;}";
    let r = project(text).resolve_at("Use.java", text.rfind("C value").unwrap() as u32);
    assert_eq!(r.status, ResolveStatus::Unresolved);
    assert!(r.targets.is_empty());
    let valid = "import p.C;class C{} class Use{C value;}";
    let r = project(valid).resolve_at("Use.java", valid.rfind("C value").unwrap() as u32);
    assert_eq!(r.status, ResolveStatus::Ambiguous);
}
#[test]
fn valid_demand_and_static_method_contracts_survive_unknown_competitors() {
    let text = "import p.*;class Use{C value;}";
    let mut p = Project::new();
    p.insert("p/C.java", "package p;public class C{}");
    p.insert("Use.java", text);
    assert_eq!(p.resolve_at("Use.java", text.rfind("C value").unwrap() as u32).targets[0].path, "p/C.java");
    for import in ["import static p.Api.pick;", "import static p.Api..pick;", "import static p.Api.;"] {
        let text = format!("{import} class Use{{void run(){{pick();}}}}");
        let mut p = Project::new();
        p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, &[])).unwrap();
        p.insert("p/Api.java", "package p;public class Api{public static void pick(){}}");
        p.insert("Use.java", &text);
        let r = p.resolve_at("Use.java", text.rfind("pick()").unwrap() as u32);
        if import == "import static p.Api.pick;" {
            assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
            assert_eq!(r.targets[0].path, "p/Api.java");
        } else {
            assert_ne!(r.status, ResolveStatus::Resolved, "{text}: {r:?}");
        }
    }
}
