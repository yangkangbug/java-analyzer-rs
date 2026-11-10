use super::*;

fn signature(origin: &str, name: &str, parent: Option<&str>) -> Arc<ClassSignature> {
    let (package, simple) = name.rsplit_once('.').unwrap_or(("", name));
    Arc::new(ClassSignature {
        origin: origin.into(),
        binary_name: name.replace('.', "/"),
        dotted_name: name.into(),
        package: package.into(),
        nesting: vec![simple.into()],
        super_class: parent.map(str::to_string),
        interfaces: Vec::new(),
        signature: None,
        major: 61,
        minor: 0,
        is_interface: false,
        is_final: false,
        access: Access::Public,
        fields: vec![member("value", "I", MemberKind::Field, Access::Public)],
        methods: vec![member("read", "()I", MemberKind::Method, Access::Public)],
    })
}

fn member(name: &str, descriptor: &str, kind: MemberKind, access: Access) -> MemberSignature {
    MemberSignature { name: name.into(), descriptor: descriptor.into(), signature: None, kind, access }
}

fn reset_counts() {
    BINARY_PAYLOAD_BUILDS.with(|count| count.set(0));
    BINARY_MEMBER_BUILDS.with(|count| count.set(0));
    BINARY_PAYLOAD_CACHE_LOOKUPS.with(|count| count.set(0));
    BINARY_PAYLOAD_CACHE_MAX_ENTRIES.with(|count| count.set(0));
}

#[test]
fn scoped_binary_payload_is_constructed_once_per_shared_signature() {
    let mut project = Project::new();
    project.insert_class_signature(signature("shared.jar!/lib/Lib.class", "lib.Lib", None));
    let projects = [project.clone(), project.clone(), project.clone(), project];
    let refs = projects.iter().collect::<Vec<_>>();
    let visibility = (0..4).map(|scope| BTreeSet::from([scope])).collect::<Vec<_>>();
    reset_counts();
    let index = Index::scoped(&refs, &visibility);
    assert_eq!(index.binaries.len(), 4, "contextual records are not deduplicated");
    assert_eq!(BINARY_PAYLOAD_BUILDS.with(|count| count.get()), 1);
    assert_eq!(BINARY_MEMBER_BUILDS.with(|count| count.get()), 2);
    assert_eq!(BINARY_PAYLOAD_CACHE_LOOKUPS.with(|count| count.get()), 4);
    assert_eq!(BINARY_PAYLOAD_CACHE_MAX_ENTRIES.with(|count| count.get()), 1);
}

#[test]
fn single_compilation_constructs_payloads_without_reuse_map_work() {
    let mut project = Project::new();
    project.insert_class_signature(signature("lib.jar!/Lib.class", "Lib", None));
    project.insert_class_signature(signature("lib.jar!/Other.class", "Other", None));
    project.insert("Use.java", "class Use extends Lib { int get() { return value; } }");
    reset_counts();
    INDEX_BUILDS.with(|count| count.set(0));
    let index = Index::scoped(&[&project], &isolated(1));
    assert_eq!(INDEX_BUILDS.with(|count| count.get()), 1);
    assert_eq!(index.files.len(), 1);
    assert_eq!(index.visibility, isolated(1));
    assert_eq!(
        index.binaries.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(),
        ["lib.jar!/Lib.class", "lib.jar!/Other.class"]
    );
    assert_eq!(BINARY_PAYLOAD_BUILDS.with(|count| count.get()), 2);
    assert_eq!(BINARY_MEMBER_BUILDS.with(|count| count.get()), 4);
    assert_eq!(BINARY_PAYLOAD_CACHE_LOOKUPS.with(|count| count.get()), 0);
    assert_eq!(BINARY_PAYLOAD_CACHE_MAX_ENTRIES.with(|count| count.get()), 0);
    for class in &index.binaries {
        assert_eq!(class.scope, 0);
        assert_eq!(Arc::strong_count(payload(class)), 1);
    }
    assert_every_offset(&[&project], &isolated(1));
}

#[test]
fn empty_scoped_index_keeps_its_existing_empty_visibility_and_build_count() {
    reset_counts();
    INDEX_BUILDS.with(|count| count.set(0));
    let index = Index::scoped(&[], &[]);
    assert_eq!(INDEX_BUILDS.with(|count| count.get()), 1);
    assert!(index.files.is_empty());
    assert!(index.binaries.is_empty());
    assert!(index.visibility.is_empty());
    assert_eq!(BINARY_PAYLOAD_BUILDS.with(|count| count.get()), 0);
    assert_eq!(BINARY_PAYLOAD_CACHE_LOOKUPS.with(|count| count.get()), 0);
    assert_eq!(BINARY_PAYLOAD_CACHE_MAX_ENTRIES.with(|count| count.get()), 0);
}

fn payload(class: &ClassRec) -> &Arc<ClassPayload> {
    let ClassData::Binary(payload) = &class.data else { panic!("expected a binary record") };
    payload
}

// Deliberately reconstruct every binary occurrence, matching the pre-reuse
// construction strategy. This is a sharing differential, not a Java oracle.
fn uncached(projects: &[&Project], visibility: &[BTreeSet<usize>]) -> Index {
    let mut files = Vec::new();
    let mut binaries = Vec::new();
    for (scope, project) in projects.iter().enumerate() {
        for (path, parsed) in &project.files {
            let mut file = index_file(path, parsed);
            file.scope = scope;
            for class in &mut file.classes {
                class.scope = scope;
            }
            files.push(file);
        }
        for signature in project.binaries.values() {
            binaries.push(ClassRec { scope, data: ClassData::Binary(Arc::new(class_from_signature(signature))) });
        }
    }
    Index::with_reference_names(files, binaries, visibility.to_vec())
}

fn isolated(scopes: usize) -> Vec<BTreeSet<usize>> {
    (0..scopes).map(|scope| BTreeSet::from([scope])).collect()
}

fn named<'a>(index: &'a Index, scope: usize, name: &str) -> &'a ClassRec {
    match index.class_named(scope, name) {
        Named::One(class) => class,
        _ => panic!("expected unique {name} in {scope}"),
    }
}

fn assert_every_offset(projects: &[&Project], visibility: &[BTreeSet<usize>]) {
    let prepared = PreparedScopedIndex::new(projects.to_vec(), visibility);
    let independent = uncached(projects, visibility);
    for scope in 0..projects.len() {
        for (owner, project) in projects.iter().enumerate() {
            for (path, parsed) in &project.files {
                for byte in (0..=parsed.text().len() as u32 + 1).chain([u32::MAX]) {
                    let expected = if visibility[scope].contains(&owner) {
                        independent.resolve(path, parsed, byte)
                    } else {
                        ResolveResult::none(ResolveStatus::Unresolved)
                    };
                    assert_eq!(prepared.resolve_at(scope, path, byte), expected, "{scope}: {path}:{byte}");
                    assert_eq!(resolve_scoped(projects, visibility, scope, path, byte), expected);
                }
            }
        }
        assert_eq!(prepared.resolve_at(scope, "missing.java", 0).status, ResolveStatus::Unresolved);
    }
}

fn query(index: &Index, project: &Project, path: &str, name: &str) -> ResolveResult {
    let parsed = project.file(path).unwrap();
    index.resolve(path, parsed, parsed.text().rfind(name).unwrap() as u32)
}

#[test]
fn binary_payload_sharing_is_index_local_and_released_with_the_index() {
    let shared = signature("lib.jar!/Lib.class", "Lib", None);
    let mut project = Project::new();
    project.insert_class_signature(Arc::clone(&shared));
    let projects = [&project; 4];
    let visibility = isolated(4);
    reset_counts();
    let first = Index::scoped(&projects, &visibility);
    let first_payload = payload(&first.binaries[0]);
    assert_eq!(Arc::strong_count(first_payload), 4, "the temporary map must have been dropped");
    assert_eq!(Arc::strong_count(&shared), 3, "one signature owner per payload, not per scope");
    for (scope, class) in first.binaries.iter().enumerate() {
        assert_eq!(class.scope, scope);
        assert!(Arc::ptr_eq(first_payload, payload(class)));
        assert!(std::ptr::eq(&first.binaries[0].methods[0], &class.methods[0]));
        if scope != 0 {
            assert!(!same_class(&first.binaries[0], class));
        }
    }
    let first_weak = Arc::downgrade(first_payload);
    let second = Index::scoped(&projects, &visibility);
    assert_eq!(BINARY_PAYLOAD_BUILDS.with(|count| count.get()), 2);
    assert_eq!(BINARY_MEMBER_BUILDS.with(|count| count.get()), 4);
    assert!(!Arc::ptr_eq(first_payload, payload(&second.binaries[0])));
    let second_weak = Arc::downgrade(payload(&second.binaries[0]));
    drop(first);
    assert!(first_weak.upgrade().is_none());
    assert!(second_weak.upgrade().is_some());
    drop(second);
    assert!(second_weak.upgrade().is_none());
    assert_eq!(Arc::strong_count(&shared), 2);
}

#[test]
fn equal_origins_names_and_bytes_are_not_signature_allocation_identity() {
    let shared = signature("same.jar!/Lib.class", "Lib", None);
    let equal = Arc::new((*shared).clone());
    let mut changed = (*shared).clone();
    changed.methods[0].name = "changed".into();
    let changed = Arc::new(changed);
    let other_origin = signature("other.jar!/Lib.class", "Lib", None);
    let mut projects = vec![Project::new(); 4];
    for (project, sig) in projects.iter_mut().zip([shared, equal, changed, other_origin]) {
        project.insert_class_signature(sig);
    }
    let refs = projects.iter().collect::<Vec<_>>();
    reset_counts();
    let index = Index::scoped(&refs, &isolated(4));
    assert_eq!(BINARY_PAYLOAD_BUILDS.with(|count| count.get()), 4);
    for (i, left) in index.binaries.iter().enumerate() {
        for right in &index.binaries[i + 1..] {
            assert!(!Arc::ptr_eq(payload(left), payload(right)));
        }
    }
    assert_eq!(index.binaries[0].methods[0].name, "read");
    assert_eq!(index.binaries[2].methods[0].name, "changed");
    assert_eq!(index.binaries[3].path, "other.jar!/Lib.class");
    let other = Arc::clone(projects[3].binaries.values().next().unwrap());
    projects[0].insert_class_signature(other);
    projects[0].insert("Use.java", "class Use { Lib value; }");
    let refs = projects.iter().collect::<Vec<_>>();
    let index = Index::scoped(&refs, &isolated(4));
    let result = query(&index, &projects[0], "Use.java", "Lib");
    assert_eq!(result.status, ResolveStatus::Ambiguous);
    assert_eq!(
        result.targets.iter().map(|t| t.path.as_str()).collect::<Vec<_>>(),
        ["other.jar!/Lib.class", "same.jar!/Lib.class"]
    );
    assert_every_offset(&refs, &isolated(4));
}

#[test]
fn shared_payload_keeps_parents_descriptor_types_access_and_dependency_owners_contextual() {
    let mut shared = (*signature("shared.jar!/lib/Child.class", "lib.Child", Some("lib.Parent"))).clone();
    shared.methods = vec![
        member("choose", "(Ldep/Arg;)V", MemberKind::Method, Access::Public),
        member("overload", "(I)V", MemberKind::Method, Access::Public),
        member("overload", "(J)V", MemberKind::Method, Access::Public),
        member("packageOnly", "()V", MemberKind::Method, Access::Package),
        member("protectedOnly", "()V", MemberKind::Method, Access::Protected),
        member("privateOnly", "()V", MemberKind::Method, Access::Private),
    ];
    let shared = Arc::new(shared);
    let mut projects = vec![Project::new(); 4];
    for (scope, project) in projects.iter_mut().enumerate() {
        project.insert_class_signature(Arc::clone(&shared));
        let mut parent = (*signature(&format!("parent-{scope}.jar!/lib/Parent.class"), "lib.Parent", None)).clone();
        parent.methods = vec![
            member("inherited", "()V", MemberKind::Method, Access::Public),
            member("choose", "(Ldep/Arg;)V", MemberKind::Method, Access::Public),
        ];
        project.insert_class_signature(Arc::new(parent));
        if scope != 1 {
            project.insert_class_signature(signature(&format!("arg-{scope}.jar!/dep/Arg.class"), "dep.Arg", None));
        }
    }
    projects[0].insert("Same.java", "package lib; public class Exposed extends Child { void run() { inherited(); choose(null); overload(1); packageOnly(); protectedOnly(); privateOnly(); } }");
    projects[1].insert("Other.java", "package other; class Other extends lib.Child { void run() { inherited(); choose(null); packageOnly(); protectedOnly(); privateOnly(); } }");
    projects[2].insert("Outside.java", "package outside; class Outside { void run() { lib.Child.packageOnly(); lib.Child.protectedOnly(); lib.Child.privateOnly(); } }");
    projects[3].insert("Caller.java", "package client; class Caller { void run() { lib.Exposed.inherited(); } }");
    let refs = projects.iter().collect::<Vec<_>>();
    let mut visibility = isolated(4);
    visibility[3].insert(0);
    let index = Index::scoped(&refs, &visibility);
    let first = named(&index, 0, "lib.Child");
    for scope in 0..4 {
        let child = named(&index, scope, "lib.Child");
        assert!(Arc::ptr_eq(payload(first), payload(child)));
        assert_eq!(index.hierarchy(child).ok().unwrap()[1].class.path, format!("parent-{scope}.jar!/lib/Parent.class"));
        let signature = index.method_signature(child, &child.methods[0]).unwrap();
        let parameter = &signature.parameters[0];
        if scope == 1 {
            assert!(parameter.is_none());
        } else {
            let TypeBase::Declared(identity) = &parameter.as_ref().unwrap().base else { panic!("reference parameter") };
            assert_eq!(identity.scope, scope);
            assert_eq!(identity.path, format!("arg-{scope}.jar!/dep/Arg.class"));
        }
    }
    for (scope, path) in [(0, "Same.java"), (1, "Other.java")] {
        let result = query(&index, &projects[scope], path, "inherited");
        assert_eq!(result.status, ResolveStatus::Resolved);
        assert_eq!(result.targets[0].path, format!("parent-{scope}.jar!/lib/Parent.class"));
        assert_eq!(query(&index, &projects[scope], path, "protectedOnly").status, ResolveStatus::Resolved);
        assert_eq!(query(&index, &projects[scope], path, "privateOnly").status, ResolveStatus::Unresolved);
    }
    assert_eq!(query(&index, &projects[0], "Same.java", "packageOnly").status, ResolveStatus::Resolved);
    assert_eq!(query(&index, &projects[1], "Other.java", "packageOnly").status, ResolveStatus::Unresolved);
    assert_eq!(query(&index, &projects[0], "Same.java", "choose").status, ResolveStatus::Resolved);
    assert_eq!(query(&index, &projects[1], "Other.java", "choose").status, ResolveStatus::SignatureNotCompared);
    assert_eq!(query(&index, &projects[0], "Same.java", "overload").status, ResolveStatus::OverloadNotResolved);
    for name in ["packageOnly", "protectedOnly", "privateOnly"] {
        assert_eq!(query(&index, &projects[2], "Outside.java", name).status, ResolveStatus::Unresolved);
    }
    let dependent = query(&index, &projects[3], "Caller.java", "inherited");
    assert_eq!(dependent.status, ResolveStatus::Resolved);
    assert_eq!(dependent.targets[0].path, "parent-0.jar!/lib/Parent.class");
    assert_every_offset(&refs, &visibility);
}

#[test]
fn sharing_preserves_constructor_validation_generic_refusal_and_member_order() {
    let mut sig = (*signature("lib.jar!/Lib.class", "Lib", None)).clone();
    sig.methods = vec![
        member("<init>", "invalid", MemberKind::Method, Access::Public),
        member("read", "(I)V", MemberKind::Method, Access::Public),
        member("read", "broken", MemberKind::Method, Access::Public),
        member("generic", "(I)V", MemberKind::Method, Access::Public),
        member("generic", "(J)V", MemberKind::Method, Access::Public),
        member("notAMethod", "I", MemberKind::Field, Access::Public),
    ];
    sig.methods[3].signature = Some("<T:Ljava/lang/Object;>(TT;)V".into());
    let sig = Arc::new(sig);
    let mut projects = [Project::new(), Project::new()];
    for (scope, project) in projects.iter_mut().enumerate() {
        project.insert_class_signature(Arc::clone(&sig));
        project.insert(
            format!("Use{scope}.java"),
            &format!("class Use{scope} extends Lib {{ void run() {{ read(1); generic(1); }} }}"),
        );
    }
    let refs = projects.iter().collect::<Vec<_>>();
    let index = Index::scoped(&refs, &isolated(2));
    let class = &index.binaries[0];
    assert!(!class.reference_inventory, "constructors still participate in inventory validation");
    assert_eq!(
        class.methods.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
        ["read", "read", "generic", "generic"]
    );
    assert!(Arc::ptr_eq(class.binary_signature.as_ref().unwrap(), &sig));
    for (scope, project) in projects.iter().enumerate() {
        for name in ["read", "generic"] {
            assert_eq!(
                query(&index, project, &format!("Use{scope}.java"), name).status,
                ResolveStatus::SignatureNotCompared
            );
        }
    }
    assert_every_offset(&refs, &isolated(2));
}

#[test]
fn fresh_inputs_rebuild_payloads_while_old_prepared_queries_remain_valid() {
    let shared = signature("same.jar!/Lib.class", "Lib", None);
    let mut old = Project::new();
    old.insert_class_signature(Arc::clone(&shared));
    old.insert("Old.java", "class Old extends Lib { int get() { return value; } }");
    let old_prepared = PreparedScopedIndex::new(vec![&old], &isolated(1));
    let old_payload = payload(&old_prepared.index.binaries[0]);
    let mut next = old.clone();
    let mut changed = (*shared).clone();
    changed.fields[0].name = "next".into();
    next.insert_class_signature(Arc::new(changed));
    let next_prepared = PreparedScopedIndex::new(vec![&next], &isolated(1));
    assert!(!Arc::ptr_eq(old_payload, payload(&next_prepared.index.binaries[0])));
    let byte = old.file("Old.java").unwrap().text().rfind("value").unwrap() as u32;
    assert_eq!(old_prepared.resolve_at(0, "Old.java", byte).status, ResolveStatus::Resolved);
    assert_eq!(next_prepared.resolve_at(0, "Old.java", byte).status, ResolveStatus::Unresolved);
    let mut removed = next.clone();
    assert!(removed.remove_class("same.jar!/Lib.class"));
    assert!(Index::scoped(&[&removed], &isolated(1)).binaries.is_empty());
    let mut overlay = old.clone();
    overlay.insert("Old.java", "class Old { int get() { return value; } }");
    let overlay_prepared = PreparedScopedIndex::new(vec![&overlay], &isolated(1));
    assert!(!Arc::ptr_eq(old_payload, payload(&overlay_prepared.index.binaries[0])));
    let overlay_byte = overlay.file("Old.java").unwrap().text().rfind("value").unwrap() as u32;
    assert_eq!(overlay_prepared.resolve_at(0, "Old.java", overlay_byte).status, ResolveStatus::Unresolved);
    std::thread::scope(|threads| {
        for _ in 0..2 {
            let old_prepared = &old_prepared;
            let next_prepared = &next_prepared;
            threads.spawn(move || {
                for _ in 0..10 {
                    assert_eq!(old_prepared.resolve_at(0, "Old.java", byte).status, ResolveStatus::Resolved);
                    assert_eq!(next_prepared.resolve_at(0, "Old.java", byte).status, ResolveStatus::Unresolved);
                }
            });
        }
    });
    for project in [&old, &next, &removed, &overlay] {
        assert_every_offset(&[project], &isolated(1));
    }
}

#[test]
fn source_payloads_remain_independently_owned() {
    let parsed = ParsedFile::parse("class Source { int field; }");
    let first = index_file("Source.java", &parsed);
    let mut second = first.clone();
    assert!(!std::ptr::eq(&*first.classes[0], &*second.classes[0]));
    second.classes[0].source_data_mut().fields[0].name = "changed".into();
    assert_eq!(first.classes[0].fields[0].name, "field");
    assert_eq!(second.classes[0].fields[0].name, "changed");
}
