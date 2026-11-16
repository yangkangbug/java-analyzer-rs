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

fn project(text: &str) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", text);
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, &[])).unwrap();
    p
}
fn query(p: &Project, marker: &str) -> ResolveResult {
    let at = p.file("Calls.java").unwrap().text().rfind(marker).unwrap() as u32;
    let r = p.resolve_at("Calls.java", at);
    assert_eq!(r, p.prepare().resolve_at("Calls.java", at));
    r
}
#[test]
fn own_reference_overloads_and_private_result_composition() {
    let p = project(
        "class A {} class B extends A {} class Product { public void use(int x){} public void use(long x){} } class Calls { private Product pick(A a){return null;} private Product pick(B b){return null;} private int count(B b){return 1;} void run(B b, Product product){pick(b).use(count(b)); product.use(count(b));} }",
    );
    for marker in ["pick(b)", "use(count(b))", "count(b)"] {
        let r = query(&p, marker);
        assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    }
    let r = query(&p, "pick(b)");
    let text = p.file("Calls.java").unwrap().text();
    assert_eq!(r.targets[0].start_byte, text.find("pick(B b)").unwrap() as u32);
}

fn strict_value_case(extra: &str, caller: &str) -> String {
    format!(
        "class Product {{public void use(int i){{}} public void use(long i){{}}}} class Calls {{private int count(int i){{return i;}} {extra} {caller}}}"
    )
}
#[test]
fn own_access_and_same_named_values_preserve_method_namespace() {
    for access in ["public", "protected", "private", ""] {
        let text = format!(
            "class Calls {{int pick; {access} int pick(int i){{return i;}} {access} long pick(long i){{return i;}} void run(int pick){{pick(1);}}}}"
        );
        let r = query(&project(&text), "pick(1)");
        assert_eq!(r.status, ResolveStatus::Resolved, "{access}: {r:?}");
        assert_eq!(r.targets.len(), 1);
        assert_eq!(r.targets[0].start_byte, text.find("pick(int i)").unwrap() as u32);
    }
}
#[test]
fn unsupported_complete_own_families_do_not_supply_values() {
    for extra in [
        "static int count(){return 1;}",
        "int count(Missing x){return 1;}",
        "<T> int count(){return 1;}",
        "int count(int... xs){return 1;}",
        "long count(int i){return 1;}",
    ] {
        let text = strict_value_case(extra, "void run(Product product){product.use(count(1));}");
        assert_ne!(query(&project(&text), "use(count(1))").status, ResolveStatus::Resolved, "{extra}");
    }
}
#[test]
fn static_generic_nested_and_lambda_callers_do_not_borrow_this() {
    for caller in [
        "static void run(Product product){product.use(count(1));}",
        "<T> void run(Product product){product.use(count(1));}",
        "class Inner {void run(Product product){product.use(count(1));}}",
        "void run(Product product){ Runnable r=()->{product.use(count(1));};}",
    ] {
        let text = strict_value_case("", caller);
        assert_ne!(query(&project(&text), "use(count(1))").status, ResolveStatus::Resolved, "{caller}");
    }
    let text = "class Product {public void use(int i){}} class Calls<T> {private int count(int i){return i;} void run(Product product){product.use(count(1));}}";
    assert_ne!(query(&project(text), "use(count(1))").status, ResolveStatus::Resolved);
}
#[test]
fn inherited_family_and_reference_ambiguity_remain_unknown() {
    let text = "class Base {public int count(long i){return 0;}} class Product {public void use(int i){} public void use(long i){}} class Calls extends Base {private int count(int i){return 0;} void run(Product product){product.use(count(1));}}";
    assert_ne!(query(&project(text), "use(count(1))").status, ResolveStatus::Resolved);
    let text = "class A {} class B {} class Product {public void use(int i){} public void use(long i){}} class Calls {private int count(A a){return 0;} private int count(B b){return 0;} void run(Product product){product.use(count(null));}}";
    assert_ne!(query(&project(text), "use(count(null))").status, ResolveStatus::Resolved);
}
#[test]
fn nested_implicit_calls_share_budget_and_exact_selection() {
    for count in [0, 5000] {
        let filler = (0..count).map(|i| format!("int unused{i};")).collect::<String>();
        let text = strict_value_case(
            "int count(long i){return 0;}",
            &format!("void run(Product product){{{filler} product.use(count(count(1)));}}"),
        );
        let r = query(&project(&text), "use(count(count(1)))");
        assert_eq!(r.status == ResolveStatus::Resolved, count == 0, "{r:?}");
        if count == 0 {
            assert_eq!(r.targets.len(), 1);
            assert_eq!(r.targets[0].start_byte, text.find("use(int i)").unwrap() as u32);
        }
    }
}
#[test]
fn previous_static_primitive_proof_still_selects_exactly() {
    let text = "class Calls {static int pick(int i){return i;} static long pick(long i){return i;} static void run(int x){pick(x);}}";
    let r = query(&project(text), "pick(x)");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].start_byte, text.find("pick(int i)").unwrap() as u32);
}
#[test]
fn own_instance_parameter_slots_do_not_gain_a_static_slot() {
    for count in [254, 255] {
        let params = (0..count).map(|i| format!("int a{i}")).collect::<Vec<_>>().join(",");
        let text = strict_value_case(
            &format!("int count({params}){{return 0;}}"),
            "void run(Product product){product.use(count(1));}",
        );
        let r = query(&project(&text), "use(count(1))");
        assert_eq!(r.status == ResolveStatus::Resolved, count == 254, "slots={count}: {r:?}");
    }
}
