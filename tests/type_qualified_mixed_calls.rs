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

fn project(api: &str, text: &str, methods: Option<&[Method<'_>]>) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", text);
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, &[])).unwrap();
    if let Some(methods) = methods {
        p.add_class_bytes("Api.class", &binary("lib/Api", Some("java/lang/Object"), 0x21, methods)).unwrap();
    } else {
        p.insert("Api.java", api);
    }
    p
}
fn text(expr: &str) -> String {
    format!(
        "import lib.Api; class Box {{public void take(int i){{}} public void take(long i){{}}}} class Calls {{void run(Box box){{box.take({expr});}}}}"
    )
}
fn strict(p: &Project, marker: &str, yes: bool) {
    let r = query(p, marker);
    assert_eq!(r.status == ResolveStatus::Resolved, yes, "{r:?}");
    if yes {
        assert_eq!(r.targets.len(), 1);
    }
}
#[test]
fn mixed_selection_then_static_legality_source_and_binary() {
    for (static_type, instance_type, yes) in [("int", "long", true), ("long", "int", false)] {
        let api = format!(
            "package lib; public class Api {{public static int pick({static_type} i){{return 1;}} public int pick({instance_type} i){{return 1;}}}}"
        );
        let sd = if static_type == "int" { "(I)I" } else { "(J)I" };
        let id = if instance_type == "int" { "(I)I" } else { "(J)I" };
        let methods = [("pick", sd, 0x109, None), ("pick", id, 0x101, None)];
        for binary in [false, true] {
            let t = text("Api.pick(1)");
            let p = project(&api, &t, if binary { Some(&methods) } else { None });
            strict(&p, "take(Api.pick(1))", yes);
            strict(&p, "pick(1)", yes);
            if yes {
                let r = query(&p, "pick(1)");
                assert_eq!(r.targets[0].owner, "lib.Api");
                if binary {
                    assert_eq!(r.targets[0].binary.as_ref().unwrap().descriptor, sd);
                } else {
                    assert_eq!(r.targets[0].start_byte, api.find("pick(int i)").unwrap() as u32);
                }
            }
        }
    }
}
#[test]
fn different_arity_instance_is_validated_but_not_applicable() {
    let api = "package lib; public class Api {public static int pick(int i){return i;} public int pick(){return 0;}}";
    let methods = [("pick", "(I)I", 0x109, None), ("pick", "()I", 0x101, None)];
    for b in [false, true] {
        let t = text("Api.pick(1)");
        strict(&project(api, &t, if b { Some(&methods) } else { None }), "take(Api.pick(1))", true);
    }
}
#[test]
fn mixed_ambiguity_and_instance_mode_are_not_relaxed() {
    let api = "package lib; public class Api {public static int pick(int a,long b){return 1;} public int pick(long a,int b){return 1;}}";
    let methods = [("pick", "(IJ)I", 0x109, None), ("pick", "(JI)I", 0x101, None)];
    for b in [false, true] {
        let t = text("Api.pick(1,1)");
        strict(&project(api, &t, if b { Some(&methods) } else { None }), "take(Api.pick(1,1))", false);
    }
    let api =
        "package lib; public class Api {public static int pick(int i){return i;} public int pick(long i){return 0;}}";
    let methods = [("pick", "(I)I", 0x109, None), ("pick", "(J)I", 0x101, None)];
    let t = "import lib.Api; class Box {public void take(int i){}} class Calls {void run(Box box,Api value){box.take(value.pick(1));}}";
    for b in [false, true] {
        strict(&project(api, t, if b { Some(&methods) } else { None }), "take(value.pick(1))", false);
    }
}
#[test]
fn source_unsupported_competitors_and_bad_declarations_refuse() {
    for other in [
        "public int pick(Missing x){return 0;}",
        "public <T> int pick(){return 0;}",
        "public int pick(int... xs){return 0;}",
        "private int pick(){return 0;}",
        "public int pick(int i){return 0;}",
        "public abstract int pick();",
        "public static abstract int pick();",
    ] {
        let api =
            format!("package lib; public abstract class Api {{public static int pick(int i){{return i;}} {other}}}");
        let t = text("Api.pick(1)");
        let p = project(&api, &t, None);
        if other == "public int pick(int... xs){return 0;}" {
            strict(&p, "take(Api.pick(1))", true);
            let r = query(&p, "take(Api.pick(1))");
            let start = t.find("take(int i)").unwrap() as u32;
            assert_eq!(
                r.targets[0],
                java_engine::Target {
                    binary: None,
                    path: "Calls.java".into(),
                    kind: java_engine::TargetKind::Method,
                    name: "take".into(),
                    owner: "Box".into(),
                    start_byte: start,
                    end_byte: start + 4,
                }
            );
            let start = api.find("pick(int i)").unwrap() as u32;
            assert_eq!(
                query(&p, "pick(1)").targets[0],
                java_engine::Target {
                    binary: None,
                    path: "Api.java".into(),
                    kind: java_engine::TargetKind::Method,
                    name: "pick".into(),
                    owner: "lib.Api".into(),
                    start_byte: start,
                    end_byte: start + 4,
                }
            );
        } else {
            strict(&p, "take(Api.pick(1))", false);
        }
    }
}
#[test]
fn binary_unsupported_competitors_refuse_even_with_other_arity() {
    for other in [
        ("pick", "(Lmissing/X;)I", 0x101, None),
        ("pick", "()I", 0x101, Some("<T:Ljava/lang/Object;>()I")),
        ("pick", "([I)I", 0x181, None),
        ("pick", "()I", 0x102, None),
        ("pick", "(I)J", 0x101, None),
        ("pick", "()I", 0x401, None),
        ("pick", "()I", 0x409, None),
    ] {
        let methods = [("pick", "(I)I", 0x109, None), other];
        let t = text("Api.pick(1)");
        let p = project("", &t, Some(&methods));
        if other == ("pick", "([I)I", 0x181, None) {
            strict(&p, "take(Api.pick(1))", true);
            let r = query(&p, "take(Api.pick(1))");
            let start = t.find("take(int i)").unwrap() as u32;
            assert_eq!(
                r.targets[0],
                java_engine::Target {
                    binary: None,
                    path: "Calls.java".into(),
                    kind: java_engine::TargetKind::Method,
                    name: "take".into(),
                    owner: "Box".into(),
                    start_byte: start,
                    end_byte: start + 4,
                }
            );
            let inner = query(&p, "pick(1)");
            assert_eq!(inner.status, ResolveStatus::Resolved);
            assert_eq!(inner.targets[0].binary.as_ref().unwrap().descriptor, "(I)I");
        } else {
            strict(&p, "take(Api.pick(1))", false);
        }
    }
}
#[test]
fn actual_member_mode_controls_parameter_slots() {
    for (instance, count, accepted) in [(true, 254, true), (true, 255, false), (false, 255, true), (false, 256, false)]
    {
        let params = (0..count).map(|i| format!("int p{i}")).collect::<Vec<_>>().join(",");
        let other_mod = if instance { "" } else { "static " };
        let api = format!(
            "package lib; public class Api {{public static int pick(int x){{return x;}} public {other_mod}int pick({params}){{return 0;}}}}"
        );
        let desc = format!("({})I", "I".repeat(count));
        let methods =
            [("pick", "(I)I", 0x109, None), ("pick", desc.as_str(), if instance { 0x101 } else { 0x109 }, None)];
        let t = text("Api.pick(1)");
        strict(&project(&api, &t, None), "take(Api.pick(1))", accepted);
        let mut p = project("", &t, Some(&[("pick", "(I)I", 0x109, None)]));
        let admission = p.add_class_bytes("Api.class", &binary("lib/Api", Some("java/lang/Object"), 0x21, &methods));
        if admission.is_err() {
            assert!(!accepted);
        } else {
            strict(&p, "take(Api.pick(1))", accepted);
        }
    }
}

#[test]
fn mixed_selection_retains_shadow_and_shared_budget_gates() {
    let api =
        "package lib; public class Api {public static int pick(int i){return i;} public int pick(long i){return 0;}}";
    let methods = [("pick", "(I)I", 0x109, None), ("pick", "(J)I", 0x101, None)];
    for b in [false, true] {
        for filler in ["int Api=0;".to_owned(), (0..5000).map(|i| format!("int unused{i};")).collect()] {
            let t = text("Api.pick(1)").replace("box.take", &format!("{filler}box.take"));
            strict(&project(api, &t, if b { Some(&methods) } else { None }), "take(Api.pick(1))", false);
        }
        let t = text("(Api).pick(1)");
        strict(&project(api, &t, if b { Some(&methods) } else { None }), "take((Api).pick(1))", false);
    }
}

#[test]
fn reference_specificity_and_reference_return_chain_keep_static_legality() {
    for static_narrow in [true, false] {
        let (static_ty, instance_ty) = if static_narrow { ("Arg", "Object") } else { ("Object", "Arg") };
        let api = format!(
            "package lib; public class Api {{public static Product pick({static_ty} x){{return null;}} public Product pick({instance_ty} x){{return null;}}}}"
        );
        let sd = format!("(L{};)Llib/Product;", if static_narrow { "lib/Arg" } else { "java/lang/Object" });
        let id = format!("(L{};)Llib/Product;", if static_narrow { "java/lang/Object" } else { "lib/Arg" });
        let methods = [("pick", sd.as_str(), 0x109, None), ("pick", id.as_str(), 0x101, None)];
        let text =
            "import lib.Api; import lib.Arg; class Product {} class Calls {void run(Arg arg){Api.pick(arg).value();}}";
        for b in [false, true] {
            let mut p = project(&api, text, if b { Some(&methods) } else { None });
            if b {
                p.add_class_bytes("Arg.class", &binary("lib/Arg", Some("java/lang/Object"), 0x21, &[])).unwrap();
                p.add_class_bytes(
                    "Product.class",
                    &binary("lib/Product", Some("java/lang/Object"), 0x21, &[("value", "()I", 0x101, None)]),
                )
                .unwrap();
            } else {
                p.insert("Arg.java", "package lib; public class Arg {}");
                p.insert("Product.java", "package lib; public class Product {public int value(){return 1;}}");
            }
            strict(&p, "pick(arg)", static_narrow);
            strict(&p, "value()", static_narrow);
            if static_narrow {
                let picked = query(&p, "pick(arg)");
                assert_eq!(picked.targets[0].owner, "lib.Api");
                if b {
                    assert_eq!(picked.targets[0].binary.as_ref().unwrap().descriptor, sd);
                } else {
                    assert_eq!(picked.targets[0].start_byte, api.find("pick(Arg x)").unwrap() as u32);
                }
                let value = query(&p, "value()");
                assert_eq!(value.targets[0].owner, "lib.Product");
                if b {
                    assert_eq!(value.targets[0].binary.as_ref().unwrap().descriptor, "()I");
                } else {
                    assert_eq!(value.targets[0].path, "Product.java");
                }
            }
        }
    }
}
