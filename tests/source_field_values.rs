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

fn array_project(text: &str) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", text);
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, None, &[])).unwrap();
    p.add_class_bytes(
        "Api.class",
        &binary(
            "api/Api",
            Some("java/lang/Object"),
            0x21,
            None,
            &[("pick", "(Ljava/lang/Object;)V", None, 0x109), ("pick", "(I)V", None, 0x109)],
        ),
    )
    .unwrap();
    p
}
fn array_pick(p: &Project, descriptor: Option<&str>) {
    let text = p.file("Calls.java").unwrap().text();
    let at = text.rfind("pick(").unwrap() as u32;
    let result = p.resolve_at("Calls.java", at);
    assert_eq!(result, p.prepare().resolve_at("Calls.java", at));
    if let Some(descriptor) = descriptor {
        assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}\n{text}");
        assert_eq!(result.targets.len(), 1);
        let target = &result.targets[0];
        assert_eq!(target.kind, java_engine::TargetKind::Method);
        assert_eq!((&*target.path, &*target.owner, &*target.name), ("Api.class", "api.Api", "pick"));
        let identity = target.binary.as_ref().unwrap();
        assert_eq!(identity.binary_name, "api/Api");
        assert_eq!(identity.descriptor, descriptor);
    } else {
        assert_ne!(result.status, ResolveStatus::Resolved, "{result:?}\n{text}");
    }
}
#[test]
fn certified_own_arrays_are_shared_actual_values() {
    for ty in ["boolean", "byte", "short", "char", "int", "long", "float", "double", "A"] {
        for rank in 1..=3 {
            for root in ["a", "this.a"] {
                let source = format!(
                    "import api.Api; class A {{}} class Calls {{ private {ty}{} a; void f() {{ Api.pick({root}); }} }}",
                    "[]".repeat(rank)
                );
                array_pick(&array_project(&source), Some("(Ljava/lang/Object;)V"));
            }
        }
    }
}

#[test]
fn field_array_projection_uses_the_same_rank_and_index_rules() {
    for (ty, desc) in [
        ("boolean", "Z"),
        ("byte", "B"),
        ("short", "S"),
        ("char", "C"),
        ("int", "I"),
        ("long", "J"),
        ("float", "F"),
        ("double", "D"),
    ] {
        for rank in 1..=3 {
            for root in ["a", "this.a"] {
                for used in 1..=rank {
                    let text = format!(
                        "import api.Api; class Calls {{ {ty}{} a; void f(int i) {{ Api.pick({root}{}); }} }}",
                        "[]".repeat(rank),
                        "[i]".repeat(used)
                    );
                    let mut p = array_project(&text);
                    let scalar = format!("({desc})V");
                    p.add_class_bytes(
                        "Api.class",
                        &binary(
                            "api/Api",
                            Some("java/lang/Object"),
                            0x21,
                            None,
                            &[("pick", "(Ljava/lang/Object;)V", None, 0x109), ("pick", &scalar, None, 0x109)],
                        ),
                    )
                    .unwrap();
                    array_pick(&p, Some(if used == rank { &scalar } else { "(Ljava/lang/Object;)V" }));
                }
            }
        }
    }
    for index in ["long", "boolean", "float", "double", "Unknown"] {
        array_pick(
            &array_project(&format!(
                "import api.Api; class Calls {{ int[] a; void f({index} i) {{ Api.pick(a[i]); }} }}"
            )),
            None,
        );
    }
    for index in ["byte", "short", "char", "int"] {
        array_pick(
            &array_project(&format!(
                "import api.Api; class A {{}} class Calls {{ A[] a; void f({index} i) {{ Api.pick(this.a[i]); }} }}"
            )),
            Some("(Ljava/lang/Object;)V"),
        );
    }
    array_pick(
        &array_project("import api.Api; class A {} class Calls { A[][] a; void f(int i) { Api.pick(a[i]); }}"),
        None,
    );
}

#[test]
fn field_array_evidence_does_not_skip_unknown_shadow_or_expand_owners() {
    for declaration in [
        "static int[] a;",
        "int[] a; int[] a;",
        "Unknown[] a;",
        "A<Unknown>[] a;",
        "@Deprecated int[] a;",
        "final volatile int[] a;",
        "int[] a = ;",
    ] {
        array_pick(
            &array_project(&format!(
                "import api.Api; class A<T> {{}} class Calls {{ {declaration} void f() {{ Api.pick(a); }} }}"
            )),
            None,
        );
    }
    for parameter in ["Unknown[] a", "Unknown a", "int[] a, int[] a"] {
        array_pick(
            &array_project(&format!(
                "import api.Api; class Calls {{ int[] a; void f({parameter}) {{ Api.pick(a); }} }}"
            )),
            None,
        );
        if !parameter.contains(',') {
            array_pick(
                &array_project(&format!(
                    "import api.Api; class Calls {{ int[] a; void f({parameter}) {{ Api.pick(this.a); }} }}"
                )),
                Some("(Ljava/lang/Object;)V"),
            );
        }
    }
    for body in [
        "static void f(){Api.pick(a);}",
        "void f(Calls c){Api.pick(c.a);}",
        "void f(){Runnable r=()->{Api.pick(a);};}",
        "class Nested {void f(){Api.pick(a);}}",
        "void f(){ Object o=new Object(){void g(){Api.pick(a);}}; }",
    ] {
        array_pick(&array_project(&format!("import api.Api; class Calls {{ int[] a; {body} }}")), None);
    }
    for expr in ["a.length", "this.a.length", "a + 1", "this..a", "super.a", "Calls.this.a"] {
        array_pick(
            &array_project(&format!("import api.Api; class Calls {{ int[] a; void f() {{ Api.pick({expr}); }} }}")),
            None,
        );
    }
    for prefix in ["class Calls<T> { T[] a;", "class Calls { void f(){class A{}} A[] a;"] {
        // The first is a formal component; the second is missing nominal A.
        array_pick(&array_project(&format!("import api.Api; {prefix} void run() {{ Api.pick(a); }} }}")), None);
    }
    let too_many = "[]".repeat(256);
    array_pick(
        &array_project(&format!("import api.Api; class Calls {{ int{too_many} a; void f(){{Api.pick(a);}} }}")),
        None,
    );
}

#[test]
fn field_array_suffixes_and_edits_preserve_declaring_evidence() {
    for (fields, expr, desc) in [
        ("int a[], b;", "a", "(Ljava/lang/Object;)V"),
        ("int a, b[];", "a", "(I)V"),
        ("int[] a[], b;", "a[0]", "(Ljava/lang/Object;)V"),
    ] {
        array_pick(
            &array_project(&format!("import api.Api; class Calls {{ {fields} void f(){{ Api.pick({expr}); }} }}")),
            Some(desc),
        );
    }
    let original = "import api.Api; class Calls { int[] a; void f(){Api.pick(a);} }";
    let mut p = array_project(original);
    let old = array_project(original);
    let snapshot = old.prepare();
    let at = original.find("[]").unwrap() as u32;
    p.edit("Calls.java", java_engine::TextEdit { start_byte: at, end_byte: at + 2, insert: "".into() }).unwrap();
    array_pick(&p, Some("(I)V"));
    let fresh = array_project(&original.replacen("[]", "", 1));
    let now = p.file("Calls.java").unwrap().text().rfind("pick(").unwrap() as u32;
    assert_eq!(p.resolve_at("Calls.java", now), fresh.resolve_at("Calls.java", now));
    p.edit("Calls.java", java_engine::TextEdit { start_byte: at, end_byte: at, insert: "[]".into() }).unwrap();
    array_pick(&p, Some("(Ljava/lang/Object;)V"));
    let query = original.rfind("pick(").unwrap() as u32;
    assert_eq!(p.resolve_at("Calls.java", query), snapshot.resolve_at("Calls.java", query));
}

#[test]
fn field_array_component_uses_its_own_compilation_and_imports() {
    use java_engine::{
        ClasspathEntry, CompilationConfig, CompilationId, ProjectConfig, WorkspaceConfig, WorkspaceSnapshot,
    };
    use std::fs;
    let root = std::env::temp_dir().join(format!("own-field-array-scope-{}", std::process::id()));
    for dir in ["one", "two", "platform/java/lang", "platform/api"] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    fs::write(root.join("platform/java/lang/Object.class"), binary("java/lang/Object", None, 0x21, None, &[])).unwrap();
    fs::write(
        root.join("platform/api/Api.class"),
        binary(
            "api/Api",
            Some("java/lang/Object"),
            0x21,
            None,
            &[("pick", "([Lmodel/A;)V", None, 0x109), ("pick", "(Ljava/lang/Object;)V", None, 0x109)],
        ),
    )
    .unwrap();
    let text = "import api.Api; import model.A; class Calls { A[] a; void f(){Api.pick(this.a);} }";
    for dir in ["one", "two"] {
        fs::write(root.join(dir).join("Calls.java"), text).unwrap();
        fs::write(root.join(dir).join("A.java"), format!("package model; public class A {{ int {dir}; }}")).unwrap();
    }
    let ids = [CompilationId::new("one", "main"), CompilationId::new("two", "main")];
    let config = WorkspaceConfig {
        compilations: ids
            .iter()
            .zip(["one", "two"])
            .map(|(id, dir)| CompilationConfig {
                id: id.clone(),
                inputs: ProjectConfig {
                    source_roots: vec![root.join(dir)],
                    classpath: vec![ClasspathEntry::Directory(root.join("platform"))],
                    ..Default::default()
                },
                dependencies: vec![],
            })
            .collect(),
        ..Default::default()
    };
    let original = WorkspaceSnapshot::load(&config).unwrap();
    let prepared = original.prepare();
    let at = text.rfind("pick(").unwrap() as u32;
    for (id, dir) in ids.iter().zip(["one", "two"]) {
        let path = root.join(dir).join("Calls.java").canonicalize().unwrap().to_str().unwrap().to_owned();
        let r = original.scope(id).unwrap().resolve_at(&path, at);
        assert_eq!(r, prepared.scope(id).unwrap().resolve_at(&path, at));
        assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
        assert_eq!(r.targets.len(), 1);
        let target = &r.targets[0];
        assert_eq!((&*target.name, &*target.owner), ("pick", "api.Api"));
        assert_eq!(target.binary.as_ref().unwrap().descriptor, "([Lmodel/A;)V");
    }
    // A second same-name component declaration in the same scope is not
    // silently combined merely because both array shapes have rank one.
    fs::write(root.join("one/Duplicate.java"), "package model; public class A {}").unwrap();
    let changed = WorkspaceSnapshot::load(&config).unwrap();
    let path = root.join("one/Calls.java").canonicalize().unwrap().to_str().unwrap().to_owned();
    assert_ne!(changed.scope(&ids[0]).unwrap().resolve_at(&path, at).status, ResolveStatus::Resolved);
    assert_eq!(prepared.scope(&ids[0]).unwrap().resolve_at(&path, at).status, ResolveStatus::Resolved);
    fs::remove_dir_all(root).unwrap();
}
