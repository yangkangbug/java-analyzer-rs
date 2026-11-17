use java_engine::{Project, ResolveResult, ResolveStatus};

type Method<'a> = (&'a str, &'a str, Option<&'a str>, u16);
fn binary(name: &str, parent: Option<&str>, flags: u16, signature: Option<&str>, methods: &[Method<'_>]) -> Vec<u8> {
    fn u(b: &mut Vec<u8>, v: u16) {
        b.extend(v.to_be_bytes());
    }
    fn utf(b: &mut Vec<u8>, n: &mut u16, s: &str) -> u16 {
        let i = *n;
        *n += 1;
        b.push(1);
        u(b, s.len() as u16);
        b.extend(s.as_bytes());
        i
    }
    fn cls(b: &mut Vec<u8>, n: &mut u16, s: &str) -> u16 {
        let t = utf(b, n, s);
        let i = *n;
        *n += 1;
        b.push(7);
        u(b, t);
        i
    }
    let mut cp = Vec::new();
    let mut n = 1;
    let this = cls(&mut cp, &mut n, name);
    let parent = parent.map(|s| cls(&mut cp, &mut n, s)).unwrap_or(0);
    let attr = utf(&mut cp, &mut n, "Signature");
    let signature = signature.map(|s| utf(&mut cp, &mut n, s));
    let methods: Vec<_> = methods
        .iter()
        .map(|(name, desc, sig, flags)| {
            (*flags, utf(&mut cp, &mut n, name), utf(&mut cp, &mut n, desc), sig.map(|s| utf(&mut cp, &mut n, s)))
        })
        .collect();
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61];
    u(&mut b, n);
    b.extend(cp);
    for v in [flags, this, parent, 0, 0, methods.len() as u16] {
        u(&mut b, v);
    }
    for (flags, name, desc, sig) in methods {
        for v in [flags, name, desc, u16::from(sig.is_some())] {
            u(&mut b, v);
        }
        if let Some(sig) = sig {
            u(&mut b, attr);
            b.extend(2u32.to_be_bytes());
            u(&mut b, sig);
        }
    }
    u(&mut b, u16::from(signature.is_some()));
    if let Some(sig) = signature {
        u(&mut b, attr);
        b.extend(2u32.to_be_bytes());
        u(&mut b, sig);
    }
    b
}
const BOX: &str = "<T:Ljava/lang/Object;>Ljava/lang/Object;";
const GET: Method<'_> = ("get", "()Ljava/lang/Object;", Some("()TT;"), 0x101);
const PUT: Method<'_> = ("put", "(Ljava/lang/Object;)Ljava/lang/Object;", Some("(TT;)TT;"), 0x101);
fn project(sig: &str, methods: &[Method<'_>], params: &str, expr: &str, interface: bool) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", &format!("import api.Box; class A {{ public void hit() {{}} }} class B {{ public void hit() {{}} }} class Calls {{ void run({params}) {{ {expr}; }} }}"));
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, None, &[])).unwrap();
    p.add_class_bytes(
        "Box.class",
        &binary("api/Box", Some("java/lang/Object"), if interface { 0x601 } else { 0x21 }, Some(sig), methods),
    )
    .unwrap();
    p
}
fn query(p: &Project) -> ResolveResult {
    let text = p.file("Calls.java").unwrap().text();
    let at = text.rfind("hit()").unwrap() as u32;
    let r = p.resolve_at("Calls.java", at);
    assert_eq!(r, p.prepare().resolve_at("Calls.java", at));
    r
}
fn exact(p: &Project, owner: &str) {
    let r = query(p);
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets.len(), 1, "{r:?}");
    let t = &r.targets[0];
    assert_eq!(t.owner, owner);
    assert_eq!(t.path, "Calls.java");
    let text = p.file("Calls.java").unwrap().text();
    let start = text.find(&format!("class {owner} ")).unwrap();
    assert_eq!(t.start_byte, (start + text[start..].find("hit()").unwrap()) as u32);
}
fn refused(p: &Project) {
    let r = query(p);
    assert_ne!(r.status, ResolveStatus::Resolved, "{r:?}");
}
#[test]
fn concrete_class_and_interface_templates_propagate_instantiated_returns() {
    for interface in [false, true] {
        let flags = if interface { 0x401 } else { 0x101 };
        let methods = [(GET.0, GET.1, GET.2, flags), (PUT.0, PUT.1, PUT.2, flags)];
        exact(&project(BOX, &methods, "Box<A> box, A a", "box.get().hit()", interface), "A");
        exact(&project(BOX, &methods, "Box<A> box, A a", "box.put(a).hit()", interface), "A");
        exact(&project(BOX, &methods, "Box<B> box, B b", "box.put(b).hit()", interface), "B");
        refused(&project(BOX, &methods, "Box<A> box, B b", "box.put(b).hit()", interface));
    }
}
#[test]
fn list_and_map_shaped_own_families_are_not_box_name_special_cases() {
    let list = [
        ("get", "(I)Ljava/lang/Object;", Some("(I)TT;"), 0x401),
        ("set", "(ILjava/lang/Object;)Ljava/lang/Object;", Some("(ITT;)TT;"), 0x401),
    ];
    exact(&project(BOX, &list, "Box<A> box, A a, int i", "box.get(i).hit()", true), "A");
    exact(&project(BOX, &list, "Box<A> box, A a, int i", "box.set(i,a).hit()", true), "A");
    let sig = "<K:Ljava/lang/Object;V:Ljava/lang/Object;>Ljava/lang/Object;";
    let map = [
        ("get", "(Ljava/lang/Object;)Ljava/lang/Object;", Some("(Ljava/lang/Object;)TV;"), 0x401),
        ("put", "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", Some("(TK;TV;)TV;"), 0x401),
    ];
    exact(&project(sig, &map, "Box<A,B> box, A a, B b", "box.get(a).hit()", true), "B");
    exact(&project(sig, &map, "Box<A,B> box, A a, B b", "box.put(a,b).hit()", true), "B");
    refused(&project(sig, &map, "Box<A,B> box, A a, B b", "box.put(b,a).hit()", true));
}
#[test]
fn unsupported_receiver_arguments_never_supply_instantiated_values() {
    for ty in ["Box", "Box<A,B>", "Box<?>", "Box<? extends A>", "Box<Box<A>>", "Box<A[]>", "Box<int>"] {
        refused(&project(BOX, &[GET], &format!("{ty} box"), "box.get().hit()", false));
    }
    for sig in [
        "<T:Ljava/lang/Number;>Ljava/lang/Object;",
        "<T:Ljava/lang/Object;T:Ljava/lang/Object;>Ljava/lang/Object;",
        "<T:Ljava/lang/Object;>Ljava/lang/Object;junk",
    ] {
        refused(&project(sig, &[GET], "Box<A> box", "box.get().hit()", false));
    }
}
#[test]
fn unknown_templates_and_erasure_mismatch_block_entire_named_family() {
    for sig in ["()TX;", "<U:Ljava/lang/Object;>()TU;", "()Ljava/lang/String;", "()[TT;", "()TT;junk"] {
        let methods = [("get", GET.1, Some(sig), 0x101)];
        refused(&project(BOX, &methods, "Box<A> box", "box.get().hit()", false));
    }
    // No Signature preserves Object, never invents T from the owner's parameter list.
    refused(&project(BOX, &[("get", GET.1, None, 0x101)], "Box<A> box", "box.get().hit()", false));
    // Even an unsupported different-arity competitor cannot be dropped before proof.
    refused(&project(
        BOX,
        &[GET, ("get", "(Ljava/lang/Object;)Ljava/lang/Object;", Some("(TX;)TT;"), 0x101)],
        "Box<A> box",
        "box.get().hit()",
        false,
    ));
}
#[test]
fn bad_flags_and_duplicate_or_collapsed_overloads_do_not_escape_through_return_chain() {
    for flags in [0x109, 0x181, 0x141, 0x1101, 0x102] {
        refused(&project(BOX, &[(GET.0, GET.1, GET.2, flags)], "Box<A> box", "box.get().hit()", false));
    }
    // Distinct descriptors erase parameters identically: invalid duplicate is not hidden by return type.
    refused(&project(BOX, &[GET, ("get", "()LA;", None, 0x101)], "Box<A> box", "box.get().hit()", false));
    // The two valid declaration parameter signatures collapse only after T := A.
    refused(&project(BOX, &[PUT, ("put", "(LA;)LA;", None, 0x101)], "Box<A> box, A a", "box.put(a).hit()", false));
}
#[test]
fn ancestor_same_name_is_not_silently_erased_from_inventory() {
    let mut p = project("<T:Ljava/lang/Object;>Lapi/Parent;", &[GET], "Box<A> box", "box.get().hit()", false);
    // Replace only Box to align the declared raw superclass with its Signature.
    p.add_class_bytes(
        "Box.class",
        &binary("api/Box", Some("api/Parent"), 0x21, Some("<T:Ljava/lang/Object;>Lapi/Parent;"), &[GET]),
    )
    .unwrap();
    p.add_class_bytes("Parent.class", &binary("api/Parent", Some("java/lang/Object"), 0x21, None, &[])).unwrap();
    exact(&p, "A"); // Complete parent without same-name methods is a positive control.
    p.add_class_bytes(
        "Parent.class",
        &binary("api/Parent", Some("java/lang/Object"), 0x21, None, &[("get", "(I)Ljava/lang/Object;", None, 0x101)]),
    )
    .unwrap();
    exact(&p, "A"); // A fully proved different-arity inherited overload is not applicable.
    p.add_class_bytes(
        "Parent.class",
        &binary(
            "api/Parent",
            Some("java/lang/Object"),
            0x21,
            None,
            &[("get", "(LMissing;)Ljava/lang/Object;", None, 0x101)],
        ),
    )
    .unwrap();
    refused(&p); // Unknown different-arity formal remains a complete-inventory blocker.
}

#[test]
fn receiver_names_and_explicit_local_declarations_use_the_same_generic_path() {
    for name in ["Sequence", "Dictionary"] {
        let mut p = project(BOX, &[GET], "Box<A> input", "Box<A> box = input; box.get().hit()", false);
        let text = p.file("Calls.java").unwrap().text().replace("Box", name);
        p.insert("Calls.java", &text);
        p.add_class_bytes(
            "Box.class",
            &binary(&format!("api/{name}"), Some("java/lang/Object"), 0x21, Some(BOX), &[GET]),
        )
        .unwrap();
        exact(&p, "A");
    }
    // Applied receivers remain receiver-only; they are not erased Object actuals.
    let p = project(BOX, &[PUT], "Box<A> box", "box.put(box).hit()", false);
    refused(&p);
}

#[test]
fn missing_platform_shadowed_arguments_and_rank_never_produce_values() {
    let mut p = project(BOX, &[GET], "Box<A> box", "box.get().hit()", false);
    assert!(p.remove_class("Object.class"));
    refused(&p);
    for params in ["Box<A>[] box", "Box<A> box[]", "Box<A>... box", "Box<A> box, Box<B> box"] {
        refused(&project(BOX, &[GET], params, "box.get().hit()", false));
    }
    for replacement in ["class Calls<A> { void run(", "class Calls { <A> void run("] {
        let mut p = project(BOX, &[GET], "Box<A> box", "box.get().hit()", false);
        let text = p.file("Calls.java").unwrap().text().replace("class Calls { void run(", replacement);
        p.insert("Calls.java", &text);
        refused(&p);
    }
    // A same-spelled value is NOT a type shadow.
    exact(&project(BOX, &[GET], "Box<A> box, int A", "box.get().hit()", false), "A");
    // Local expiry/duplicate overlap cannot be bypassed by a navigated type hint.
    refused(&project(BOX, &[GET], "Box<A> input", "{ Box<A> box = input; } box.get().hit()", false));
    refused(&project(BOX, &[GET], "Box<A> box", "Box<B> box = null; box.get().hit()", false));
}

#[test]
fn generic_receivers_share_work_and_depth_limits() {
    let locals: String = (0..5000).map(|i| format!("int local{i} = 0;")).collect();
    refused(&project(BOX, &[GET], "Box<A> box", &format!("{locals} box.get().hit()"), false));
    let expr = format!("{}box{}.get().hit()", "(".repeat(40), ")".repeat(40));
    refused(&project(BOX, &[GET], "Box<A> box", &expr, false));
}

#[test]
fn object_bound_requires_identity_across_compilation_links() {
    use java_engine::{
        AnalysisSnapshot, ClasspathEntry, CompilationConfig, CompilationId, ProjectConfig, WorkspaceConfig,
        WorkspaceSnapshot,
    };
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "generic-bound-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    for split in [false, true] {
        let base = root.join(if split { "split" } else { "local" });
        let mut configs = Vec::new();
        for name in ["dep", "client"] {
            let src = base.join(name).join("src");
            let bin = base.join(name).join("bin");
            fs::create_dir_all(src.join("models")).unwrap();
            fs::create_dir_all(bin.join("java/lang")).unwrap();
            fs::write(bin.join("java/lang/Object.class"), binary("java/lang/Object", None, 0x21, None, &[])).unwrap();
            if (name == "dep") == split {
                fs::write(src.join("models/A.java"), "package models; public class A { public void hit() {} }")
                    .unwrap();
            }
            if name == "client" {
                fs::create_dir_all(bin.join("api")).unwrap();
                fs::write(
                    bin.join("api/Box.class"),
                    binary("api/Box", Some("java/lang/Object"), 0x21, Some(BOX), &[GET]),
                )
                .unwrap();
                fs::write(
                    src.join("Calls.java"),
                    "import api.Box; import models.A; class Calls { void run(Box<A> box) { box.get().hit(); } }",
                )
                .unwrap();
            }
            configs.push(CompilationConfig {
                id: CompilationId::new(name, "main"),
                inputs: ProjectConfig {
                    source_roots: vec![src],
                    classpath: vec![ClasspathEntry::Directory(bin)],
                    ..Default::default()
                },
                dependencies: if name == "client" && split { vec![CompilationId::new("dep", "main")] } else { vec![] },
            });
        }
        let analysis = AnalysisSnapshot::new(
            WorkspaceSnapshot::load(&WorkspaceConfig { compilations: configs, ..Default::default() }).unwrap(),
        );
        let path = base.join("client/src/Calls.java").canonicalize().unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let result = analysis
            .scope(&CompilationId::new("client", "main"))
            .unwrap()
            .resolve_at(path.to_str().unwrap(), text.find("hit()").unwrap() as u32);
        if split {
            assert_ne!(result.status, ResolveStatus::Resolved, "{result:?}");
        } else {
            assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
            assert_eq!(result.targets[0].owner, "models.A");
        }
    }
}

fn marked_query(p: &Project) -> ResolveResult {
    let at = (p.file("Calls.java").unwrap().text().find("/*Q*/").unwrap() + 5) as u32;
    let r = p.resolve_at("Calls.java", at);
    assert_eq!(r, p.prepare().resolve_at("Calls.java", at));
    r
}
#[test]
fn unbounded_wildcard_own_family_without_unknown_types_is_proved() {
    for interface in [false, true] {
        let flags = if interface { 0x401 } else { 0x101 };
        let p = project(BOX, &[("ready", "()Z", None, flags)], "Box<?> b", "b./*Q*/ready()", interface);
        let r = marked_query(&p);
        assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
        assert_eq!(r.targets.len(), 1);
        assert_eq!(r.targets[0].owner, "api.Box");
        assert_eq!(r.targets[0].binary.as_ref().unwrap().descriptor, "()Z");
    }
}
#[test]
fn unbounded_wildcard_never_becomes_object_or_void_evidence() {
    for (methods, expr) in [
        (vec![GET], "b./*Q*/get()"),
        (vec![PUT], "b./*Q*/put(null)"),
        (
            vec![("ready", "()Z", None, 0x101), ("ready", "(Ljava/lang/Object;)Z", Some("(TT;)Z"), 0x101)],
            "b./*Q*/ready()",
        ),
        (
            vec![("ready", "()Z", None, 0x101), ("ready", "(I)Ljava/lang/Object;", Some("(I)TT;"), 0x101)],
            "b./*Q*/ready()",
        ),
    ] {
        let p = project(BOX, &methods, "Box<?> b", expr, false);
        assert_ne!(marked_query(&p).status, ResolveStatus::Resolved);
    }
}
#[test]
fn mixed_wildcard_slots_only_materialize_concrete_arguments() {
    let sig = "<T:Ljava/lang/Object;U:Ljava/lang/Object;>Ljava/lang/Object;";
    let methods = [("put", "(Ljava/lang/Object;)V", Some("(TT;)V"), 0x101)];
    let p = project(sig, &methods, "Box<A,?> b, A a", "b./*Q*/put(a)", false);
    assert_eq!(marked_query(&p).status, ResolveStatus::Resolved);
    let p = project(sig, &methods, "Box<?,A> b, A a", "b./*Q*/put(a)", false);
    assert_ne!(marked_query(&p).status, ResolveStatus::Resolved);
}
#[test]
fn wildcard_scope_keeps_bounds_rank_arity_and_source_owners_out() {
    let methods = [("ready", "()Z", None, 0x101)];
    for params in ["Box<? extends A> b", "Box<? super A> b", "Box<A,?> b", "Box<?>[] b", "Box<@Deprecated ?> b"] {
        assert_ne!(
            marked_query(&project(BOX, &methods, params, "b./*Q*/ready()", false)).status,
            ResolveStatus::Resolved,
            "{params}"
        );
    }
    for sig in [
        "<T:Ljava/lang/Number;>Ljava/lang/Object;",
        "<T:Ljava/lang/Object;:Ljava/io/Serializable;>Ljava/lang/Object;",
        "<T:TT;>Ljava/lang/Object;",
    ] {
        assert_ne!(
            marked_query(&project(sig, &methods, "Box<?> b", "b./*Q*/ready()", false)).status,
            ResolveStatus::Resolved
        );
    }
    let mut p = project(BOX, &methods, "Box<?> b", "b./*Q*/ready()", false);
    p.remove_class("Box.class");
    p.insert("Box.java", "package api; public class Box<T> {public boolean ready(){return true;}}");
    assert_ne!(marked_query(&p).status, ResolveStatus::Resolved);
}
#[test]
fn wildcard_parent_edges_do_not_gain_substitution_certificates() {
    for own in [false, true] {
        let mut p = project(BOX, &[], "Box<?> b", "b./*Q*/ready()", false);
        p.add_class_bytes(
            "Parent.class",
            &binary("api/Parent", Some("java/lang/Object"), 0x21, Some(BOX), &[("ready", "()Z", None, 0x101)]),
        )
        .unwrap();
        let methods = if own { vec![("ready", "()Z", None, 0x101)] } else { vec![] };
        p.add_class_bytes(
            "Box.class",
            &binary("api/Box", Some("api/Parent"), 0x21, Some("<T:Ljava/lang/Object;>Lapi/Parent<TT;>;"), &methods),
        )
        .unwrap();
        assert_ne!(marked_query(&p).status, ResolveStatus::Resolved);
    }
    let mut p = project(BOX, &[("ready", "()Z", None, 0x101)], "Box<?> b", "b./*Q*/ready()", false);
    let text = p.file("Calls.java").unwrap().text().replace("class Calls {", "class Calls extends Box<?> {");
    p.insert("Calls.java", &text);
    assert_ne!(marked_query(&p).status, ResolveStatus::Resolved);
}
#[test]
fn wildcard_source_edit_preserves_old_snapshot_and_fresh_result() {
    use java_engine::TextEdit;
    let mut p = project(BOX, &[GET], "Box<A> b", "b.get()./*Q*/hit()", false);
    let old = p.clone();
    let before = marked_query(&old);
    assert_eq!(before.status, ResolveStatus::Resolved);
    let text = p.file("Calls.java").unwrap().text();
    let at = text.find("Box<A>").unwrap() as u32 + 4;
    p.edit("Calls.java", TextEdit { start_byte: at, end_byte: at + 1, insert: "?".into() }).unwrap();
    assert_ne!(marked_query(&p).status, ResolveStatus::Resolved);
    let fresh = project(BOX, &[GET], "Box<?> b", "b.get()./*Q*/hit()", false);
    assert_eq!(marked_query(&p), marked_query(&fresh));
    assert_eq!(marked_query(&old), before);
}
#[test]
fn wildcard_receiver_evidence_is_not_a_scalar_argument_or_source_formal() {
    for formal in ["Object value", "api.Box<?> value"] {
        let mut p = project(BOX, &[], "Box<?> b, Sink sink", "sink./*Q*/take(b)", false);
        p.insert("Sink.java", &format!("class Sink {{ public void take({formal}) {{}} }}"));
        assert_ne!(marked_query(&p).status, ResolveStatus::Resolved, "{formal}");
    }
}
#[test]
fn wildcard_owned_fields_share_receiver_boundary() {
    for expression in ["saved./*Q*/ready()", "this.saved./*Q*/ready()"] {
        let mut p = project(BOX, &[("ready", "()Z", None, 0x101)], "", expression, false);
        let text = p.file("Calls.java").unwrap().text().replace("class Calls {", "class Calls { api.Box<?> saved;");
        p.insert("Calls.java", &text);
        assert_eq!(marked_query(&p).status, ResolveStatus::Resolved);
    }
}
