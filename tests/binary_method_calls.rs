use java_engine::{Project, ResolveStatus};
fn u2(value: u16) -> [u8; 2] {
    value.to_be_bytes()
}

struct Pool {
    bytes: Vec<u8>,
    count: u16,
}

impl Pool {
    fn new() -> Self {
        Self { bytes: Vec::new(), count: 1 }
    }

    fn utf8(&mut self, text: &str) -> u16 {
        let index = self.count;
        let raw = text.as_bytes();
        self.bytes.push(1);
        self.bytes.extend_from_slice(&u2(raw.len() as u16));
        self.bytes.extend_from_slice(raw);
        self.count += 1;
        index
    }

    fn class(&mut self, name: u16) -> u16 {
        let index = self.count;
        self.bytes.push(7);
        self.bytes.extend_from_slice(&u2(name));
        self.count += 1;
        index
    }
}

struct Member {
    access: u16,
    name: String,
    descriptor: String,
}

fn class_bytes(
    binary: &str,
    super_binary: Option<&str>,
    access: u16,
    interfaces: &[&str],
    fields: &[Member],
    methods: &[Member],
    signature: Option<&str>,
) -> Vec<u8> {
    let mut pool = Pool::new();
    let this_name = pool.utf8(binary);
    let this = pool.class(this_name);
    let super_index = match super_binary {
        Some(name) => {
            let super_name = pool.utf8(name);
            pool.class(super_name)
        }
        None => 0,
    };
    let mut interface_indexes = Vec::new();
    for name in interfaces {
        let iface = pool.utf8(name);
        interface_indexes.push(pool.class(iface));
    }
    let mut member_indexes = Vec::new();
    for member in fields.iter().chain(methods.iter()) {
        let name = pool.utf8(&member.name);
        let descriptor = pool.utf8(&member.descriptor);
        member_indexes.push((name, descriptor));
    }
    let signature_name = if signature.is_some() { Some(pool.utf8("Signature")) } else { None };
    let signature_value = signature.map(|text| pool.utf8(text));

    let mut out = Vec::new();
    out.extend_from_slice(&0xCAFEBABEu32.to_be_bytes());
    out.extend_from_slice(&u2(0));
    out.extend_from_slice(&u2(61));
    out.extend_from_slice(&u2(pool.count));
    out.extend_from_slice(&pool.bytes);
    out.extend_from_slice(&u2(access));
    out.extend_from_slice(&u2(this));
    out.extend_from_slice(&u2(super_index));
    out.extend_from_slice(&u2(interface_indexes.len() as u16));
    for index in interface_indexes {
        out.extend_from_slice(&u2(index));
    }
    out.extend_from_slice(&u2(fields.len() as u16));
    for (index, field) in fields.iter().enumerate() {
        let (name, descriptor) = member_indexes[index];
        out.extend_from_slice(&u2(field.access));
        out.extend_from_slice(&u2(name));
        out.extend_from_slice(&u2(descriptor));
        out.extend_from_slice(&u2(0));
    }
    out.extend_from_slice(&u2(methods.len() as u16));
    for (index, method) in methods.iter().enumerate() {
        let (name, descriptor) = member_indexes[fields.len() + index];
        out.extend_from_slice(&u2(method.access));
        out.extend_from_slice(&u2(name));
        out.extend_from_slice(&u2(descriptor));
        out.extend_from_slice(&u2(0));
    }
    match (signature_name, signature_value) {
        (Some(name), Some(value)) => {
            out.extend_from_slice(&u2(1));
            out.extend_from_slice(&u2(name));
            out.extend_from_slice(&2u32.to_be_bytes());
            out.extend_from_slice(&u2(value));
        }
        _ => out.extend_from_slice(&u2(0)),
    }
    out
}

fn member(access: u16, name: &str, descriptor: &str) -> Member {
    Member { access, name: name.to_string(), descriptor: descriptor.to_string() }
}

fn project(text: &str, methods: &[Member]) -> Project {
    let mut p = Project::new();
    p.insert("Use.java", text);
    p.add_class_bytes("Object.class", &class_bytes("java/lang/Object", None, 0x21, &[], &[], &[], None)).unwrap();
    p.add_class_bytes("Lib.class", &class_bytes("lib/Lib", Some("java/lang/Object"), 0x21, &[], &[], methods, None))
        .unwrap();
    p
}
fn query(p: &Project, needle: &str) -> java_engine::ResolveResult {
    let at = p.file("Use.java").unwrap().text().rfind(needle).unwrap() as u32;
    let result = p.resolve_at("Use.java", at);
    assert_eq!(result, p.prepare().resolve_at("Use.java", at));
    result
}
#[test]
fn direct_binary_overloads_choose_strict_primitive_winner() {
    let s = "class Use { void run(lib.Lib lib) { lib.pick(1); lib.pick(1,2); } }";
    let p = project(s, &[member(1, "pick", "(I)V"), member(1, "pick", "(J)V"), member(1, "pick", "(II)V")]);
    let one = query(&p, "pick(1)");
    let two = query(&p, "pick(1,2)");
    assert_eq!(one.status, ResolveStatus::Resolved);
    assert_eq!(two.status, ResolveStatus::Resolved);
    assert_eq!(one.targets.len(), 1);
    assert_eq!(two.targets.len(), 1);
    assert_eq!(one.targets[0].binary.as_ref().unwrap().descriptor, "(I)V");
    assert_eq!(two.targets[0].binary.as_ref().unwrap().descriptor, "(II)V");
    assert_ne!(one.targets, two.targets, "binary JVM descriptors must distinguish overload identities");
}
#[test]
fn unique_binary_name_is_not_applicability_evidence() {
    for args in ["", "true", "1,2", "unknown"] {
        let s = format!("class Use {{ void run(lib.Lib lib) {{ lib.pick({args}); }} }}");
        let p = project(&s, &[member(1, "pick", "(I)V")]);
        assert_ne!(query(&p, "pick(").status, ResolveStatus::Resolved, "{args}");
    }
}
#[test]
fn unsupported_same_name_flags_are_not_filtered_to_make_a_winner() {
    for flags in [0x0009, 0x0081, 0x0041, 0x1001] {
        let p = project(
            "class Use { void run(lib.Lib lib) { lib.pick(1); } }",
            &[member(1, "pick", "(I)V"), member(flags, "pick", "(J)V")],
        );
        assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    }
}

#[test]
fn raw_member_flags_retain_unknown_bits_for_fail_closed_proofs() {
    let bytes = class_bytes(
        "lib/Lib",
        Some("java/lang/Object"),
        0x21,
        &[],
        &[member(0x2001, "field", "I")],
        &[member(0x2001, "pick", "(I)V")],
        None,
    );
    let sig = java_engine::parse_class_bytes("Lib.class", &bytes).unwrap();
    assert_eq!(sig.fields[0].raw_access_flags, 0x2001);
    assert_eq!(sig.methods[0].raw_access_flags, 0x2001);
    let p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[member(0x2001, "pick", "(I)V")]);
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
}

#[test]
fn exact_method_identity_keeps_full_descriptor_and_standalone_context() {
    let p =
        project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[member(1, "pick", "(I)[Ljava/lang/Object;")]);
    let r = query(&p, "pick(1)");
    assert_eq!(r.status, ResolveStatus::Resolved);
    let target = &r.targets[0];
    let id = target.binary.as_ref().unwrap();
    assert_eq!(id.binary_name, "lib/Lib");
    assert_eq!(id.descriptor, "(I)[Ljava/lang/Object;");
    assert_eq!(id.linking_context, java_engine::BinaryLinkingContext::Standalone);
    assert_eq!((target.start_byte, target.end_byte), (0, 0));
    assert_eq!(target.path, "Lib.class");
}
#[test]
fn invalid_flags_and_array_formals_never_hide_competitors() {
    for flags in [0x0003, 0x0005, 0x0201, 0x0411, 0x0421, 0x0501, 0x0c01, 0x0409] {
        let p = project(
            "class Use { void run(lib.Lib lib) { lib.pick(1); } }",
            &[member(1, "pick", "(I)V"), member(flags, "pick", "(JJ)V")],
        );
        assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved, "flags={flags:x}");
    }
    let p = project(
        "class Use { void run(lib.Lib lib) { lib.pick(1); } }",
        &[member(1, "pick", "(I)V"), member(1, "pick", "([I)V")],
    );
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
}
#[test]
fn legal_native_abstract_final_synchronized_declarations_and_return_only_collisions() {
    for flags in [0x0101, 0x0401, 0x0011, 0x0021] {
        let p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[member(flags, "pick", "(I)V")]);
        assert_eq!(query(&p, "pick(1)").status, ResolveStatus::Resolved, "flags={flags:x}");
    }
    let p = project(
        "class Use { void run(lib.Lib lib) { lib.pick(1); } }",
        &[member(1, "pick", "(I)I"), member(1, "pick", "(I)J")],
    );
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
}
#[test]
fn binary_return_values_use_strict_invocation_proof() {
    let p = project(
        "class Use { void run(lib.Lib lib,int pick) { lib.self().pick(1); } }",
        &[member(1, "self", "()Llib/Lib;"), member(1, "pick", "(I)V")],
    );
    let r = query(&p, "pick(1)");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].binary.as_ref().unwrap().binary_name, "lib/Lib");
    assert_eq!(r.targets[0].binary.as_ref().unwrap().descriptor, "(I)V");
}
#[test]
fn inherited_binary_overload_keeps_the_parent_competitor() {
    let mut p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[member(1, "pick", "(I)V")]);
    p.add_class_bytes(
        "Parent.class",
        &class_bytes("lib/Parent", Some("java/lang/Object"), 0x21, &[], &[], &[member(1, "pick", "(J)V")], None),
    )
    .unwrap();
    p.add_class_bytes(
        "Lib.class",
        &class_bytes("lib/Lib", Some("lib/Parent"), 0x21, &[], &[], &[member(1, "pick", "(I)V")], None),
    )
    .unwrap();
    exact_effective_target(&p, "lib/Lib", "(I)V");
}

#[test]
fn class_initializer_filter_does_not_shift_raw_method_flags() {
    let bytes = class_bytes(
        "lib/Lib",
        Some("java/lang/Object"),
        0x21,
        &[],
        &[],
        &[member(1, "first", "()V"), member(8, "<clinit>", "()V"), member(0x2101, "last", "()V")],
        None,
    );
    let sig = java_engine::parse_class_bytes("Lib.class", &bytes).unwrap();
    assert_eq!(
        sig.methods.iter().map(|m| (m.name.as_str(), m.raw_access_flags)).collect::<Vec<_>>(),
        vec![("first", 1), ("last", 0x2101)]
    );
}

#[test]
fn proven_source_reference_return_composes_with_binary_return() {
    let p = project(
        "class Maker { public lib.Lib make() { return null; } } class Use { void run(Maker maker,int pick) { maker.make().pick(1); maker.make().self().pick(1); } }",
        &[member(1, "pick", "(I)V"), member(1, "pick", "(J)V"), member(1, "self", "()Llib/Lib;")],
    );
    let text = p.file("Use.java").unwrap().text();
    let at = text.find("pick(1)").unwrap() as u32;
    let r = p.resolve_at("Use.java", at);
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets[0].binary.as_ref().unwrap().descriptor, "(I)V");
    assert_eq!(r, p.prepare().resolve_at("Use.java", at));
    assert_eq!(query(&p, "pick(1)"), r);
}

#[test]
fn malformed_descriptor_and_return_only_information_cannot_supply_applicability() {
    for descriptor in ["(I)", "(V)V", "(I)Vextra", "(I)[V", "(I)Lmissing", "(I)Q"] {
        let mut p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[member(1, "pick", "(I)V")]);
        assert_eq!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
        // The primary class-file parser may reject malformed descriptors before
        // they reach the binder. If admitted, strict selection must still refuse.
        if p.add_class_bytes(
            "Lib.class",
            &class_bytes("lib/Lib", Some("java/lang/Object"), 0x21, &[], &[], &[member(1, "pick", descriptor)], None),
        )
        .is_err()
        {
            continue;
        }
        assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved, "{descriptor}");
    }
}

#[test]
fn strict_flag_is_only_proven_in_its_defined_classfile_versions() {
    for (major, expected) in [
        (45, ResolveStatus::CallNotResolved),
        (46, ResolveStatus::Resolved),
        (60, ResolveStatus::Resolved),
        (61, ResolveStatus::CallNotResolved),
    ] {
        let mut p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[]);
        let mut bytes =
            class_bytes("lib/Lib", Some("java/lang/Object"), 0x21, &[], &[], &[member(0x0801, "pick", "(I)V")], None);
        bytes[6..8].copy_from_slice(&u2(major));
        p.add_class_bytes("Lib.class", &bytes).unwrap();
        assert_eq!(query(&p, "pick(1)").status, expected, "major {major}");
    }
}

#[test]
fn nested_binary_receiver_retains_receiver_not_bound_without_erased_hints() {
    let mut p = project("class Use { void run(lib.Outer.Inner lib) { lib.pick(1); } }", &[]);
    p.add_class_bytes(
        "Lib.class",
        &class_bytes("lib/Outer$Inner", Some("java/lang/Object"), 0x21, &[], &[], &[member(1, "pick", "(I)V")], None),
    )
    .unwrap();
    let result = query(&p, "pick(1)");
    assert_eq!(result.status, ResolveStatus::ReceiverNotBound);
    assert!(result.targets.is_empty());
}

#[test]
fn binary_formal_dollar_name_cannot_bind_a_different_slash_name() {
    let mut p = project(
        "class Use { void run(lib.Lib lib) { lib.pick(null); } }",
        &[member(1, "pick", "(Llib/Outer$Inner;)V")],
    );
    p.add_class_bytes(
        "Wrong.class",
        &class_bytes("lib/Outer/Inner", Some("java/lang/Object"), 0x21, &[], &[], &[], None),
    )
    .unwrap();
    assert_ne!(query(&p, "pick(null)").status, ResolveStatus::Resolved);
}

#[test]
fn inaccessible_binary_methods_do_not_become_public_failure_hints() {
    for access in [0x0002, 0x0004, 0x0000] {
        let p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[member(access, "pick", "(I)V")]);
        let result = query(&p, "pick(1)");
        assert_eq!(result.status, ResolveStatus::Unresolved, "{access:x}");
        assert!(result.targets.is_empty());
    }
}

#[test]
fn descriptor_parenthesis_inside_class_name_does_not_hide_later_dollar_formal() {
    let mut p = project(
        "class Use { void run(lib.Lib lib) { lib.pick(null, null); } }",
        &[member(1, "pick", "(Llib/Before)Name;Llib/Outer$Inner;)V")],
    );
    for name in ["lib/Before)Name", "lib/Outer/Inner"] {
        p.add_class_bytes(name, &class_bytes(name, Some("java/lang/Object"), 0x21, &[], &[], &[], None)).unwrap();
    }
    assert_ne!(query(&p, "pick(null").status, ResolveStatus::Resolved);
}

#[test]
fn protected_failure_hints_cannot_walk_unbounded_caller_inheritance() {
    let mut source = String::from("class C0 extends lib.Lib {}\n");
    for depth in 1..80 {
        source.push_str(&format!("class C{depth} extends C{} {{}}\n", depth - 1));
    }
    source.push_str("class Use extends C79 { void run(lib.Lib lib) { lib.pick(1); } }");
    let p = project(&source, &[member(4, "pick", "(I)V")]);
    let result = query(&p, "pick(1)");
    assert_eq!(result.status, ResolveStatus::ReceiverNotBound);
    assert!(result.targets.is_empty());
}

fn ancestor_identity_project(raw: &str, interface: bool) -> Project {
    let mut p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[]);
    let interfaces = if interface { vec![raw] } else { vec![] };
    p.add_class_bytes(
        "Lib.class",
        &class_bytes(
            "lib/Lib",
            Some(if interface { "java/lang/Object" } else { raw }),
            0x21,
            &interfaces,
            &[],
            &[member(1, "pick", "(I)V")],
            None,
        ),
    )
    .unwrap();
    p
}

#[test]
fn raw_binary_ancestor_names_do_not_accept_dotted_aliases() {
    for interface in [true, false] {
        let mut p = ancestor_identity_project("lib/Outer$Inner", interface);
        assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
        p.add_class_bytes(
            "Alias.class",
            &class_bytes(
                "lib/Outer/Inner",
                Some("java/lang/Object"),
                if interface { 0x601 } else { 0x21 },
                &[],
                &[],
                &[],
                None,
            ),
        )
        .unwrap();
        assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved, "interface={interface}");
    }
}

#[test]
fn exact_raw_binary_ancestor_names_preserve_closed_graphs() {
    for interface in [true, false] {
        let mut p = ancestor_identity_project("lib/Outer$Inner", interface);
        p.add_class_bytes(
            "Actual.class",
            &class_bytes(
                "lib/Outer$Inner",
                Some("java/lang/Object"),
                if interface { 0x601 } else { 0x21 },
                &[],
                &[],
                &[],
                None,
            ),
        )
        .unwrap();
        assert_eq!(query(&p, "pick(1)").status, ResolveStatus::Resolved, "interface={interface}");
        // Both spelling aliases present remain ambiguous, never choose the raw
        // match by silently dropping another indexed declaration.
        p.add_class_bytes(
            "Alias.class",
            &class_bytes(
                "lib/Outer/Inner",
                Some("java/lang/Object"),
                if interface { 0x601 } else { 0x21 },
                &[],
                &[],
                &[],
                None,
            ),
        )
        .unwrap();
        assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    }
}

#[test]
fn source_replacement_must_match_the_raw_binary_ancestor_identity() {
    let mut wrong = ancestor_identity_project("lib/Outer$Inner", false);
    wrong.insert("Alias.java", "package lib.Outer; public class Inner {}");
    assert_ne!(query(&wrong, "pick(1)").status, ResolveStatus::Resolved);
    let mut exact = ancestor_identity_project("lib/Parent", false);
    exact.insert("Parent.java", "package lib; public class Parent {}");
    assert_eq!(query(&exact, "pick(1)").status, ResolveStatus::Resolved);
    let mut nested = ancestor_identity_project("lib/Outer$Inner", false);
    nested.insert("Outer.java", "package lib; public class Outer { public static class Inner {} }");
    // Static source member classes retain their existing unsupported inventory.
    assert_ne!(query(&nested, "pick(1)").status, ResolveStatus::Resolved);
    nested.insert("Outer.java", "package lib; public class Outer { public class Inner {} }");
    assert_eq!(query(&nested, "pick(1)").status, ResolveStatus::Resolved);
}

#[test]
fn distinct_raw_ancestor_edges_with_one_dotted_name_never_collapse() {
    let mut p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[]);
    p.add_class_bytes("I.class", &class_bytes("lib/Outer/Inner", Some("java/lang/Object"), 0x601, &[], &[], &[], None))
        .unwrap();
    p.add_class_bytes(
        "Lib.class",
        &class_bytes(
            "lib/Lib",
            Some("java/lang/Object"),
            0x21,
            &["lib/Outer$Inner", "lib/Outer/Inner"],
            &[],
            &[member(1, "pick", "(I)V")],
            None,
        ),
    )
    .unwrap();
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
}

#[test]
fn class_signatures_preserve_raw_ancestor_order_beside_legacy_projections() {
    let bytes =
        class_bytes("lib/Lib", Some("lib/Outer$Base"), 0x21, &["lib/Outer$First", "lib/Outer/Second"], &[], &[], None);
    let signature = java_engine::parse_class_bytes("Lib.class", &bytes).unwrap();
    assert_eq!(signature.binary_super_class.as_deref(), Some("lib/Outer$Base"));
    assert_eq!(signature.super_class.as_deref(), Some("lib.Outer.Base"));
    assert_eq!(signature.binary_interfaces, ["lib/Outer$First", "lib/Outer/Second"]);
    assert_eq!(signature.interfaces, ["lib.Outer.First", "lib.Outer.Second"]);
}

fn effective_class(
    p: &mut Project,
    name: &str,
    parent: &str,
    interfaces: &[&str],
    interface: bool,
    methods: &[Member],
) {
    p.add_class_bytes(
        format!("{}.class", name.rsplit('/').next().unwrap()),
        &class_bytes(name, Some(parent), if interface { 0x601 } else { 0x21 }, interfaces, &[], methods, None),
    )
    .unwrap();
}

fn exact_effective_target(p: &Project, owner: &str, descriptor: &str) {
    let result = query(p, "pick(1)");
    assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
    assert_eq!(result.targets.len(), 1);
    let identity = result.targets[0].binary.as_ref().unwrap();
    assert_eq!(identity.binary_name, owner);
    assert_eq!(identity.descriptor, descriptor);
    assert_eq!(result.targets[0].name, "pick");
}

#[test]
fn effective_binary_class_inheritance_overrides_and_overloads() {
    for mode in 0..3 {
        let mut p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[]);
        effective_class(
            &mut p,
            "lib/Base",
            "java/lang/Object",
            &[],
            false,
            &[member(1, "pick", if mode == 2 { "(J)V" } else { "(I)V" })],
        );
        let own = if mode == 0 { vec![] } else { vec![member(1, "pick", "(I)V")] };
        effective_class(&mut p, "lib/Lib", "lib/Base", &[], false, &own);
        exact_effective_target(&p, if mode == 0 { "lib/Base" } else { "lib/Lib" }, "(I)V");
    }
}

#[test]
fn effective_binary_abstract_interface_direct_inherited_and_implemented() {
    for mode in 0..4 {
        let receiver = if mode < 3 { "lib.I" } else { "lib.Lib" };
        let mut p = project(&format!("class Use {{ void run({receiver} lib) {{ lib.pick(1); }} }}"), &[]);
        effective_class(&mut p, "lib/Parent", "java/lang/Object", &[], true, &[member(0x401, "pick", "(I)V")]);
        let supers = if mode == 0 { vec![] } else { vec!["lib/Parent"] };
        let own = if mode == 1 { vec![] } else { vec![member(0x401, "pick", "(I)V")] };
        effective_class(&mut p, "lib/I", "java/lang/Object", &supers, true, &own);
        if mode == 3 {
            effective_class(&mut p, "lib/Lib", "java/lang/Object", &["lib/I"], false, &[member(1, "pick", "(I)V")]);
        }
        exact_effective_target(
            &p,
            if mode == 1 {
                "lib/Parent"
            } else if mode == 3 {
                "lib/Lib"
            } else {
                "lib/I"
            },
            "(I)V",
        );
    }
}

#[test]
fn effective_binary_complete_competitors_and_exact_returns_fail_closed() {
    for flags in [0x11, 0x41, 0x81, 0x1001, 0x9, 0x2, 0x4] {
        let mut p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[]);
        effective_class(&mut p, "lib/Base", "java/lang/Object", &[], false, &[member(flags, "pick", "(I)V")]);
        effective_class(&mut p, "lib/Lib", "lib/Base", &[], false, &[member(1, "pick", "(I)V")]);
        assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved, "flags={flags:x}");
    }
    for result in ["I", "J", "Ljava/lang/Object;", "Llib/Lib;"] {
        let mut p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[]);
        effective_class(&mut p, "lib/Base", "java/lang/Object", &[], false, &[member(1, "pick", "(I)V")]);
        effective_class(&mut p, "lib/Lib", "lib/Base", &[], false, &[member(1, "pick", &format!("(I){result}"))]);
        assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved, "{result}");
    }
}

#[test]
fn effective_binary_interface_conflicts_and_cross_branch_implementation_refuse() {
    let mut p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[]);
    for name in ["lib/I", "lib/J"] {
        effective_class(&mut p, name, "java/lang/Object", &[], true, &[member(0x401, "pick", "(I)V")]);
    }
    effective_class(&mut p, "lib/Lib", "java/lang/Object", &["lib/I", "lib/J"], false, &[]);
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    effective_class(&mut p, "lib/Base", "java/lang/Object", &[], false, &[member(1, "pick", "(I)V")]);
    effective_class(&mut p, "lib/Lib", "lib/Base", &["lib/I"], false, &[]);
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    // When the declaration itself establishes the interface relation, the
    // inherited concrete declaration is sufficient and remains the target.
    effective_class(&mut p, "lib/Base", "java/lang/Object", &["lib/I"], false, &[member(1, "pick", "(I)V")]);
    exact_effective_target(&p, "lib/Base", "(I)V");
}

#[test]
fn effective_binary_defaults_object_and_mixed_source_candidates_refuse() {
    for interface_flags in [1, 9, 2, 0x441, 0x481, 0x1401] {
        let mut p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[]);
        effective_class(&mut p, "lib/I", "java/lang/Object", &[], true, &[member(interface_flags, "pick", "(J)V")]);
        effective_class(&mut p, "lib/Lib", "java/lang/Object", &["lib/I"], false, &[member(1, "pick", "(I)V")]);
        assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    }
    let mut p = project("class Use { void run(lib.I lib) { lib.pick(1); } }", &[]);
    p.add_class_bytes(
        "Object.class",
        &class_bytes("java/lang/Object", None, 0x21, &[], &[], &[member(1, "pick", "(I)V")], None),
    )
    .unwrap();
    effective_class(&mut p, "lib/I", "java/lang/Object", &[], true, &[member(0x401, "pick", "(I)V")]);
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    let mut p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[]);
    p.insert("Base.java", "package lib; public class Base { public void pick(int n) {} }");
    effective_class(&mut p, "lib/Lib", "lib/Base", &[], false, &[member(1, "pick", "(I)V")]);
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
}

#[test]
fn effective_binary_generic_nonparticipants_do_not_hide_real_competitors() {
    for same_name in [false, true] {
        let mut p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[]);
        p.add_class_bytes(
            "G.class",
            &class_bytes(
                "lib/G",
                Some("java/lang/Object"),
                0x601,
                &[],
                &[],
                &[member(0x401, if same_name { "pick" } else { "other" }, "(I)V")],
                Some("<T:Ljava/lang/Object;>Ljava/lang/Object;"),
            ),
        )
        .unwrap();
        effective_class(&mut p, "lib/I", "java/lang/Object", &[], true, &[member(0x401, "pick", "(I)V")]);
        p.add_class_bytes(
            "Lib.class",
            &class_bytes(
                "lib/Lib",
                Some("java/lang/Object"),
                0x21,
                &["lib/G", "lib/I"],
                &[],
                &[member(1, "pick", "(I)V")],
                Some("Ljava/lang/Object;Llib/G<Ljava/lang/Object;>;Llib/I;"),
            ),
        )
        .unwrap();
        if same_name {
            assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
        } else {
            exact_effective_target(&p, "lib/Lib", "(I)V");
        }
    }
}

#[test]
fn effective_binary_final_ancestor_is_checked_even_when_intermediate_is_overridden() {
    let mut p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[]);
    effective_class(&mut p, "lib/Grand", "java/lang/Object", &[], false, &[member(0x11, "pick", "(I)V")]);
    effective_class(&mut p, "lib/Base", "lib/Grand", &[], false, &[member(1, "pick", "(I)V")]);
    effective_class(&mut p, "lib/Lib", "lib/Base", &[], false, &[member(1, "pick", "(I)V")]);
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
}

#[test]
fn effective_binary_reference_return_exactness_and_transitive_override() {
    let mut p = project("class Use { void run(lib.Lib lib) { lib.pick(1); } }", &[]);
    effective_class(&mut p, "lib/Result", "java/lang/Object", &[], false, &[]);
    for (name, parent) in [("lib/Grand", "java/lang/Object"), ("lib/Base", "lib/Grand"), ("lib/Lib", "lib/Base")] {
        effective_class(&mut p, name, parent, &[], false, &[member(1, "pick", "(I)Llib/Result;")]);
    }
    exact_effective_target(&p, "lib/Lib", "(I)Llib/Result;");
    effective_class(&mut p, "lib/Lib", "lib/Base", &[], false, &[member(1, "pick", "(I)Llib/Lib;")]);
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
}

#[test]
fn effective_binary_interface_diamond_retains_one_actual_declaration() {
    let mut p = project("class Use { void run(lib.D lib) { lib.pick(1); } }", &[]);
    effective_class(&mut p, "lib/I", "java/lang/Object", &[], true, &[member(0x401, "pick", "(I)V")]);
    for name in ["lib/J", "lib/K"] {
        effective_class(&mut p, name, "java/lang/Object", &["lib/I"], true, &[]);
    }
    effective_class(&mut p, "lib/D", "java/lang/Object", &["lib/J", "lib/K"], true, &[]);
    exact_effective_target(&p, "lib/I", "(I)V");
}

#[test]
fn binary_return_primitive_scalars_feed_existing_strict_selector() {
    for tag in ['Z', 'B', 'C', 'S', 'I', 'J', 'F', 'D'] {
        let p = project(
            "class Use { void run(lib.Lib lib) { lib.consume(lib.value()); } }",
            &[member(1, "value", &format!("(){tag}")), member(1, "consume", &format!("({tag})V"))],
        );
        let result = query(&p, "consume(lib.value())");
        assert_eq!(result.status, ResolveStatus::Resolved, "{tag}: {result:?}");
        assert_eq!(result.targets[0].binary.as_ref().unwrap().descriptor, format!("({tag})V"));
    }
}

#[test]
fn binary_return_reference_chains_and_argument_targets_are_exact() {
    let ptext = "class Use { void run(lib.Lib lib) { lib.value().hit(1); lib.consume(lib.value()); } }";
    let mut p = project(ptext, &[member(1, "value", "()Llib/Result;"), member(1, "consume", "(Llib/Result;)V")]);
    effective_class(&mut p, "lib/Result", "java/lang/Object", &[], false, &[member(1, "hit", "(I)V")]);
    let result = query(&p, "hit(1)");
    assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
    assert_eq!(result.targets[0].binary.as_ref().unwrap().binary_name, "lib/Result");
    assert_eq!(query(&p, "consume(lib.value())").status, ResolveStatus::Resolved);
    p.insert("Sink.java", "public class Sink { public void take(lib.Result value) {} }");
    p.insert("Use.java", "class Use { void run(lib.Lib lib, Sink sink) { sink.take(lib.value()); } }");
    let result = query(&p, "take(lib.value())");
    assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
    assert_eq!(result.targets[0].path, "Sink.java");
}

#[test]
fn binary_return_uses_selected_overload_not_result_type_or_receiver() {
    let mut p = project("class Use { void run(lib.Lib lib) { lib.value(1).hit(1); } }", &[]);
    effective_class(
        &mut p,
        "lib/Base",
        "java/lang/Object",
        &[],
        false,
        &[member(1, "value", "(I)Llib/Result;"), member(1, "value", "(J)Llib/Other;")],
    );
    effective_class(&mut p, "lib/Lib", "lib/Base", &[], false, &[]);
    effective_class(&mut p, "lib/Result", "java/lang/Object", &[], false, &[member(1, "hit", "(I)V")]);
    effective_class(&mut p, "lib/Other", "java/lang/Object", &[], false, &[member(1, "hit", "(J)V")]);
    let result = query(&p, "hit(1)");
    assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
    assert_eq!(result.targets[0].binary.as_ref().unwrap().binary_name, "lib/Result");
    effective_class(
        &mut p,
        "lib/Base",
        "java/lang/Object",
        &[],
        false,
        &[member(1, "value", "(I)Llib/Result;"), member(1, "value", "(I)Llib/Other;")],
    );
    assert_ne!(query(&p, "hit(1)").status, ResolveStatus::Resolved);
}

#[test]
fn binary_return_void_arrays_missing_nested_and_generic_types_refuse() {
    for descriptor in ["()V", "()[I", "()[Llib/Result;", "()Lmissing/Result;", "()Llib/Outer$Result;"] {
        let mut p =
            project("class Use { void run(lib.Lib lib) { lib.value().hit(1); } }", &[member(1, "value", descriptor)]);
        effective_class(&mut p, "lib/Result", "java/lang/Object", &[], false, &[member(1, "hit", "(I)V")]);
        effective_class(&mut p, "lib/Outer/Result", "java/lang/Object", &[], false, &[member(1, "hit", "(I)V")]);
        assert_ne!(query(&p, "hit(1)").status, ResolveStatus::Resolved, "{descriptor}");
    }
    for (access, signature, parent) in [
        (0x21, Some("<T:Ljava/lang/Object;>Ljava/lang/Object;"), "java/lang/Object"),
        (0x20, None, "java/lang/Object"),
        (0x21, None, "missing/Base"),
    ] {
        let mut p = project(
            "class Use { void run(lib.Lib lib) { lib.value().hit(1); } }",
            &[member(1, "value", "()Llib/Result;")],
        );
        p.add_class_bytes(
            "Result.class",
            &class_bytes("lib/Result", Some(parent), access, &[], &[], &[member(1, "hit", "(I)V")], signature),
        )
        .unwrap();
        assert_ne!(query(&p, "hit(1)").status, ResolveStatus::Resolved);
    }
}

#[test]
fn binary_return_source_replacement_requires_exact_raw_identity() {
    for (descriptor, source, expected) in [
        ("()Llib/Result;", "package lib; public class Result { public void hit(int n) {} }", true),
        ("()Llib/Outer$Result;", "package lib.Outer; public class Result { public void hit(int n) {} }", false),
        // No '$' in the descriptor: lookup finds the same dotted source name,
        // but its actual JVM name is lib/Outer$Result, not lib/Outer/Result.
        (
            "()Llib/Outer/Result;",
            "package lib; public class Outer { public static class Result { public void hit(int n) {} } }",
            false,
        ),
    ] {
        let mut p =
            project("class Use { void run(lib.Lib lib) { lib.value().hit(1); } }", &[member(1, "value", descriptor)]);
        p.insert("Result.java", source);
        assert_eq!(query(&p, "hit(1)").status == ResolveStatus::Resolved, expected);
    }
}

#[test]
fn binary_return_inner_candidates_and_hidden_ancestors_remain_checked() {
    for flags in [0x9, 0x41, 0x81, 0x1001] {
        let p = project(
            "class Use { void run(lib.Lib lib) { lib.consume(lib.value()); } }",
            &[member(1, "value", "()I"), member(flags, "value", "(I)I"), member(1, "consume", "(I)V")],
        );
        assert_ne!(query(&p, "consume(lib.value())").status, ResolveStatus::Resolved);
    }
    let mut p = project("class Use { void run(lib.Lib lib) { lib.consume(lib.value()); } }", &[]);
    effective_class(&mut p, "lib/Grand", "java/lang/Object", &[], false, &[member(0x11, "value", "()I")]);
    effective_class(&mut p, "lib/Base", "lib/Grand", &[], false, &[member(1, "value", "()I")]);
    effective_class(
        &mut p,
        "lib/Lib",
        "lib/Base",
        &[],
        false,
        &[member(1, "value", "()I"), member(1, "consume", "(I)V")],
    );
    assert_ne!(query(&p, "consume(lib.value())").status, ResolveStatus::Resolved);
}

#[test]
fn binary_return_chains_share_depth_and_work_limits() {
    for (calls, width, expected) in [(2, 0, true), (40, 0, false), (2, 5000, false)] {
        let text = format!("class Use {{ void run(lib.Lib lib) {{ lib{}.hit(1); }} }}", ".next()".repeat(calls));
        let mut methods = vec![member(1, "next", "()Llib/Lib;"), member(1, "hit", "(I)V")];
        methods.extend((0..width).map(|i| member(1, &format!("other{i}"), "()V")));
        let p = project(&text, &methods);
        assert_eq!(query(&p, "hit(1)").status == ResolveStatus::Resolved, expected, "calls={calls}, width={width}");
    }
}

#[test]
fn binary_return_declaring_scope_cannot_be_replaced_by_caller_scope() {
    use java_engine::{
        ClasspathEntry, CompilationConfig, CompilationId, ProjectConfig, WorkspaceConfig, WorkspaceSnapshot,
    };
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let root = Fixture(std::env::temp_dir().join(format!(
        "binary-return-scopes-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    )));
    for dir in ["lib", "app", "libclasses/dep", "appclasses/dep", "platform/java/lang"] {
        fs::create_dir_all(root.0.join(dir)).unwrap();
    }
    fs::write(
        root.0.join("platform/java/lang/Object.class"),
        class_bytes("java/lang/Object", None, 0x21, &[], &[], &[], None),
    )
    .unwrap();
    fs::write(
        root.0.join("lib/Provider.java"),
        "package api; public class Provider { public dep.Factory factory() { return null; } }",
    )
    .unwrap();
    let caller = "package app; public class Use { void run(api.Provider p) { p.factory().value().hit(1); } }";
    fs::write(root.0.join("app/Use.java"), caller).unwrap();
    fs::write(
        root.0.join("libclasses/dep/Factory.class"),
        class_bytes(
            "dep/Factory",
            Some("java/lang/Object"),
            0x21,
            &[],
            &[],
            &[member(1, "value", "()Ldep/Token;")],
            None,
        ),
    )
    .unwrap();
    fs::write(
        root.0.join("appclasses/dep/Token.class"),
        class_bytes("dep/Token", Some("java/lang/Object"), 0x21, &[], &[], &[member(1, "hit", "(J)V")], None),
    )
    .unwrap();
    let lib = CompilationId::new("lib", "main");
    let app = CompilationId::new("app", "main");
    let inputs = |name: &str| ProjectConfig {
        source_roots: vec![root.0.join(name)],
        classpath: vec![
            ClasspathEntry::Directory(root.0.join(format!("{name}classes"))),
            ClasspathEntry::Directory(root.0.join("platform")),
        ],
        ..Default::default()
    };
    let config = WorkspaceConfig {
        compilations: vec![
            CompilationConfig { id: lib.clone(), dependencies: vec![], inputs: inputs("lib") },
            CompilationConfig { id: app.clone(), dependencies: vec![lib.clone()], inputs: inputs("app") },
        ],
        ..Default::default()
    };
    let path = root.0.join("app/Use.java").canonicalize().unwrap().to_str().unwrap().to_owned();
    for present in [false, true] {
        if present {
            fs::write(
                root.0.join("libclasses/dep/Token.class"),
                class_bytes("dep/Token", Some("java/lang/Object"), 0x21, &[], &[], &[member(1, "hit", "(I)V")], None),
            )
            .unwrap();
        }
        let snapshot = WorkspaceSnapshot::load(&config).unwrap();
        let at = caller.find("hit(1)").unwrap() as u32;
        let result = snapshot.scope(&app).unwrap().resolve_at(&path, at);
        assert_eq!(result, snapshot.prepare().scope(&app).unwrap().resolve_at(&path, at));
        if present {
            assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
            assert_eq!(result.targets[0].binary.as_ref().unwrap().descriptor, "(I)V");
            assert_eq!(
                result.targets[0].binary.as_ref().unwrap().linking_context,
                java_engine::BinaryLinkingContext::Compilation(lib.clone())
            );
            assert!(result.targets[0].path.contains("libclasses"));
        } else {
            assert_ne!(result.status, ResolveStatus::Resolved, "caller-only Token must not fill declaration-scope gap");
        }
    }
}

#[test]
fn binary_return_unsupported_values_do_not_change_terminal_navigation() {
    for descriptor in ["()V", "()[I", "()[Llib/Result;", "()Lmissing/Result;"] {
        let p = project(
            "class Use { void run(lib.Lib lib) { lib.consume(lib.value()); } }",
            &[member(1, "value", descriptor), member(1, "consume", "(I)V"), member(1, "consume", "(J)V")],
        );
        assert_eq!(query(&p, "value()").status, ResolveStatus::Resolved, "terminal {descriptor}");
        assert_ne!(query(&p, "consume(lib.value())").status, ResolveStatus::Resolved, "outer {descriptor}");
    }
}

#[test]
fn binary_return_ambiguous_types_and_dotted_raw_aliases_never_supply_values() {
    let mut p =
        project("class Use { void run(lib.Lib lib) { lib.value().hit(1); } }", &[member(1, "value", "()Llib/Result;")]);
    for file in ["First.java", "Second.java"] {
        p.insert(file, "package lib; public class Result { public void hit(int n) {} }");
    }
    assert_ne!(query(&p, "hit(1)").status, ResolveStatus::Resolved);
    let mut p = project(
        "class Use { void run(lib.Lib lib) { lib.value().hit(1); } }",
        &[member(1, "value", "()Llib/Outer/Result;")],
    );
    effective_class(&mut p, "lib/Outer$Result", "java/lang/Object", &[], false, &[member(1, "hit", "(I)V")]);
    assert_ne!(query(&p, "hit(1)").status, ResolveStatus::Resolved);
}
