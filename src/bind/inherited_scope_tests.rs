use super::*;

fn binary(name: &str, parent: Option<&str>, methods: &[(&str, &str)], public: bool) -> Vec<u8> {
    fn u(b: &mut Vec<u8>, n: u16) {
        b.extend(n.to_be_bytes());
    }
    fn utf(b: &mut Vec<u8>, s: &str) {
        b.push(1);
        u(b, s.len() as u16);
        b.extend(s.as_bytes());
    }
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61];
    u(&mut b, (5 + 2 * methods.len()) as u16);
    utf(&mut b, name);
    b.push(7);
    u(&mut b, 1);
    utf(&mut b, parent.unwrap_or(name));
    b.push(7);
    u(&mut b, 3);
    for (name, descriptor) in methods {
        utf(&mut b, name);
        utf(&mut b, descriptor);
    }
    u(&mut b, if public { 0x21 } else { 0x20 });
    u(&mut b, 2);
    u(&mut b, if parent.is_some() { 4 } else { 0 });
    u(&mut b, 0);
    u(&mut b, 0);
    u(&mut b, methods.len() as u16);
    for i in 0..methods.len() {
        u(&mut b, 0x101);
        u(&mut b, 5 + 2 * i as u16);
        u(&mut b, 6 + 2 * i as u16);
        u(&mut b, 0);
    }
    u(&mut b, 0);
    b
}

#[test]
fn same_logical_binary_formal_in_two_scopes_is_not_proven_different() {
    let mut lib = Project::new();
    lib.add_class_bytes("Object.class", &binary("java/lang/Object", None, &[], true)).unwrap();
    lib.add_class_bytes("Token.class", &binary("dep/Token", Some("java/lang/Object"), &[], true)).unwrap();
    let mut app = lib.clone();
    lib.insert("Parent.java", "package api; public class Parent { public void pick(dep.Token t) {} }");
    app.insert("Child.java", "package app; public class Child extends api.Parent { public void pick(dep.Token t) {} }");
    app.insert("Use.java", "package app; class Use { void run(Child c, dep.Token t) { c.pick(t); } }");
    let visibility = [BTreeSet::from([0]), BTreeSet::from([0, 1])];
    let at = app.file("Use.java").unwrap().text().rfind("pick(").unwrap() as u32;
    let result = resolve_scoped(&[&lib, &app], &visibility, 1, "Use.java", at);
    assert_ne!(result.status, ResolveStatus::Resolved, "{result:?}");
    assert_eq!(result.targets.len(), 2);
    assert_eq!(result, PreparedScopedIndex::new(vec![&lib, &app], &visibility).resolve_at(1, "Use.java", at));
}
