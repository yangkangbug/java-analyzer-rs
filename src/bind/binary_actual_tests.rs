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
        if mode == 1 {
            // Plain arrays now retain the same exact cross-scope component proof.
            selected(&lib, &app);
            let mut different = app.clone();
            different
                .add_class_bytes(
                    "library.jar!/lib/Token.class",
                    &binary("lib/Token", Some("java/lang/Object"), &[("extra", "()I")], true),
                )
                .unwrap();
            refused(&lib, &different);
            app.insert("Token.java", "package lib; public class Token {}");
            refused(&lib, &app);
        } else {
            refused(&lib, &app);
        }
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

fn wrapper_binary(name: &str, parent: Option<&str>, methods: &[(&str, &str)], public: bool) -> Vec<u8> {
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
    u(&mut b, if public { 0x31 } else { 0x30 });
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
fn loose_invocation_initial_boxing_and_unboxing() {
    for (methods, call, expected) in [
        ("void pick(java.lang.Integer x) {}", "pick(1)", "pick(java.lang.Integer"),
        ("void pick(long x) {} void pick(java.lang.Integer x) {}", "pick(1)", "pick(long"),
        ("void pick(long x) {} void pick(double x) {}", "pick(value)", "pick(long"),
        ("void pick(java.lang.Object x) {} void pick(int x) {}", "pick(value)", "pick(java.lang.Object"),
    ] {
        let mut p = Project::new();
        p.add_class_bytes("platform!/java/lang/Object.class", &binary("java/lang/Object", None, &[], true)).unwrap();
        p.add_class_bytes(
            "platform!/java/lang/Integer.class",
            &wrapper_binary("java/lang/Integer", Some("java/lang/Object"), &[], true),
        )
        .unwrap();
        let text = format!("class C {{ {methods} void test(java.lang.Integer value) {{ {call}; }} }}");
        p.insert("C.java", &text);
        let got = p.resolve_at("C.java", text.rfind(call).unwrap() as u32);
        assert_eq!(got.status, ResolveStatus::Resolved, "{text}: {got:?}");
        assert_eq!(got.targets[0].start_byte, text.find(expected).unwrap() as u32, "{text}");
    }
}

fn loose_platform() -> Project {
    let mut p = Project::new();
    p.add_class_bytes("platform!/Object", &binary("java/lang/Object", None, &[], true)).unwrap();
    p.add_class_bytes("platform!/Number", &binary("java/lang/Number", Some("java/lang/Object"), &[], true)).unwrap();
    for name in ["Boolean", "Byte", "Character", "Short", "Integer", "Long", "Float", "Double"] {
        let parent = if matches!(name, "Boolean" | "Character") { "java/lang/Object" } else { "java/lang/Number" };
        p.add_class_bytes(
            format!("platform!/{name}"),
            &wrapper_binary(&format!("java/lang/{name}"), Some(parent), &[], true),
        )
        .unwrap();
    }
    p
}
#[test]
fn loose_invocation_complete_phase_matrix() {
    for (methods, parameters, call, expected) in [
        (
            "void pick(java.lang.Number n){} void pick(java.lang.Object o){}",
            "",
            "pick(1)",
            Some("pick(java.lang.Number"),
        ),
        ("void pick(long n){} void pick(java.lang.Integer o){}", "", "pick(1)", Some("pick(long")),
        ("void pick(long n){} void pick(double o){}", "java.lang.Integer v", "pick(v)", Some("pick(long")),
        (
            "void pick(java.lang.Object n){} void pick(int o){}",
            "java.lang.Integer v",
            "pick(v)",
            Some("pick(java.lang.Object"),
        ),
        ("void pick(java.lang.Long n){} void pick(java.lang.Boolean o){}", "", "pick(1)", None),
        ("void pick(int n){} void pick(boolean o){}", "java.lang.Number v", "pick(v)", None),
        ("void pick(int n){} void pick(long o){}", "", "pick(true)", None),
        ("void pick(int n){} void pick(long o){}", "", "pick(null)", None),
        ("void pick(java.lang.Byte n){} void pick(java.lang.Boolean o){}", "", "pick(1)", None),
        ("void pick(int n,java.lang.Integer o){} void pick(java.lang.Integer n,int o){}", "", "pick(1,1)", None),
        ("void pick(java.lang.Integer n){} void pick(Missing o){}", "", "pick(1)", None),
        ("void pick(java.lang.Integer n,int o){} void pick(boolean n,Missing o){}", "", "pick(1,1)", None),
        ("void pick(java.lang.Integer n){} void pick(int... o){}", "", "pick(1)", Some("pick(java.lang.Integer")),
        ("void pick(java.lang.Integer n){} void pick(boolean o){}", "int[] v", "pick(v)", None),
        ("void pick(long n){} static void pick(java.lang.Integer o){}", "", "C.pick(1)", None),
    ] {
        let mut p = loose_platform();
        let text = format!("class C {{ {methods} void test({parameters}) {{ {call}; }} }}");
        p.insert("C.java", &text);
        let at = text.rfind(call).unwrap() + call.find("pick").unwrap();
        let got = p.resolve_at("C.java", at as u32);
        if let Some(expected) = expected {
            assert_eq!(got.status, ResolveStatus::Resolved, "{text}: {got:?}");
            assert_eq!(got.targets[0].start_byte, text.find(expected).unwrap() as u32, "{text}");
        } else {
            assert_ne!(got.status, ResolveStatus::Resolved, "{text}: {got:?}");
        }
    }
}
#[test]
fn loose_wrapper_all_pairs_and_budget_identity() {
    let mut p = loose_platform();
    p.insert("Use.java", "class Use {}");
    let index = Index::scoped(&[&p], &[BTreeSet::from([0])]);
    let file = &index.files[0];
    for (primitive, name) in [
        (Primitive::Boolean, "Boolean"),
        (Primitive::Byte, "Byte"),
        (Primitive::Char, "Character"),
        (Primitive::Short, "Short"),
        (Primitive::Int, "Integer"),
        (Primitive::Long, "Long"),
        (Primitive::Float, "Float"),
        (Primitive::Double, "Double"),
    ] {
        let class = index.binaries.iter().find(|c| c.simple() == name).unwrap();
        let actual = StrictActual::Primitive(PrimitiveValue { primitive, rank: 0 });
        let formal = ReferenceFormal::Reference(class);
        let mut proof = ReferenceCallProof::new(&index, file, None);
        assert_eq!(proof.invocation_conversion(&actual, &formal, InvocationPhase::Strict), Some(false));
        let mut proof = ReferenceCallProof::new(&index, file, None);
        let before = proof.work_left;
        assert_eq!(proof.invocation_conversion(&actual, &formal, InvocationPhase::Loose), Some(true));
        let used = before - proof.work_left;
        for budget in [0, used - 1, used] {
            let mut proof = ReferenceCallProof::new(&index, file, None);
            proof.work_left = budget;
            assert_eq!(
                proof.invocation_conversion(&actual, &formal, InvocationPhase::Loose),
                if budget == used { Some(true) } else { None }
            );
            if budget < used {
                assert!(proof.exhausted);
                assert_eq!(proof.invocation_conversion(&actual, &formal, InvocationPhase::Strict), None);
            }
        }
        let mut proof = ReferenceCallProof::new(&index, file, None);
        assert_eq!(
            proof.invocation_conversion(
                &StrictActual::Reference(ReferenceValue { class }),
                &ReferenceFormal::Primitive(primitive),
                InvocationPhase::Loose
            ),
            Some(true)
        );
    }
}
#[test]
fn loose_wrapper_unknown_inventory_never_certifies_by_spelling() {
    for variant in 0..5 {
        let mut p = loose_platform();
        p.insert("Use.java", "class Use {}");
        match variant {
            0 => {
                p.remove_class("platform!/Integer");
            }
            1 => {
                p.add_class_bytes(
                    "other!/Integer",
                    &wrapper_binary("java/lang/Integer", Some("java/lang/Number"), &[], true),
                )
                .unwrap();
            }
            2 => {
                p.remove_class("platform!/Integer");
                p.insert("Integer.java", "package java.lang; public final class Integer {}");
            }
            3 => {
                p.remove_class("platform!/Number");
            }
            _ => {
                Arc::make_mut(p.binaries.get_mut("platform!/Integer").unwrap()).class_signature.signature =
                    Some("<T:Ljava/lang/Object;>Ljava/lang/Number;".into());
            }
        }
        let index = Index::scoped(&[&p], &[BTreeSet::from([0])]);
        let file = index.files.iter().find(|f| f.path == "Use.java").unwrap();
        let formal = index.binaries.iter().find(|c| c.simple() == "Object").unwrap();
        let mut proof = ReferenceCallProof::new(&index, file, None);
        assert_eq!(
            proof.invocation_conversion(
                &StrictActual::Primitive(PrimitiveValue { primitive: Primitive::Int, rank: 0 }),
                &ReferenceFormal::Reference(formal),
                InvocationPhase::Loose
            ),
            None,
            "variant {variant}"
        );
    }
}

#[test]
fn loose_phase_never_retries_nonempty_strict_ambiguity() {
    let mut p = loose_platform();
    for name in ["A", "B"] {
        p.add_class_bytes(format!("{name}.class"), &binary(name, Some("java/lang/Object"), &[], true)).unwrap();
        Arc::make_mut(p.binaries.get_mut(&format!("{name}.class")).unwrap()).class_signature.is_interface = true;
    }
    Arc::make_mut(p.binaries.get_mut("platform!/Integer").unwrap()).class_signature.interfaces =
        vec!["A".into(), "B".into()];
    Arc::make_mut(p.binaries.get_mut("platform!/Integer").unwrap()).class_signature.binary_interfaces =
        vec!["A".into(), "B".into()];
    let text = "class C { void pick(A a){} void pick(B b){} void pick(int i){} void test(java.lang.Integer value){pick(value);} }";
    p.insert("C.java", text);
    let index = Index::scoped(&[&p], &[BTreeSet::from([0])]);
    let file = &index.files[0];
    let actual = index.binaries.iter().find(|c| c.simple() == "Integer").unwrap();
    for name in ["A", "B"] {
        let formal = index.binaries.iter().find(|c| c.simple() == name).unwrap();
        let mut proof = ReferenceCallProof::new(&index, file, None);
        assert_eq!(
            proof.invocation_conversion(
                &StrictActual::Reference(ReferenceValue { class: actual }),
                &ReferenceFormal::Reference(formal),
                InvocationPhase::Strict
            ),
            Some(true)
        );
    }
    let mut proof = ReferenceCallProof::new(&index, file, None);
    assert_eq!(
        proof.invocation_conversion(
            &StrictActual::Reference(ReferenceValue { class: actual }),
            &ReferenceFormal::Primitive(Primitive::Int),
            InvocationPhase::Loose
        ),
        Some(true)
    );
    let got = p.resolve_at("C.java", text.rfind("pick(").unwrap() as u32);
    assert_ne!(got.status, ResolveStatus::Resolved, "{got:?}");
}

#[test]
fn loose_unboxing_links_in_actual_scope_not_caller_scope() {
    let mut lib = loose_platform();
    let mut app = loose_platform();
    lib.insert("Lib.java", "class Lib {}");
    app.insert("Use.java", "class Use {}");
    // An unrelated caller-scope replacement cannot stand in for a wrapper
    // selected in the actual value's linking context.
    app.remove_class("platform!/Integer");
    app.insert("Integer.java", "package java.lang; public final class Integer {}");
    let visibility = [BTreeSet::from([0]), BTreeSet::from([1])];
    let index = Index::scoped(&[&lib, &app], &visibility);
    let file = index.files.iter().find(|f| f.path == "Use.java").unwrap();
    let actual = index.binaries.iter().find(|c| c.scope == 0 && c.simple() == "Integer").unwrap();
    let mut proof = ReferenceCallProof::new(&index, file, None);
    assert_eq!(
        proof.invocation_conversion(
            &StrictActual::Reference(ReferenceValue { class: actual }),
            &ReferenceFormal::Primitive(Primitive::Long),
            InvocationPhase::Loose
        ),
        Some(true)
    );
    let source = index.files.iter().find(|f| f.path == "Integer.java").unwrap().classes.first().unwrap();
    let mut proof = ReferenceCallProof::new(&index, file, None);
    assert_eq!(
        proof.invocation_conversion(
            &StrictActual::Reference(ReferenceValue { class: source }),
            &ReferenceFormal::Primitive(Primitive::Long),
            InvocationPhase::Loose
        ),
        None
    );
}

#[test]
fn loose_phase_edit_fresh_and_retained_snapshot() {
    let mut p = loose_platform();
    let text = "class C { void pick(java.lang.Integer a){} void pick(java.lang.Boolean b){} void test(){pick(1);} }";
    p.insert("C.java", text);
    let old = p.clone();
    let position = text.rfind("pick(").unwrap() as u32;
    let before = p.resolve_at("C.java", position);
    assert_eq!(before.status, ResolveStatus::Resolved);
    let at = text.rfind("1").unwrap() as u32;
    p.edit("C.java", crate::TextEdit { start_byte: at, end_byte: at + 1, insert: "true".into() }).unwrap();
    let mut fresh = loose_platform();
    fresh.insert("C.java", &text.replace("pick(1)", "pick(true)"));
    assert_eq!(p.resolve_at("C.java", position), fresh.resolve_at("C.java", position));
    assert_eq!(
        p.resolve_at("C.java", position).targets[0].start_byte,
        text.find("pick(java.lang.Boolean").unwrap() as u32
    );
    assert_eq!(old.resolve_at("C.java", position), before);
    p.edit("C.java", crate::TextEdit { start_byte: at, end_byte: at + 4, insert: "1".into() }).unwrap();
    assert_eq!(p.resolve_at("C.java", position), before);
    let visibility = [BTreeSet::from([0])];
    let prepared = PreparedScopedIndex::new(vec![&p], &visibility);
    assert_eq!(prepared.resolve_at(0, "C.java", position), before);
}

#[test]
fn array_length_shared_primitive_and_reference_domains() {
    for (ty, call) in [
        ("int[]", "pick(array.length)"),
        ("int[][]", "pick((array).length)"),
        ("int[]", "C.pick(array.length)"),
        ("java.lang.Object[]", "C.pick(array.length)"),
    ] {
        let mut p = loose_platform();
        let text = format!(
            "class C {{ public static void pick(int x){{}} public static void pick(long x){{}} static void run({ty} array){{{call};}} }}"
        );
        p.insert("C.java", &text);
        let at = text.rfind(call).unwrap() + call.find("pick").unwrap();
        let result = p.resolve_at("C.java", at as u32);
        assert_eq!(result.status, ResolveStatus::Resolved, "{text}: {result:?}");
        assert_eq!(result.targets[0].start_byte, text.find("pick(int").unwrap() as u32);
    }
}

#[test]
fn array_length_shape_components_and_phase_boundaries() {
    for (prefix, params, body, call, expected) in [
        ("", "int[] array", "", "C.pick(((array)).length)", Some("pick(int")),
        ("", "", "int array[][] = null;", "C.pick(array.length)", Some("pick(int")),
        ("", "", "java.lang.Object array[] = null;", "C.pick(array.length)", Some("pick(int")),
        ("", "int[] array", "", "C.pick(array.length + 1)", Some("pick(int")),
        ("", "int[] array", "", "C.pick(array.length - 1)", Some("pick(int")),
        ("", "int[] array", "", "C.pick(array..length)", None),
        ("", "int[] array", "", "C.pick(array.)", None),
        ("", "int[] array", "", "C.pick(array.length.extra)", None),
        ("", "int[][] array", "", "C.pick(array[0].length)", None),
        ("", "Missing[] array", "", "C.pick(array.length)", None),
        ("", "Scalar array", "", "C.pick(array.length)", None),
        ("", "int[] array", "", "C.pick(new int[1].length)", None),
        ("", "int[] array", "", "C.pick(get().length)", None),
        ("int[] array;", "", "", "C.pick(array.length)", None),
        ("int[] array;", "", "", "C.pick(this.array.length)", None),
        ("long length;", "", "", "C.pick(this.length)", Some("pick(long")),
    ] {
        let mut p = loose_platform();
        let text = format!(
            "class Scalar {{ int length; }} class C {{ {prefix} public static void pick(int x){{}} public static void pick(long x){{}} int[] get(){{return null;}} void run({params}){{{body}{call};}} }}"
        );
        p.insert("C.java", &text);
        let at = text.rfind(call).unwrap() + call.find("pick").unwrap();
        let got = p.resolve_at("C.java", at as u32);
        if let Some(name) = expected {
            assert_eq!(got.status, ResolveStatus::Resolved, "{text}: {got:?}");
            assert_eq!(got.targets[0].start_byte, text.find(name).unwrap() as u32);
        } else {
            assert_ne!(got.status, ResolveStatus::Resolved, "{text}: {got:?}");
        }
    }
    for (methods, call, expected) in [
        (
            "public static void pick(long x){} public static void pick(java.lang.Integer x){}",
            "C.pick(array.length)",
            Some("pick(long"),
        ),
        (
            "public static void pick(int x,int y){} public static void pick(long x,Missing y){}",
            "C.pick(array.length,1)",
            None,
        ),
    ] {
        let mut p = loose_platform();
        let text = format!("class C {{ {methods} void run(int[] array){{{call};}} }}");
        p.insert("C.java", &text);
        let at = text.rfind(call).unwrap() + call.find("pick").unwrap();
        let got = p.resolve_at("C.java", at as u32);
        if let Some(name) = expected {
            assert_eq!(got.status, ResolveStatus::Resolved, "{got:?}");
            assert_eq!(got.targets[0].start_byte, text.find(name).unwrap() as u32);
        } else {
            assert_ne!(got.status, ResolveStatus::Resolved, "{got:?}");
        }
    }
}

#[test]
fn array_length_budget_and_unsupported_local_kinds() {
    fn call_at(node: &GreenNode, start: u32, wanted: u32) -> Option<(&GreenNode, u32)> {
        if start == wanted && node.kind() == SyntaxKind::CallExpr {
            return Some((node, start));
        }
        let mut at = start;
        for child in node.children() {
            if let Some(inner) = child.as_node()
                && let Some(found) = call_at(inner, at, wanted)
            {
                return Some(found);
            }
            at += child.utf8_len();
        }
        None
    }

    // A real long field remains the existing this_field path, never synthetic int.
    let mut field_project = loose_platform();
    let field_text = "class C { long length; void run() { take(this.length); } }";
    field_project.insert("C.java", field_text);
    let parsed = field_project.file("C.java").unwrap();
    let wanted = field_text.find("take(").unwrap() as u32;
    let caller = direct_caller(parsed.root(), wanted).unwrap();
    let (call, at) = call_at(parsed.root(), 0, wanted).unwrap();
    let (args, at) = child_kind(call, at, SyntaxKind::ArgList).unwrap();
    let nodes = delimited_nodes(args, at).unwrap();
    let (node, at) = nodes[0];
    let field_index = Index::scoped(&[&field_project], &[BTreeSet::from([0])]);
    let mut proof = ReferenceCallProof::new(&field_index, &field_index.files[0], Some(caller));
    assert!(matches!(
        proof.actual(node, at, 0),
        Some(StrictActual::Primitive(PrimitiveValue { primitive: Primitive::Long, rank: 0 }))
    ));
    // actual can pay its own node scan but the new length shape scan exhausts.
    // It must not fall through to this_field after that failed proof.
    let mut proof = ReferenceCallProof::new(&field_index, &field_index.files[0], Some(caller));
    proof.work_left = node.children().len().max(1);
    assert!(proof.actual(node, at, 0).is_none());
    assert!(proof.exhausted);
    assert_eq!(proof.work_left, 0);

    let mut p = loose_platform();
    let text = "class C { void run(int[] array) { take((array).length); } }";
    p.insert("C.java", text);
    let parsed = p.file("C.java").unwrap();
    let wanted = text.find("take(").unwrap() as u32;
    let caller = direct_caller(parsed.root(), wanted).unwrap();
    let (call, at) = call_at(parsed.root(), 0, wanted).unwrap();
    let (args, at) = child_kind(call, at, SyntaxKind::ArgList).unwrap();
    let nodes = delimited_nodes(args, at).unwrap();
    let (node, at) = nodes[0];
    let mut index = Index::scoped(&[&p], &[BTreeSet::from([0])]);
    for variant in 0..4 {
        let file = &mut index.files[0];
        let local = file.locals.iter_mut().find(|l| l.name == "array").unwrap();
        local.guard_only = variant == 1;
        local.catch_origin = variant == 2;
        local.kind = if variant == 3 { TargetKind::Resource } else { TargetKind::Parameter };
        let mut proof = ReferenceCallProof::new(&index, &index.files[0], Some(caller));
        let before = proof.work_left;
        let result = proof.array_length(node, at, 0);
        if variant == 0 {
            assert_eq!(result, Some(PrimitiveValue { primitive: Primitive::Int, rank: 0 }));
            let used = before - proof.work_left;
            for budget in [0, used - 1, used] {
                let mut proof = ReferenceCallProof::new(&index, &index.files[0], Some(caller));
                proof.work_left = budget;
                assert_eq!(proof.array_length(node, at, 0), if budget == used { result } else { None });
                if budget < used {
                    assert!(proof.exhausted);
                }
            }
            let mut proof = ReferenceCallProof::new(&index, &index.files[0], Some(caller));
            assert!(proof.array_length(node, at, REFERENCE_PROOF_DEPTH).is_none());
            assert!(proof.exhausted);
        } else {
            assert!(result.is_none(), "variant {variant}");
        }
    }
}

#[test]
fn array_length_edit_fresh_and_retained_snapshot() {
    let mut p = loose_platform();
    let text = "class C { public static void pick(int x){} public static void pick(long x){} void run(int[] array){C.pick(array.length);} }";
    p.insert("C.java", text);
    let old = p.clone();
    let at = text.rfind("pick(").unwrap() as u32;
    let before = p.resolve_at("C.java", at);
    assert_eq!(before.status, ResolveStatus::Resolved);
    let dim = text.find("[]").unwrap() as u32;
    p.edit("C.java", crate::TextEdit { start_byte: dim, end_byte: dim + 2, insert: "  ".into() }).unwrap();
    let mut fresh = loose_platform();
    fresh.insert("C.java", &text.replacen("[]", "  ", 1));
    assert_eq!(p.resolve_at("C.java", at), fresh.resolve_at("C.java", at));
    assert_ne!(p.resolve_at("C.java", at).status, ResolveStatus::Resolved);
    assert_eq!(old.resolve_at("C.java", at), before);
    p.edit("C.java", crate::TextEdit { start_byte: dim, end_byte: dim + 2, insert: "[]".into() }).unwrap();
    assert_eq!(p.resolve_at("C.java", at), before);
    let vis = [BTreeSet::from([0])];
    let prepared = PreparedScopedIndex::new(vec![&p], &vis);
    assert_eq!(prepared.resolve_at(0, "C.java", at), before);
}

#[test]
fn array_length_lexical_component_and_caller_boundaries() {
    for source in [
        "class T{} class C<T>{void run(T[] array){Target.pick(array.length);}}",
        "class C{void run(){class L{} L[] array=null;Target.pick(array.length);}}",
        "class C{void run(){int[] array=null;int[] array=null;Target.pick(array.length);}}",
        "class C{void run(int[] array){Object o=new Object(){void run(){Target.pick(array.length);}};}}",
        "class C{void run(int[] array){Runnable r=()->{Target.pick(array.length);};}}",
        "class C{void run(){int scalar=1;int array[]=null, other=1;Target.pick(other.length);}}",
    ] {
        let mut p = loose_platform();
        p.insert(
            "Target.java",
            "public class Target {public static void pick(int x){} public static void pick(long x){}}",
        );
        p.insert("C.java", source);
        let at = source.rfind("pick(").unwrap() as u32;
        let got = p.resolve_at("C.java", at);
        assert_ne!(got.status, ResolveStatus::Resolved, "{source}: {got:?}");
    }
}

#[test]
fn array_length_primitive_budget_and_sibling_dimensions() {
    let text = "class C { public static void pick(int x){} public static void pick(long x){} static void run(){int array[]=null, scalar=0;pick(array.length);} }";
    let mut p = loose_platform();
    p.insert("C.java", text);
    let pos = text.rfind("pick(").unwrap() as u32;
    assert_eq!(p.resolve_at("C.java", pos).status, ResolveStatus::Resolved);
    let index = Index::scoped(&[&p], &[BTreeSet::from([0])]);
    let file = &index.files[0];
    let parsed = p.file("C.java").unwrap();
    fn length(node: &GreenNode, at: u32) -> Option<(&GreenNode, u32)> {
        if node.kind() == SyntaxKind::FieldExpr {
            return Some((node, at));
        }
        let mut offset = at;
        for c in node.children() {
            if let Some(n) = c.as_node()
                && let Some(found) = length(n, offset)
            {
                return Some(found);
            }
            offset += c.utf8_len();
        }
        None
    }
    let (node, at) = length(parsed.root(), 0).unwrap();
    let mut proof = PrimitiveProof::new(file, 0, text.len() as u32, None);
    let before = proof.work_left;
    let value = proof.expression(node, at, 0);
    assert_eq!(value, Some(PrimitiveValue { primitive: Primitive::Int, rank: 0 }));
    let used = before - proof.work_left;
    for work in [0, used - 1, used] {
        let mut proof = PrimitiveProof::new(file, 0, text.len() as u32, None);
        proof.work_left = work;
        assert_eq!(proof.expression(node, at, 0), if work == used { value } else { None });
        if work < used {
            assert!(proof.exhausted);
        }
    }
}

#[test]
fn array_length_preserves_existing_declaration_facts_not_annotation_inference() {
    for source in [
        "@interface A {} class C { void run(@A int[] array){Target.pick(array.length);} }",
        "class C { void run(int... array){Target.pick(array.length);} }",
    ] {
        let mut p = loose_platform();
        p.insert(
            "Target.java",
            "public class Target {public static void pick(int x){} public static void pick(long x){}}",
        );
        p.insert("C.java", source);
        let result = p.resolve_at("C.java", source.rfind("pick(").unwrap() as u32);
        assert_eq!(result.status, ResolveStatus::Resolved, "{source}: {result:?}");
        assert_eq!(result.targets[0].start_byte, 40);
    }
    let mut p = loose_platform();
    let source = "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface A {} class C { void run(int @A [] array){Target.pick(array.length);} }";
    p.insert("Target.java", "public class Target {public static void pick(int x){} public static void pick(long x){}}");
    p.insert("C.java", source);
    assert_ne!(p.resolve_at("C.java", source.rfind("pick(").unwrap() as u32).status, ResolveStatus::Resolved);
}

#[test]
fn explicit_array_invocation_model() {
    for (methods, params, call, expected) in [
        (
            "public static void pick(java.lang.Object x){} public static void pick(int x){}",
            "int[] a",
            "C.pick(a)",
            "pick(java.lang.Object",
        ),
        (
            "public static void pick(int[] x){} public static void pick(java.lang.Object x){}",
            "int[] a",
            "C.pick(a)",
            "pick(int[]",
        ),
        (
            "public static void pick(java.lang.Object[] x){} public static void pick(java.lang.Object x){}",
            "int[][] a",
            "C.pick(a)",
            "pick(java.lang.Object[]",
        ),
        (
            "public static void pick(java.lang.Number[] x){} public static void pick(java.lang.Object[] x){}",
            "java.lang.Integer[] a",
            "C.pick(a)",
            "pick(java.lang.Number[]",
        ),
        (
            "public static void pick(int[] x){} public static void pick(java.lang.Object x){}",
            "",
            "C.pick(null)",
            "pick(int[]",
        ),
    ] {
        let mut p = loose_platform();
        let text = format!("class C {{ {methods} void run({params}) {{ {call}; }} }}");
        p.insert("C.java", &text);
        let at = text.rfind(call).unwrap() + call.find("pick").unwrap();
        let got = p.resolve_at("C.java", at as u32);
        assert_eq!(got.status, ResolveStatus::Resolved, "{text}: {got:?}");
        assert_eq!(got.targets[0].start_byte, text.find(expected).unwrap() as u32, "{text}");
    }
}

fn array_platform_project() -> Project {
    let mut p = loose_platform();
    for (path, raw) in
        [("Cloneable", "java/lang/Cloneable"), ("Serializable", "java/io/Serializable"), ("Other", "api/Other")]
    {
        p.add_class_bytes(path, &binary(raw, Some("java/lang/Object"), &[], true)).unwrap();
        Arc::make_mut(p.binaries.get_mut(path).unwrap()).class_signature.is_interface = true;
    }
    p.insert("Use.java", "class Use {}");
    p
}
#[test]
fn explicit_array_platform_relation_and_budget() {
    let p = array_platform_project();
    let index = Index::scoped(&[&p], &[BTreeSet::from([0])]);
    let file = &index.files[0];
    let find = |name: &str| index.binaries.iter().find(|c| c.dotted() == name).unwrap();
    let arr = ArrayValue { component: ArrayComponent::Primitive(Primitive::Int), rank: 1, link_scope: 0 };
    for (name, expected) in [
        ("java.lang.Object", true),
        ("java.lang.Cloneable", true),
        ("java.io.Serializable", true),
        ("api.Other", false),
        ("java.lang.Number", false),
    ] {
        let formal = ReferenceFormal::Reference(find(name));
        let mut proof = ReferenceCallProof::new(&index, file, None);
        assert_eq!(proof.array_conversion(arr, &formal), Some(expected), "{name}");
        let used = REFERENCE_PROOF_WORK - proof.work_left;
        assert!(used > 0);
        for budget in [0, used - 1, used] {
            let mut proof = ReferenceCallProof::new(&index, file, None);
            proof.work_left = budget;
            assert_eq!(proof.array_conversion(arr, &formal), if budget == used { Some(expected) } else { None });
            if budget < used {
                assert!(proof.exhausted);
            }
        }
    }
    for (ar, fr, component, expected) in [
        (1, 1, ArrayComponent::Primitive(Primitive::Long), false),
        (2, 1, ArrayComponent::Reference(find("java.lang.Object")), true),
        (2, 2, ArrayComponent::Reference(find("java.lang.Object")), false),
        (1, 2, ArrayComponent::Primitive(Primitive::Int), false),
    ] {
        let a = ArrayValue { rank: ar, ..arr };
        let f = ReferenceFormal::Array(ArrayValue { component, rank: fr, link_scope: 0 });
        let mut proof = ReferenceCallProof::new(&index, file, None);
        assert_eq!(proof.array_conversion(a, &f), Some(expected));
        let mut proof = ReferenceCallProof::new(&index, file, None);
        assert_eq!(proof.formal_conversion(&ReferenceFormal::Array(a), &f), Some(expected));
        let mut proof = ReferenceCallProof::new(&index, file, None);
        assert_eq!(proof.invocation_conversion(&StrictActual::Array(a), &f, InvocationPhase::Loose), Some(expected));
    }
}
#[test]
fn explicit_array_platform_unknown_is_not_negative() {
    for mode in 0..5 {
        let mut p = array_platform_project();
        match mode {
            0 => {
                p.binaries.remove("Cloneable");
            }
            1 => {
                let sig = &mut Arc::make_mut(p.binaries.get_mut("Cloneable").unwrap()).class_signature;
                sig.binary_interfaces.push("api/Other".into());
                sig.interfaces.push("api.Other".into());
            }
            2 => {
                p.add_class_bytes("duplicate", &binary("java/lang/Cloneable", Some("java/lang/Object"), &[], true))
                    .unwrap();
            }
            3 => {
                p.binaries.remove("Cloneable");
                p.insert("Cloneable.java", "package java.lang; public interface Cloneable {}");
            }
            _ => {
                Arc::make_mut(p.binaries.get_mut("Serializable").unwrap()).class_signature.signature =
                    Some("<T:Ljava/lang/Object;>Ljava/lang/Object;".into());
            }
        }
        let index = Index::scoped(&[&p], &[BTreeSet::from([0])]);
        let arr = ArrayValue { component: ArrayComponent::Primitive(Primitive::Int), rank: 1, link_scope: 0 };
        let find = |name: &str| index.binaries.iter().find(|c| c.dotted() == name).unwrap();
        let mut proof = ReferenceCallProof::new(&index, &index.files[0], None);
        assert_eq!(proof.array_to_reference(arr, find("api.Other")), None, "mode{mode}");
        let mut proof = ReferenceCallProof::new(&index, &index.files[0], None);
        assert_eq!(
            proof.array_to_reference(arr, find("java.lang.Object")),
            Some(true),
            "Object is demand-directed {mode}"
        );
    }
}
#[test]
fn explicit_array_cross_scope_identity_and_collisions() {
    let left = array_platform_project();
    let mut right = left.clone();
    for changed in [false, true] {
        if changed {
            right
                .add_class_bytes("platform!/Object", &binary("java/lang/Object", None, &[("extra", "()I")], true))
                .unwrap();
        }
        let index = Index::scoped(&[&left, &right], &[BTreeSet::from([0]), BTreeSet::from([1])]);
        let target = index.binaries.iter().find(|c| c.scope == 1 && c.dotted() == "java.lang.Object").unwrap();
        let mut proof = ReferenceCallProof::new(&index, &index.files[0], None);
        assert_eq!(
            proof.array_to_reference(
                ArrayValue { component: ArrayComponent::Primitive(Primitive::Int), rank: 1, link_scope: 0 },
                target
            ),
            if changed { None } else { Some(true) }
        );
    }
}
#[test]
fn explicit_array_complete_families_and_no_element_boxing() {
    for (methods, params, call) in [
        ("public static void pick(long[] a){} public static void pick(boolean[] a){}", "int[] a", "C.pick(a)"),
        (
            "public static void pick(java.lang.Integer[] a){} public static void pick(boolean[] a){}",
            "int[] a",
            "C.pick(a)",
        ),
        (
            "public static void pick(java.lang.Cloneable a){} public static void pick(java.io.Serializable a){}",
            "int[] a",
            "C.pick(a)",
        ),
        (
            "public static void pick(java.lang.Object a){} public static void pick(Missing[] a){}",
            "int[] a",
            "C.pick(a)",
        ),
        (
            "public static void pick(java.lang.Object a,int b){} public static void pick(int a,Missing[] b){}",
            "int[] a",
            "C.pick(a,1)",
        ),
        ("public static void pick(int[] a){} public static void pick(int[] b){}", "int[] a", "C.pick(a)"),
        ("public static <T> void pick(T[] a){} public static void pick(java.lang.Object a){}", "int[] a", "C.pick(a)"),
    ] {
        let mut p = array_platform_project();
        let text = format!("class C {{ {methods} void run({params}) {{ {call}; }} }}");
        p.insert("C.java", &text);
        let at = text.rfind(call).unwrap() + call.find("pick").unwrap();
        let got = p.resolve_at("C.java", at as u32);
        assert_ne!(got.status, ResolveStatus::Resolved, "{text}: {got:?}");
    }
}
#[test]
fn explicit_array_void_header_only_checks_result_dimensions() {
    fn method(n: &GreenNode, at: u32) -> Option<(&GreenNode, u32)> {
        if n.kind() == SyntaxKind::Method {
            return Some((n, at));
        }
        let mut offset = at;
        for c in n.children() {
            if let Some(node) = c.as_node()
                && let Some(found) = method(node, offset)
            {
                return Some(found);
            }
            offset += c.utf8_len();
        }
        None
    }
    for (text, valid) in [
        ("class C { void f(int[] a){} }", true),
        ("class C { void f(int a){ int[] b=null; } }", true),
        ("class C { void f()[]{} }", false),
        ("class C { void[] f(){} }", false),
    ] {
        let mut p = Project::new();
        p.insert("C.java", text);
        let parsed = p.file("C.java").unwrap();
        let (n, at) = method(parsed.root(), 0).unwrap();
        assert_eq!(source_call(n, at).is_some(), valid, "{text}");
    }
}

#[test]
fn explicit_array_binary_formals_and_edit_snapshots() {
    let mut p = array_platform_project();
    p.add_class_bytes(
        "Box",
        &binary("api/Box", Some("java/lang/Object"), &[("pick", "([I)V"), ("pick", "(Ljava/lang/Object;)V")], true),
    )
    .unwrap();
    let text = "class C { void run(api.Box box,int[] a) { box.pick(a); } }";
    p.insert("C.java", text);
    let at = text.find("pick(a)").unwrap() as u32;
    let before = p.resolve_at("C.java", at);
    assert_eq!(before.status, ResolveStatus::Resolved, "{before:?}");
    assert_eq!(before.targets[0].binary.as_ref().unwrap().descriptor, "([I)V");
    let old = p.clone();
    let array_at = text.find("[]").unwrap() as u32;
    p.edit("C.java", crate::TextEdit { start_byte: array_at, end_byte: array_at + 2, insert: "  ".into() }).unwrap();
    let mut fresh = old.clone();
    fresh.insert("C.java", &text.replacen("[]", "  ", 1));
    assert_eq!(p.resolve_at("C.java", at), fresh.resolve_at("C.java", at));
    // Scalar int can box to Object; it must not retain the old array winner.
    let after = p.resolve_at("C.java", at);
    assert_eq!(after.status, ResolveStatus::Resolved, "{after:?}");
    assert_eq!(after.targets[0].binary.as_ref().unwrap().descriptor, "(Ljava/lang/Object;)V");
    assert_eq!(old.resolve_at("C.java", at), before);
    p.edit("C.java", crate::TextEdit { start_byte: array_at, end_byte: array_at + 2, insert: "[]".into() }).unwrap();
    assert_eq!(p.resolve_at("C.java", at), before);
    let prepared = PreparedScopedIndex::new(vec![&p], &[BTreeSet::from([0])]);
    assert_eq!(prepared.resolve_at(0, "C.java", at), before);
}
#[test]
fn explicit_array_lexical_component_and_root_refusals() {
    for (prefix, parameters, argument) in [
        ("class C<T>", "T[] a", "a"),
        ("class C", "Missing[] a", "a"),
        ("class C", "int[] a", "new int[1]"),
        ("class C", "int[] a", "(int[])null"),
        ("class C", "int[][] a", "a[0]"),
    ] {
        let mut p = array_platform_project();
        let text = format!(
            "{prefix} {{ public static void pick(java.lang.Object x){{}} public static void pick(int x){{}} void run({parameters}) {{ C.pick({argument}); }} }}"
        );
        p.insert("C.java", &text);
        let got = p.resolve_at("C.java", text.rfind("pick(").unwrap() as u32);
        if argument == "a[0]" {
            assert_eq!(got.status, ResolveStatus::Resolved);
            assert_eq!(got.targets.len(), 1);
            let start = text.find("pick(java.lang.Object").unwrap() as u32;
            assert_eq!(
                got.targets[0],
                Target {
                    binary: None,
                    path: "C.java".into(),
                    kind: TargetKind::Method,
                    name: "pick".into(),
                    owner: "C".into(),
                    start_byte: start,
                    end_byte: start + 4,
                }
            );
        } else {
            assert_ne!(got.status, ResolveStatus::Resolved, "{text}: {got:?}");
        }
    }
}

#[test]
fn explicit_array_inherited_signature_identity_is_not_assignability() {
    let left = array_platform_project();
    let right = left.clone();
    let index = Index::scoped(&[&left, &right], &[BTreeSet::from([0]), BTreeSet::from([1])]);
    let get = |scope, name: &str| index.binaries.iter().find(|c| c.scope == scope && c.dotted() == name).unwrap();
    let a = ArrayValue { component: ArrayComponent::Reference(get(0, "java.lang.Integer")), rank: 1, link_scope: 0 };
    let same_name =
        ArrayValue { component: ArrayComponent::Reference(get(1, "java.lang.Integer")), rank: 1, link_scope: 1 };
    assert!(!a.same_type(same_name), "different declaration scopes are not identity");
    assert!(!a.definitely_different(same_name), "same logical component cannot discard an inherited competitor");
    assert!(!ReferenceFormal::Array(a).same_type(&ReferenceFormal::Array(same_name)));
    let mut proof = ReferenceCallProof::new(&index, &index.files[0], None);
    assert_eq!(
        proof.array_conversion(a, &ReferenceFormal::Array(same_name)),
        Some(true),
        "conversion compatibility must not become override equality"
    );
    assert!(a.definitely_different(ArrayValue { rank: 2, ..same_name }));
    assert!(a.definitely_different(ArrayValue {
        component: ArrayComponent::Reference(get(1, "java.lang.Long")),
        ..same_name
    }));
    let primitive = ArrayValue { component: ArrayComponent::Primitive(Primitive::Int), rank: 1, link_scope: 0 };
    let identical = ArrayValue { link_scope: 1, ..primitive };
    assert!(primitive.same_type(identical));
    assert!(!primitive.definitely_different(identical));
    assert!(
        primitive
            .definitely_different(ArrayValue { component: ArrayComponent::Primitive(Primitive::Long), ..identical })
    );
    assert!(primitive.definitely_different(ArrayValue { rank: 2, ..identical }));
    assert!(primitive.definitely_different(a));
}
