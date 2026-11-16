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

fn project(text: &str, binary_methods: Option<&[Method<'_>]>) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", text);
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, &[])).unwrap();
    if let Some(methods) = binary_methods {
        p.add_class_bytes("I.class", &binary("I", Some("java/lang/Object"), 0x601, methods)).unwrap();
    }
    p
}
fn query(p: &Project) -> ResolveResult {
    let text = p.file("Calls.java").unwrap().text();
    let at = (text.find("/*Q*/").unwrap() + 5) as u32;
    let r = p.resolve_at("Calls.java", at);
    assert_eq!(r, p.prepare().resolve_at("Calls.java", at));
    r
}
fn missing(r: ResolveResult) {
    assert_ne!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert!(r.targets.is_empty(), "non-inherited method leaked: {r:?}");
}
#[test]
fn interface_static_methods_are_not_inherited_in_any_navigation_form() {
    for call in ["/*Q*/util();", "C./*Q*/util();", "c./*Q*/util();", "J./*Q*/util();"] {
        let body = format!("interface J extends I {{}} class C implements I {{void run(C c){{{call}}}}}");
        missing(query(&project(&format!("interface I {{static void util(){{}}}} {body}"), None)));
        missing(query(&project(&body, Some(&[("util", "()V", 0x9, None)]))));
    }
}
#[test]
fn direct_interface_static_declaration_remains_a_navigation_target() {
    let body = "class C {void run(){I./*Q*/util();}}";
    for p in [
        project(&format!("interface I {{static void util(){{}}}} {body}"), None),
        project(body, Some(&[("util", "()V", 0x9, None)])),
    ] {
        let r = query(&p);
        assert_eq!(r.targets.len(), 1, "{r:?}");
        assert_eq!(r.targets[0].owner, "I");
        assert_eq!(r.targets[0].name, "util");
    }
}
#[test]
fn unsupported_static_signatures_do_not_become_inherited_navigation() {
    for declaration in
        ["static <T> void util(T x){}", "static void util(int... xs){}", "@Deprecated static void util(){}"]
    {
        missing(query(&project(
            &format!("interface I {{{declaration}}} class C implements I {{void run(){{/*Q*/util();}}}}"),
            None,
        )));
    }
    for method in
        [("util", "(Ljava/lang/Object;)V", 0x9, Some("<T:Ljava/lang/Object;>(TT;)V")), ("util", "([I)V", 0x89, None)]
    {
        missing(query(&project("class C implements I {void run(){/*Q*/util();}}", Some(&[method]))));
    }
}
#[test]
fn interface_instance_methods_still_inherit_and_private_methods_do_not() {
    for declaration in ["void util();", "default void util(){}"] {
        let p = project(
            &format!("interface I {{{declaration}}} abstract class C implements I {{void run(){{/*Q*/util();}}}}"),
            None,
        );
        let r = query(&p);
        assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
        assert_eq!(r.targets[0].owner, "I");
    }
    for flags in [0x401, 0x1] {
        let r = query(&project(
            "abstract class C implements I {void run(){/*Q*/util();}}",
            Some(&[("util", "()V", flags, None)]),
        ));
        assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
        assert_eq!(r.targets[0].owner, "I");
    }
    for declaration in ["private void util(){}", "private static void util(){}"] {
        missing(query(&project(
            &format!("interface I {{{declaration}}} class C implements I {{void run(){{/*Q*/util();}}}}"),
            None,
        )));
    }
    for flags in [0x2, 0xa] {
        missing(query(&project(
            "class C implements I {void run(){/*Q*/util();}}",
            Some(&[("util", "()V", flags, None)]),
        )));
    }
}
#[test]
fn filtered_navigation_never_certifies_malformed_ancestor_inventory() {
    for method in [
        ("make", "()LProduct;", 0x409, None),
        ("make", "(Q)LProduct;", 0x9, None),
        ("make", "()LProduct;", 0x9, Some("()TUnknown;")),
    ] {
        let text = "class Product {public void hit(){}} class C implements I {void run(){make()./*Q*/hit();}}";
        let mut p = project(text, None);
        // Admission may already reject malformed descriptors; either boundary must prevent proof.
        let _ = p.add_class_bytes("I.class", &binary("I", Some("java/lang/Object"), 0x601, &[method]));
        assert_ne!(query(&p).status, ResolveStatus::Resolved, "{method:?}");
    }
}

#[test]
fn contradictory_modifiers_do_not_authorize_navigation_exclusions() {
    for declaration in [
        "abstract static void util(){}",
        "public private static void util(){}",
        "static static void util(){}",
        "static void util();",
    ] {
        let p = project(
            &format!("interface I {{{declaration}}} class C implements I {{void run(){{/*Q*/util();}}}}"),
            None,
        );
        assert!(query(&p).targets.iter().any(|t| t.owner == "I" && t.name == "util"), "{declaration}");
    }
    for flags in [0x409, 0xb, 0x19, 0x109] {
        let p = project("class C implements I {void run(){/*Q*/util();}}", Some(&[("util", "()V", flags, None)]));
        assert!(query(&p).targets.iter().any(|t| t.owner == "I" && t.name == "util"), "flags {flags:x}");
    }
}

#[test]
fn interface_constants_still_inherit_and_class_private_navigation_is_unchanged() {
    for access in ["/*Q*/VALUE", "C./*Q*/VALUE", "J./*Q*/VALUE"] {
        let text = format!(
            "interface I {{int VALUE=1;}} interface J extends I {{}} class C implements I {{void run(){{int n={access};}}}}"
        );
        let r = query(&project(&text, None));
        assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
        assert_eq!(r.targets.len(), 1);
        assert_eq!(r.targets[0].owner, "I");
        assert_eq!(r.targets[0].name, "VALUE");
        assert_eq!(r.targets[0].kind, java_engine::TargetKind::Field);
    }
    let p = project("class C {private void util(){} void run(){/*Q*/util();}}", None);
    let r = query(&p);
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].owner, "C");
    missing(query(&project("class B {private void util(){}} class C extends B {void run(){/*Q*/util();}}", None)));
}

#[test]
fn editing_static_to_default_reindexes_inheritance_fact() {
    use java_engine::TextEdit;
    let text = "interface I {static void util(){}} class C implements I {void run(){/*Q*/util();}}";
    let mut p = project(text, None);
    let old = p.clone();
    let before = query(&old);
    missing(before.clone());
    let at = text.find("static").unwrap() as u32;
    p.edit("Calls.java", TextEdit { start_byte: at, end_byte: at + 6, insert: "default".into() }).unwrap();
    let r = query(&p);
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].owner, "I");
    assert_eq!(r.targets[0].start_byte, p.file("Calls.java").unwrap().text().find("util()").unwrap() as u32);
    assert_eq!(r, query(&project(p.file("Calls.java").unwrap().text(), None)));
    assert_eq!(query(&old), before);
    p.edit("Calls.java", TextEdit { start_byte: at, end_byte: at + 7, insert: "static".into() }).unwrap();
    assert_eq!(query(&p), before);
}
