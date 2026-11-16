use java_engine::{Project, ResolveResult, ResolveStatus, TargetKind};

// Hand-assembled native signatures: no runtime JVM.
fn class(name: &str, parent: Option<&str>) -> Vec<u8> {
    fn u(b: &mut Vec<u8>, n: u16) {
        b.extend(n.to_be_bytes());
    }
    fn utf(b: &mut Vec<u8>, s: &str) {
        b.push(1);
        u(b, s.len() as u16);
        b.extend(s.as_bytes());
    }
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61];
    u(&mut b, if parent.is_some() { 5 } else { 3 });
    utf(&mut b, name);
    b.push(7);
    u(&mut b, 1);
    if let Some(parent) = parent {
        utf(&mut b, parent);
        b.push(7);
        u(&mut b, 3);
    }
    for n in [0x21, 2, if parent.is_some() { 4 } else { 0 }, 0, 0, 0, 0] {
        u(&mut b, n);
    }
    b
}
fn project(source: &str) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", source);
    for (name, parent) in
        [("Object", None), ("Throwable", Some("java/lang/Object")), ("RuntimeException", Some("java/lang/Throwable"))]
    {
        p.add_class_bytes(format!("{name}.class"), &class(&format!("java/lang/{name}"), parent)).unwrap();
    }
    p
}
fn query(p: &Project, marker: &str) -> ResolveResult {
    let at = p.file("Calls.java").unwrap().text().rfind(marker).unwrap() as u32;
    let r = p.resolve_at("Calls.java", at);
    assert_eq!(r, p.prepare().resolve_at("Calls.java", at));
    r
}
const TYPES: &str = "class Box { public void pick(int x){} public void pick(long x){} public void take(E e){} public void take(Object e){} } class E extends RuntimeException { public Box box(){return null;} } class F extends RuntimeException { public Box box(){return null;} }";
#[test]
fn single_catch_reference_drives_return_chain_and_actual_selection() {
    for header in ["E e", "final E e"] {
        let source = format!(
            "{TYPES} class Calls {{ void run(Box box) {{ try {{}} catch({header}) {{ e.box().pick(1); box.take(e); }} }} }}"
        );
        let p = project(&source);
        assert!(p.file("Calls.java").unwrap().errors().is_empty());
        for (marker, decl) in [("pick(1)", "pick(int x)"), ("take(e)", "take(E e)")] {
            let r = query(&p, marker);
            assert_eq!(r.status, ResolveStatus::Resolved, "{header}: {r:?}");
            assert_eq!(r.targets.len(), 1);
            let t = &r.targets[0];
            assert_eq!(t.kind, TargetKind::Method);
            assert_eq!(t.owner, "Box");
            assert_eq!(t.path, "Calls.java");
            assert_eq!(t.start_byte, source.find(decl).unwrap() as u32);
        }
    }
}
#[test]
fn unmodeled_or_malformed_catch_headers_refuse_new_chain_proof() {
    for header in [
        "E | F e",
        "@Tag E e",
        "E[] e",
        "E e[]",
        "E<T> e",
        "public E e",
        "final final E e",
        "E e, F f",
        "E e = value",
        "E | e",
    ] {
        let source =
            format!("{TYPES} class Calls {{ void run() {{ try {{}} catch({header}) {{ e.box().pick(1); }} }} }}");
        assert_ne!(query(&project(&source), "pick(1)").status, ResolveStatus::Resolved, "{header}");
    }
}
#[test]
fn non_throwable_or_incomplete_hierarchy_never_borrows_catch_evidence() {
    for declaration in [
        "class E { public Box box(){return null;} }",
        "class E extends Missing { public Box box(){return null;} }",
        "class E<T> extends RuntimeException { public Box box(){return null;} }",
    ] {
        let source = format!(
            "class Box {{ public void pick(int x){{}} public void pick(long x){{}} }} {declaration} class Calls {{ void run() {{ try {{}} catch(E e) {{ e.box().pick(1); }} }} }}"
        );
        assert_ne!(query(&project(&source), "pick(1)").status, ResolveStatus::Resolved, "{declaration}");
    }
    let source = format!("{TYPES} class Calls {{ void run() {{ try {{}} catch(E e) {{ e.box().pick(1); }} }} }}");
    let mut p = project(&source);
    p.remove_class("Throwable.class");
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
}
#[test]
fn catch_scope_and_captured_calls_stay_separate() {
    for body in [
        "try {} catch(E e) {} e.box().pick(1);",
        "try {} catch(E e) {} finally { e.box().pick(1); }",
        "try {} catch(E e) { Runnable r=()->{ e.box().pick(1); }; }",
        "try {} catch(E e) { E e=null; e.box().pick(1); }",
    ] {
        let source = format!("{TYPES} class Calls {{ void run() {{ {body} }} }}");
        assert_ne!(query(&project(&source), "pick(1)").status, ResolveStatus::Resolved, "{body}");
    }
}

#[test]
fn catch_qualification_rejects_non_throwable_actuals_and_interface_types() {
    for declaration in ["class E {}", "interface E {}"] {
        let source = format!(
            "class Box {{ public void take(E e){{}} public void take(Object e){{}} }} {declaration} class Calls {{ void run(Box box) {{ try {{}} catch(E e) {{ box.take(e); }} }} }}"
        );
        assert_ne!(query(&project(&source), "take(e)").status, ResolveStatus::Resolved, "{declaration}");
    }
}
#[test]
fn sequential_catches_keep_their_own_parameter_identity() {
    let source = "class Box { public void take(E e){} public void take(F e){} public void take(Object e){} } class E extends RuntimeException {} class F extends RuntimeException {} class Calls { void run(Box box) { try {} catch(E e) { box.take(e); } try {} catch(F e) { box.take(e); } } }";
    let p = project(source);
    for (at, decl) in source.match_indices("take(e)").zip(["take(E e)", "take(F e)"]) {
        let r = p.resolve_at("Calls.java", at.0 as u32);
        assert_eq!(r.status, ResolveStatus::Resolved);
        assert_eq!(r.targets.len(), 1);
        assert_eq!(r.targets[0].start_byte, source.find(decl).unwrap() as u32);
    }
}
#[test]
fn catch_reference_uses_shared_work_and_hierarchy_depth_limits() {
    for count in [0, 5000] {
        let filler = (0..count).map(|i| format!("int unused{i};")).collect::<String>();
        let source =
            format!("{TYPES} class Calls {{ void run() {{ {filler} try {{}} catch(E e) {{ e.box().pick(1); }} }} }}");
        assert_eq!(query(&project(&source), "pick(1)").status == ResolveStatus::Resolved, count == 0);
    }
    for count in [2, 40] {
        let parents = (0..count)
            .map(|i| {
                format!(
                    "class Parent{i} extends {} {{}}",
                    if i + 1 == count { "RuntimeException".into() } else { format!("Parent{}", i + 1) }
                )
            })
            .collect::<String>();
        let source = format!(
            "class Box {{ public void pick(int x){{}} public void pick(long x){{}} }} {parents} class E extends Parent0 {{ public Box box(){{return null;}} }} class Calls {{ void run() {{ try {{}} catch(E e) {{ e.box().pick(1); }} }} }}"
        );
        assert_eq!(query(&project(&source), "pick(1)").status == ResolveStatus::Resolved, count == 2);
    }
}
#[test]
fn throwable_identity_is_local_to_each_compilation() {
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
        "catch-scopes-{}-{}",
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
    let source = format!("{TYPES} class Calls {{ void run() {{ try {{}} catch(E e) {{ e.box().pick(1); }} }} }}");
    let mut compilations = Vec::new();
    for name in ["good", "shadowed"] {
        let src = root.join(name).join("src");
        let bin = root.join(name).join("bin/java/lang");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&bin).unwrap();
        fs::write(src.join("Calls.java"), &source).unwrap();
        for (n, parent) in [
            ("Object", None),
            ("Throwable", Some("java/lang/Object")),
            ("RuntimeException", Some("java/lang/Throwable")),
        ] {
            fs::write(bin.join(format!("{n}.class")), class(&format!("java/lang/{n}"), parent)).unwrap();
        }
        if name == "shadowed" {
            fs::write(src.join("Throwable.java"), "package java.lang; public class Throwable {}").unwrap();
        }
        compilations.push(CompilationConfig {
            id: CompilationId::new(name, "main"),
            inputs: ProjectConfig {
                source_roots: vec![src],
                classpath: vec![ClasspathEntry::Directory(root.join(name).join("bin"))],
                ..Default::default()
            },
            dependencies: vec![],
        });
    }
    let analysis = AnalysisSnapshot::new(
        WorkspaceSnapshot::load(&WorkspaceConfig { compilations, ..Default::default() }).unwrap(),
    );
    for name in ["good", "shadowed"] {
        let path = root.join(name).join("src/Calls.java").canonicalize().unwrap();
        let id = CompilationId::new(name, "main");
        let r =
            analysis.scope(&id).unwrap().resolve_at(path.to_str().unwrap(), source.rfind("pick(1)").unwrap() as u32);
        assert_eq!(r.status == ResolveStatus::Resolved, name == "good", "{name}: {r:?}");
        if name == "good" {
            assert_eq!(r.targets[0].path, path.to_str().unwrap());
        }
    }
}
