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
fn binary_fields(name: &str, parent: Option<&str>, flags: u16, methods: &[Method<'_>]) -> Vec<u8> {
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
    for v in [flags, this, parent, 0, records.len() as u16] {
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
    u(&mut b, 0);
    b
}
fn project(api: &str, text: &str) -> Project {
    let mut p = Project::new();
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, &[])).unwrap();
    p.insert("Api.java", api);
    p.insert("Calls.java", text);
    p
}
fn query(p: &Project, marker: &str) -> ResolveResult {
    let at = p.file("Calls.java").unwrap().text().rfind(marker).unwrap() as u32;
    let result = p.resolve_at("Calls.java", at);
    assert_eq!(result, p.prepare().resolve_at("Calls.java", at));
    result
}
const API: &str = "package lib; public class Api { public static final int VALUE=1; }";
#[test]
fn explicit_own_static_field_in_initializer_and_method() {
    let text = "import static lib.Api.VALUE; class Calls { int n=VALUE; int f(){return VALUE;} }";
    let p = project(API, text);
    for marker in ["VALUE;", "VALUE;}"] {
        let r = query(&p, marker);
        assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
        assert_eq!(r.targets.len(), 1);
        assert_eq!(r.targets[0].owner, "lib.Api");
        assert_eq!(r.targets[0].start_byte, API.find("VALUE=").unwrap() as u32);
    }
}

fn refused(p: &Project, marker: &str) {
    assert_eq!(query(p, marker).status, ResolveStatus::StaticImportNotApplied);
}
#[test]
fn duplicate_imports_and_own_hiding_are_identity_based() {
    let text = "import static lib.Api.VALUE; import static lib.Api.VALUE; class Calls{int n=VALUE;}";
    assert_eq!(query(&project(API, text), "VALUE;").status, ResolveStatus::Resolved);
    let mut p = project("package lib; public class Api extends Base {public static int VALUE=2;}", text);
    p.insert("Base.java", "package lib; class Base{public static int VALUE=1;}");
    assert_eq!(query(&p, "VALUE;").targets[0].owner, "lib.Api");
    p.insert("Other.java", "package lib; public class Other{public static int VALUE=3;}");
    p.insert("Calls.java", "import static lib.Api.VALUE; import static lib.Other.VALUE; class Calls{int n=VALUE;}");
    refused(&p, "VALUE;");
}
#[test]
fn unknown_competitors_and_inherited_aliases_never_disappear() {
    for text in [
        "import static lib.Api.VALUE; import static missing.Api.VALUE; class Calls{int n=VALUE;}",
        "import static lib.Api.VALUE; import static lib.Other.*; class Calls{int n=VALUE;}",
        "import static lib.Api.VALUE; class Calls extends Missing{int n=VALUE;}",
    ] {
        refused(&project(API, text), "VALUE;");
    }
    for api in [
        "package lib; public class Api extends Missing {public static int VALUE=1;}",
        "package lib; public class Api {public int VALUE=1;}",
        "package lib; public class Api {public static int VALUE=1; public static int VALUE=2;}",
        "package lib; public class Api {public static final volatile int VALUE=1;}",
        "package lib; public class Api {@Deprecated public static int VALUE=1;}",
        "package lib; public class Api {public static int OTHER=1;}",
    ] {
        refused(&project(api, "import static lib.Api.VALUE; class Calls{int n=VALUE;}"), "VALUE;");
    }
    assert_eq!(
        query(&project(API, "import static lib.Api.*; class Calls{int n=VALUE;}"), "VALUE;").status,
        ResolveStatus::Unresolved
    );
    let mut p = project(API, "import static lib.Api.VALUE; import static lib.Child.VALUE; class Calls{int n=VALUE;}");
    p.insert("Child.java", "package lib; public class Child extends Api {}");
    refused(&p, "VALUE;");
    p.insert("Duplicate.java", API);
    refused(&p, "VALUE;");
}
#[test]
fn package_and_private_access_are_import_site_rules() {
    for (modifier, package, ok) in [
        ("public static", "", "true"),
        ("static", "package lib;", "true"),
        ("protected static", "package lib;", "true"),
        ("static", "", "false"),
        ("protected static", "", "false"),
        ("private static", "package lib;", "false"),
    ] {
        let api = format!("package lib; public class Api {{{modifier} int VALUE=1;}}");
        let text = format!("{package} import static lib.Api.VALUE; class Calls {{int n=VALUE;}}");
        let r = query(&project(&api, &text), "VALUE;");
        assert_eq!(r.status == ResolveStatus::Resolved, ok == "true", "{modifier} {package} {r:?}");
    }
}
#[test]
fn lexical_values_win_before_invalid_imports() {
    for text in [
        "import static missing.Api.VALUE; class Calls {int f(){int VALUE=2; return VALUE;}}",
        "import static missing.Api.VALUE; class Calls {int VALUE=2; int f(){return VALUE;}}",
        "import static missing.Api.VALUE; class Calls extends Base {int f(){return VALUE;}}",
        "import static missing.Api.VALUE; class Outer {int VALUE=2; class Calls{int f(){return VALUE;}}}",
    ] {
        let mut p = project(API, text);
        p.insert("Base.java", "class Base{int VALUE=2;}");
        let r = query(&p, "VALUE;");
        assert_eq!(r.status, ResolveStatus::Resolved, "{text} {r:?}");
        assert_ne!(r.targets[0].owner, "lib.Api");
    }
}
#[test]
fn namespace_and_unknown_type_fallback_are_separate() {
    let text = "import static lib.Api.VALUE; class VALUE{} class Calls{VALUE field; void f(){VALUE();}}";
    let p = project(API, text);
    assert_eq!(query(&p, "VALUE field").targets[0].kind, java_engine::TargetKind::Class);
    assert_ne!(query(&p, "VALUE();").status, ResolveStatus::Resolved);
    for api in [
        "package lib; public class Api {}",
        "package lib; public class Api {public int VALUE;}",
        "package lib; public class Api {public static class VALUE{}}",
        "package lib; public class Api {public static void VALUE(){} public static class VALUE{}}",
    ] {
        refused(&project(api, "import static lib.Api.VALUE; class VALUE{} class Calls{Object n=VALUE;}"), "VALUE;");
    }
    let p = project(
        "package lib; public class Api{public static void VALUE(){}}",
        "import static lib.Api.VALUE; class VALUE{} class Calls{Object n=VALUE;}",
    );
    refused(&p, "VALUE;");
    let p = project(API, "import static lib.Api.VALUE; class VALUE{static int x;} class Calls{int n=VALUE.x;}");
    assert_eq!(query(&p, "VALUE.x").targets[0].kind, java_engine::TargetKind::Class);
    let p = project(API, "import static lib.Api.VALUE; class VALUE{static int x;} class Calls{int n=(VALUE).x;}");
    assert_eq!(query(&p, "VALUE).x").targets[0].kind, java_engine::TargetKind::Class);
    let p = project(API, "import static lib.Api.VALUE; class Calls{void f(int x){} void run(){f(VALUE);}}");
    assert_eq!(query(&p, "VALUE);").targets[0].owner, "lib.Api");
}
#[test]
fn binary_fields_match_source_and_reject_bad_flags() {
    let text = "import static lib.Api.VALUE; class Calls{int n=VALUE;}";
    for (flags, ok) in [
        (0x19, true),
        (0x09, true),
        (0x01, false),
        (0x0a, false),
        (0x0d, false),
        (0x59, false),
        (0x89, true),
        (0x109, false),
        (0x1019, false),
        (0x4019, false),
    ] {
        let mut p = project("", text);
        p.add_class_bytes(
            "Api.class",
            &binary_fields("lib/Api", Some("java/lang/Object"), 0x21, &[("VALUE", "I", flags, None)]),
        )
        .unwrap();
        let r = query(&p, "VALUE;");
        assert_eq!(r.status == ResolveStatus::Resolved, ok, "{flags:x} {r:?}");
    }
}
#[test]
fn edits_fresh_and_old_snapshot_keep_import_facts_separate() {
    let text = "import static lib.Api.VALUE; class Calls{int n=VALUE;}";
    let original = project(API, text);
    let old = original.prepare();
    let mut changed = original.clone();
    let at = API.find("static ").unwrap() as u32;
    changed
        .edit("Api.java", java_engine::TextEdit { start_byte: at, end_byte: at + 7, insert: String::new() })
        .unwrap();
    let fresh = project(&API.replace("static ", ""), text);
    assert_eq!(query(&changed, "VALUE;"), query(&fresh, "VALUE;"));
    refused(&changed, "VALUE;");
    let pos = text.rfind("VALUE;").unwrap() as u32;
    assert_eq!(old.resolve_at("Calls.java", pos), query(&original, "VALUE;"));
    changed.edit("Api.java", java_engine::TextEdit { start_byte: at, end_byte: at, insert: "static ".into() }).unwrap();
    assert_eq!(query(&changed, "VALUE;"), query(&original, "VALUE;"));
}

#[test]
fn bounded_graphs_and_pattern_shadow_do_not_manufacture_imports() {
    let text = "import static lib.Api.VALUE; class Calls{int n=VALUE;}";
    refused(&project("package lib; public class Api extends Api{public static int VALUE;}", text), "VALUE;");
    let imports = "import static lib.Api.VALUE;".repeat(500);
    refused(&project(API, &format!("{imports} class Calls{{int n=VALUE;}}")), "VALUE;");
    let text = "import static lib.Api.VALUE; class Calls{Object f(Object o){if(o instanceof String VALUE){return VALUE;}return null;}}";
    let r = query(&project(API, text), "VALUE;");
    assert!(!r.targets.iter().any(|t| t.owner == "lib.Api"), "{r:?}");
}

#[test]
fn compilation_identity_and_invisible_duplicates_remain_distinct() {
    use java_engine::{
        ClasspathEntry, CompilationConfig, CompilationId, ProjectConfig, WorkspaceConfig, WorkspaceSnapshot,
    };
    use std::fs;
    let root = std::env::temp_dir().join(format!("java-static-fields-{}", std::process::id()));
    for folder in ["lib", "app", "other", "platform/java/lang"] {
        fs::create_dir_all(root.join(folder)).unwrap();
    }
    fs::write(root.join("platform/java/lang/Object.class"), binary("java/lang/Object", None, 0x21, &[])).unwrap();
    fs::write(root.join("lib/Api.java"), API).unwrap();
    fs::write(root.join("other/Api.java"), API.replace("=1", "=2")).unwrap();
    let text = "import static lib.Api.VALUE; class Calls{int n=VALUE;}";
    fs::write(root.join("app/Calls.java"), text).unwrap();
    let lib = CompilationId::new("lib", "main");
    let app = CompilationId::new("app", "main");
    let other = CompilationId::new("other", "main");
    let config = |both: bool| WorkspaceConfig {
        compilations: vec![
            CompilationConfig {
                id: lib.clone(),
                inputs: ProjectConfig {
                    source_roots: vec![root.join("lib")],
                    classpath: vec![ClasspathEntry::Directory(root.join("platform"))],
                    ..Default::default()
                },
                dependencies: vec![],
            },
            CompilationConfig {
                id: other.clone(),
                inputs: ProjectConfig {
                    source_roots: vec![root.join("other")],
                    classpath: vec![ClasspathEntry::Directory(root.join("platform"))],
                    ..Default::default()
                },
                dependencies: vec![],
            },
            CompilationConfig {
                id: app.clone(),
                inputs: ProjectConfig {
                    source_roots: vec![root.join("app")],
                    classpath: vec![ClasspathEntry::Directory(root.join("platform"))],
                    ..Default::default()
                },
                dependencies: if both { vec![lib.clone(), other.clone()] } else { vec![lib.clone()] },
            },
        ],
        ..Default::default()
    };
    let path = root.join("app/Calls.java").canonicalize().unwrap().to_str().unwrap().to_owned();
    let at = text.rfind("VALUE;").unwrap() as u32;
    for both in [false, true] {
        let w = WorkspaceSnapshot::load(&config(both)).unwrap();
        let r = w.scope(&app).unwrap().resolve_at(&path, at);
        assert_eq!(r, w.prepare().scope(&app).unwrap().resolve_at(&path, at));
        if both {
            assert_eq!(r.status, ResolveStatus::StaticImportNotApplied);
        } else {
            assert_eq!(r.status, ResolveStatus::Resolved);
            assert_eq!(r.targets[0].path, root.join("lib/Api.java").canonicalize().unwrap().to_str().unwrap());
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn index_receiver_is_legacy_but_index_operand_is_a_value() {
    let api = "package lib; public class Api {public static int[] ARR; public static int VALUE;}";
    let p = project(api, "import static lib.Api.ARR; class Calls{int n=ARR[0];}");
    assert_eq!(query(&p, "ARR[0]").status, ResolveStatus::StaticImportNotApplied);
    let p = project(api, "import static lib.Api.VALUE; class Calls{int[] arr; int n=arr[VALUE];}");
    assert_eq!(query(&p, "VALUE];").targets[0].owner, "lib.Api");
}

#[test]
fn wildcard_owner_name_is_not_an_explicit_member_import() {
    let text = "import static lib.VALUE.*; class VALUE{} class Calls{Object n=VALUE;}";
    let mut p = project("", text);
    p.insert("Wildcard.java", "package lib; public class VALUE{public static int OTHER;}");
    let r = query(&p, "VALUE;");
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets[0].kind, java_engine::TargetKind::Class);
    assert_eq!(r.targets[0].owner, "VALUE");
}
