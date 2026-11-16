use java_engine::{Project, ResolveStatus};
type Method<'a> = (&'a str, &'a str, Option<&'a str>, u16);
fn binary(
    name: &str,
    parent: Option<&str>,
    interfaces: &[&str],
    flags: u16,
    signature: Option<&str>,
    methods: &[Method<'_>],
) -> Vec<u8> {
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
    let interfaces: Vec<_> = interfaces.iter().map(|name| cls(&mut cp, &mut n, name)).collect();
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
    for v in [flags, this, parent, interfaces.len() as u16] {
        u(&mut b, v);
    }
    for interface in interfaces {
        u(&mut b, interface);
    }
    for v in [0, methods.len() as u16] {
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

const GET: Method<'_> = ("get", "()Ljava/lang/Object;", Some("()TT;"), 0x401);
const SIG: &str = "<T:Ljava/lang/Object;>Ljava/lang/Object;";
fn add(p: &mut Project, name: &str, interfaces: &[&str], sig: Option<&str>, methods: &[Method<'_>]) {
    p.add_class_bytes(
        format!("{name}.class"),
        &binary(name, Some("java/lang/Object"), interfaces, 0x601, sig, methods),
    )
    .unwrap();
}
fn project(params: &str, expression: &str) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", &format!("import api.Root; class A {{public void hit(){{}}}} class B {{public void hit(){{}}}} class Calls {{void run({params}){{{expression};}}}}"));
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, &[], 0x21, None, &[])).unwrap();
    p
}
fn check(p: &Project, wanted: Option<&str>) {
    let text = p.file("Calls.java").unwrap().text();
    let at = text.rfind("hit()").unwrap() as u32;
    let r = p.resolve_at("Calls.java", at);
    assert_eq!(r, p.prepare().resolve_at("Calls.java", at));
    if let Some(owner) = wanted {
        assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}\n{text}");
        assert_eq!(r.targets.len(), 1);
        assert_eq!(r.targets[0].owner, owner);
        assert_eq!(r.targets[0].path, "Calls.java");
        let start = text.find(&format!("class {owner} ")).unwrap();
        assert_eq!(r.targets[0].start_byte, (start + text[start..].find("hit()").unwrap()) as u32);
    } else {
        assert_ne!(r.status, ResolveStatus::Resolved, "{r:?}\n{text}");
    }
}
fn identity(root_methods: &[Method<'_>], parent_methods: &[Method<'_>]) -> Project {
    let mut p = project("Root<A> root", "root.get().hit()");
    add(&mut p, "api/Root", &["api/Mid"], Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Mid<TT;>;"), root_methods);
    add(&mut p, "api/Mid", &["api/Parent"], Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Parent<TT;>;"), &[]);
    add(&mut p, "api/Parent", &[], Some(SIG), parent_methods);
    p
}
#[test]
fn identity_chain_and_exact_override_propagate_declared_return() {
    check(&identity(&[], &[GET]), Some("A"));
    check(&identity(&[GET], &[GET]), Some("A"));
    let mut p = identity(&[GET], &[GET]);
    add(&mut p, "api/Marker", &[], None, &[]);
    add(
        &mut p,
        "api/Root",
        &["api/Mid", "api/Marker"],
        Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Mid<TT;>;Lapi/Marker;"),
        &[GET],
    );
    check(&p, Some("A"));
}
#[test]
fn reordered_and_constant_parent_arguments_are_composed_not_position_guessed() {
    let mut p = project("Root<A,B> root, B b", "root.pick(b).hit()");
    add(
        &mut p,
        "api/Root",
        &["api/Mid"],
        Some("<T:Ljava/lang/Object;U:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Mid<TU;TT;>;"),
        &[],
    );
    add(
        &mut p,
        "api/Mid",
        &["api/Parent"],
        Some("<T:Ljava/lang/Object;U:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Parent<TT;TU;>;"),
        &[],
    );
    add(
        &mut p,
        "api/Parent",
        &[],
        Some("<X:Ljava/lang/Object;Y:Ljava/lang/Object;>Ljava/lang/Object;"),
        &[("pick", "(Ljava/lang/Object;)Ljava/lang/Object;", Some("(TX;)TY;"), 0x401)],
    );
    check(&p, Some("A"));
    let mut p = project("Root<B> root", "root.get().hit()");
    add(&mut p, "api/Root", &["api/Mid"], Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Mid;"), &[]);
    add(&mut p, "api/Mid", &["api/Parent"], Some("Ljava/lang/Object;Lapi/Parent<LA;>;"), &[]);
    add(&mut p, "api/Parent", &[], Some(SIG), &[GET]);
    check(&p, Some("A"));
}
#[test]
fn list_shaped_overloads_keep_all_candidates_and_select_precisely() {
    let put = ("put", "(Ljava/lang/Object;)Ljava/lang/Object;", Some("(TT;)TT;"), 0x401);
    let indexed = ("put", "(ILjava/lang/Object;)Ljava/lang/Object;", Some("(ITT;)TT;"), 0x401);
    let mut p = project("Root<A> root, A a", "root.put(a).hit()");
    add(
        &mut p,
        "api/Root",
        &["api/Parent"],
        Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Parent<TT;>;"),
        &[put, indexed],
    );
    add(&mut p, "api/Parent", &[], Some(SIG), &[put]);
    check(&p, Some("A"));
    let text = p.file("Calls.java").unwrap().text().replace("root.put(a)", "root.put(1,a)");
    p.insert("Calls.java", &text);
    check(&p, Some("A"));
}
#[test]
fn branching_same_or_different_environments_remains_unknown() {
    for right in ["TT;", "LB;"] {
        let mut p = identity(&[], &[GET]);
        add(
            &mut p,
            "api/Root",
            &["api/Mid", "api/Right"],
            Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Mid<TT;>;Lapi/Right<TT;>;"),
            &[],
        );
        add(
            &mut p,
            "api/Right",
            &["api/Parent"],
            Some(&format!("<T:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Parent<{right}>;")),
            &[],
        );
        check(&p, None);
    }
    let mut p = identity(&[], &[GET]);
    // A nominally empty side interface with ancestry is not a neutral marker.
    add(&mut p, "api/Marker", &["api/Parent"], Some("Ljava/lang/Object;Lapi/Parent<LA;>;"), &[]);
    add(
        &mut p,
        "api/Root",
        &["api/Mid", "api/Marker"],
        Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Mid<TT;>;Lapi/Marker;"),
        &[],
    );
    check(&p, None);
}
#[test]
fn same_erasure_never_certifies_incompatible_instantiated_override() {
    let mut p = identity(&[GET], &[GET]);
    add(&mut p, "api/Mid", &["api/Parent"], Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Parent<LB;>;"), &[]);
    check(&p, None);
    // A generic method signature, malformed erasure or duplicated declaration blocks the family.
    for methods in [
        vec![("get", GET.1, Some("<U:Ljava/lang/Object;>()TU;"), 0x401)],
        vec![("get", GET.1, Some("()LA;"), 0x401)],
        vec![GET, ("get", "()LA;", None, 0x401)],
    ] {
        check(&identity(&[], &methods), None);
    }
}
#[test]
fn raw_wrong_arity_bound_or_malformed_parent_certificates_refuse() {
    for parent in
        ["Lapi/Parent;", "Lapi/Parent<TT;TT;>;", "Lapi/Parent<TX;>;", "Lapi/Parent<+TT;>;", "Lapi/Parent<TT;>;junk"]
    {
        let mut p = identity(&[], &[GET]);
        add(
            &mut p,
            "api/Mid",
            &["api/Parent"],
            Some(&format!("<T:Ljava/lang/Object;>Ljava/lang/Object;{parent}")),
            &[],
        );
        check(&p, None);
    }
    let mut p = identity(&[], &[GET]);
    add(&mut p, "api/Parent", &[], Some("<T:LA;>Ljava/lang/Object;"), &[GET]);
    check(&p, None);
}
#[test]
fn unsupported_member_flags_and_complete_inventory_are_not_filtered() {
    for flags in [0x441, 0x1401, 0x401 | 8, 0x401 | 0x10, 0x402, 0x101, 0x481] {
        check(&identity(&[], &[(GET.0, GET.1, GET.2, flags)]), None);
    }
    let mut p = identity(&[GET], &[GET]);
    add(
        &mut p,
        "api/Parent",
        &[],
        Some(SIG),
        &[GET, ("get", "(Ljava/lang/Object;)Ljava/lang/Object;", Some("(TX;)TT;"), 0x401)],
    );
    check(&p, None);
}
#[test]
fn chain_budget_and_missing_root_have_no_fallback_value() {
    let mut p = identity(&[], &[GET]);
    p.remove_class("Object.class");
    check(&p, None);
    let mut p = identity(&[], &[GET]);
    let text = p
        .file("Calls.java")
        .unwrap()
        .text()
        .replace("root.get()", &format!("{}root.get()", (0..5000).map(|i| format!("int x{i}=0;")).collect::<String>()));
    p.insert("Calls.java", &text);
    check(&p, None);
    let mut p = project("Root<A> root", "root.get().hit()");
    for i in 0..40 {
        let name = if i == 0 { "api/Root".to_string() } else { format!("api/P{i}") };
        let next = format!("api/P{}", i + 1);
        add(&mut p, &name, &[&next], Some(&format!("<T:Ljava/lang/Object;>Ljava/lang/Object;L{next}<TT;>;")), &[]);
    }
    add(&mut p, "api/P40", &[], Some(SIG), &[GET]);
    check(&p, None);
}

#[test]
fn equal_erasure_but_distinct_instantiated_parameters_is_not_an_overload() {
    let put = ("put", "(Ljava/lang/Object;)Ljava/lang/Object;", Some("(TT;)TT;"), 0x401);
    let mut p = project("Root<B> root, B b", "root.put(b).hit()");
    add(&mut p, "api/Root", &["api/Parent"], Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Parent<LA;>;"), &[put]);
    add(&mut p, "api/Parent", &[], Some(SIG), &[put]);
    // javac rejects C<T> extends P<A> redeclaring put(T): same erasure, neither overrides.
    check(&p, None);
}

#[test]
fn inherited_and_overridden_targets_keep_original_binary_declarations() {
    for (methods, owner) in [(&[][..], "api.Parent"), (&[GET][..], "api.Root")] {
        let p = identity(methods, &[GET]);
        let text = p.file("Calls.java").unwrap().text();
        let r = p.resolve_at("Calls.java", text.rfind("get()").unwrap() as u32);
        assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
        assert_eq!(r.targets.len(), 1);
        assert_eq!(r.targets[0].owner, owner);
        assert_eq!(r.targets[0].binary.as_ref().unwrap().descriptor, "()Ljava/lang/Object;");
        assert_eq!(r.targets[0].path, format!("{}.class", owner.replace('.', "/")));
    }
}
#[test]
fn inherited_environment_cannot_erase_cross_compilation_bound_identity() {
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
        "generic-chain-bound-{}-{}",
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
            fs::write(bin.join("java/lang/Object.class"), binary("java/lang/Object", None, &[], 0x21, None, &[]))
                .unwrap();
            if (name == "dep") == split {
                fs::write(src.join("models/A.java"), "package models; public class A { public void hit() {} }")
                    .unwrap();
            }
            if name == "client" {
                fs::create_dir_all(bin.join("api")).unwrap();
                fs::write(
                    bin.join("api/Root.class"),
                    binary(
                        "api/Root",
                        Some("java/lang/Object"),
                        &["api/Parent"],
                        0x601,
                        Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Parent<TT;>;"),
                        &[],
                    ),
                )
                .unwrap();
                fs::write(
                    bin.join("api/Parent.class"),
                    binary("api/Parent", Some("java/lang/Object"), &[], 0x601, Some(SIG), &[GET]),
                )
                .unwrap();
                fs::write(
                    src.join("Calls.java"),
                    "import api.Root; import models.A; class Calls { void run(Root<A> box) { box.get().hit(); } }",
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

#[test]
fn concrete_arguments_do_not_legalize_declaration_level_erasure_clashes() {
    let put = ("put", "(Ljava/lang/Object;)Ljava/lang/Object;", Some("(TT;)TT;"), 0x401);
    let mut p = project("Root<A> root, A a", "root.put(a).hit()");
    add(&mut p, "api/Root", &["api/Parent"], Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Parent<LA;>;"), &[put]);
    add(&mut p, "api/Parent", &[], Some(SIG), &[put]);
    // The very same compiler-invalid Root<T> declaration stays invalid when T := A.
    // Coincidental concrete equality is not a declaration-level subsignature certificate.
    check(&p, None);
}

#[test]
fn normalized_formal_indices_not_spelling_or_equal_actuals_certify_overrides() {
    let mut p = identity(&[GET], &[GET]);
    add(
        &mut p,
        "api/Parent",
        &[],
        Some("<U:Ljava/lang/Object;>Ljava/lang/Object;"),
        &[("get", GET.1, Some("()TU;"), 0x401)],
    );
    check(&p, Some("A")); // Different formal spelling, same declaration position.
    let mut p = project("Root<A,A> root", "root.get().hit()");
    add(
        &mut p,
        "api/Root",
        &["api/Parent"],
        Some("<T:Ljava/lang/Object;U:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Parent<TT;TU;>;"),
        &[GET],
    );
    add(&mut p, "api/Parent", &[], Some("<U:Ljava/lang/Object;T:Ljava/lang/Object;>Ljava/lang/Object;"), &[GET]);
    check(&p, None); // The same spelling T refers to indices 0 and 1, even when both actuals are A.
}

#[test]
fn missing_signature_object_is_not_a_class_formal_override_certificate() {
    let mut p = project("Root<Object> root", "root.put(null).hit()");
    add(
        &mut p,
        "api/Root",
        &["api/Parent"],
        Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Parent<TT;>;"),
        &[("put", "(Ljava/lang/Object;)LA;", None, 0x401)],
    );
    add(&mut p, "api/Parent", &[], Some(SIG), &[("put", "(Ljava/lang/Object;)LA;", Some("(TT;)LA;"), 0x401)]);
    check(&p, None); // Conservative boundary even though concrete substitution gives Object in both slots.
}

#[test]
fn only_the_path_between_overlapping_declarations_requires_identity() {
    let mut p = project("Root<B> root", "root.get().hit()");
    add(&mut p, "api/Root", &["api/Mid"], Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Mid<LA;>;"), &[]);
    add(&mut p, "api/Mid", &["api/Parent"], Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Parent<TT;>;"), &[GET]);
    add(&mut p, "api/Parent", &[], Some(SIG), &[GET]);
    check(&p, Some("A")); // Root's constant edge does not alter validity of Mid<T> overriding Parent<T>.
    let mut p = identity(&[GET], &[GET]);
    add(&mut p, "api/Mid", &["api/Parent"], Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Lapi/Parent<LA;>;"), &[]);
    check(&p, None); // Constant edge lies between the two overlapping declarations, although actual T is A.
}
