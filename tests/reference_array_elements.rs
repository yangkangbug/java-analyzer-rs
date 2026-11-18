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
fn explicit_reference_array_element_is_a_receiver_and_strict_argument() {
    let text = "class A {} class B extends A {} class Item {public B get(){return null;} public void run(int i){} public void run(long i){}} class Sink {public void take(A a){} public void take(B b){}} class Calls {void use(Item[] items, B[] values, int i, Sink sink){items[i].run(i); sink.take(values[i]); sink.take(items[i].get());}}";
    let p = project(text);
    for (needle, decl) in
        [("run(i);", "run(int i)"), ("take(values[i])", "take(B b)"), ("take(items[i].get())", "take(B b)")]
    {
        let r = query(&p, needle);
        assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
        assert_eq!(r.targets[0].start_byte, text.find(decl).unwrap() as u32);
    }
}

const TYPES: &str = "class Item {public Item next(){return null;} public void run(int i){} public void run(long i){}} class Sink {public void take(Item x){} public void take(long x){}}";
fn checked(text: &str, marker: &str, yes: bool) {
    let r = query(&project(text), marker);
    assert_eq!(r.status == ResolveStatus::Resolved, yes, "{text}\n{r:?}");
    if yes {
        assert_eq!(r.targets.len(), 1);
    }
}
#[test]
fn arrays_never_become_scalar_references() {
    for expr in ["items", "items[0][0]", "matrix[0]", "field[0]", "this.field[0]", "make()[0]"] {
        let text = format!(
            "{TYPES} class Calls {{Item[] field; Item[] make(){{return null;}} void m(Item[] items, Item[][] matrix,Sink sink){{sink.take({expr});}}}}"
        );
        if matches!(expr, "field[0]" | "this.field[0]") {
            // Shared certified field ArrayValue now composes with the original
            // rank-one argument projection, not general receiver inference.
            let r = query(&project(&text), &format!("take({expr})"));
            assert_eq!(r.status, ResolveStatus::Resolved);
            assert_eq!(
                r.targets,
                vec![java_engine::Target {
                    binary: None,
                    path: "Calls.java".into(),
                    kind: java_engine::TargetKind::Method,
                    name: "take".into(),
                    owner: "Sink".into(),
                    start_byte: 121,
                    end_byte: 125,
                }]
            );
        } else {
            checked(&text, &format!("take({expr})"), false);
        }
    }
    let text = format!("{TYPES} class Calls {{void m(Item[] items){{items.run(1);}}}}");
    checked(&text, "run(1)", false);
}
#[test]
fn explicit_prefix_suffix_and_per_declarator_dimensions() {
    for (decl, expr, yes) in [
        ("Item[] items", "items[0]", true),
        ("Item items[]", "items[0]", true),
        ("Item[] items[]", "items[0]", false),
        ("Item[][] items", "items[0]", false),
        ("Item... items", "items[0]", false),
    ] {
        let text = format!("{TYPES} class Calls {{void m(Sink sink,{decl}){{sink.take({expr});}}}}");
        checked(&text, &format!("take({expr})"), yes);
    }
    for (decl, expr, yes) in [
        ("Item a=null,b[]=null;", "b[0]", true),
        ("Item a[]=null,b=null;", "b[0]", false),
        ("Item[] a=null,b[]=null;", "a[0]", true),
        ("Item[] a=null,b[]=null;", "b[0]", false),
        ("Item[] a=null,b=null;", "b[0]", true),
    ] {
        let text = format!("{TYPES} class Calls {{void m(Sink sink){{{decl}sink.take({expr});}}}}");
        checked(&text, &format!("take({expr})"), yes);
    }
}
#[test]
fn index_types_are_proved_not_guessed() {
    for (ty, yes) in [
        ("byte", true),
        ("short", true),
        ("char", true),
        ("int", true),
        ("long", false),
        ("float", false),
        ("double", false),
        ("boolean", false),
        ("Item", false),
    ] {
        let text = format!("{TYPES} class Calls {{void m(Item[] items,{ty} i){{items[i].run(1);}}}}");
        checked(&text, "run(1)", yes);
    }
    for expr in ["null", "unknown", "1L", "true", "", "0,1"] {
        let text = format!("{TYPES} class Calls {{void m(Item[] items){{items[{expr}].run(1);}}}}");
        checked(&text, "run(1)", false);
    }
}
#[test]
fn generic_fields_catches_and_expired_locals_do_not_supply_arrays() {
    for text in [
        format!("{TYPES} class Calls {{void m(Sink sink){{{{Item[] items=null;}}sink.take(items[0]);}}}}"),
        format!("{TYPES} class Calls {{void m(Sink sink){{try{{}}catch(Item items){{sink.take(items[0]);}}}}}}"),
        format!("{TYPES} class Calls {{void m(Sink sink){{try{{}}catch(Item[] items){{sink.take(items[0]);}}}}}}"),
        format!("{TYPES} class Calls {{void m(Sink sink,Item[] items){{int items=0;sink.take(items[0]);}}}}"),
        "class Item<T> {public void run(int i){}} class Calls {void m(Item<Item>[] items){items[0].run(1);}}".into(),
        "class Item<T> {public void run(int i){}} class Calls {void m(Item[] items){items[0].run(1);}}".into(),
    ] {
        let marker = if text.contains("sink.take") { "take(items[0])" } else { "run(1)" };
        checked(&text, marker, false);
    }
}
#[test]
fn element_chains_parentheses_and_implicit_index_results_compose() {
    let text = format!(
        "{TYPES} class Calls {{int index(){{return 0;}} void m(Item[] items,int i,Sink sink){{((items))[index()].next().run(i);sink.take((items[i]));}}}}"
    );
    checked(&text, "run(i)", true);
    checked(&text, "take((items[i]))", true);
}
#[test]
fn array_proof_keeps_shared_work_and_depth_limits() {
    for n in [0, 5000] {
        let filler = (0..n).map(|i| format!("int unused{i};")).collect::<String>();
        let text = format!("{TYPES} class Calls {{void m(Item[] items){{{filler}items[0].run(1);}}}}");
        checked(&text, "run(1)", n == 0);
    }
    for n in [1, 40] {
        let expr = format!("{}items{}[0]", "(".repeat(n), ")".repeat(n));
        let text = format!("{TYPES} class Calls {{void m(Item[] items){{{expr}.run(1);}}}}");
        checked(&text, "run(1)", n == 1);
    }
}

#[test]
fn binary_element_identity_and_qualified_source_types_are_preserved() {
    let text = "import lib.Item; class Calls {void m(Item[] items,int i){items[i].run(i);}}";
    let mut p = project(text);
    p.add_class_bytes(
        "Item.class",
        &binary(
            "lib/Item",
            Some("java/lang/Object"),
            0x21,
            &[("run", "(I)V", 0x101, None), ("run", "(J)V", 0x101, None)],
        ),
    )
    .unwrap();
    let r = query(&p, "run(i)");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].owner, "lib.Item");
    assert_eq!(r.targets[0].binary.as_ref().unwrap().descriptor, "(I)V");
    let text = "class Item {} class Calls {void m(lib.Item[] items,int i){items[i].run(i);}}";
    let mut p = project(text);
    p.insert("SourceItem.java", "package lib; public class Item {public void run(int i){} public void run(long i){}}");
    let r = query(&p, "run(i)");
    assert_eq!(r.status, ResolveStatus::Resolved);
    assert_eq!(r.targets.len(), 1);
    assert_eq!(r.targets[0].owner, "lib.Item");
    assert_eq!(r.targets[0].path, "SourceItem.java");
    p.insert("DuplicateItem.java", "package lib; public class Item {public void run(int i){}}");
    assert_ne!(query(&p, "run(i)").status, ResolveStatus::Resolved);
}
