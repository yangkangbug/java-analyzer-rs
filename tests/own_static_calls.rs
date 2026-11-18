use java_engine::{Project, ResolveResult, ResolveStatus, TextEdit};

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
fn bare_static_uses_complete_array_family_and_private_access() {
    let text = "class Calls { private static void pick(int[] a,int n){} private static void pick(Object[] a,int n){} static void run(int[] a){pick(a,a.length);} }";
    let p = project(text);
    let r = query(&p, "pick(a,a.length)");
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets[0].start_byte, text.find("pick(int[]").unwrap() as u32);
}
#[test]
fn bare_static_result_composition_is_strict() {
    let text = "class A {} class B extends A {} class Product { public void use(int n){} public void use(long n){} } class Calls { private static Product pick(A a){return null;} private static Product pick(B b){return null;} private static int count(B b){return 1;} static void run(B b){pick(b).use(count(b));} }";
    let p = project(text);
    for marker in ["pick(b)", "use(count(b))"] {
        assert_eq!(query(&p, marker).status, ResolveStatus::Resolved, "{marker}");
    }
}
#[test]
fn illegal_instance_winner_must_not_fall_back() {
    for family in ["static void pick(long x){} void pick(int x){}", "static void pick(Object x){} void pick(int x){}"] {
        let text = format!("class Calls {{{family} static void run(){{pick(1);}}}}");
        assert_ne!(query(&project(&text), "pick(1)").status, ResolveStatus::Resolved, "{family}");
    }
}

#[test]
fn own_static_method_namespace_and_access_do_not_become_type_qualification() {
    for access in ["private", "protected", "", "public"] {
        let text = format!(
            "class Calls {{ int pick; {access} static void pick(int[] a){{}} {access} static void pick(Object[] a){{}} static void run(int[] a,int pick){{pick(a);}} }}"
        );
        let r = query(&project(&text), "pick(a)");
        assert_eq!(r.status, ResolveStatus::Resolved, "{access}: {r:?}");
        assert_eq!(r.targets[0].start_byte, text.find("pick(int[]").unwrap() as u32);
    }
}
#[test]
fn static_winner_keeps_instance_competitors_and_strict_order() {
    let text = "class Calls { static void pick(int x){} void pick(long x){} static void run(){pick(1);} }";
    let r = query(&project(text), "pick(1)");
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets[0].start_byte, text.find("pick(int").unwrap() as u32);
}
#[test]
fn complete_family_unknowns_and_inherited_names_are_terminal() {
    for extra in [
        "static void pick(Missing x){}",
        "static <T> void pick(T x){}",
        "static void pick(int... x){}",
        "static void pick(int[] x){}",
    ] {
        let text = format!(
            "class Calls {{static void pick(int[] a){{}} static void pick(Object[] a){{}} {extra} static void run(int[] a){{pick(a);}}}}"
        );
        assert_ne!(query(&project(&text), "pick(a)").status, ResolveStatus::Resolved, "{extra}");
    }
    let text = "class Base { static void pick(int[] a){} } class Calls extends Base {static void pick(Object a){} static void pick(long a){} static void run(int[] a){pick(a);} }";
    assert_ne!(query(&project(text), "pick(a)").status, ResolveStatus::Resolved);
}
#[test]
fn imports_do_not_override_an_own_method_family() {
    let text = "import static Other.pick; class Other { public static void pick(int[] a){} } class Calls {private static void pick(int[] a){} private static void pick(Object[] a){} static void run(int[] a){pick(a);} }";
    let r = query(&project(text), "pick(a)");
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets[0].owner, "Calls");
    assert_eq!(r.targets[0].start_byte, text.find("pick(int[] a){} private").unwrap() as u32);
}
#[test]
fn unsupported_owner_contexts_do_not_borrow_static_proof() {
    for text in [
        "class Calls<T> {static void pick(int[] a){} static void pick(Object[] a){} static void run(int[] a){pick(a);} }",
        "class Outer {static class Calls {static void pick(int[] a){} static void pick(Object[] a){} static void run(int[] a){pick(a);} }}",
        "class Calls {static void pick(int[] a){} static void pick(Object[] a){} static <T> void run(int[] a){pick(a);} }",
    ] {
        assert_ne!(query(&project(text), "pick(a)").status, ResolveStatus::Resolved, "{text}");
    }
}
#[test]
fn edits_rebuild_complete_static_family_without_changing_old_snapshot() {
    let good =
        "class Calls {static void pick(int[] a){} static void pick(Object[] a){} static void run(int[] a){pick(a);} }";
    let bad = good.replace("static void pick(int[]", "void pick(int[]");
    let mut p = project(good);
    let old_project = project(good);
    let old = old_project.prepare();
    let at = good.rfind("pick(a)").unwrap() as u32;
    let start = good.find("static void pick(int[]").unwrap() as u32;
    for (text, width, insert) in [(&bad[..], 7, ""), (good, 0, "static ")] {
        p.edit("Calls.java", TextEdit { start_byte: start, end_byte: start + width, insert: insert.into() }).unwrap();
        let now = text.rfind("pick(a)").unwrap() as u32;
        assert_eq!(p.resolve_at("Calls.java", now), project(text).resolve_at("Calls.java", now));
        assert_eq!(p.resolve_at("Calls.java", now), p.prepare().resolve_at("Calls.java", now));
    }
    assert_eq!(old.resolve_at("Calls.java", at).status, ResolveStatus::Resolved);
}

#[test]
fn fixed_arity_varargs_uses_declared_array_shape_without_expansion() {
    for (declarations, call, target) in [
        ("static void pick(boolean a){} static void pick(Object... a){}", "pick(true)", "pick(boolean"),
        ("static void pick(int a){} static void pick(Object... a){}", "pick(1)", "pick(int"),
        ("static void pick(int... a){} static void pick(Object a){}", "pick(values)", "pick(int"),
        ("static void pick(int[]... a){} static void pick(Object a){}", "pick(rows)", "pick(int"),
        ("static void pick(int... a){} static void pick(Object a){}", "pick(null)", "pick(int"),
    ] {
        let text = format!("class Calls {{{declarations} static void run(int[] values,int[][] rows){{{call};}}}}");
        let r = query(&project(&text), call);
        assert_eq!(r.status, ResolveStatus::Resolved, "{text}: {r:?}");
        assert_eq!(r.targets[0].start_byte, text.find(target).unwrap() as u32);
    }
    let text =
        "class Calls { static void pick(int... a){} static void pick(Object... a){} static void run(){pick(1,2);} }";
    assert_ne!(query(&project(text), "pick(1,2)").status, ResolveStatus::Resolved);
}
#[test]
fn binary_varargs_shape_is_validated_before_fixed_applicability() {
    for bad in ["()V", "(I)V"] {
        let text = "class Calls {static void run(int[] a){Api.pick(a);}}";
        let mut p = project(text);
        p.add_class_bytes(
            "Api.class",
            &binary("Api", Some("java/lang/Object"), 0x21, &[("pick", "([I)V", 0x9, None), ("pick", bad, 0x89, None)]),
        )
        .unwrap();
        assert_ne!(query(&p, "pick(a)").status, ResolveStatus::Resolved, "{bad}");
    }
    let text = "class Calls {static void run(int[] a){Api.pick(a);}}";
    let mut p = project(text);
    p.add_class_bytes(
        "Api.class",
        &binary(
            "Api",
            Some("java/lang/Object"),
            0x21,
            &[("pick", "([I)V", 0x89, None), ("pick", "(Ljava/lang/Object;)V", 0x9, None)],
        ),
    )
    .unwrap();
    let r = query(&p, "pick(a)");
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets[0].binary.as_ref().unwrap().descriptor, "([I)V");
}
#[test]
fn varargs_and_array_declarations_have_identical_erasure() {
    let text =
        "class Calls {static void pick(int[] a){} static void pick(int... a){} static void run(int[] a){pick(a);} }";
    assert_ne!(query(&project(text), "pick(a)").status, ResolveStatus::Resolved);
}
#[test]
fn primitive_array_projection_uses_all_integral_index_types_not_unboxing() {
    for primitive in ["boolean", "byte", "short", "char", "int", "long", "float", "double"] {
        for index in ["byte", "short", "char", "int"] {
            let text = format!(
                "class Calls {{static void pick({primitive} a){{}} static void pick(Object a){{}} static void run({primitive}[][] values,{index} i){{pick((values[i])[i]);}}}}"
            );
            let r = query(&project(&text), "pick((values[i])[i])");
            assert_eq!(r.status, ResolveStatus::Resolved, "{primitive}/{index}: {r:?}");
            assert_eq!(r.targets[0].start_byte, text.find(&format!("pick({primitive}")).unwrap() as u32);
        }
    }
    for index in ["long", "boolean", "float", "double", "Integer"] {
        let text = format!(
            "class Calls {{static void pick(int a){{}} static void pick(Object a){{}} static void run(int[] values,{index} i){{pick(values[i]);}}}}"
        );
        assert_ne!(query(&project(&text), "pick(values[i])").status, ResolveStatus::Resolved, "{index}");
    }
}
#[test]
fn reference_rank_one_projection_survives_without_opening_rank_two() {
    let text = "class A {} class B extends A {} class Calls {static void pick(A a){} static void pick(B b){} static void run(B[] a,int i){pick(a[i]);} }";
    let r = query(&project(text), "pick(a[i])");
    assert_eq!(r.status, ResolveStatus::Resolved, "{r:?}");
    assert_eq!(r.targets[0].start_byte, text.find("pick(B b)").unwrap() as u32);
    let text = text.replace("B[] a", "B[][] a").replace("pick(a[i]);", "pick(a[i][i]);");
    assert_ne!(query(&project(&text), "pick(a[i][i])").status, ResolveStatus::Resolved);
}
