use java_engine::{Project, ResolveStatus};
type Method<'a> = (&'a str, &'a str, Option<&'a str>, u16);
fn binary(name: &str, parent: Option<&str>, flags: u16, signature: Option<&str>, methods: &[Method<'_>]) -> Vec<u8> {
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
    let attr = utf(&mut cp, &mut n, "Signature");
    let signature = signature.map(|s| utf(&mut cp, &mut n, s));
    let methods: Vec<_> = methods
        .iter()
        .map(|(name, desc, sig, flags)| {
            (*flags, utf(&mut cp, &mut n, name), utf(&mut cp, &mut n, desc), sig.map(|s| utf(&mut cp, &mut n, s)))
        })
        .collect();
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61];
    u(&mut b, n);
    b.extend(cp);
    for v in [flags, this, parent, 0, 0, methods.len() as u16] {
        u(&mut b, v);
    }
    for (flags, name, desc, sig) in methods {
        for v in [flags, name, desc, u16::from(sig.is_some())] {
            u(&mut b, v);
        }
        if let Some(sig) = sig {
            u(&mut b, attr);
            b.extend(2u32.to_be_bytes());
            u(&mut b, sig);
        }
    }
    u(&mut b, u16::from(signature.is_some()));
    if let Some(sig) = signature {
        u(&mut b, attr);
        b.extend(2u32.to_be_bytes());
        u(&mut b, sig);
    }
    b
}

fn project(body: &str) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", &format!("class A {{public A next(){{return this;}} public void hit(){{}}}} class B {{public void hit(){{}}}} class Sink {{public A take(A a){{return a;}} public B take(B b){{return b;}} public A num(int i){{return null;}} public B num(long i){{return null;}}}} class Calls {{ A field; int number; A helper(){{return null;}} {body} }}"));
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, None, &[])).unwrap();
    p
}
fn check(p: &Project, wanted: Option<&str>) {
    let text = p.file("Calls.java").unwrap().text();
    let at = text.rfind("hit()").unwrap() as u32;
    let r = p.resolve_at("Calls.java", at);
    assert_eq!(r, p.prepare().resolve_at("Calls.java", at));
    if let Some(owner) = wanted {
        assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}\n{text}");
        assert_eq!(r.targets.len(), 1);
        assert_eq!(r.targets[0].owner, owner);
        assert_eq!(r.targets[0].path, "Calls.java");
        let start = text.find(&format!("class {owner} ")).unwrap();
        assert_eq!(r.targets[0].start_byte, (start + text[start..].find("hit()").unwrap()) as u32);
    } else {
        assert_ne!(r.status, ResolveStatus::Resolved, "{r:?}\n{text}");
    }
}
#[test]
fn ordinary_constructors_reuse_parameter_local_and_own_instance_proofs() {
    for expr in [
        "a.next().hit()",
        "A local=a; local.next().hit()",
        "sink.take(a).hit()",
        "sink.num(i).hit()",
        "this.field.next().hit()",
        "field.next().hit()",
        "sink.take(this.field).hit()",
        "sink.num(number).hit()",
        "helper().hit()",
    ] {
        check(&project(&format!("Calls(A a,Sink sink,int i){{{expr};}}")), Some("A"));
    }
    for access in ["public", "protected", "private", ""] {
        check(&project(&format!("{access} Calls(A a){{a.next().hit();}}")), Some("A"));
    }
}
#[test]
fn delegation_is_a_whole_constructor_boundary_including_type_arguments() {
    for call in ["this();", "super();", "<A>this();", "<A>super();", "outer.super();", "outer.<A>super();"] {
        check(&project(&format!("Calls(){{}} Calls(A a, Object outer){{{call} a.next().hit();}}")), None);
    }
    // Calls inside delegation arguments also cannot borrow a post-delegation instance context.
    check(&project("Calls(A a){} Calls(){this(helper().next()); helper().hit();}"), None);
}
#[test]
fn constructor_and_owner_headers_do_not_bypass_caller_restrictions() {
    for body in [
        "<T> Calls(A a){a.next().hit();}",
        "Wrong(A a){a.next().hit();}",
        "static Calls(A a){a.next().hit();}",
        "Calls(A a {a.next().hit();}",
        "Calls(A a,A a){a.next().hit();}",
        "{field.next().hit();}",
        "static {field.next().hit();}",
        "class Nested {Nested(A a){a.next().hit();}}",
        "Calls(A a){Runnable r=()->{a.next().hit();};}",
    ] {
        check(&project(body), None);
    }
    for decl in ["class Calls<T>", "record Calls()"] {
        let mut p = project("Calls(A a){a.next().hit();}");
        let text = p.file("Calls.java").unwrap().text().replace("class Calls", decl);
        p.insert("Calls.java", &text);
        check(&p, None);
    }
}
#[test]
fn constructor_shadowing_and_expired_locals_keep_declaration_types() {
    check(&project("Calls(B field){field.hit();}"), Some("B"));
    check(&project("Calls(B field){this.field.next().hit();}"), Some("A"));
    check(&project("Calls(){ {B field=null;} field.next().hit();}"), Some("A"));
    check(&project("Calls(Unknown field){field.next().hit();}"), None);
    check(&project("Calls(A a){B a=null; a.next().hit();}"), None);
    check(&project("Calls(Sink sink,long number){sink.num(number).hit();}"), Some("B"));
    check(&project("Calls(Sink sink,long number){sink.num(this.number).hit();}"), Some("A"));
}
#[test]
fn ordinary_method_behavior_is_preserved_and_constructor_is_not_a_method_result() {
    check(&project("Calls(){} void run(A a){a.next().hit();}"), Some("A"));
    check(&project("Calls(){} void run(){Calls().hit();}"), None);
    check(&project("Calls(){} Calls(A a){Calls().hit();}"), None);
}
#[test]
fn constructor_context_reuses_work_and_depth_limits() {
    let locals: String = (0..5000).map(|i| format!("int x{i}=0;")).collect();
    check(&project(&format!("Calls(A a){{{locals} a.next().hit();}}")), None);
    let expr = format!("{}a{}.next().hit()", "(".repeat(40), ")".repeat(40));
    check(&project(&format!("Calls(A a){{{expr};}}")), None);
}

#[test]
fn constructor_only_modifiers_and_valid_delegations_do_not_borrow_method_permissions() {
    for modifier in ["final", "synchronized", "native", "abstract", "strictfp", "public private"] {
        check(&project(&format!("{modifier} Calls(A a){{a.next().hit();}}")), None);
    }
    for body in
        ["<T> Calls(T arg,int i){} Calls(A a){<A>this(a,0);a.next().hit();}", "Calls(A a){super();a.next().hit();}"]
    {
        check(&project(body), None);
    }
    // An explicit delegation occurring after a query still invalidates the whole constructor certificate.
    check(&project("Calls(){} Calls(A a){a.next().hit();this();}"), None);
}
