use java_engine::{Project, ResolveResult, ResolveStatus};

type Method<'a> = (&'a str, &'a str, u16, Option<&'a str>);
fn binary(name: &str, parent: Option<&str>, flags: u16, methods: &[Method<'_>]) -> Vec<u8> {
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
    for v in [flags, this, parent, 0, 0, records.len() as u16] {
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
    u(&mut b, 0);
    b
}
const INT: Method<'static> = ("pick", "(I)I", 0x109, None);
const LONG: Method<'static> = ("pick", "(J)J", 0x109, None);
fn project(source: &str, methods: &[Method<'_>]) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", source);
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, &[])).unwrap();
    p.add_class_bytes("Api.class", &binary("lib/Api", Some("java/lang/Object"), 0x21, methods)).unwrap();
    p
}
fn query(p: &Project, marker: &str) -> ResolveResult {
    let at = p.file("Calls.java").unwrap().text().rfind(marker).unwrap() as u32;
    let r = p.resolve_at("Calls.java", at);
    assert_eq!(r, p.prepare().resolve_at("Calls.java", at));
    r
}
fn selected(p: &Project, marker: &str, owner: &str, desc: &str) {
    let r = query(p, marker);
    assert_eq!(r.status, ResolveStatus::Resolved, "{marker}: {r:?}");
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].owner, owner);
    assert_eq!(r.targets[0].binary.as_ref().unwrap().descriptor, desc);
}
#[test]
fn binary_type_static_overloads_use_strict_primitive_selection() {
    for (parameter, desc) in [("int", "(I)I"), ("long", "(J)J")] {
        let text = format!("import lib.Api; class Calls {{ void run({parameter} value){{Api.pick(value);}}}}");
        selected(&project(&text, &[INT, LONG]), "pick(value)", "lib.Api", desc);
    }
    let p = project("import lib.Api; class Calls { void run(){Api.pick();}}", &[INT]);
    assert_ne!(query(&p, "pick()").status, ResolveStatus::Resolved);
}
#[test]
fn static_result_values_compose_without_using_legacy_navigation() {
    let text = "import lib.Api; class Product {} class Box {public void take(int i){} public void take(long i){}} class Calls {void run(Box box){box.take(Api.pick(1)); Api.make().value();}}";
    let mut p = project(text, &[INT, LONG, ("make", "()Llib/Product;", 0x109, None)]);
    p.add_class_bytes(
        "Product.class",
        &binary("lib/Product", Some("java/lang/Object"), 0x21, &[("value", "()I", 0x101, None)]),
    )
    .unwrap();
    let r = query(&p, "take(Api.pick(1))");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].start_byte, text.find("take(int i)").unwrap() as u32);
    selected(&p, "value()", "lib.Product", "()I");
}
#[test]
fn unsupported_static_families_are_not_pruned_by_arity() {
    // A complete concrete instance competitor of different arity is now supported.
    let p = project("import lib.Api; class Calls {void run(){Api.pick(1);}}", &[INT, ("pick", "()I", 0x101, None)]);
    selected(&p, "pick(1)", "lib.Api", "(I)I");

    for other in [
        ("pick", "()I", 0x108, None),   // package access
        ("pick", "()I", 0x10a, None),   // private
        ("pick", "()I", 0x10c, None),   // protected
        ("pick", "()I", 0x149, None),   // bridge
        ("pick", "()I", 0x1109, None),  // synthetic
        ("pick", "([I)I", 0x189, None), // varargs
        ("pick", "()I", 0x409, None),   // abstract static
        ("pick", "()I", 0x109, Some("<T:Ljava/lang/Object;>()I")),
    ] {
        let p = project("import lib.Api; class Calls {void run(){Api.pick(1);}}", &[INT, other]);
        assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved, "{other:?}");
    }
}
#[test]
fn interface_inherited_and_duplicate_static_families_refuse() {
    let text = "import lib.Api; class Calls {void run(){Api.pick(1);}}";
    let mut p = project(text, &[INT]);
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, &[("pick", "()I", 0x101, None)]))
        .unwrap();
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    let mut p = project(text, &[INT]);
    p.add_class_bytes("Api.class", &binary("lib/Api", Some("java/lang/Object"), 0x601, &[INT])).unwrap();
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    let p = project(text, &[INT, ("pick", "(I)J", 0x109, None)]);
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
}
#[test]
fn local_type_variable_and_parenthesized_qualifiers_do_not_become_types() {
    for text in [
        "import lib.Api; class Calls {void run(int Api){Api.pick(1);}}",
        "import lib.Api; class Calls {void run(Api Api){Api.pick(1);}}",
        "import lib.Api; class Calls {<Api> void run(){Api.pick(1);}}",
        "import lib.Api; class Calls<Api> {void run(){Api.pick(1);}}",
        "import lib.Api; class Calls {void run(){(Api).pick(1);}}",
    ] {
        assert_ne!(query(&project(text, &[INT]), "pick(1)").status, ResolveStatus::Resolved, "{text}");
    }
    for body in ["int Api=0; box.take(Api.pick(1));", "box.take((Api).pick(1));"] {
        let text = format!(
            "import lib.Api; class Box {{public void take(int i){{}}}} class Calls {{void run(Box box){{{body}}}}}"
        );
        let p = project(&text, &[INT]);
        assert_ne!(
            query(&p, if body.contains("(Api)") { "take((Api).pick(1))" } else { "take(Api.pick(1))" }).status,
            ResolveStatus::Resolved
        );
    }
}
#[test]
fn field_shadows_cannot_supply_nested_static_return_evidence() {
    for field in ["int Api;", "lib.Api Api;"] {
        let text = format!(
            "import lib.Api; class Box {{public void take(int i){{}}}} class Calls {{{field} void run(Box box){{box.take(Api.pick(1));}}}}"
        );
        assert_ne!(query(&project(&text, &[INT]), "take(Api.pick(1))").status, ResolveStatus::Resolved);
    }
}
#[test]
fn static_parameter_slot_limit_and_work_budget_are_bounded() {
    for count in [255, 256] {
        let descriptor = format!("({})I", "I".repeat(count));
        let args = vec!["0"; count].join(",");
        let text = format!("import lib.Api; class Calls {{void run(){{Api.pick({args});}}}}");
        let mut p = project(&text, &[INT]);
        let admitted = p.add_class_bytes(
            "Api.class",
            &binary("lib/Api", Some("java/lang/Object"), 0x21, &[("pick", &descriptor, 0x109, None)]),
        );
        if count == 256 && admitted.is_err() {
            continue;
        }
        admitted.unwrap();
        assert_eq!(query(&p, &format!("pick({args})")).status == ResolveStatus::Resolved, count == 255, "{count}");
    }
    for count in [0, 5000] {
        let filler = (0..count).map(|i| format!("int unused{i};")).collect::<String>();
        let text = format!("import lib.Api; class Calls {{void run(){{{filler} Api.pick(1);}}}}");
        assert_eq!(query(&project(&text, &[INT]), "pick(1)").status == ResolveStatus::Resolved, count == 0);
    }
}
#[test]
fn same_binary_name_in_separate_compilations_keeps_static_result_identity() {
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
        "static-scope-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let text = "import lib.Api; class Calls {void run(){Api.pick(1);}}";
    let mut configs = Vec::new();
    for (name, method) in [("a", INT), ("b", LONG)] {
        let src = root.join(name).join("src");
        let bin = root.join(name).join("bin");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(bin.join("lib")).unwrap();
        fs::create_dir_all(bin.join("java/lang")).unwrap();
        fs::write(src.join("Calls.java"), text).unwrap();
        fs::write(bin.join("java/lang/Object.class"), binary("java/lang/Object", None, 0x21, &[])).unwrap();
        fs::write(bin.join("lib/Api.class"), binary("lib/Api", Some("java/lang/Object"), 0x21, &[method])).unwrap();
        configs.push(CompilationConfig {
            id: CompilationId::new(name, "main"),
            inputs: ProjectConfig {
                source_roots: vec![src],
                classpath: vec![ClasspathEntry::Directory(bin)],
                ..Default::default()
            },
            dependencies: vec![],
        });
    }
    let a = AnalysisSnapshot::new(
        WorkspaceSnapshot::load(&WorkspaceConfig { compilations: configs, ..Default::default() }).unwrap(),
    );
    for (name, desc) in [("a", "(I)I"), ("b", "(J)J")] {
        let path = root.join(name).join("src/Calls.java").canonicalize().unwrap();
        let r = a
            .scope(&CompilationId::new(name, "main"))
            .unwrap()
            .resolve_at(path.to_str().unwrap(), text.find("pick(1)").unwrap() as u32);
        assert_eq!(r.status, ResolveStatus::Resolved);
        assert_eq!(r.targets.len(), 1);
        assert_eq!(r.targets[0].binary.as_ref().unwrap().descriptor, desc);
        assert_eq!(
            r.targets[0].path,
            root.join(name).join("bin/lib/Api.class").canonicalize().unwrap().to_str().unwrap()
        );
    }
}
