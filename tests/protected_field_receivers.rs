use java_engine::{Project, ResolveStatus};
fn check(a: &str, b: &str, extra: &[(&str, &str)], status: ResolveStatus, owners: &[&str]) {
    let mut p = Project::new();
    platform(&mut p);
    p.insert("p/A.java", a);
    p.insert("q/B.java", b);
    for (path, text) in extra {
        p.insert(*path, text);
    }
    let at = (b.find("/*Q*/").unwrap() + 5) as u32;
    let r = p.resolve_at("q/B.java", at);
    assert_eq!(r, p.prepare().resolve_at("q/B.java", at));
    assert_eq!(r.status, status, "{r:?}");
    let mut actual: Vec<_> = r.targets.iter().map(|x| x.owner.as_str()).collect();
    actual.sort();
    let mut expected = owners.to_vec();
    expected.sort();
    assert_eq!(actual, expected, "{r:?}");
}
const A: &str = "package p; public class A {protected int x;}";
#[test]
fn cross_package_instance_field_requires_subclass_receiver() {
    for receiver in ["A", "Sibling"] {
        check(
            A,
            &format!(
                "package q; import p.A; class Sibling extends A {{}} class B extends A {{int f({receiver} a){{return a./*Q*/x;}}}}"
            ),
            &[],
            ResolveStatus::Unresolved,
            &[],
        );
    }
    for receiver in ["B", "Child"] {
        check(
            A,
            &format!(
                "package q; import p.A; class Child extends B {{}} class B extends A {{int f({receiver} b){{return b./*Q*/x;}}}}"
            ),
            &[],
            ResolveStatus::Resolved,
            &["p.A"],
        );
    }
}
#[test]
fn static_same_package_and_implicit_receivers_remain_valid() {
    check(
        "package p; public class A {protected static int x;}",
        "package q; import p.A; class B extends A {int f(A a){return a./*Q*/x;}}",
        &[],
        ResolveStatus::Resolved,
        &["p.A"],
    );
    check(A, "package p; class B {int f(A a){return a./*Q*/x;}}", &[], ResolveStatus::Resolved, &["p.A"]);
    for expr in ["/*Q*/x", "this./*Q*/x"] {
        check(
            A,
            &format!("package q; import p.A; class B extends A {{int f(){{return {expr};}}}}"),
            &[],
            ResolveStatus::Resolved,
            &["p.A"],
        );
    }
}
#[test]
fn nested_context_can_use_enclosing_subclass_not_base_receiver() {
    check(
        A,
        "package q; import p.A; class B extends A {static class N {int f(B b){return b./*Q*/x;}}}",
        &[],
        ResolveStatus::Resolved,
        &["p.A"],
    );
    check(
        A,
        "package q; import p.A; class B extends A {static class N {int f(A a){return a./*Q*/x;}}}",
        &[],
        ResolveStatus::Unresolved,
        &[],
    );
}
#[test]
fn unknown_receiver_graph_must_not_manufacture_unique_competitor() {
    let i = ("p/I.java", "package p; public interface I {int x=1;}");
    for missing in [false, true] {
        let c =
            format!("package p; public class C extends A implements I{} {{}}", if missing { ", Missing" } else { "" });
        check(
            A,
            "package q; import p.A; import p.C; class B extends A {int f(C c){return c./*Q*/x;}}",
            &[i, ("p/C.java", &c)],
            if missing { ResolveStatus::Ambiguous } else { ResolveStatus::Resolved },
            if missing { &["p.A", "p.I"] } else { &["p.I"] },
        );
    }
}

type Method<'a> = (&'a str, &'a str, u16, Option<&'a str>);
fn binary_fields(name: &str, parent: Option<&str>, flags: u16, methods: &[Method<'_>]) -> Vec<u8> {
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
    for v in [flags, this, parent, 0, records.len() as u16] {
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
    u(&mut b, 0); // methods
    u(&mut b, 0); // class attributes
    b
}

fn platform(p: &mut Project) {
    p.add_class_bytes("Object.class", &binary_fields("java/lang/Object", None, 0x21, &[])).unwrap();
}
fn query(p: &Project) -> java_engine::ResolveResult {
    let t = p.file("q/B.java").unwrap().text();
    let at = (t.find("/*Q*/").unwrap() + 5) as u32;
    let r = p.resolve_at("q/B.java", at);
    assert_eq!(r, p.prepare().resolve_at("q/B.java", at));
    r
}
#[test]
fn binary_receiver_guard_matches_source_and_retains_unknown_flags() {
    for (flags, status) in
        [(4, ResolveStatus::Unresolved), (12, ResolveStatus::Resolved), (0x54, ResolveStatus::Resolved)]
    {
        let mut p = Project::new();
        platform(&mut p);
        p.add_class_bytes("A.class", &binary_fields("p/A", Some("java/lang/Object"), 0x21, &[("x", "I", flags, None)]))
            .unwrap();
        p.insert("q/B.java", "package q; import p.A; class B extends A {int f(A a){return a./*Q*/x;}}");
        let r = query(&p);
        assert_eq!(r.status, status, "{flags:x}: {r:?}");
        if status == ResolveStatus::Resolved {
            assert_eq!(r.targets.len(), 1);
            assert_eq!(r.targets[0].path, "A.class");
        }
    }
}
#[test]
fn unknown_source_field_fact_keeps_competitor_and_super_is_not_newly_supported() {
    check(
        "package p; public class A {protected @Unknown int x;}",
        "package q; import p.A; import p.C; class B extends A {int f(C c){return c./*Q*/x;}}",
        &[
            ("p/I.java", "package p; public interface I {int x=1;}"),
            ("p/C.java", "package p; public class C extends A implements I {}"),
        ],
        ResolveStatus::Ambiguous,
        &["p.A", "p.I"],
    );
    let mut p = Project::new();
    platform(&mut p);
    p.insert("p/A.java", A);
    p.insert("q/B.java", "package q; import p.A; class B extends A {int f(){return super./*Q*/x;}}");
    assert_ne!(query(&p).status, ResolveStatus::Resolved);
}
#[test]
fn missing_platform_keeps_previous_candidate() {
    let mut p = Project::new();
    p.insert("p/A.java", A);
    p.insert("q/B.java", "package q; import p.A; class B extends A {int f(A a){return a./*Q*/x;}}");
    let r = query(&p);
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets[0].owner, "p.A");
}
#[test]
fn receiver_type_edit_matches_fresh_and_preserves_old_snapshot() {
    use java_engine::TextEdit;
    let text = "package q; import p.A; class B extends A {int f(A a){return a./*Q*/x;}}";
    let mut p = Project::new();
    platform(&mut p);
    p.insert("p/A.java", A);
    p.insert("q/B.java", text);
    let old = p.clone();
    let before = query(&old);
    assert_eq!(before.status, ResolveStatus::Unresolved);
    let at = text.find("A a").unwrap() as u32;
    p.edit("q/B.java", TextEdit { start_byte: at, end_byte: at + 1, insert: "B".into() }).unwrap();
    let r = query(&p);
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets[0].owner, "p.A");
    let mut fresh = Project::new();
    platform(&mut fresh);
    fresh.insert("p/A.java", A);
    fresh.insert("q/B.java", p.file("q/B.java").unwrap().text());
    assert_eq!(r, query(&fresh));
    assert_eq!(query(&old), before);
}
