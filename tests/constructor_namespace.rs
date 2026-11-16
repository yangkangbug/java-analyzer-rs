use java_engine::{Project, ResolveResult, ResolveStatus, TargetKind};
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

fn project(text: &str, platform: bool) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", text);
    if platform {
        p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, None, &[])).unwrap();
    }
    p
}
fn query(p: &Project, marker: &str) -> ResolveResult {
    let text = p.file("Calls.java").unwrap().text();
    let at = (text.find(marker).unwrap() + marker.len()) as u32;
    let r = p.resolve_at("Calls.java", at);
    assert_eq!(r, p.prepare().resolve_at("Calls.java", at));
    r
}
fn exact(p: &Project, marker: &str, owner: &str, decl: &str) {
    let r = query(p, marker);
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].kind, TargetKind::Method);
    assert_eq!(r.targets[0].owner, owner);
    assert_eq!(r.targets[0].path, "Calls.java");
    assert_eq!(r.targets[0].start_byte, p.file("Calls.java").unwrap().text().find(decl).unwrap() as u32);
}
#[test]
fn same_spelled_constructor_never_competes_with_an_ordinary_method() {
    let text = "class C { C(){} public void C(){} void run(){/*call*/C();} }";
    for platform in [false, true] {
        let p = project(text, platform);
        exact(&p, "/*call*/", "C", "C(){} void run");
    }
    let p = project(
        "class C {C(int x){} C(long x){} public void C(int x){} public void C(long x){} void run(int value){/*call*/C(value);}}",
        true,
    );
    exact(&p, "/*call*/", "C", "C(int x){} public void");
}
#[test]
fn constructors_are_neither_callable_methods_nor_inherited_candidates() {
    for text in [
        "class C {C(){} void run(){/*call*/C();}}",
        "class Base {public Base(){}} class C extends Base {void run(){/*call*/Base();}}",
        "class C {public C(){} void run(C c){c./*call*/C();}}",
    ] {
        for platform in [false, true] {
            let r = query(&project(text, platform), "/*call*/");
            assert_ne!(r.status, ResolveStatus::Resolved, "{text}: {r:?}");
            assert!(r.targets.is_empty(), "constructor candidate leaked: {r:?}");
        }
    }
}
#[test]
fn same_named_ordinary_method_is_still_inherited_and_qualified() {
    for platform in [false, true] {
        let p = project(
            "class Base {public Base(){} public void Base(){}} class C extends Base {void run(){/*call*/Base();}}",
            platform,
        );
        exact(&p, "/*call*/", "Base", "Base(){}} class C");
    }
    let p = project("class C {public C(){} public void C(){} void run(C c){c./*call*/C();}}", true);
    exact(&p, "/*call*/", "C", "C(){} void run");
}
#[test]
fn declaration_navigation_and_new_expression_keep_their_existing_target_kinds() {
    let p = project("class C { /*ctor*/C(){} void C(){} void run(){new /*new*/C();}}", true);
    let r = query(&p, "/*ctor*/");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].kind, TargetKind::Method);
    assert_eq!(r.targets[0].start_byte, p.file("Calls.java").unwrap().text().find("/*ctor*/").unwrap() as u32 + 8);
    let r = query(&p, "/*new*/");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].kind, TargetKind::Class);
    assert_eq!(r.targets[0].start_byte, 6);
}
#[test]
fn source_and_binary_method_inventories_agree_about_constructor_exclusion() {
    for include_method in [false, true] {
        let source = format!(
            "class Base {{public Base(){{}} {}}} class Calls extends Base {{void run(){{/*call*/Base();}}}}",
            if include_method { "public void Base(){}" } else { "" }
        );
        let p = project(&source, true);
        let sr = query(&p, "/*call*/");
        let mut b = project("class Calls extends Base {void run(){/*call*/Base();}}", true);
        let mut methods = vec![("<init>", "()V", None, 0x1)];
        if include_method {
            methods.push(("Base", "()V", None, 0x101));
        }
        b.add_class_bytes("Base.class", &binary("Base", Some("java/lang/Object"), 0x21, None, &methods)).unwrap();
        let br = query(&b, "/*call*/");
        assert_eq!(sr.targets.len(), usize::from(include_method), "{sr:?}");
        assert_eq!(br.targets.len(), usize::from(include_method), "{br:?}");
        if include_method {
            assert_eq!(br.targets[0].name, "Base");
            assert_eq!(br.targets[0].binary.as_ref().unwrap().descriptor, "()V");
        }
    }
}

#[test]
fn edits_repartition_declarations_and_preserve_independent_old_project() {
    use java_engine::TextEdit;
    let text = "class C { /*decl*/C(){} void run(){/*call*/C();}}";
    let mut p = project(text, true);
    let old = p.clone();
    let old_prepared = old.prepare();
    let call = (text.find("/*call*/").unwrap() + 8) as u32;
    let before = old_prepared.resolve_at("Calls.java", call);
    assert!(before.targets.is_empty(), "{before:?}");
    let at = (text.find("/*decl*/").unwrap() + 8) as u32;
    p.edit("Calls.java", TextEdit { start_byte: at, end_byte: at, insert: "void ".into() }).unwrap();
    exact(&p, "/*call*/", "C", "C(){} void run");
    let fresh = project(p.file("Calls.java").unwrap().text(), true);
    assert_eq!(query(&p, "/*call*/"), query(&fresh, "/*call*/"));
    assert_eq!(old_prepared.resolve_at("Calls.java", call), before);
    p.edit("Calls.java", TextEdit { start_byte: at, end_byte: at + 5, insert: String::new() }).unwrap();
    assert_eq!(p.file("Calls.java").unwrap().text(), text);
    assert_eq!(query(&p, "/*call*/"), before);
    let decl = query(&p, "/*decl*/");
    assert_eq!(decl.status, ResolveStatus::Resolved);
    assert_eq!(decl.targets[0].kind, TargetKind::Method);
}
