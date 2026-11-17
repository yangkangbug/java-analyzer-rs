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
    let once = resolve_scoped(&[lib, app], &visibility, 1, "Use.java", at, None);
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
fn ancestor_bridge_enables_proved_cross_scope_binary_widening() {
    let (lib, mut app) = pair(false);
    app.add_class_bytes("child.jar!/lib/Child.class", &binary("lib/Child", Some("lib/Token"), &[], true)).unwrap();
    app.insert("Use.java", &CALLER.replace("lib.Token token", "lib.Child token"));
    selected(&lib, &app);
    app.add_class_bytes(
        "library.jar!/lib/Token.class",
        &binary("lib/Token", Some("java/lang/Object"), &[("changed", "()I")], true),
    )
    .unwrap();
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
                    Arc::make_mut(p.binaries.get_mut("library.jar!/lib/Token.class").unwrap())
                        .class_signature
                        .signature = Some("<T:Ljava/lang/Object;>Ljava/lang/Object;".into());
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
    let got = query(&lib, &app);
    assert_eq!(got.status, ResolveStatus::Resolved, "{got:?}");
    assert_eq!(got.targets.len(), 1);
    assert_eq!(got.targets[0].path, "Box.java");
    assert_eq!(got.targets[0].start_byte, lib.file("Box.java").unwrap().text().find("pick(int").unwrap() as u32);
    // The previously unknown ancestor now has a complete compatibility proof.
    // A genuinely changed closure must still block this entire competitor.
    app.add_class_bytes(
        "library.jar!/lib/Parent.class",
        &binary("lib/Parent", Some("java/lang/Object"), &[("changed", "()I")], true),
    )
    .unwrap();
    refused(&lib, &app);
}

#[test]
fn same_origin_raw_member_flag_change_is_not_binary_closure_identity() {
    let (mut lib, mut app) = pair(false);
    let bytes = binary("lib/Token", Some("java/lang/Object"), &[("marker", "()V")], true);
    lib.add_class_bytes("library.jar!/lib/Token.class", &bytes).unwrap();
    app.add_class_bytes("library.jar!/lib/Token.class", &bytes).unwrap();
    selected(&lib, &app);
    Arc::make_mut(app.binaries.get_mut("library.jar!/lib/Token.class").unwrap()).class_signature.methods[0]
        .raw_access_flags ^= 0x20;
    refused(&lib, &app);
}

fn conversion(lib: &Project, app: &Project, actual_name: &str, formal_name: &str) -> Option<bool> {
    let index = Index::scoped(&[lib, app], &[BTreeSet::from([0]), BTreeSet::from([0, 1])]);
    let file = index.files.iter().find(|f| f.scope == 1).unwrap();
    let actual = index.reference_named(1, actual_name).into_iter().find(|c| c.scope == 1).unwrap();
    let formal = index.reference_named(0, formal_name).into_iter().find(|c| c.scope == 0).unwrap();
    ReferenceCallProof::new(&index, file, None).converts(actual, formal)
}

#[test]
fn cross_scope_ancestor_conversion_uses_strict_path_and_binary_closure() {
    let (lib, mut app) = pair(false);
    assert_eq!(conversion(&lib, &app, "lib.Token", "java.lang.Object"), Some(true));
    app.insert("Child.java", "package app; class Child extends lib.Token {}");
    assert_eq!(conversion(&lib, &app, "app.Child", "lib.Token"), Some(true));
    app.add_class_bytes(
        "library.jar!/lib/Token.class",
        &binary("lib/Token", Some("java/lang/Object"), &[("changed", "()I")], true),
    )
    .unwrap();
    assert_eq!(conversion(&lib, &app, "app.Child", "lib.Token"), None);
}

#[test]
fn cross_scope_unknown_branch_does_not_hide_independent_strict_path() {
    let (lib, mut app) = pair(false);
    app.insert("Child.java", "package app; class G<T> {} interface I {} class Child extends G<Child> implements I {}");
    assert_eq!(conversion(&lib, &app, "app.Child", "java.lang.Object"), Some(true));
    app.insert("Child.java", "package app; class G<T> {} class Child extends G<Child> {}");
    assert_eq!(conversion(&lib, &app, "app.Child", "java.lang.Object"), None);
}

#[test]
fn exact_nested_root_compatibility_is_preserved_but_new_nested_ancestor_is_not_added() {
    let (mut lib, mut app) = pair(false);
    for p in [&mut lib, &mut app] {
        p.add_class_bytes(
            "library.jar!/lib/Outer$Inner.class",
            &binary("lib/Outer$Inner", Some("java/lang/Object"), &[], true),
        )
        .unwrap();
    }
    assert_eq!(conversion(&lib, &app, "lib.Outer.Inner", "lib.Outer.Inner"), Some(true));
    app.add_class_bytes("Child.class", &binary("lib/Child", Some("lib/Outer$Inner"), &[], true)).unwrap();
    assert_eq!(conversion(&lib, &app, "lib.Child", "lib.Outer.Inner"), None);
}

#[test]
fn source_generic_and_raw_name_collisions_are_collected_before_bridge_eligibility() {
    let (mut lib, mut app) = pair(false);
    // Two complete branches with the same raw JVM name, but different dotted
    // projections. A valid binary candidate cannot hide the source competitor.
    for p in [&mut lib, &mut app] {
        p.add_class_bytes("Nested.class", &binary("p/A$B", Some("java/lang/Object"), &[], true)).unwrap();
    }
    app.add_class_bytes("library.jar!/lib/Token.class", &binary("lib/Token", Some("p/A$B"), &[], true)).unwrap();
    app.insert("Shadow.java", "package p; public interface A$B {}");
    app.insert("Child.java", "package app; class Child extends lib.Token implements p.A$B {}");
    assert_eq!(conversion(&lib, &app, "app.Child", "p.A.B"), None);
    // A generic declaration with the requested name is unknown, not absence.
    let (lib, mut app) = pair(false);
    Arc::make_mut(app.binaries.get_mut("library.jar!/lib/Token.class").unwrap()).class_signature.signature =
        Some("<T:Ljava/lang/Object;>Ljava/lang/Object;".into());
    assert_eq!(conversion(&lib, &app, "lib.Token", "lib.Token"), None);
    let (mut lib, app) = pair(false);
    lib.remove_class("library.jar!/lib/Token.class");
    lib.insert("Token.java", "package lib; public class Token {}");
    assert_eq!(conversion(&lib, &app, "lib.Token", "lib.Token"), None);
}

#[test]
fn bridge_budget_is_shared_even_with_warm_closure_cache() {
    let (lib, app) = pair(false);
    let index = Index::scoped(&[&lib, &app], &[BTreeSet::from([0]), BTreeSet::from([0, 1])]);
    let file = index.files.iter().find(|f| f.scope == 1).unwrap();
    let actual = index.binaries.iter().find(|c| c.scope == 1 && c.dotted() == "lib.Token").unwrap();
    let formal = index.binaries.iter().find(|c| c.scope == 0 && c.dotted() == "java.lang.Object").unwrap();
    let mut proof = ReferenceCallProof::new(&index, file, None);
    proof.graph(actual).unwrap();
    proof.graph(formal).unwrap();
    assert_eq!(proof.converts(actual, formal), Some(true));
    let before = proof.work_left;
    assert_eq!(proof.converts(actual, formal), Some(true));
    let used = before - proof.work_left;
    proof.work_left = used - 1;
    assert_eq!(proof.converts(actual, formal), None);
    assert!(proof.exhausted);
}

#[test]
fn compatible_bound_preserves_actual_slot_and_return_declaration_identity() {
    let (mut lib, mut app) = pair(false);
    lib.insert("Box.java", "package api; public class Box<T> { public T pick(T value){return value;} }");
    app.insert("Value.java", "package app; public class Value {public void hit(){}}");
    let source = "package app; class Use { void run(api.Box<Value> box, Value value) { box.pick(value).hit(); } }";
    app.insert("Use.java", source);
    let visibility = [BTreeSet::from([0]), BTreeSet::from([0, 1])];
    let index = Index::scoped(&[&lib, &app], &visibility);
    let file = index.files.iter().find(|f| f.path == "Use.java").unwrap();
    let at = source.find("box.pick").unwrap() as u32;
    let caller = direct_caller(app.file("Use.java").unwrap().root(), at).unwrap();
    let mut proof = ReferenceCallProof::new(&index, file, Some(caller));
    let declaration = ReferenceDeclaration::Applied {
        name: vec!["api".into(), "Box".into()],
        arguments: vec![Some(vec!["Value".into()])],
    };
    let receiver = proof.declared_receiver(&declaration, at).unwrap();
    let value = receiver.instantiation.unwrap().arguments[0].unwrap();
    assert_eq!(value.scope, 1);
    assert_eq!(value.path, "Value.java");
    let hit = source.find("hit()").unwrap() as u32;
    let prepared = PreparedScopedIndex::new(vec![&lib, &app], &visibility);
    let result = prepared.resolve_at(1, "Use.java", hit);
    assert_eq!(result, resolve_scoped(&[&lib, &app], &visibility, 1, "Use.java", hit, None));
    assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
    assert_eq!(result.targets[0].path, "Value.java");
    assert_eq!(result.targets[0].start_byte, app.file("Value.java").unwrap().text().find("hit()").unwrap() as u32);
}

#[test]
fn bridge_edit_fresh_and_retained_snapshot_keep_their_own_evidence() {
    let (lib, mut app) = pair(false);
    let text = "package app; class Child extends lib.Token {}";
    app.insert("Child.java", text);
    app.insert("Use.java", &CALLER.replace("lib.Token token", "Child token"));
    let old = app.clone();
    selected(&lib, &app);
    let at = text.find("lib.Token").unwrap() as u32;
    app.edit("Child.java", crate::TextEdit { start_byte: at, end_byte: at + 9, insert: "Missing".into() }).unwrap();
    let mut fresh = old.clone();
    fresh.insert("Child.java", &text.replace("lib.Token", "Missing"));
    assert_eq!(query(&lib, &app), query(&lib, &fresh));
    refused(&lib, &app);
    selected(&lib, &old);
    app.edit("Child.java", crate::TextEdit { start_byte: at, end_byte: at + 7, insert: "lib.Token".into() }).unwrap();
    assert_eq!(query(&lib, &app), query(&lib, &old));
}

#[test]
fn incompatible_first_slot_cannot_hide_a_genuinely_unknown_generic_ancestor() {
    let (mut lib, mut app) = pair(false);
    for p in [&mut lib, &mut app] {
        p.add_class_bytes("Parent.class", &binary("lib/Parent", Some("java/lang/Object"), &[], true)).unwrap();
        Arc::make_mut(p.binaries.get_mut("Parent.class").unwrap()).class_signature.signature =
            Some("<T:Ljava/lang/Object;>Ljava/lang/Object;".into());
        p.add_class_bytes("library.jar!/lib/Token.class", &binary("lib/Token", Some("lib/Parent"), &[], true)).unwrap();
        Arc::make_mut(p.binaries.get_mut("library.jar!/lib/Token.class").unwrap()).class_signature.signature =
            Some("Llib/Parent<Ljava/lang/Object;>;".into());
    }
    // Exact Token compatibility still succeeds; only the other vector slot's
    // parameterized ancestor remains unproved.
    selected(&lib, &app);
    lib.insert("Box.java","package api; public class Box { public void pick(int i, lib.Token t) {} public void pick(boolean b, lib.Parent p) {} }");
    app.insert(
        "Use.java",
        "package app; class Use { void run(api.Box box, lib.Token token, byte b) { box.pick(b,token); } }",
    );
    refused(&lib, &app);
}

#[test]
fn substituted_specificity_uses_the_same_cross_scope_conversion_certificate() {
    let (mut lib, mut app) = pair(false);
    for p in [&mut lib, &mut app] {
        p.add_class_bytes("Parent.class", &binary("lib/Parent", Some("java/lang/Object"), &[], true)).unwrap();
        p.add_class_bytes("library.jar!/lib/Token.class", &binary("lib/Token", Some("lib/Parent"), &[], true)).unwrap();
    }
    let callee =
        "package api; public class Box<T> { public void pick(T value) {} public void pick(lib.Parent value) {} }";
    lib.insert("Box.java", callee);
    app.insert("Use.java", &CALLER.replace("api.Box box", "api.Box<lib.Token> box"));
    let result = query(&lib, &app);
    assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
    assert_eq!(result.targets.len(), 1);
    assert_eq!(result.targets[0].path, "Box.java");
    assert_eq!(result.targets[0].start_byte, callee.find("pick(T").unwrap() as u32);
}

#[test]
fn scheduled_conversion_keeps_unknown_targets_and_closed_negative_evidence() {
    let (lib, mut app) = pair(false);
    // Same-scope complete unknown generic path: exact identity is present,
    // but must never be replaced by a binary compatibility bridge.
    app.insert(
        "Model.java",
        "package app; class Parent {} class G<T> extends Parent {} class Child2 extends G<Child2> {} class Other {}",
    );
    let index = Index::scoped(&[&lib, &app], &[BTreeSet::from([0]), BTreeSet::from([0, 1])]);
    let file = index.files.iter().find(|f| f.scope == 1 && f.path == "Model.java").unwrap();
    let actual = file.classes.iter().find(|c| c.simple() == "Child2").unwrap();
    for (name, expected) in [("Parent", None), ("Other", Some(false))] {
        let formal = file.classes.iter().find(|c| c.simple() == name).unwrap();
        let mut strict = ReferenceCallProof::new(&index, file, None);
        let mut scheduled = ReferenceCallProof::new(&index, file, None);
        assert_eq!(strict.strict_converts(actual, formal), expected);
        assert_eq!(scheduled.converts(actual, formal), expected);
    }
    let formal = file.classes.iter().find(|c| c.simple() == "Other").unwrap();
    let mut proof = ReferenceCallProof::new(&index, file, None);
    proof.graph(actual).unwrap();
    proof.graph(formal).unwrap();
    proof.work_left = 2;
    assert_eq!(proof.converts(actual, formal), None);
    assert!(proof.exhausted);
}
