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

fn project(fields: &str, method: &str) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", &format!("import api.Box; class A {{ public void hit() {{}} public A next() {{return this;}} }} class B {{ public void hit() {{}} }} class Sink {{ public A pick(A a) {{return a;}} public B pick(B b) {{return b;}} public A number(int i) {{return null;}} public B number(long l) {{return null;}} }} class Calls {{ {fields} {method} }}"));
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, None, &[])).unwrap();
    p.add_class_bytes(
        "Box.class",
        &binary(
            "api/Box",
            Some("java/lang/Object"),
            0x21,
            Some("<T:Ljava/lang/Object;>Ljava/lang/Object;"),
            &[("get", "()Ljava/lang/Object;", Some("()TT;"), 0x101)],
        ),
    )
    .unwrap();
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
fn own_plain_fields_supply_actual_and_receiver_evidence() {
    for access in ["private", "protected", "public", ""] {
        for expr in [
            "a.next().hit()",
            "this.a.next().hit()",
            "sink.pick(a).hit()",
            "sink.pick(this.a).hit()",
            "sink.number(n).hit()",
            "sink.number(this.n).hit()",
        ] {
            check(
                &project(&format!("{access} A a; {access} int n;"), &format!("void run(Sink sink) {{ {expr}; }}")),
                Some("A"),
            );
        }
    }
}
#[test]
fn applied_fields_reuse_receiver_instantiation_without_raw_actuals() {
    for expr in ["box.get().hit()", "this.box.get().hit()"] {
        check(&project("private Box<A> box;", &format!("void run() {{ {expr}; }}")), Some("A"));
    }
    check(&project("private Box<A> box;", "void run(Sink sink) { sink.pick(box).hit(); }"), None);
    for decl in ["Box box;", "Box<?> box;", "Box<Box<A>> box;", "Box<A>[] box;"] {
        check(&project(decl, "void run() { box.get().hit(); }"), None);
    }
}
#[test]
fn local_shadow_is_not_absence_but_this_bypasses_it() {
    check(&project("A a;", "void run(B a) { a.hit(); }"), Some("B"));
    check(&project("A a;", "void run(B a) { this.a.next().hit(); }"), Some("A"));
    for params in ["Unknown a", "int a", "B a, B a"] {
        check(&project("A a;", &format!("void run({params}) {{ a.next().hit(); }}")), None);
    }
    check(&project("A a;", "void run() { Unknown a = null; a.next().hit(); }"), None);
    check(&project("A a;", "void run() { { Unknown a = null; } a.next().hit(); }"), Some("A"));
    // A method-local type parameter cannot resolve to the top-level A field type name.
    check(&project("A a;", "<A> void run() { a.next().hit(); }"), None);
}
#[test]
fn static_ambiguous_array_or_invalid_field_declarations_refuse() {
    for fields in
        ["static A a;", "A a; A a;", "A[] a;", "A a[];", "Unknown a;", "final volatile A a;", "public private A a;"]
    {
        for expr in ["a.next().hit()", "this.a.next().hit()"] {
            check(&project(fields, &format!("void run() {{ {expr}; }}")), None);
        }
    }
    check(&project("A a, other[];", "void run() { a.next().hit(); }"), Some("A"));
    check(&project("A a[], other;", "void run() { a.next().hit(); }"), None);
}
#[test]
fn unsupported_receiver_and_caller_contexts_never_supply_field_types() {
    for method in [
        "static void run() { a.next().hit(); }",
        "static void run() { this.a.next().hit(); }",
        "Calls() { a.next().hit(); }",
        "{ a.next().hit(); }",
        "void run(Calls other) { other.a.next().hit(); }",
        "void run() { Runnable r = () -> { a.next().hit(); }; }",
        "class Nested { void run() { a.next().hit(); } }",
    ] {
        // An ordinary constructor now has its own caller certificate; the
        // field's declared A type is unchanged (no initializer inference).
        let wanted = (method == "Calls() { a.next().hit(); }").then_some("A");
        check(&project("A a;", method), wanted);
    }
    let mut p = project("", "void run() { a.next().hit(); }");
    let text = p
        .file("Calls.java")
        .unwrap()
        .text()
        .replace("class Calls {", "class Parent { A a; } class Calls extends Parent {");
    p.insert("Calls.java", &text);
    check(&p, None);
}
#[test]
fn field_proofs_share_existing_work_and_depth_budgets() {
    let locals: String = (0..5000).map(|i| format!("int x{i}=0;")).collect();
    check(&project("A a;", &format!("void run() {{ {locals} a.next().hit(); }}")), None);
    let expr = format!("{}this.a{}.next().hit()", "(".repeat(40), ")".repeat(40));
    check(&project("A a;", &format!("void run() {{ {expr}; }}")), None);
}

#[test]
fn own_field_hiding_is_not_inherited_selection_or_value_inference() {
    let mut p = project("private A a;", "void run() { this.a.next().hit(); }");
    let text = p
        .file("Calls.java")
        .unwrap()
        .text()
        .replace("class Calls {", "class Parent { public B a; } class Calls extends Parent {");
    p.insert("Calls.java", &text);
    check(&p, Some("A"));
    // An unknown own field hides the parent; it cannot borrow either the initializer or parent type.
    let text = text.replace("private A a;", "Unknown a = new A();");
    p.insert("Calls.java", &text);
    check(&p, None);
    for fields in ["@Deprecated A a;", "A a, broken[;", "A a", "int a;"] {
        check(&project(fields, "void run() { this.a.next().hit(); }"), None);
    }
    for expression in ["Calls.this.a.next().hit()", "super.a.next().hit()", "(this).a.next().hit()"] {
        check(&project("A a;", &format!("void run() {{ {expression}; }}")), None);
    }
}

#[test]
fn primitive_and_applied_shadows_keep_their_own_declared_category() {
    check(&project("int n;", "void run(Sink sink, long n) { sink.number(n).hit(); }"), Some("B"));
    check(&project("int n;", "void run(Sink sink, long n) { sink.number(this.n).hit(); }"), Some("A"));
    check(&project("Box<A> box;", "void run(Box<B> box) { box.get().hit(); }"), Some("B"));
    check(&project("Box<A> box;", "void run(Box<B> box) { this.box.get().hit(); }"), Some("A"));
    check(&project("Box<A> box;", "void run(Unknown box) { box.get().hit(); }"), None);
    let fields: String = (0..5000).map(|i| format!("int x{i};")).collect();
    check(&project(&format!("{fields} A a;"), "void run() { a.next().hit(); }"), None);
}
