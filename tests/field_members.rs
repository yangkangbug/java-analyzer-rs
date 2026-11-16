use java_engine::{Project, ResolveResult, ResolveStatus};
fn check(files: &[(&str, &str)], status: ResolveStatus, owners: &[&str]) {
    let mut p = Project::new();
    for (path, text) in files {
        p.insert(*path, text);
    }
    let (path, text) = files.iter().find(|(_, text)| text.contains("/*Q*/")).unwrap();
    let at = (text.find("/*Q*/").unwrap() + 5) as u32;
    let r: ResolveResult = p.resolve_at(path, at);
    assert_eq!(r, p.prepare().resolve_at(path, at));
    assert_eq!(r.status, status, "{r:?}");
    let mut actual: Vec<_> = r.targets.iter().map(|t| t.owner.as_str()).collect();
    actual.sort();
    let mut expected = owners.to_vec();
    expected.sort();
    assert_eq!(actual, expected, "{r:?}");
}
#[test]
fn inaccessible_declaration_hides_before_caller_access() {
    check(
        &[(
            "C.java",
            "class A {public int x;} class B extends A {private int x;} class C {int f(B b){return b./*Q*/x;}}",
        )],
        ResolveStatus::Unresolved,
        &[],
    );
}
#[test]
fn unrelated_fields_are_ambiguous_at_different_depths() {
    check(
        &[(
            "C.java",
            "interface I {int x=1;} interface J {int x=2;} class A implements I {} class C extends A implements J {int f(){return /*Q*/x;}}",
        )],
        ResolveStatus::Ambiguous,
        &["I", "J"],
    );
}
#[test]
fn hidden_field_can_arrive_along_another_direct_parent() {
    check(
        &[(
            "C.java",
            "interface I {int x=1;} interface J extends I {int x=2;} interface K extends I {} class C implements J,K {int f(){return /*Q*/x;}}",
        )],
        ResolveStatus::Ambiguous,
        &["I", "J"],
    );
}
#[test]
fn identical_diamond_declaration_is_one_field() {
    check(
        &[(
            "C.java",
            "interface I {int x=1;} interface J extends I {} interface K extends I {} class C implements J,K {int f(){return /*Q*/x;}}",
        )],
        ResolveStatus::Resolved,
        &["I"],
    );
}
#[test]
fn private_declaration_blocks_only_its_inheritance_path() {
    check(
        &[(
            "C.java",
            "interface I {int x=1;} class A {public int x;} class B extends A {private int x;} class C extends B implements I {int f(){return /*Q*/x;}}",
        )],
        ResolveStatus::Resolved,
        &["I"],
    );
}
#[test]
fn package_field_does_not_reappear_after_foreign_package_edge() {
    check(
        &[
            ("p/A.java", "package p; public class A {int x;}"),
            ("q/B.java", "package q; public class B extends p.A {}"),
            ("p/C.java", "package p; public class C extends q.B {int f(){return /*Q*/x;}}"),
        ],
        ResolveStatus::Unresolved,
        &[],
    );
}
#[test]
fn caller_access_filters_effective_members_before_ambiguity() {
    check(
        &[
            ("p/A.java", "package p; public class A {protected int x;}"),
            ("q/I.java", "package q; public interface I {int x=1;}"),
            ("q/B.java", "package q; public class B extends p.A implements I {}"),
            ("r/C.java", "package r; import q.B; public class C {int f(B b){return b./*Q*/x;}}"),
        ],
        ResolveStatus::Resolved,
        &["q.I"],
    );
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

fn query(p: &Project) -> ResolveResult {
    let t = p.file("Calls.java").unwrap().text();
    let at = (t.find("/*Q*/").unwrap() + 5) as u32;
    let r = p.resolve_at("Calls.java", at);
    assert_eq!(r, p.prepare().resolve_at("Calls.java", at));
    r
}
#[test]
fn binary_and_source_fields_share_hidden_declaration_rules() {
    for private in [false, true] {
        let text = "class Calls {int f(B b){return b./*Q*/x;}}";
        let mut p = Project::new();
        p.insert("Calls.java", text);
        p.add_class_bytes("A.class", &binary_fields("A", None, 0x21, &[("x", "I", 1, None)])).unwrap();
        p.add_class_bytes(
            "B.class",
            &binary_fields("B", Some("A"), 0x21, &[("x", "I", if private { 2 } else { 1 }, None)]),
        )
        .unwrap();
        let r = query(&p);
        assert_eq!(r.status, if private { ResolveStatus::Unresolved } else { ResolveStatus::Resolved });
        if !private {
            assert_eq!(r.targets.len(), 1);
            assert_eq!(r.targets[0].owner, "B");
            assert_eq!(r.targets[0].path, "B.class");
            assert_eq!(r.targets[0].kind, java_engine::TargetKind::Field);
        }
        let mut source = Project::new();
        source.insert(
            "Calls.java",
            &format!(
                "class A {{public int x;}} class B extends A {{{} int x;}} {text}",
                if private { "private" } else { "public" }
            ),
        );
        assert_eq!(query(&source).status, r.status);
    }
}
#[test]
fn unknown_access_and_duplicate_declarations_never_make_unique_target() {
    check(
        &[(
            "C.java",
            "interface I {public private int x=1;} interface J {int x=2;} class C implements I,J {int f(){return /*Q*/x;}}",
        )],
        ResolveStatus::NotDetermined,
        &["I", "J"],
    );
    check(
        &[(
            "C.java",
            "interface I {int x=1; int x=2;} interface J extends I {} interface K extends I {} class C implements J,K {int f(){return /*Q*/x;}}",
        )],
        ResolveStatus::Ambiguous,
        &["I", "I"],
    );
}
#[test]
fn cycles_and_ambiguous_parent_declarations_remain_errors() {
    check(
        &[("C.java", "class A extends B {int x;} class B extends A {} class C {int f(A a){return a./*Q*/x;}}")],
        ResolveStatus::Cycle,
        &[],
    );
    check(
        &[
            ("A.java", "class A {int x;}"),
            ("Other.java", "class A {int x;}"),
            ("C.java", "class C extends A {int f(){return /*Q*/x;}}"),
        ],
        ResolveStatus::Ambiguous,
        &["A", "A"],
    );
}
#[test]
fn edit_hidden_field_access_matches_fresh_and_keeps_old_snapshot() {
    use java_engine::TextEdit;
    let text = "class A {public int x;} class B extends A {private int x;} class Calls {int f(B b){return b./*Q*/x;}}";
    let mut p = Project::new();
    p.insert("Calls.java", text);
    let old = p.clone();
    let before = query(&old);
    assert_eq!(before.status, ResolveStatus::Unresolved);
    let at = text.find("private").unwrap() as u32;
    p.edit("Calls.java", TextEdit { start_byte: at, end_byte: at + 7, insert: "public".into() }).unwrap();
    let r = query(&p);
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].owner, "B");
    let mut fresh = Project::new();
    fresh.insert("Calls.java", p.file("Calls.java").unwrap().text());
    assert_eq!(r, query(&fresh));
    assert_eq!(query(&old), before);
}
