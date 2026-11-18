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
fn query(p: &Project, marker: &str) -> ResolveResult {
    let at = p.file("Calls.java").unwrap().text().rfind(marker).unwrap() as u32;
    let r = p.resolve_at("Calls.java", at);
    assert_eq!(r, p.prepare().resolve_at("Calls.java", at));
    r
}

const API: &str = "package lib; public class Api { public static int pick(int i){return i;} public static long pick(long i){return i;} }";
fn project(api: &str, text: &str) -> Project {
    let mut p = Project::new();
    p.insert("Api.java", api);
    p.insert("Calls.java", text);
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, &[])).unwrap();
    p
}
fn wrapped(expr: &str) -> String {
    format!(
        "import lib.Api; class Box {{public void take(int i){{}} public void take(long i){{}}}} class Calls {{void run(Box box){{box.take({expr});}}}}"
    )
}
fn proved(p: &Project, marker: &str, owner: &str) {
    let r = query(p, marker);
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].owner, owner);
}
#[test]
fn source_binary_same_api_select_exact_overload() {
    for (ty, decl, desc) in [("int", "pick(int i)", "(I)I"), ("long", "pick(long i)", "(J)J")] {
        let text = format!("import lib.Api; class Calls {{void run({ty} i){{Api.pick(i);}}}}");
        let p = project(API, &text);
        let r = query(&p, "pick(i)");
        assert_eq!(r.status, ResolveStatus::Resolved);
        assert_eq!(r.targets.len(), 1);
        assert_eq!(r.targets[0].start_byte, API.find(decl).unwrap() as u32);
        let mut p = project("", &text);
        p.add_class_bytes(
            "Api.class",
            &binary(
                "lib/Api",
                Some("java/lang/Object"),
                0x21,
                &[("pick", "(I)I", 0x109, None), ("pick", "(J)J", 0x109, None)],
            ),
        )
        .unwrap();
        let r = query(&p, "pick(i)");
        assert_eq!(r.status, ResolveStatus::Resolved);
        assert_eq!(r.targets[0].binary.as_ref().unwrap().descriptor, desc);
    }
}
#[test]
fn source_static_returns_compose_in_declaration_scope() {
    let text = wrapped("Api.pick(1)");
    let p = project(API, &text);
    proved(&p, "take(Api.pick(1))", "Box");
    assert_eq!(query(&p, "take(Api.pick(1))").targets[0].start_byte, text.find("take(int i)").unwrap() as u32);
    let api = "package lib; public class Api {public static Product make(){return null;}}";
    let text = "import lib.Api; class Product {} class Calls {void run(){Api.make().value();}}";
    let mut p = project(api, text);
    let product = "package lib; public class Product {public int value(){return 1;}}";
    p.insert("Product.java", product);
    proved(&p, "value()", "lib.Product");
    assert_eq!(query(&p, "value()").targets[0].path, "Product.java");
}
#[test]
fn unsupported_complete_families_never_supply_nested_values() {
    let api = "package lib; public class Api {public static int pick(int i){return i;} public int pick(){return 0;}}";
    let text = wrapped("Api.pick(1)");
    let p = project(api, &text);
    proved(&p, "take(Api.pick(1))", "Box");
    assert_eq!(query(&p, "take(Api.pick(1))").targets[0].start_byte, text.find("take(int i)").unwrap() as u32);
    proved(&p, "pick(1)", "lib.Api");
    assert_eq!(query(&p, "pick(1)").targets[0].start_byte, api.find("pick(int i)").unwrap() as u32);

    for other in [
        "private static int pick(){return 0;}",
        "public static int pick(int... xs){return 0;}",
        "public static <T> int pick(){return 0;}",
        "public static int pick(Missing m){return 0;}",
        "public static long pick(int i){return 0;}",
    ] {
        let api = format!("package lib; public class Api {{public static int pick(int i){{return i;}} {other}}}");
        let text = wrapped("Api.pick(1)");
        if other == "public static int pick(int... xs){return 0;}" {
            let p = project(&api, &text);
            proved(&p, "take(Api.pick(1))", "Box");
            assert_eq!(query(&p, "take(Api.pick(1))").targets[0].start_byte, text.find("take(int i)").unwrap() as u32);
            proved(&p, "pick(1)", "lib.Api");
            for (marker, path, owner, name, start) in [
                ("take(Api.pick(1))", "Calls.java", "Box", "take", text.find("take(int i)").unwrap() as u32),
                ("pick(1)", "Api.java", "lib.Api", "pick", api.find("pick(int i)").unwrap() as u32),
            ] {
                assert_eq!(
                    query(&p, marker).targets[0],
                    java_engine::Target {
                        binary: None,
                        path: path.into(),
                        kind: java_engine::TargetKind::Method,
                        name: name.into(),
                        owner: owner.into(),
                        start_byte: start,
                        end_byte: start + name.len() as u32,
                    }
                );
            }
        } else {
            assert_ne!(query(&project(&api, &text), "take(Api.pick(1))").status, ResolveStatus::Resolved, "{other}");
        }
    }
}
#[test]
fn declaration_file_errors_interfaces_and_inheritance_refuse_values() {
    for api in [
        "package lib; public class Api {public static int pick(int i){return i;} void broken( { }",
        "package lib; public interface Api {public static int pick(int i){return i;}}",
        "package lib; public class Api<T> {public static int pick(int i){return i;}}",
        "package lib; class Base {public int pick(){return 0;}} public class Api extends Base {public static int pick(int i){return i;}}",
    ] {
        let text = wrapped("Api.pick(1)");
        assert_ne!(query(&project(api, &text), "take(Api.pick(1))").status, ResolveStatus::Resolved, "{api}");
    }
}
#[test]
fn shadows_and_parentheses_do_not_produce_strict_values() {
    for (head, params, body) in [
        ("", "Box box,int Api", "box.take(Api.pick(1));"),
        ("int Api;", "Box box", "box.take(Api.pick(1));"),
        ("lib.Api Api;", "Box box", "box.take(Api.pick(1));"),
        ("", "Box box", "box.take((Api).pick(1));"),
    ] {
        let text = format!(
            "import lib.Api; class Box {{public void take(int i){{}}}} class Calls {{{head} void run({params}){{{body}}}}}"
        );
        let marker = if body.contains("(Api)") { "take((Api).pick(1))" } else { "take(Api.pick(1))" };
        assert_ne!(query(&project(API, &text), marker).status, ResolveStatus::Resolved, "{text}");
    }
    for head in ["class Calls<Api> {void run(Box box)", "class Calls {<Api> void run(Box box)"] {
        let text =
            format!("import lib.Api; class Box {{public void take(int i){{}}}} {head}{{box.take(Api.pick(1));}}}}");
        assert_ne!(query(&project(API, &text), "take(Api.pick(1))").status, ResolveStatus::Resolved);
    }
}
#[test]
fn slots_and_shared_work_budget_bound_nested_proofs() {
    for count in [255, 256] {
        let params = (0..count).map(|i| format!("int a{i}")).collect::<Vec<_>>().join(",");
        let args = vec!["0"; count].join(",");
        let api = format!("package lib; public class Api {{public static int pick({params}){{return 0;}}}}");
        let text = wrapped(&format!("Api.pick({args})"));
        assert_eq!(
            query(&project(&api, &text), &format!("take(Api.pick({args}))")).status == ResolveStatus::Resolved,
            count == 255,
            "{count}"
        );
    }
    for count in [0, 5000] {
        let filler = (0..count).map(|i| format!("int unused{i};")).collect::<String>();
        let text = wrapped("Api.pick(1)").replace("box.take", &format!("{filler}box.take"));
        assert_eq!(query(&project(API, &text), "take(Api.pick(1))").status == ResolveStatus::Resolved, count == 0);
    }
}
#[test]
fn same_source_name_in_separate_compilations_keeps_static_result_identity() {
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
        "source-static-scope-{}-{}",
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
    for (name, method) in [("a", "int"), ("b", "long")] {
        let src = root.join(name).join("src");
        let bin = root.join(name).join("bin");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(bin.join("lib")).unwrap();
        fs::create_dir_all(bin.join("java/lang")).unwrap();
        fs::write(src.join("Calls.java"), text).unwrap();
        fs::write(bin.join("java/lang/Object.class"), binary("java/lang/Object", None, 0x21, &[])).unwrap();
        fs::write(
            src.join("Api.java"),
            format!("package lib; public class Api {{public static {method} pick({method} i){{return i;}}}}"),
        )
        .unwrap();
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
    for name in ["a", "b"] {
        let path = root.join(name).join("src/Calls.java").canonicalize().unwrap();
        let r = a
            .scope(&CompilationId::new(name, "main"))
            .unwrap()
            .resolve_at(path.to_str().unwrap(), text.find("pick(1)").unwrap() as u32);
        assert_eq!(r.status, ResolveStatus::Resolved);
        assert_eq!(r.targets.len(), 1);
        assert!(r.targets[0].binary.is_none());
        assert_eq!(r.targets[0].owner, "lib.Api");
        assert_eq!(r.targets[0].path, root.join(name).join("src/Api.java").canonicalize().unwrap().to_str().unwrap());
    }
}

#[test]
fn declaration_body_opacity_is_not_caller_proof_permission() {
    for body in ["switch(i){case 0: break; default: break;}", "class Local {}"] {
        let api = format!("package lib; public class Api {{public static int pick(int i){{{body} return i;}}}}");
        let text = wrapped("Api.pick(1)");
        let errors = java_engine::ParsedFile::parse(&api).errors().len();
        // Local types remain parser errors; only traditional switch is syntax-valid.
        assert_eq!(errors == 0, body.starts_with("switch"));
        assert_eq!(
            query(&project(&api, &text), "take(Api.pick(1))").status == ResolveStatus::Resolved,
            errors == 0,
            "{body}"
        );
        let caller = text.replace("box.take", &format!("int i=0; {body} box.take"));
        assert_ne!(query(&project(API, &caller), "take(Api.pick(1))").status, ResolveStatus::Resolved, "{body}");
    }
    for bad in [
        "public static abstract int invalid();",
        "public int invalid(int same, int same){return 0;}",
        "public void invalid( { }",
    ] {
        let api = format!("package lib; public class Api {{public static int pick(int i){{return i;}} {bad}}}");
        let text = wrapped("Api.pick(1)");
        assert_ne!(query(&project(&api, &text), "take(Api.pick(1))").status, ResolveStatus::Resolved, "{bad}");
    }
}
