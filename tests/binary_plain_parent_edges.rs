use java_engine::{Project, ResolveResult, ResolveStatus};

fn binary(name: &str, parent: Option<&str>, interfaces: &[&str], flags: u16, signature: Option<&str>) -> Vec<u8> {
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
    let interfaces: Vec<_> = interfaces.iter().map(|s| cls(&mut cp, &mut n, s)).collect();
    let attribute = signature.map(|s| (utf(&mut cp, &mut n, "Signature"), utf(&mut cp, &mut n, s)));
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61];
    u(&mut b, n);
    b.extend(cp);
    for v in [flags, this, parent, interfaces.len() as u16] {
        u(&mut b, v);
    }
    for v in interfaces {
        u(&mut b, v);
    }
    for v in [0, 0, if attribute.is_some() { 1 } else { 0 }] {
        u(&mut b, v);
    }
    if let Some((name, value)) = attribute {
        u(&mut b, name);
        b.extend(2u32.to_be_bytes());
        u(&mut b, value);
    }
    b
}
const GOOD: &str = "Ljava/lang/Object;Llib/Marker;Llib/Gen<Ljava/lang/Object;>;";
fn inputs(signature: &str) -> Vec<(String, Vec<u8>)> {
    vec![
        ("java/lang/Object".into(), binary("java/lang/Object", None, &[], 0x21, None)),
        ("lib/Marker".into(), binary("lib/Marker", Some("java/lang/Object"), &[], 0x601, None)),
        ("lib/Other".into(), binary("lib/Other", Some("java/lang/Object"), &[], 0x601, None)),
        (
            "lib/Gen".into(),
            binary(
                "lib/Gen",
                Some("java/lang/Object"),
                &["lib/Other"],
                0x601,
                Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Llib/Other;"),
            ),
        ),
        (
            "lib/Value".into(),
            binary("lib/Value", Some("java/lang/Object"), &["lib/Marker", "lib/Gen"], 0x21, Some(signature)),
        ),
    ]
}
fn project(signature: &str, methods: &str) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java",&format!("import lib.Value; import lib.Marker; import lib.Other; class Api {{ {methods} }} class Calls {{ void run(Api api, Value value) {{ api.pick(value); }} }}"));
    for (n, b) in inputs(signature) {
        p.add_class_bytes(format!("{n}.class"), &b).unwrap();
    }
    p
}
fn query(p: &Project) -> ResolveResult {
    let text = p.file("Calls.java").unwrap().text();
    let at = text.rfind("pick(value)").unwrap() as u32;
    let r = p.resolve_at("Calls.java", at);
    assert_eq!(r, p.prepare().resolve_at("Calls.java", at));
    r
}
fn winner(p: &Project, decl: &str) {
    let r = query(p);
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].owner, "Api");
    assert_eq!(r.targets[0].path, "Calls.java");
    assert_eq!(r.targets[0].start_byte, p.file("Calls.java").unwrap().text().find(decl).unwrap() as u32);
}
#[test]
fn plain_super_and_interface_edges_enable_conversion_and_specificity() {
    winner(&project(GOOD, "public void pick(Object x){}"), "pick(Object x)");
    winner(&project(GOOD, "public void pick(Marker x){} public void pick(Object x){}"), "pick(Marker x)");
}
#[test]
fn malformed_or_mismatched_signature_never_supplies_a_plain_edge() {
    for sig in [
        "",
        "Ljava/lang/Object;Llib/Marker;", // omitted interface
        "Ljava/lang/Object;Llib/Gen<Ljava/lang/Object;>;Llib/Marker;", // reordered
        "Ljava/lang/Object;Llib/Other;Llib/Gen<Ljava/lang/Object;>;",
        "Ljava/lang/Object;Llib/Marker;Llib/Gen<Ljava/lang/Object;>;junk",
        "Ljava/lang/Object;Llib/Marker;Llib/Gen<>;",
        "Ljava/lang/Object;Llib/Marker;Llib/Gen<Ljava/lang/Object;>",
        "Ljava/lang/Object;Llib/Marker;Llib/Gen<TT;>;",
        "Ljava/lang/Object;Llib/Marker;Llib/Gen<*>;",
        "Ljava/lang/Object;Llib/Marker;Llib/Gen<[Ljava/lang/Object;>;",
        "Ljava/lang/Object;Llib/Marker;Llib/Gen<Lpkg/Outer.Inner;>;",
        "<T:Ljava/lang/Object;>Ljava/lang/Object;Llib/Marker;Llib/Gen<TT;>;",
    ] {
        assert_ne!(query(&project(sig, "public void pick(Object x){}")).status, ResolveStatus::Resolved, "{sig}");
    }
}
#[test]
fn unknown_parameterized_path_cannot_eliminate_a_competing_overload() {
    // Value -> Gen<...> -> Other remains unknown, NOT false. Choosing Object
    // merely because its independent conversion succeeds would be unsound.
    for methods in ["public void pick(Other x){}", "public void pick(Object x){} public void pick(Other x){}"] {
        assert_ne!(query(&project(GOOD, methods)).status, ResolveStatus::Resolved);
    }
}
#[test]
fn unvalidated_generic_arity_does_not_certify_that_path() {
    // This contract checks framing/raw-edge consistency, not generic arity/bounds.
    // The unrelated Object edge is still proved; Gen/Other remains unknown.
    let sig = "Ljava/lang/Object;Llib/Marker;Llib/Gen<Ljava/lang/Object;Ljava/lang/Object;>;";
    winner(&project(sig, "public void pick(Object x){}"), "pick(Object x)");
    assert_ne!(
        query(&project(sig, "public void pick(Object x){} public void pick(Other x){}")).status,
        ResolveStatus::Resolved
    );
}
#[test]
fn signature_work_and_nested_argument_depth_are_bounded() {
    for count in [1, 500] {
        let sig = format!("Ljava/lang/Object;Llib/Marker;Llib/Gen<{}>;", "Ljava/lang/Object;".repeat(count));
        assert_eq!(query(&project(&sig, "public void pick(Object x){}")).status == ResolveStatus::Resolved, count == 1);
    }
    for depth in [2, 40] {
        let mut arg = "Ljava/lang/Object;".to_string();
        for _ in 0..depth {
            arg = format!("Llib/Gen<{arg}>;");
        }
        let sig = format!("Ljava/lang/Object;Llib/Marker;Llib/Gen<{arg}>;");
        assert_eq!(query(&project(&sig, "public void pick(Object x){}")).status == ResolveStatus::Resolved, depth == 2);
    }
}
#[test]
fn catch_qualification_uses_only_the_proven_throwable_path() {
    let mut p = project(GOOD, "public void pick(Object x){} public void pick(Throwable x){}");
    for (name, parent, sig, interfaces) in [
        ("java/lang/Throwable", "java/lang/Object", None, vec![]),
        ("java/lang/RuntimeException", "java/lang/Throwable", None, vec![]),
        (
            "lib/E",
            "java/lang/RuntimeException",
            Some("Ljava/lang/RuntimeException;Llib/Gen<Ljava/lang/Object;>;"),
            vec!["lib/Gen"],
        ),
    ] {
        p.add_class_bytes(format!("{name}.class"), &binary(name, Some(parent), &interfaces, 0x21, sig)).unwrap();
    }
    p.insert("Calls.java","import lib.E; class Api { public void pick(Object x){} public void pick(Throwable x){} } class Calls { void run(Api api){try{}catch(E value){api.pick(value);}}}");
    winner(&p, "pick(Throwable x)");
}
#[test]
fn identical_binary_names_in_distinct_compilations_do_not_share_certificates() {
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
        "plain-parent-{}-{}",
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
    let text = "import lib.Value; class Api { public void pick(Object x){} } class Calls { void run(Api api,Value value){api.pick(value);} }";
    let mut compilations = Vec::new();
    for (name, sig) in [("good", GOOD), ("bad", "Ljava/lang/Object;Llib/Marker;")] {
        let src = root.join(name).join("src");
        let bin = root.join(name).join("bin");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("Calls.java"), text).unwrap();
        for (n, bytes) in inputs(sig) {
            let file = bin.join(format!("{n}.class"));
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, bytes).unwrap();
        }
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
    for name in ["good", "bad"] {
        let path = root.join(name).join("src/Calls.java").canonicalize().unwrap();
        let r = analysis
            .scope(&CompilationId::new(name, "main"))
            .unwrap()
            .resolve_at(path.to_str().unwrap(), text.rfind("pick(value)").unwrap() as u32);
        assert_eq!(r.status == ResolveStatus::Resolved, name == "good", "{name}: {r:?}");
        if name == "good" {
            assert_eq!(r.targets.len(), 1);
            assert_eq!(r.targets[0].path, path.to_str().unwrap());
        }
    }
}

#[test]
fn raw_generic_actual_and_generic_intermediate_parent_remain_unsupported() {
    // A raw actual of a class declaring T must not borrow its erased Object edge.
    let mut p = project(GOOD, "public void pick(Object x){}");
    p.add_class_bytes(
        "lib/Value.class",
        &binary(
            "lib/Value",
            Some("java/lang/Object"),
            &["lib/Marker", "lib/Gen"],
            0x21,
            Some("<T:Ljava/lang/Object;>Ljava/lang/Object;Llib/Marker;Llib/Gen<TT;>;"),
        ),
    )
    .unwrap();
    assert_ne!(query(&p).status, ResolveStatus::Resolved);

    // Even a syntactically plain raw superclass name does not supply a
    // substitution environment through a generic intermediate declaration.
    let mut p = project(GOOD, "public void pick(Object x){}");
    p.add_class_bytes(
        "lib/Mid.class",
        &binary("lib/Mid", Some("java/lang/Object"), &[], 0x21, Some("<T:Ljava/lang/Object;>Ljava/lang/Object;")),
    )
    .unwrap();
    p.add_class_bytes("lib/Value.class", &binary("lib/Value", Some("lib/Mid"), &[], 0x21, Some("Llib/Mid;"))).unwrap();
    assert_ne!(query(&p).status, ResolveStatus::Resolved);
}

#[test]
fn malformed_catch_signature_cannot_supply_throwable_qualification() {
    for signature in [
        "Ljava/lang/RuntimeException;Llib/Gen<Ljava/lang/Object;>;junk",
        "Ljava/lang/RuntimeException;", // raw interface missing
        "Llib/Gen<Ljava/lang/Object;>;Ljava/lang/RuntimeException;", // wrong order
        "Ljava/lang/RuntimeException;Llib/Gen<>;",
    ] {
        let mut p = project(GOOD, "public void pick(Object x){}");
        for (name, parent) in
            [("java/lang/Throwable", "java/lang/Object"), ("java/lang/RuntimeException", "java/lang/Throwable")]
        {
            p.add_class_bytes(format!("{name}.class"), &binary(name, Some(parent), &[], 0x21, None)).unwrap();
        }
        p.add_class_bytes(
            "lib/E.class",
            &binary("lib/E", Some("java/lang/RuntimeException"), &["lib/Gen"], 0x21, Some(signature)),
        )
        .unwrap();
        p.insert(
            "Calls.java",
            "import lib.E; class Api { public void pick(Object x){} public void pick(Throwable x){} } class Calls { void run(Api api){try{}catch(E value){api.pick(value);}}}",
        );
        assert_ne!(query(&p).status, ResolveStatus::Resolved, "{signature}");
    }
}
