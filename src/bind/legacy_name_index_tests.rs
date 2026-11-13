use super::*;

fn binary(origin: &str, package: &str, nesting: &[&str], raw: &str) -> Arc<ClassSignature> {
    Arc::new(ClassSignature {
        origin: origin.into(),
        binary_name: raw.into(),
        dotted_name: if package.is_empty() { nesting.join(".") } else { format!("{package}.{}", nesting.join(".")) },
        package: package.into(),
        nesting: nesting.iter().map(|s| (*s).into()).collect(),
        super_class: None,
        interfaces: vec![],
        binary_super_class: None,
        binary_interfaces: vec![],
        signature: None,
        major: 61,
        minor: 0,
        is_interface: false,
        is_final: false,
        access: Access::Public,
        fields: vec![],
        methods: vec![],
    })
}

fn locations(classes: Vec<&ClassRec>) -> Vec<*const ClassRec> {
    classes.into_iter().map(|class| class as *const ClassRec).collect()
}

fn assert_scan_equivalence(index: &Index) {
    let mut names: BTreeSet<String> =
        index.files.iter().flat_map(|f| f.classes.iter()).chain(index.binaries.iter()).map(ClassRec::dotted).collect();
    names.extend(["missing.Type".into(), "".into(), "p.Outer$Inner".into()]);
    for scope in 0..index.visibility.len() {
        for name in &names {
            let expected = index.classes(scope).filter(|class| class.dotted() == *name).collect();
            assert_eq!(locations(index.legacy_named(scope, name)), locations(expected), "scope={scope}, name={name}");
        }
    }
}

#[test]
fn legacy_name_index_preserves_visibility_order_duplicates_and_dotted_aliases() {
    let mut projects = vec![Project::new(); 4];
    for (i, project) in projects.iter_mut().enumerate() {
        project.insert(format!("s{i}/Z.java"), "package p; public class Same {}");
        project.insert(format!("s{i}/A.java"), "package p; class Same {} class Outer { class Inner {} }");
        project.insert_class_signature(binary(&format!("scope{i}/one.class"), "p", &["Same"], "p/Same"));
        project.insert_class_signature(binary(&format!("scope{i}/two.class"), "p", &["Same"], "p/Same"));
        // Both legacy dotted aliases must survive; strict JVM identity is not changed here.
        project.insert_class_signature(binary(
            &format!("scope{i}/nested.class"),
            "p",
            &["Outer", "Inner"],
            "p/Outer$Inner",
        ));
        project.insert_class_signature(binary(
            &format!("scope{i}/package.class"),
            "p.Outer",
            &["Inner"],
            "p/Outer/Inner",
        ));
    }
    let visibility =
        vec![BTreeSet::from([0, 2]), BTreeSet::from([0, 1, 2]), BTreeSet::from([0, 2]), BTreeSet::from([3])];
    let index = Index::scoped(&projects.iter().collect::<Vec<_>>(), &visibility);
    assert_scan_equivalence(&index);
    for scope in [0, 2] {
        let found = index.legacy_named(scope, "p.Same");
        assert_eq!(found.len(), 6);
        assert!(found[..4].iter().all(|c| !c.binary));
        assert!(found[4..].iter().all(|c| c.binary && c.scope == scope));
        assert_eq!(found.iter().map(|c| c.scope).collect::<Vec<_>>(), [0, 0, 2, 2, scope, scope]);
        assert_eq!(index.legacy_named(scope, "p.Outer.Inner").len(), 4);
    }
}

fn assert_query_equal(a: ResolveResult, b: ResolveResult) {
    assert_eq!(a.status, b.status);
    assert_eq!(a.targets, b.targets);
}

#[test]
fn legacy_name_index_public_lookup_and_old_new_prepared_snapshots() {
    let use_text = "package app; import p.Base; class Use extends Base { p.Base field; int run() { return value; } }";
    let mut old = Project::new();
    old.insert("Use.java", use_text);
    old.insert("Base.java", "package p; public class Base { public int value; }");
    let old_prepared = old.prepare();
    let old_index = Index::build(&old.files, &old.binaries);
    assert_scan_equivalence(&old_index);
    let queries = [
        use_text.find("p.Base").unwrap() + 2,
        use_text.find("extends Base").unwrap() + 8,
        use_text.find("p.Base field").unwrap() + 2,
        use_text.find("value").unwrap(),
    ];
    let mut new = old.clone();
    new.insert("Duplicate.java", "package p; public class Base { public int value; }");
    let new_prepared = new.prepare();
    assert_scan_equivalence(&Index::build(&new.files, &new.binaries));
    for byte in queries {
        let old_result = old_prepared.resolve_at("Use.java", byte as u32);
        assert_query_equal(old_result, old.resolve_at("Use.java", byte as u32));
        assert_query_equal(new_prepared.resolve_at("Use.java", byte as u32), new.resolve_at("Use.java", byte as u32));
    }
    assert_eq!(old_prepared.resolve_at("Use.java", queries[2] as u32).status, ResolveStatus::Resolved);
    assert_eq!(new_prepared.resolve_at("Use.java", queries[2] as u32).status, ResolveStatus::Ambiguous);
    // Missing explicit imports do not fall back to another package or nested spelling.
    let mut missing = Project::new();
    missing.insert("Use.java", "import absent.Base; class Use { Base field; }");
    missing.insert("Base.java", "class Base {}");
    let text = missing.file("Use.java").unwrap().text();
    let at = text.find("Base field").unwrap() as u32;
    assert_eq!(missing.prepare().resolve_at("Use.java", at).status, ResolveStatus::Unresolved);
}

#[test]
fn legacy_name_index_visits_only_matching_buckets() {
    let mut first = Project::new();
    let mut second = Project::new();
    for i in 0..128 {
        first.insert(format!("Unrelated{i}.java"), &format!("package unrelated; class Type{i} {{}}"));
        second.insert_class_signature(binary(
            &format!("unrelated/{i}.class"),
            "unrelated",
            &[&format!("Binary{i}")],
            &format!("unrelated/Binary{i}"),
        ));
    }
    first.insert("Same.java", "package p; class Same {}");
    second.insert("Same.java", "package p; class Same {}");
    first.insert_class_signature(binary("first/Same.class", "p", &["Same"], "p/Same"));
    second.insert_class_signature(binary("second/Same.class", "p", &["Same"], "p/Same"));
    let index = Index::scoped(&[&first, &second], &[BTreeSet::from([0, 1]), BTreeSet::from([1])]);
    LEGACY_NAME_BUCKET_ENTRIES.with(|count| count.set(0));
    let found = index.legacy_named(0, "p.Same");
    assert_eq!(found.len(), 3);
    assert_eq!(
        LEGACY_NAME_BUCKET_ENTRIES.with(|count| count.get()),
        4,
        "matching bucket entries include the correctly rejected dependency binary"
    );
    LEGACY_NAME_BUCKET_ENTRIES.with(|count| count.set(0));
    assert!(index.legacy_named(0, "not.present").is_empty());
    assert_eq!(LEGACY_NAME_BUCKET_ENTRIES.with(|count| count.get()), 0);
    LEGACY_NAME_BUCKET_ENTRIES.with(|count| count.set(0));
    assert_eq!(index.legacy_named(1, "p.Same").len(), 2);
    assert_eq!(LEGACY_NAME_BUCKET_ENTRIES.with(|count| count.get()), 2, "invisible scope buckets are not inspected");
}

#[test]
fn legacy_name_index_own_binaries_survive_empty_or_self_excluded_visibility() {
    let mut own = Project::new();
    own.insert("Own.java", "package p; class Same {}");
    own.insert_class_signature(binary("own/Same.class", "p", &["Same"], "p/Same"));
    let mut dependency = Project::new();
    dependency.insert("Dependency.java", "package p; class Same {}");
    dependency.insert_class_signature(binary("dependency/Same.class", "p", &["Same"], "p/Same"));
    for visible in [BTreeSet::new(), BTreeSet::from([1])] {
        let index = Index::scoped(&[&own, &dependency], &[visible.clone(), BTreeSet::new()]);
        assert_scan_equivalence(&index);
        let found = index.legacy_named(0, "p.Same");
        assert_eq!(found.len(), visible.len() + 1);
        assert!(found.last().unwrap().binary);
        assert_eq!(found.last().unwrap().scope, 0);
        assert!(found[..found.len() - 1].iter().all(|c| !c.binary && c.scope == 1));
    }
}
