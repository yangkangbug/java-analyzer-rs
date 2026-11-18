use java_engine::{Project, ResolveResult, ResolveStatus};

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

fn project(source: &str) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", source);
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, &[])).unwrap();
    p.add_class_bytes(
        "String.class",
        &binary("java/lang/String", Some("java/lang/Object"), 0x31, &[("length", "()I", 0x101, None)]),
    )
    .unwrap();
    p
}
fn query(p: &Project) -> ResolveResult {
    let at = p.file("Calls.java").unwrap().text().find("/*Q*/").unwrap() as u32 + 5;
    let r = p.resolve_at("Calls.java", at);
    assert_eq!(r, p.prepare().resolve_at("Calls.java", at));
    r
}
#[test]
fn ordinary_string_literal_receiver_uses_platform_method() {
    for literal in [r#""hello""#, r#"("中文😀")"#, r#"(("\n"))"#] {
        let p = project(&format!("class Calls {{int f(){{return {literal}./*Q*/length();}}}}"));
        let r = query(&p);
        assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
        assert_eq!(r.targets.len(), 1);
        assert_eq!(r.targets[0].owner, "java.lang.String");
        assert_eq!(r.targets[0].binary.as_ref().unwrap().descriptor, "()I");
    }
}
#[test]
fn literal_receiver_ignores_lexical_platform_name_shadows() {
    for prefix in ["class String {}", "class java {}", ""] {
        let p = project(&format!(r#"{prefix} class Calls {{int f(){{return "x"./*Q*/length();}}}}"#));
        assert_eq!(query(&p).status, ResolveStatus::Resolved);
    }
}
#[test]
fn unsupported_literals_and_call_shapes_remain_refused() {
    for receiver in ["null", "1", "true", "'x'", r#""\q""#, r#""\u0061""#, r#""a"+"b""#] {
        let p = project(&format!("class Calls {{int f(){{return ({receiver})./*Q*/length();}}}}"));
        assert_ne!(query(&p).status, ResolveStatus::Resolved, "{receiver}");
    }
    for expression in [r#""x"./*Q*/length(1)"#, r#""x"./*Q*/missing()"#] {
        assert_ne!(
            query(&project(&format!("class Calls {{int f(){{return {expression};}}}}"))).status,
            ResolveStatus::Resolved
        );
    }
    for body in [r#"int n="x"./*Q*/length();"#, r#"<T> int f(){return "x"./*Q*/length();}"#] {
        assert_ne!(query(&project(&format!("class Calls {{{body}}}"))).status, ResolveStatus::Resolved);
    }
}
#[test]
fn literal_receiver_requires_unique_complete_binary_platform() {
    let source = r#"class Calls {int f(){return "x"./*Q*/length();}}"#;
    for missing in ["Object.class", "String.class"] {
        let mut p = project(source);
        p.remove_class(missing);
        assert_ne!(query(&p).status, ResolveStatus::Resolved);
    }
    let mut p = project(source);
    p.insert("Shadow.java", "package java.lang; public final class String {public int length(){return 1;}}");
    assert_ne!(query(&p).status, ResolveStatus::Resolved);
    p.remove_class("String.class");
    assert_ne!(query(&p).status, ResolveStatus::Resolved);
    for flags in [0x21, 0x30, 0x231] {
        let mut p = project(source);
        p.add_class_bytes(
            "String.class",
            &binary("java/lang/String", Some("java/lang/Object"), flags, &[("length", "()I", 0x101, None)]),
        )
        .unwrap();
        assert_ne!(query(&p).status, ResolveStatus::Resolved);
    }
    let mut p = project(source);
    p.add_class_bytes(
        "Other.class",
        &binary("java/lang/String", Some("java/lang/Object"), 0x31, &[("length", "()I", 0x101, None)]),
    )
    .unwrap();
    assert_ne!(query(&p).status, ResolveStatus::Resolved);
}
#[test]
fn literal_receiver_preserves_method_inventory_and_budget_guards() {
    let source = r#"class Calls {int f(){return "x"./*Q*/length();}}"#;
    for methods in [
        vec![("length", "()I", 2, None)],
        vec![("length", "()I", 1, None), ("length", "([I)I", 0x81, None)],
        // ACC_VARARGS without an array tail remains malformed inventory.
        vec![("length", "()I", 1, None), ("length", "(I)I", 0x81, None)],
        vec![("length", "()I", 1, Some("<T:Ljava/lang/Object;>()I"))],
    ] {
        let mut p = project(source);
        p.add_class_bytes("String.class", &binary("java/lang/String", Some("java/lang/Object"), 0x31, &methods))
            .unwrap();
        if methods == vec![("length", "()I", 1, None), ("length", "([I)I", 0x81, None)] {
            let r = query(&p);
            assert_eq!(r.status, ResolveStatus::Resolved);
            assert_eq!(r.targets.len(), 1);
            assert_eq!(r.targets[0].owner, "java.lang.String");
            assert_eq!(r.targets[0].binary.as_ref().unwrap().descriptor, "()I");
        } else {
            assert_ne!(query(&p).status, ResolveStatus::Resolved);
        }
    }
    for receiver in [format!("\"{}\"", "x".repeat(5000)), format!("{}\"x\"{}", "(".repeat(40), ")".repeat(40))] {
        assert_ne!(
            query(&project(&format!("class Calls {{int f(){{return {receiver}./*Q*/length();}}}}"))).status,
            ResolveStatus::Resolved
        );
    }
}
#[test]
fn editing_literal_receiver_keeps_old_snapshot_and_matches_fresh() {
    use java_engine::TextEdit;
    let source = r#"class Calls {int f(){return "中文😀"./*Q*/length();}}"#;
    let mut p = project(source);
    let old = p.clone();
    let before = query(&old);
    assert_eq!(before.status, ResolveStatus::Resolved);
    let start = source.find("\"中文").unwrap() as u32;
    let end = source.find("./*Q*/").unwrap() as u32;
    p.edit("Calls.java", TextEdit { start_byte: start, end_byte: end, insert: "null".into() }).unwrap();
    assert_ne!(query(&p).status, ResolveStatus::Resolved);
    assert_eq!(query(&p), query(&project(p.file("Calls.java").unwrap().text())));
    assert_eq!(query(&old), before);
}
#[test]
fn literal_receiver_uses_existing_overload_selection() {
    for (arg, descriptor) in [("1", "(I)I"), (r#""x""#, "(Ljava/lang/String;)I")] {
        let mut p = project(&format!("class Calls {{int f(){{return \"xyz\"./*Q*/indexOf({arg});}}}}"));
        p.add_class_bytes(
            "String.class",
            &binary(
                "java/lang/String",
                Some("java/lang/Object"),
                0x31,
                &[("indexOf", "(I)I", 0x101, None), ("indexOf", "(Ljava/lang/String;)I", 0x101, None)],
            ),
        )
        .unwrap();
        let r = query(&p);
        assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
        assert_eq!(r.targets[0].binary.as_ref().unwrap().descriptor, descriptor);
    }
}
