use java_engine::{Project, ResolveStatus};

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

#[test]
fn bounded_prefix_falsification() {
    for (text, needle) in [
        ("import static p.C.x;class Use{int f(){return x;}}", "x;"),
        ("import static p.C.var.x;class Use{int f(){return x;}}", "x;"),
        ("import static var.pkg.C.var.x;class Use{int f(){return C;}}", "C;"),
        ("import var.pkg.C.var;class Use{C value;}", "C value"),
    ] {
        let mut p = Project::new();
        p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, &[])).unwrap();
        p.insert("p/C.java", "package p;public class C{public static int x;}");
        if text.starts_with("import static var.") {
            p.insert("var/pkg.java", "package var;public class pkg{public static int C;}");
        } else if text.starts_with("import var.") {
            p.insert("var/pkg/C.java", "package var.pkg;public class C{}");
        }
        p.insert("Use.java", text);
        let result = p.resolve_at("Use.java", text.rfind(needle).unwrap() as u32);
        eprintln!("CASE={text} RESULT={result:?}");
        if text.starts_with("import static p.C.x;") {
            assert_eq!(result.status, ResolveStatus::Resolved);
            assert_eq!(result.targets[0].path, "p/C.java");
        } else {
            assert_ne!(result.status, ResolveStatus::Resolved);
            assert!(result.targets.is_empty());
        }
    }
}

#[test]
fn legal_var_package_names_retain_canonical_type_identity() {
    for package in ["var.pkg", "p.var"] {
        let mut p = Project::new();
        let source = format!("package {package};public class C{{}}");
        p.insert("Library.java", &source);
        let text = format!("import {package}.C;class Use{{ C value; }}");
        p.insert("Use.java", &text);
        let at = text.rfind("C value").unwrap() as u32;
        let result = p.resolve_at("Use.java", at);
        assert_eq!(result.status, ResolveStatus::Resolved, "{package}: {result:?}");
        assert_eq!(result.targets[0].owner, format!("{package}.C"));
        assert_eq!(result.targets[0].path, "Library.java");
        assert_eq!(result, p.prepare().resolve_at("Use.java", at));
    }
}

#[test]
fn malformed_names_do_not_shorten_to_existing_targets() {
    let cases = [
        ("import p.C.var; class Use{ C value; }", "C value"),
        ("import static p.C.var.x; class Use{ int value=x; }", "x;"),
        ("class Use{p.C.var field;int f(){return field.x;}}", "x;"),
        ("class Use{int f(p.C.var param){return param.x;}}", "x;"),
        ("class Use{int f(){p.C.var local=null;return local.x;}}", "x;"),
        ("class Use{p.C.var field;}", "var field"),
    ];
    let mut failures = Vec::new();
    for (text, mark) in cases {
        let mut p = Project::new();
        p.insert("p/C.java", "package p;public class C{public static int x;}");
        p.insert("Use.java", text);
        let at = text.rfind(mark).unwrap() as u32;
        let r = p.resolve_at("Use.java", at);
        eprintln!("CASE={text} QUERY={mark} RESULT={r:?}");
        if r.status == ResolveStatus::Resolved && r.targets.iter().any(|t| t.path == "p/C.java") {
            failures.push(text);
        }
    }
    assert!(failures.is_empty(), "shortened malformed names: {failures:?}");
}
