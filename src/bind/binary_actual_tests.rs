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

const CALLER: &str = "package app; class Use { void run(api.Box box, lib.Token token) { box.pick(token); } }";
const CALLEE: &str = "package api; public class Box { public void pick(lib.Token value) {} }";
fn pair(shared: bool) -> (Project, Project) {
    let mut lib = Project::new();
    lib.add_class_bytes("platform!/java/lang/Object.class", &binary("java/lang/Object", None, &[], true)).unwrap();
    lib.add_class_bytes("library.jar!/lib/Token.class", &binary("lib/Token", Some("java/lang/Object"), &[], true))
        .unwrap();
    let mut app = if shared {
        lib.clone()
    } else {
        let mut app = Project::new();
        app.add_class_bytes("platform!/java/lang/Object.class", &binary("java/lang/Object", None, &[], true)).unwrap();
        app.add_class_bytes("library.jar!/lib/Token.class", &binary("lib/Token", Some("java/lang/Object"), &[], true))
            .unwrap();
        app
    };
    lib.insert("Box.java", CALLEE);
    app.insert("Use.java", CALLER);
    (lib, app)
}
fn query(lib: &Project, app: &Project) -> ResolveResult {
    let visibility = [BTreeSet::from([0]), BTreeSet::from([0, 1])];
    let parsed = app.file("Use.java").unwrap();
    let at = parsed.text().rfind("pick(").unwrap() as u32;
    let once = resolve_scoped(&[lib, app], &visibility, 1, "Use.java", at);
    let prepared = PreparedScopedIndex::new(vec![lib, app], &visibility);
    assert_eq!(once, prepared.resolve_at(1, "Use.java", at));
    once
}
fn selected(lib: &Project, app: &Project) {
    let got = query(lib, app);
    assert_eq!(got.status, ResolveStatus::Resolved, "{got:?}");
    assert_eq!(got.targets.len(), 1);
    assert_eq!(got.targets[0].path, "Box.java");
    assert_eq!(got.targets[0].start_byte, CALLEE.find("pick(").unwrap() as u32);
}
fn refused(lib: &Project, app: &Project) {
    let got = query(lib, app);
    assert_ne!(got.status, ResolveStatus::Resolved, "{got:?}");
}

#[test]
fn exact_non_string_binary_root_accepts_shared_and_independently_parsed_evidence() {
    for shared in [false, true] {
        let (lib, app) = pair(shared);
        selected(&lib, &app);
    }
}
#[test]
fn equal_signature_at_another_root_or_mr_entry_origin_is_not_compatibility() {
    for origin in ["copy.jar!/lib/Token.class", "library.jar!/META-INF/versions/17/lib/Token.class"] {
        let (lib, mut app) = pair(true);
        app.remove_class("library.jar!/lib/Token.class");
        app.add_class_bytes(origin, &binary("lib/Token", Some("java/lang/Object"), &[], true)).unwrap();
        refused(&lib, &app);
    }
}
#[test]
fn shared_root_cannot_hide_different_ancestor_origin_or_signature() {
    for different_origin in [false, true] {
        let (lib, mut app) = pair(true);
        app.remove_class("platform!/java/lang/Object.class");
        let origin =
            if different_origin { "other!/java/lang/Object.class" } else { "platform!/java/lang/Object.class" };
        let methods = if different_origin { vec![] } else { vec![("changed", "()I")] };
        app.add_class_bytes(origin, &binary("java/lang/Object", None, &methods, true)).unwrap();
        refused(&lib, &app);
    }
}
#[test]
fn same_origin_with_changed_root_signature_is_not_identity() {
    let (lib, mut app) = pair(false);
    app.add_class_bytes(
        "library.jar!/lib/Token.class",
        &binary("lib/Token", Some("java/lang/Object"), &[("changed", "()I")], true),
    )
    .unwrap();
    refused(&lib, &app);
}
#[test]
fn missing_duplicate_or_source_shadowed_closure_refuses() {
    for mode in 0..4 {
        let (lib, mut app) = pair(true);
        match mode {
            0 => {
                app.remove_class("platform!/java/lang/Object.class");
            }
            1 => {
                app.insert("Shadow.java", "package java.lang; public class Object {}");
            }
            2 => {
                app.insert("Shadow.java", "package lib; public class Token {}");
            }
            _ => {
                app.add_class_bytes(
                    "duplicate!/lib/Token.class",
                    &binary("lib/Token", Some("java/lang/Object"), &[], true),
                )
                .unwrap();
            }
        }
        refused(&lib, &app);
    }
}
#[test]
fn exact_bridge_does_not_enable_cross_scope_binary_widening() {
    let (lib, mut app) = pair(false);
    app.add_class_bytes("child.jar!/lib/Child.class", &binary("lib/Child", Some("lib/Token"), &[], true)).unwrap();
    app.insert("Use.java", &CALLER.replace("lib.Token token", "lib.Child token"));
    refused(&lib, &app);
}

#[test]
fn independent_signature_comparison_consumes_shared_work_budget() {
    let (mut lib, mut app) = pair(false);
    let long_name = "x".repeat(5000);
    let bytes = binary("lib/Token", Some("java/lang/Object"), &[(&long_name, "()I")], true);
    for p in [&mut lib, &mut app] {
        p.add_class_bytes("library.jar!/lib/Token.class", &bytes).unwrap();
    }
    refused(&lib, &app);
}
#[test]
fn generic_arrays_cycles_and_unknown_competitors_remain_refused() {
    for mode in 0..4 {
        let (mut lib, mut app) = pair(false);
        match mode {
            0 => {
                for p in [&mut lib, &mut app] {
                    Arc::make_mut(p.binaries.get_mut("library.jar!/lib/Token.class").unwrap()).signature =
                        Some("<T:Ljava/lang/Object;>Ljava/lang/Object;".into());
                }
            }
            1 => {
                lib.insert("Box.java", &CALLEE.replace("lib.Token value", "lib.Token[] value"));
                app.insert("Use.java", &CALLER.replace("lib.Token token", "lib.Token[] token"));
            }
            2 => {
                app.add_class_bytes("library.jar!/lib/Token.class", &binary("lib/Token", Some("lib/Token"), &[], true))
                    .unwrap();
            }
            _ => {
                lib.insert("Box.java", "package api; public class Box { public void pick(lib.Token value) {} public void pick(Missing x, int y) {} }");
            }
        }
        refused(&lib, &app);
    }
}
#[test]
fn modeled_member_signatures_are_compared_not_resolved_as_dependency_closure() {
    let (mut lib, mut app) = pair(false);
    let root = binary("lib/Token", Some("java/lang/Object"), &[("member", "()Llib/Dependency;")], true);
    for p in [&mut lib, &mut app] {
        p.add_class_bytes("library.jar!/lib/Token.class", &root).unwrap();
    }
    lib.add_class_bytes(
        "lib-choice!/lib/Dependency.class",
        &binary("lib/Dependency", Some("java/lang/Object"), &[], true),
    )
    .unwrap();
    app.add_class_bytes(
        "app-choice!/lib/Dependency.class",
        &binary("lib/Dependency", Some("java/lang/Object"), &[("extra", "()I")], true),
    )
    .unwrap();
    // Only direct Token argument compatibility is proved. This does not assert
    // that a later member() result has the same identity in both compilations.
    selected(&lib, &app);
}

#[test]
fn unsupported_cross_scope_widening_competitor_cannot_create_a_unique_winner() {
    let (mut lib, mut app) = pair(false);
    for p in [&mut lib, &mut app] {
        p.add_class_bytes("library.jar!/lib/Parent.class", &binary("lib/Parent", Some("java/lang/Object"), &[], true))
            .unwrap();
        p.add_class_bytes("library.jar!/lib/Token.class", &binary("lib/Token", Some("lib/Parent"), &[], true)).unwrap();
    }
    lib.insert("Box.java","package api; public class Box { public void pick(lib.Token t, int i) {} public void pick(lib.Parent p, byte b) {} }");
    app.insert(
        "Use.java",
        "package app; class Use { void run(api.Box box, lib.Token token, byte b) { box.pick(token,b); } }",
    );
    refused(&lib, &app);
}

#[test]
fn string_object_cross_specificity_is_not_resolved_by_discarding_widening() {
    let (mut lib, mut app) = pair(false);
    for p in [&mut lib, &mut app] {
        p.add_class_bytes("platform!/java/lang/String.class", &{
            let mut bytes = binary("java/lang/String", Some("java/lang/Object"), &[], true);
            let flags = bytes.len() - 13;
            bytes[flags] |= 0x10;
            bytes
        })
        .unwrap();
    }
    lib.insert(
        "Box.java",
        "package api; public class Box { public void pick(String t, int i) {} public void pick(Object p, byte b) {} }",
    );
    app.insert(
        "Use.java",
        "package app; class Use { void run(api.Box box, String value, byte b) { box.pick(value,b); } }",
    );
    refused(&lib, &app);
    let literal = app.file("Use.java").unwrap().text().replace("pick(value,b)", "pick(\"x\",b)");
    app.insert("Use.java", &literal);
    refused(&lib, &app);
}

#[test]
fn complete_unrelated_binary_formal_can_still_be_proved_inapplicable() {
    let (mut lib, mut app) = pair(false);
    for p in [&mut lib, &mut app] {
        p.add_class_bytes("library.jar!/lib/Other.class", &binary("lib/Other", Some("java/lang/Object"), &[], true))
            .unwrap();
    }
    lib.insert(
        "Box.java",
        "package api; public class Box { public void pick(lib.Token value) {} public void pick(lib.Other other) {} }",
    );
    selected(&lib, &app);
}

#[test]
fn known_incompatible_slot_does_not_skip_unknown_cross_scope_ancestor() {
    let (mut lib, mut app) = pair(false);
    for p in [&mut lib, &mut app] {
        p.add_class_bytes("library.jar!/lib/Parent.class", &binary("lib/Parent", Some("java/lang/Object"), &[], true))
            .unwrap();
        p.add_class_bytes("library.jar!/lib/Token.class", &binary("lib/Token", Some("lib/Parent"), &[], true)).unwrap();
    }
    lib.insert("Box.java","package api; public class Box { public void pick(int i, lib.Token t) {} public void pick(boolean b, lib.Parent p) {} }");
    app.insert(
        "Use.java",
        "package app; class Use { void run(api.Box box, lib.Token token, byte b) { box.pick(b,token); } }",
    );
    refused(&lib, &app);
}
