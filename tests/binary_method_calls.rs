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
fn binary_return_values_are_not_propagated_by_source_invocation_proof() {
    let p = project(
        "class Use { void run(lib.Lib lib,int pick) { lib.self().pick(1); } }",
        &[member(1, "self", "()Llib/Lib;"), member(1, "pick", "(I)V")],
    );
    let r = query(&p, "pick(1)");
    assert_eq!(r.status, ResolveStatus::ReceiverNotBound);
    assert!(r.targets.is_empty());
}
#[test]
fn inherited_binary_same_name_is_not_direct_owner_evidence() {
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
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
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
fn proven_source_reference_return_can_select_binary_terminal_but_not_return_it() {
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
    assert_eq!(query(&p, "pick(1)").status, ResolveStatus::ReceiverNotBound);
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
