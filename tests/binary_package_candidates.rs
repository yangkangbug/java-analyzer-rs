use java_engine::{Project, ResolveResult, ResolveStatus};

type Method<'a> = (&'a str, u16, Option<&'a str>);
// Native declaration signatures, assembled without a JVM.
fn binary(name: &str, parent: Option<&str>, major: u16, class_flags: u16, methods: &[Method<'_>]) -> Vec<u8> {
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
    fn class(b: &mut Vec<u8>, n: &mut u16, s: &str) -> u16 {
        let t = utf(b, n, s);
        let i = *n;
        *n += 1;
        b.push(7);
        u(b, t);
        i
    }
    let mut cp = Vec::new();
    let mut n = 1;
    let this = class(&mut cp, &mut n, name);
    let parent = parent.map(|p| class(&mut cp, &mut n, p)).unwrap_or(0);
    let method_name = utf(&mut cp, &mut n, "pick");
    let signature_name = utf(&mut cp, &mut n, "Signature");
    let records: Vec<_> =
        methods.iter().map(|(d, f, s)| (*f, utf(&mut cp, &mut n, d), s.map(|s| utf(&mut cp, &mut n, s)))).collect();
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0];
    u(&mut b, major);
    u(&mut b, n);
    b.extend(cp);
    for v in [class_flags, this, parent, 0, 0, records.len() as u16] {
        u(&mut b, v);
    }
    for (flags, desc, sig) in records {
        for v in [flags, method_name, desc, if sig.is_some() { 1 } else { 0 }] {
            u(&mut b, v);
        }
        if let Some(sig) = sig {
            u(&mut b, signature_name);
            b.extend(2u32.to_be_bytes());
            u(&mut b, sig);
        }
    }
    u(&mut b, 0);
    b
}
fn project(source: &str, methods: &[Method<'_>]) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", source);
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 61, 0x21, &[])).unwrap();
    p.add_class_bytes("Api.class", &binary("lib/Api", Some("java/lang/Object"), 61, 0x21, methods)).unwrap();
    p
}
fn source(package: &str) -> String {
    format!("{package} import lib.Api; class Calls {{ void run(Api api,int value) {{ api.pick(value); }} }}")
}
fn query(p: &Project) -> ResolveResult {
    let at = p.file("Calls.java").unwrap().text().rfind("pick(value)").unwrap() as u32;
    let r = p.resolve_at("Calls.java", at);
    assert_eq!(r, p.prepare().resolve_at("Calls.java", at));
    r
}
const PUBLIC: Method<'static> = ("(J)V", 0x0101, None);
const PACKAGE: Method<'static> = ("(I)V", 0x0100, None);
const ARRAY_HELPER: Method<'static> = ("([BI)V", 0x0108, None);
#[test]
fn foreign_package_exclusion_preserves_all_public_competitors() {
    for package in ["package client;", "package lib.child;", ""] {
        let r = query(&project(&source(package), &[PUBLIC, PACKAGE, ARRAY_HELPER]));
        assert_eq!(r.status, ResolveStatus::Resolved, "{package}: {r:?}");
        assert_eq!(r.targets.len(), 1);
        let t = &r.targets[0];
        assert_eq!(t.owner, "lib.Api");
        assert_eq!(t.path, "Api.class");
        assert_eq!(t.binary.as_ref().unwrap().descriptor, "(J)V");
    }
}
#[test]
fn same_package_private_protected_and_unknown_candidates_still_refuse() {
    assert_ne!(query(&project(&source("package lib;"), &[PUBLIC, PACKAGE])).status, ResolveStatus::Resolved);
    for helper in [
        ("(I)V", 0x0102, None),
        ("(I)V", 0x0104, None),
        ("([I)V", 0x0101, None),
        ("(I)V", 0x0109, None),
        ("(I)V", 0x0140, None),
        ("(I)V", 0x1100, None),
        ("([I)V", 0x0180, None),
        ("(I)V", 0x0400, None),
        ("(I)V", 0x0100, Some("(I)V")),
    ] {
        assert_ne!(
            query(&project(&source("package client;"), &[PUBLIC, ARRAY_HELPER, helper])).status,
            ResolveStatus::Resolved,
            "{helper:?}"
        );
    }
}
#[test]
fn malformed_or_duplicate_inventory_cannot_be_hidden_by_access() {
    for helper in [
        ("(I)Q", 0x0100, None),
        ("(V)V", 0x0100, None),
        ("(I)Vextra", 0x0100, None),
        ("(I)V", 0x0103, None),
        ("(I)V", 0x2100, None),
        ("(J)I", 0x0100, None),
        ("(J)I", 0x0108, None),
    ] {
        let mut p = project(&source("package client;"), &[PUBLIC]);
        let added =
            p.add_class_bytes("Api.class", &binary("lib/Api", Some("java/lang/Object"), 61, 0x21, &[PUBLIC, helper]));
        // Malformed descriptors can be rejected at class admission; duplicate
        // parameter lists with distinct returns must reach the selector guard.
        if helper.0.starts_with("(J)") {
            added.unwrap();
        } else if added.is_err() {
            continue;
        }
        assert_ne!(query(&p).status, ResolveStatus::Resolved, "{helper:?}");
    }
}
#[test]
fn excluded_static_and_instance_slot_limits_and_versions_are_checked() {
    for (count, flags, expected) in [(255, 0x0108, true), (255, 0x0100, false), (256, 0x0108, false)] {
        let descriptor = format!("({})V", "I".repeat(count));
        let methods = [PUBLIC, (&descriptor, flags, None)];
        let mut p = project(&source("package client;"), &[PUBLIC]);
        let added = p.add_class_bytes("Api.class", &binary("lib/Api", Some("java/lang/Object"), 61, 0x21, &methods));
        if !expected && added.is_err() {
            continue;
        }
        added.unwrap();
        assert_eq!(query(&p).status == ResolveStatus::Resolved, expected, "{count} {flags}");
    }
    for (major, expected) in [(60, true), (61, false)] {
        let mut p = project(&source("package client;"), &[PUBLIC]);
        p.add_class_bytes(
            "Api.class",
            &binary("lib/Api", Some("java/lang/Object"), major, 0x21, &[PUBLIC, ("(I)V", 0x0808, None)]),
        )
        .unwrap();
        assert_eq!(query(&p).status == ResolveStatus::Resolved, expected, "major{major}");
    }
}
#[test]
fn inherited_same_name_and_unknown_caller_context_are_not_new_paths() {
    let mut p = project(&source("package client;"), &[PUBLIC, PACKAGE]);
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 61, 0x21, &[("()V", 0x0101, None)])).unwrap();
    assert_ne!(query(&p).status, ResolveStatus::Resolved);
    for text in [
        "package lib; package client; import lib.Api; class Calls { void run(Api api,int value){api.pick(value);} }",
        "package client; import lib.Api; class Calls { class Nested { void run(Api api,int value){api.pick(value);} } }",
    ] {
        assert_ne!(query(&project(text, &[PUBLIC, PACKAGE])).status, ResolveStatus::Resolved);
    }
}
#[test]
fn no_exclusion_needed_preserves_existing_abstract_method_proof() {
    let mut p = project(&source("package client;"), &[PUBLIC]);
    p.add_class_bytes("Api.class", &binary("lib/Api", Some("java/lang/Object"), 61, 0x0421, &[("(J)V", 0x0401, None)]))
        .unwrap();
    assert_eq!(query(&p).status, ResolveStatus::Resolved);
}

#[test]
fn long_shared_prefix_comparisons_consume_the_same_work_budget() {
    for count in [1, 8] {
        let names: Vec<_> = (0..count).map(|i| format!("pkg/{}{i}", "Common".repeat(45))).collect();
        let descriptors: Vec<_> = names.iter().map(|n| format!("(L{n};)V")).collect();
        let mut methods = vec![PUBLIC];
        methods.extend(descriptors.iter().map(|s| (s.as_str(), 0x0100, None)));
        let mut p = project(&source("package client;"), &methods);
        for (i, name) in names.iter().enumerate() {
            p.add_class_bytes(format!("Type{i}.class"), &binary(name, Some("java/lang/Object"), 61, 0x21, &[]))
                .unwrap();
        }
        assert_eq!(query(&p).status == ResolveStatus::Resolved, count == 1, "count{count}");
    }
}
#[test]
fn same_binary_names_in_separate_compilations_keep_their_own_inventory() {
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
        "package-inventory-{}-{}",
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
    let text = source("package client;");
    let mut compilations = Vec::new();
    for name in ["a", "b"] {
        let src = root.join(name).join("src");
        let bin = root.join(name).join("bin");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(bin.join("lib")).unwrap();
        fs::create_dir_all(bin.join("java/lang")).unwrap();
        fs::write(src.join("Calls.java"), &text).unwrap();
        fs::write(bin.join("java/lang/Object.class"), binary("java/lang/Object", None, 61, 0x21, &[])).unwrap();
        let methods = if name == "a" { [PUBLIC, PACKAGE] } else { [("(I)V", 0x0101, None), ("(J)V", 0x0100, None)] };
        fs::write(bin.join("lib/Api.class"), binary("lib/Api", Some("java/lang/Object"), 61, 0x21, &methods)).unwrap();
        compilations.push(CompilationConfig {
            id: CompilationId::new(name, "main"),
            inputs: ProjectConfig {
                source_roots: vec![src],
                classpath: vec![ClasspathEntry::Directory(bin)],
                ..Default::default()
            },
            dependencies: vec![],
        });
    }
    let analysis = AnalysisSnapshot::new(
        WorkspaceSnapshot::load(&WorkspaceConfig { compilations, ..Default::default() }).unwrap(),
    );
    for (name, expected) in [("a", "(J)V"), ("b", "(I)V")] {
        let path = root.join(name).join("src/Calls.java").canonicalize().unwrap();
        let id = CompilationId::new(name, "main");
        let r =
            analysis.scope(&id).unwrap().resolve_at(path.to_str().unwrap(), text.rfind("pick(value)").unwrap() as u32);
        assert_eq!(r.status, ResolveStatus::Resolved, "{name}: {r:?}");
        assert_eq!(r.targets.len(), 1);
        assert_eq!(r.targets[0].binary.as_ref().unwrap().descriptor, expected);
        assert_eq!(
            r.targets[0].path,
            root.join(name).join("bin/lib/Api.class").canonicalize().unwrap().to_str().unwrap()
        );
    }
}

#[test]
fn switch_isolation_preserves_package_competitor_exclusion_boundary() {
    let sw = "switch(1){case 1:break;}";
    let healthy = format!(
        "package client;import lib.Api;class Calls{{void run(Api api,int value){{api.pick(value);}}void other(){{{sw}}}}}"
    );
    let r = query(&project(&healthy, &[PUBLIC, PACKAGE, ARRAY_HELPER]));
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].binary.as_ref().unwrap().descriptor, "(J)V");
    for s in [
        format!("package client;import lib.Api;class Calls{{void run(Api api,int value){{api.pick(value);{sw}}}}}"),
        healthy.replace("case 1:break;", "case 1:int broken=;"),
        healthy.replace("package client;", "package lib;"),
    ] {
        assert_ne!(query(&project(&s, &[PUBLIC, PACKAGE, ARRAY_HELPER])).status, ResolveStatus::Resolved, "{s}");
    }
    // A rootless field receiver already had this legacy name-navigation
    // result in the published baseline. It is not strict applicability evidence.
    let rootless = format!(
        "package client;import lib.Api;class Calls{{int value;Api api;Object field=api.pick(value);void other(){{{sw}}}}}"
    );
    let legacy = query(&project(&rootless, &[PUBLIC, PACKAGE, ARRAY_HELPER]));
    assert_eq!(legacy.status, ResolveStatus::Resolved);
    assert_eq!(legacy.targets.len(), 1);
    assert_eq!(legacy.targets[0].binary.as_ref().unwrap().descriptor, "(J)V");
    for bad in [("(I)V", 0x102, None), ("(I)V", 0x104, None), ("(I)V", 0x100, Some("<T:Ljava/lang/Object;>(I)V"))] {
        assert_ne!(query(&project(&healthy, &[PUBLIC, bad])).status, ResolveStatus::Resolved);
    }
}
