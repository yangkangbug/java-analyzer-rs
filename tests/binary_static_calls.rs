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
    fn check(text: &str) {
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
    check("import lib.Api; class Calls {void run(){Api.pick(1);}}");
    check("import static lib.Api.pick; class Calls {void run(){pick(1);}}");
}

#[test]
fn boolean_literal_binary_static_overloads_keep_exact_descriptors() {
    const BOOLEAN: Method<'static> = ("pick", "(Z)Z", 0x109, None);
    for value in ["true", "false", "((true))"] {
        let s = format!("import lib.Api; class Calls{{void run(){{Api.pick({value});}}}}");
        selected(&project(&s, &[BOOLEAN, INT, LONG]), &format!("pick({value})"), "lib.Api", "(Z)Z");
        assert_ne!(query(&project(&s, &[INT, LONG]), &format!("pick({value})")).status, ResolveStatus::Resolved);
    }
    let s = "import lib.Api; class Calls{void run(){Api.pick(Api.pick(true));}}";
    selected(&project(s, &[BOOLEAN, INT, LONG]), "pick(Api.pick(true))", "lib.Api", "(Z)Z");
}
#[test]
fn boolean_literal_edits_keep_prepared_versions_and_fresh_selection() {
    use java_engine::TextEdit;
    const BOOLEAN: Method<'static> = ("pick", "(Z)Z", 0x109, None);
    let s = "import lib.Api; class Calls{void run(){Api.pick(true);}}";
    let mut p = project(s, &[BOOLEAN, INT, LONG]);
    let old = p.clone();
    let old_prepared = old.prepare();
    let at = s.find("true").unwrap();
    p.edit("Calls.java", TextEdit { start_byte: at as u32, end_byte: (at + 4) as u32, insert: "1".into() }).unwrap();
    selected(&p, "pick(1)", "lib.Api", "(I)I");
    let changed = p.file("Calls.java").unwrap().text();
    let fresh = project(changed, &[BOOLEAN, INT, LONG]);
    assert_eq!(query(&p, "pick(1)"), query(&fresh, "pick(1)"));
    let before = old_prepared.resolve_at("Calls.java", s.find("pick(true)").unwrap() as u32);
    assert_eq!(before.status, ResolveStatus::Resolved);
    assert_eq!(before.targets[0].binary.as_ref().unwrap().descriptor, "(Z)Z");
    p.edit("Calls.java", TextEdit { start_byte: at as u32, end_byte: (at + 1) as u32, insert: "false".into() })
        .unwrap();
    selected(&p, "pick(false)", "lib.Api", "(Z)Z");
}

#[test]
fn explicit_static_import_calls_reuse_exact_binary_selection() {
    for prefix in ["import static lib.Api.pick;", "import static lib.Api.pick; import static lib.Api.pick;"] {
        for (value, desc) in [("1", "(I)I"), ("true", "(Z)Z")] {
            let s = format!("{prefix} class Calls{{void run(){{int pick=0;pick({value});}}}}");
            selected(
                &project(&s, &[INT, LONG, ("pick", "(Z)Z", 0x109, None)]),
                &format!("pick({value})"),
                "lib.Api",
                desc,
            );
        }
    }
    let s = "import static lib.Api.pick; class Calls{static void run(){pick(1);}}";
    selected(&project(s, &[INT, LONG]), "pick(1)", "lib.Api", "(I)I");
    let s = "import static lib.Api.pick; class Calls{Calls(){pick(1);}}";
    selected(&project(s, &[INT, LONG]), "pick(1)", "lib.Api", "(I)I");
}

#[test]
fn static_import_requires_complete_lexical_absence_and_import_inventory() {
    for s in [
        "import static lib.Api.pick; import static missing.Other.pick; class Calls{void run(){pick(1);}}",
        "import static lib.Api.pick; import static lib.Other.*; class Calls{void run(){pick(1);}}",
        "import static lib.Api.*; class Calls{void run(){pick(1);}}",
        "import static missing.Api.pick; class Calls{void run(){pick(1);}}",
        "import static lib.Api.pick; class Calls extends Missing{void run(){pick(1);}}",
        "import static lib.Api.pick; class Calls<T>{void run(){pick(1);}}",
        "import static lib.Api.pick; class Calls{void run(){Runnable r=()->pick(1);}}",
        "import static lib.Api.pick; class Calls{class Inner{void run(){pick(1);}}}",
    ] {
        let p = project(s, &[INT, LONG]);
        assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved, "{s}");
    }
    let nested = "import static lib.Api.pick; class Calls{void run(){pick(pick(1));}}";
    assert_ne!(query(&project(nested, &[INT]), "pick(pick(1))").status, ResolveStatus::Resolved);
    let s = "import static lib.Api.pick; class Calls extends Parent{void run(){pick(1);}}";
    let mut p = project(s, &[INT]);
    p.insert("Parent.java", "class Parent{private void pick(int x){}}");
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    let s = "import static lib.Api.pick; class Calls{void pick(long x){} void run(){pick(1);}}";
    let p = project(s, &[INT]);
    let r = query(&p, "pick(1)");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets[0].owner, "Calls");
    assert!(r.targets[0].binary.is_none());
}

#[test]
fn static_import_owner_identity_access_and_unsupported_competitors_are_not_filtered() {
    let s = "import static lib.Api.pick; class Calls{void run(){pick(1);}}";
    let mut p = project(s, &[INT]);
    p.insert("Duplicate.java", "package lib; public class Api{public static int pick(int x){return x;}}");
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    for methods in [
        vec![("pick", "(I)I", 0x101, None)],
        vec![INT, ("pick", "(Ljava/lang/Object;)V", 0x109, Some("<T:Ljava/lang/Object;>(TT;)V"))],
        vec![("pick", "(I)I", 0x10a, None)],
        vec![("pick", "([I)I", 0x189, None)],
    ] {
        assert_ne!(query(&project(s, &methods), "pick(1)").status, ResolveStatus::Resolved);
    }
    for (name, flags) in [("lib/Api", 0x20), ("lib/Api$Inner", 0x21), ("Api", 0x21)] {
        let canonical = name.replace(['/', '$'], ".");
        let s = format!("import static {canonical}.pick; class Calls{{void run(){{pick(1);}}}}");
        let mut p = Project::new();
        p.insert("Calls.java", &s);
        p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, &[])).unwrap();
        p.add_class_bytes("Other.class", &binary(name, Some("java/lang/Object"), flags, &[INT])).unwrap();
        assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    }
}

#[test]
fn explicit_static_source_import_preserves_declaration_and_edit_versions() {
    use java_engine::TextEdit;
    let s = "import static lib.Api.pick; class Calls{void run(){pick(1);}}";
    let build = |text: &str| {
        let mut p = project(text, &[]);
        // Use a different owner from the binary helper.
        p.insert("Source.java", "package lib; public class Source{public static int pick(int x){return x;}}");
        p
    };
    let s = s.replace("lib.Api", "lib.Source");
    let mut p = build(&s);
    let old = p.clone();
    let prepared = old.prepare();
    let r = query(&p, "pick(1)");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets[0].owner, "lib.Source");
    assert_eq!(r.targets[0].path, "Source.java");
    let at = s.find("lib.Source").unwrap();
    p.edit("Calls.java", TextEdit { start_byte: at as u32, end_byte: (at + 10) as u32, insert: "lost.Source".into() })
        .unwrap();
    assert_eq!(query(&p, "pick(1)"), query(&build(p.file("Calls.java").unwrap().text()), "pick(1)"));
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    assert_eq!(prepared.resolve_at("Calls.java", s.rfind("pick(1)").unwrap() as u32), r);
    p.insert("Calls.java", &s);
    assert_eq!(query(&p, "pick(1)"), r);
}

#[test]
fn static_import_mixed_instance_family_is_conservative_without_changing_selector() {
    let s = "import static lib.Api.pick; class Calls{void run(){pick(1);}}";
    assert_ne!(query(&project(s, &[LONG, ("pick", "(I)I", 0x101, None)]), "pick(1)").status, ResolveStatus::Resolved);
    selected(&project(s, &[INT, ("pick", "(J)J", 0x101, None)]), "pick(1)", "lib.Api", "(I)I");
    let s = "import static lib.Api.pick; class lib{} class Calls{void run(){pick(1);}}";
    selected(&project(s, &[INT]), "pick(1)", "lib.Api", "(I)I");
}

#[test]
fn static_import_refuses_cycles_inherited_imports_and_broken_headers() {
    let s = "import static lib.Api.pick; class Calls extends Parent{void run(){pick(1);}}";
    let mut p = project(s, &[INT]);
    p.insert("Parent.java", "class Parent extends Calls{}");
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    let s = "import static lib.Api.pick; class Calls{void run(){pick(1);}}";
    let mut p = project(s, &[]);
    p.add_class_bytes("Parent.class", &binary("lib/Parent", Some("java/lang/Object"), 0x21, &[INT])).unwrap();
    p.add_class_bytes("Api.class", &binary("lib/Api", Some("lib/Parent"), 0x21, &[])).unwrap();
    assert_ne!(query(&p, "pick(1)").status, ResolveStatus::Resolved);
    for s in [
        "import static lib.Api.pick class Calls{void run(){pick(1);}}",
        "import static lib.Api.pick; class Calls{void run( {pick(1);}}",
        "import static lib.Api.pick; class Calls{void run(){int x=;pick(1);}}",
        "import static lib.Api.pick; class Calls{void run(){int x=pick;}}",
    ] {
        let p = project(s, &[INT]);
        let marker = if s.contains("pick(1)") { "pick(1)" } else { "pick;" };
        assert_ne!(query(&p, marker).status, ResolveStatus::Resolved);
    }
}
