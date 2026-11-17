use java_engine::{Project, ResolveResult, ResolveStatus};

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
const BOX: &str = "<T:Ljava/lang/Object;>Ljava/lang/Object;";
const GET: Method<'_> = ("get", "()Ljava/lang/Object;", Some("()TT;"), 0x101);
const PUT: Method<'_> = ("put", "(Ljava/lang/Object;)Ljava/lang/Object;", Some("(TT;)TT;"), 0x101);
fn project(sig: &str, methods: &[Method<'_>], params: &str, expr: &str, interface: bool) -> Project {
    let mut p = Project::new();
    p.insert("Calls.java", &format!("import api.Box; class A {{ public void hit() {{}} }} class B {{ public void hit() {{}} }} class Calls {{ void run({params}) {{ {expr}; }} }}"));
    p.add_class_bytes("Object.class", &binary("java/lang/Object", None, 0x21, None, &[])).unwrap();
    p.add_class_bytes(
        "Box.class",
        &binary("api/Box", Some("java/lang/Object"), if interface { 0x601 } else { 0x21 }, Some(sig), methods),
    )
    .unwrap();
    p
}
fn query(p: &Project) -> ResolveResult {
    let text = p.file("Calls.java").unwrap().text();
    let at = text.rfind("hit()").unwrap() as u32;
    let r = p.resolve_at("Calls.java", at);
    assert_eq!(r, p.prepare().resolve_at("Calls.java", at));
    r
}
fn exact(p: &Project, owner: &str) {
    let r = query(p);
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets.len(), 1, "{r:?}");
    let t = &r.targets[0];
    assert_eq!(t.owner, owner);
    assert_eq!(t.path, "Calls.java");
    let text = p.file("Calls.java").unwrap().text();
    let start = text.find(&format!("class {owner} ")).unwrap();
    assert_eq!(t.start_byte, (start + text[start..].find("hit()").unwrap()) as u32);
}
fn refused(p: &Project) {
    let r = query(p);
    assert_ne!(r.status, ResolveStatus::Resolved, "{r:?}");
}
fn source_box(declaration: &str, params: &str, expr: &str) -> Project {
    let mut p = project(BOX, &[GET, PUT], params, expr, false);
    p.remove_class("Box.class");
    p.insert("Box.java", declaration);
    p
}
const SOURCE: &str =
    "package api; public class Box<T> { public T get() { return null; } public T put(T value) { return value; } }";
#[test]
fn source_binary_direct_families_and_return_chains_agree() {
    for (params, expr, owner) in [
        ("Box<A> box, A a", "box.get().hit()", "A"),
        ("Box<A> box, A a", "box.put(a).hit()", "A"),
        ("Box<B> box, B b", "box.put(b).hit()", "B"),
    ] {
        exact(&project(BOX, &[GET, PUT], params, expr, false), owner);
        exact(&source_box(SOURCE, params, expr), owner);
    }
    refused(&source_box(SOURCE, "Box<A> box, B b", "box.put(b).hit()"));
}
#[test]
fn explicit_object_bound_is_identity_not_spelling() {
    let p = source_box(
        "package api; public class Box<Object,T extends Object> { public T get(){return null;} }",
        "Box<A,A> box",
        "box.get().hit()",
    );
    refused(&p);
    let mut missing = source_box(SOURCE, "Box<A> box", "box.get().hit()");
    missing.remove_class("Object.class");
    refused(&missing);
    exact(&source_box(&SOURCE.replace("<T>", "<T extends java.lang.Object>"), "Box<A> box", "box.get().hit()"), "A");
    exact(&source_box(&SOURCE.replace("<T>", "<T extends Object>"), "Box<A> box", "box.get().hit()"), "A");
    let mut p = source_box(&SOURCE.replace("<T>", "<T extends Object>"), "Box<A> box", "box.get().hit()");
    p.insert("Object.java", "package api; public class Object {}");
    refused(&p);
    for declaration in [
        SOURCE.replace("<T>", "<T extends T>"),
        SOURCE.replace("<T>", "<T extends Number>"),
        SOURCE.replace("<T>", "<T extends Object & Runnable>"),
        SOURCE.replace("<T>", "<T,T>"),
    ] {
        refused(&source_box(&declaration, "Box<A> box", "box.get().hit()"));
    }
}
#[test]
fn member_type_shadow_does_not_become_owner_formal() {
    let declaration = SOURCE.replace("public T get()", "class T {} public T get()");
    refused(&source_box(&declaration, "Box<A> box", "box.get().hit()"));
    let declaration = "package api; public class Box<Box> { public Box get(){return null;} }";
    exact(&source_box(declaration, "Box<A> box", "box.get().hit()"), "A");
}
#[test]
fn full_family_rejects_unknown_static_and_erasure_clashes() {
    let p = source_box(
        "package api; public class Box<T,U> { public T put(T x){return x;} public U put(U x){return x;} }",
        "Box<A,B> box, A a",
        "box.put(a).hit()",
    );
    refused(&p);
    for addition in [
        "public Object put(Object x){return x;}", // erased duplicate, even with distinct actual A
        "public T put(T x, int y){return x;} public T put(T x, int y){return x;}",
        "public <U> T put(U x, int y){return null;}",
        "public T[] put(int x){return null;}",
        "public static T put(int x){return null;}",
        "public T put(java.util.List<T> x, int y){return null;}",
        "public T put(T.Inner x, int y){return null;}",
    ] {
        let declaration = SOURCE.replace("public T get()", &format!("{addition} public T get()"));
        refused(&source_box(&declaration, "Box<A> box, A a", "box.put(a).hit()"));
    }
}
#[test]
fn substituted_collision_is_not_selected_arbitrarily() {
    let declaration = "package api; public class Box<T> { public T put(T x){return x;} public Object put(java.lang.Object x, int y){return x;} }";
    exact(&source_box(declaration, "Box<A> box, A a", "box.put(a).hit()"), "A");
    // A declaration whose nominal formal coincides after substitution stays ambiguous.
    let mut p = source_box(
        "package api; public class Box<T> { public T put(T x){return x;} public Item put(Item x){return x;} }",
        "Box<api.Item> box, api.Item a",
        "box.put(a).hit()",
    );
    p.insert("Item.java", "package api; public class Item {public void hit(){}}");
    refused(&p);
}
#[test]
fn unsupported_owner_and_actual_forms_remain_refused() {
    for declaration in [
        SOURCE.replace("class Box<T>", "class Box<T> extends Object"),
        SOURCE.replace("class Box<T>", "class Box<T> implements Runnable"),
        SOURCE.replace("class Box<T>", "interface Box<T>"),
        SOURCE.replace("class Box<T>", "class Box<T extends java.lang.Number>"),
    ] {
        refused(&source_box(&declaration, "Box<A> box", "box.get().hit()"));
    }
    for ty in ["Box", "Box<?>", "Box<? extends A>", "Box<A[]>", "Box<Box<A>>", "Box<A,B>"] {
        refused(&source_box(SOURCE, &format!("{ty} box"), "box.get().hit()"));
    }
}
#[test]
fn nominal_types_use_declaration_imports_and_void_stays_void() {
    let mut p = source_box(
        "package api; import one.Item; public class Box<T> { public Item get(){return null;} public void put(T value){} }",
        "Box<A> box, A a",
        "box.get().hit()",
    );
    p.insert("Item.java", "package one; public class Item { public void hit(){} }");
    p.insert("Other.java", "package two; public class Item { public void hit(){} }");
    p.insert(
        "Calls.java",
        "import api.Box; import two.Item; class A {} class Calls { void run(Box<A> box){box.get().hit();} }",
    );
    let r = query(&p);
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets[0].owner, "one.Item");
    let p = source_box(
        "package api; public class Box<T> { public void put(T value){} }",
        "Box<A> box, A a",
        "box.put(a).hit()",
    );
    refused(&p);
    let text = p.file("Calls.java").unwrap().text();
    let r = p.resolve_at("Calls.java", text.rfind("put(a)").unwrap() as u32);
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets[0].owner, "api.Box");
}
#[test]
fn throws_headers_and_primitive_results_compose() {
    let p = source_box(
        "package api; public class Box<T> { public T get() throws java.lang.Exception {return null;} public int size(){return 1;} }",
        "Box<A> box",
        "box.get().hit()",
    );
    exact(&p, "A");
    let p = source_box(
        "package api; public class Box<T> { public T get(){return null;} public T put(int value){return null;} public int size(){return 1;} }",
        "Box<A> box",
        "box.put(box.size()).hit()",
    );
    exact(&p, "A");
}
#[test]
fn edit_fresh_and_old_snapshot_agree() {
    let original = source_box(SOURCE, "Box<A> box, A a", "box.put(a).hit()");
    let mut changed = original.clone();
    let old = original.prepare();
    let source = SOURCE.replace("public T put(T value)", "public T put(int value)");
    let from = SOURCE.find("T value").unwrap() as u32;
    changed
        .edit("Box.java", java_engine::TextEdit { start_byte: from, end_byte: from + 1, insert: "int".into() })
        .unwrap();
    let fresh = source_box(&source, "Box<A> box, A a", "box.put(a).hit()");
    assert_eq!(query(&changed), query(&fresh));
    refused(&changed);
    exact(&original, "A");
    let text = original.file("Calls.java").unwrap().text();
    let at = text.rfind("hit()").unwrap() as u32;
    assert_eq!(old.resolve_at("Calls.java", at), original.resolve_at("Calls.java", at));
}
#[test]
fn source_owner_formal_limits_are_closed() {
    for count in [32, 33] {
        let formals = (0..count).map(|i| format!("T{i}")).collect::<Vec<_>>().join(",");
        let actuals = std::iter::repeat_n("A", count).collect::<Vec<_>>().join(",");
        let declaration = format!("package api; public class Box<{formals}> {{ public T0 get(){{return null;}} }}");
        let p = source_box(&declaration, &format!("Box<{actuals}> box"), "box.get().hit()");
        if count == 32 {
            exact(&p, "A");
        } else {
            refused(&p);
        }
    }
    let declaration = SOURCE.replace("<T>", &format!("<T /*{}*/>", "x".repeat(4096)));
    refused(&source_box(&declaration, "Box<A> box", "box.get().hit()"));
}
#[test]
fn cross_compilation_compatible_bound_keeps_actual_result_identity() {
    use java_engine::{
        ClasspathEntry, CompilationConfig, CompilationId, ProjectConfig, WorkspaceConfig, WorkspaceSnapshot,
    };
    use std::fs;
    let root = std::env::temp_dir().join(format!("java-source-generics-{}", std::process::id()));
    for folder in ["lib", "app", "platform/java/lang"] {
        fs::create_dir_all(root.join(folder)).unwrap();
    }
    fs::write(root.join("platform/java/lang/Object.class"), binary("java/lang/Object", None, 0x21, None, &[])).unwrap();
    fs::write(root.join("lib/Box.java"), SOURCE).unwrap();
    fs::write(root.join("lib/Item.java"), "package api; public class Item { public void hit(){} }").unwrap();
    let caller = "import api.Box; import api.Item; class Local {public void hit(){}} class Calls { void good(Box<Item> box){box.get().hit();} void bad(Box<Local> box){box.get().hit();} }";
    fs::write(root.join("app/Calls.java"), caller).unwrap();
    let lib = CompilationId::new("lib", "main");
    let app = CompilationId::new("app", "main");
    let config = WorkspaceConfig {
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
                id: app.clone(),
                inputs: ProjectConfig {
                    source_roots: vec![root.join("app")],
                    classpath: vec![ClasspathEntry::Directory(root.join("platform"))],
                    ..Default::default()
                },
                dependencies: vec![lib],
            },
        ],
        ..Default::default()
    };
    let workspace = WorkspaceSnapshot::load(&config).unwrap();
    let prepared = workspace.prepare();
    let path = root.join("app/Calls.java").canonicalize().unwrap().to_str().unwrap().to_owned();
    for (at, owner) in [(caller.find("hit();").unwrap(), "api.Item"), (caller.rfind("hit();").unwrap(), "Local")] {
        let fresh = workspace.scope(&app).unwrap().resolve_at(&path, at as u32);
        assert_eq!(fresh, prepared.scope(&app).unwrap().resolve_at(&path, at as u32));
        assert_eq!(fresh.status, ResolveStatus::Resolved, "{fresh:?}");
        assert_eq!(fresh.targets.len(), 1);
        assert_eq!(fresh.targets[0].owner, owner);
        if owner == "Local" {
            assert_eq!(fresh.targets[0].path, path);
            assert_eq!(fresh.targets[0].start_byte, caller.find("hit(){}").unwrap() as u32);
        }
    }
    // Equal names/bytes from a different platform origin are not this
    // compatibility certificate. The retained original snapshot stays valid.
    fs::create_dir_all(root.join("other-platform/java/lang")).unwrap();
    fs::write(root.join("other-platform/java/lang/Object.class"), binary("java/lang/Object", None, 0x21, None, &[]))
        .unwrap();
    let mut different = config.clone();
    different.compilations.iter_mut().find(|c| c.id == app).unwrap().inputs.classpath =
        vec![ClasspathEntry::Directory(root.join("other-platform"))];
    let incompatible = WorkspaceSnapshot::load(&different).unwrap();
    let at = caller.rfind("hit();").unwrap() as u32;
    let refused = incompatible.scope(&app).unwrap().resolve_at(&path, at);
    assert_ne!(refused.status, ResolveStatus::Resolved);
    assert_eq!(refused, incompatible.prepare().scope(&app).unwrap().resolve_at(&path, at));
    assert_eq!(prepared.scope(&app).unwrap().resolve_at(&path, at).status, ResolveStatus::Resolved);
    fs::remove_dir_all(root).unwrap();
}
