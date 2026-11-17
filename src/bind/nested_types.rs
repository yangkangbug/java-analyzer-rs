//! Direct member-type facts only: no inheritance, import fallback or access filtering.
use super::*;
use crate::InnerClassEntry;

fn valid_flags(entry: &InnerClassEntry, child: &ClassRec) -> bool {
    let flags = entry.raw_access_flags;
    const KNOWN: u16 = 0x761f;
    flags & !KNOWN == 0
        && (flags & 7).count_ones() <= 1
        && !(flags & 0x10 != 0 && flags & 0x400 != 0)
        && (flags & 0x200 != 0) == child.is_interface
        && (flags & 0x10 != 0) == child.reference_final
        && (flags & 0x200 == 0 || flags & 0x408 == 0x408)
        && (flags & 0x2000 == 0 || flags & 0x200 != 0)
        && (flags & 0x4000 == 0 || flags & 0x208 == 0x008)
}

// Certify the exact pair, without discarding private/package/protected candidates.
fn pair_flags(owner: &ClassRec, child: &ClassRec, entry: &InnerClassEntry) -> Option<u16> {
    let parent = owner.binary_signature.as_ref()?;
    let data = child.binary_signature.as_ref()?;
    if owner.scope != child.scope
        || data.has_enclosing_method
        || parent.has_enclosing_method
        || entry.outer_binary_name.as_deref() != Some(parent.binary_name.as_str())
        || entry.binary_name != data.binary_name
        || entry.inner_name.as_deref().is_none_or(str::is_empty)
        || entry.binary_name == parent.binary_name
        || child.package != owner.package
        || (owner.is_interface && entry.raw_access_flags & 0x000f != 0x0009)
        || !valid_flags(entry, child)
    {
        return None;
    }
    let [group] = data.inner_classes.as_slice() else { return None };
    let mut own = group.iter().filter(|e| e.binary_name == data.binary_name);
    let first = own.next()?;
    if first != entry || own.next().is_some() {
        return None;
    }
    Some(entry.raw_access_flags)
}

fn jvm_name(class: &ClassRec) -> String {
    if let Some(data) = &class.binary_signature {
        return data.binary_name.clone();
    }
    let prefix = if class.package.is_empty() { String::new() } else { format!("{}/", class.package.replace('.', "/")) };
    format!("{prefix}{}", class.nesting.join("$"))
}

impl Index {
    pub(super) fn nested_identity_unique(&self, scope: usize, selected: &ClassRec) -> bool {
        let name = jvm_name(selected);
        !self.classes(scope).any(|other| !same_class(other, selected) && jvm_name(other) == name)
    }
    pub(super) fn direct_nested_type<'a>(&'a self, owner: &'a ClassRec, name: &str) -> TypeLookup<'a> {
        if !owner.binary {
            let found: Vec<_> = self
                .files
                .iter()
                .filter(|file| file.scope == owner.scope && file.path == owner.path)
                .flat_map(|file| &file.classes)
                .filter(|inner| {
                    inner.nesting.len() == owner.nesting.len() + 1
                        && inner.nesting[..owner.nesting.len()] == owner.nesting[..]
                        && inner.simple() == name
                        && owner.range_start < inner.range_start
                        && inner.range_end < owner.range_end
                })
                .collect();
            return match found.len() {
                0 => TypeLookup::None,
                1 => TypeLookup::One(found[0]),
                _ => TypeLookup::Many(found),
            };
        }
        let Some(parent) = owner.binary_signature.as_ref() else { return TypeLookup::MissingImport };
        if parent.has_enclosing_method || parent.inner_classes.len() > 1 {
            return TypeLookup::MissingImport;
        }
        let entries: Vec<_> = parent
            .inner_classes
            .first()
            .into_iter()
            .flatten()
            .filter(|entry| {
                entry.outer_binary_name.as_deref() == Some(parent.binary_name.as_str())
                    && entry.inner_name.as_deref() == Some(name)
            })
            .collect();
        let loaded: Vec<_> = self.binaries.iter().filter(|child| child.scope == owner.scope).collect();
        // Child-side claims are evidence too. A parent inventory cannot silently
        // hide an unlisted claimant, even when another listed child looks valid.
        for child in &loaded {
            let Some(data) = child.binary_signature.as_ref() else { return TypeLookup::MissingImport };
            let claims = data.inner_classes.iter().flatten().any(|entry| {
                entry.binary_name == data.binary_name
                    && entry.outer_binary_name.as_deref() == Some(parent.binary_name.as_str())
                    && entry.inner_name.as_deref() == Some(name)
            });
            if claims && !entries.iter().any(|entry| entry.binary_name == data.binary_name) {
                return TypeLookup::MissingImport;
            }
        }
        let mut found = Vec::new();
        let mut seen = BTreeSet::new();
        for entry in entries {
            // Repeated descriptions of one binary child are malformed evidence,
            // not independent declarations that can be deduplicated to success.
            if !seen.insert(entry.binary_name.as_str()) {
                return TypeLookup::MissingImport;
            }
            // Conflicting parent descriptions of this child must not disappear
            // just because their simple name/outer differs from this request.
            if parent.inner_classes[0].iter().filter(|e| e.binary_name == entry.binary_name).count() != 1 {
                return TypeLookup::MissingImport;
            }
            // A visible source declaration with this JVM identity competes
            // with the binary input. It cannot certify the binary owner's
            // relationship, nor be silently discarded to select the binary.
            if self.classes(owner.scope).filter(|class| !class.binary).any(|class| {
                let prefix = if class.package.is_empty() {
                    String::new()
                } else {
                    format!("{}/", class.package.replace('.', "/"))
                };
                format!("{prefix}{}", class.nesting.join("$")) == entry.binary_name
            }) {
                return TypeLookup::MissingImport;
            }
            let children: Vec<_> = loaded
                .iter()
                .copied()
                .filter(|child| {
                    child.binary_signature.as_ref().is_some_and(|data| data.binary_name == entry.binary_name)
                })
                .collect();
            if children.is_empty() {
                return TypeLookup::MissingImport;
            }
            for child in children {
                if pair_flags(owner, child, entry).is_none() {
                    return TypeLookup::MissingImport;
                }
                found.push(child);
            }
        }
        match found.len() {
            0 => TypeLookup::None,
            1 => TypeLookup::One(found[0]),
            _ => TypeLookup::Many(found),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(binary: &str, name: &str, flags: u16) -> InnerClassEntry {
        InnerClassEntry {
            binary_name: binary.into(),
            outer_binary_name: Some("p/Owner".into()),
            inner_name: Some(name.into()),
            raw_access_flags: flags,
        }
    }
    fn data(origin: &str, binary: &str, groups: Vec<Vec<InnerClassEntry>>) -> Arc<ClassFileData> {
        let (pkg, simple) = binary.rsplit_once('/').unwrap_or(("", binary));
        Arc::new(ClassFileData {
            class_signature: ClassSignature {
                origin: origin.into(),
                binary_name: binary.into(),
                dotted_name: binary.replace(['/', '$'], "."),
                package: pkg.replace('/', "."),
                nesting: simple.split('$').map(str::to_string).collect(),
                super_class: Some("java.lang.Object".into()),
                interfaces: vec![],
                binary_super_class: Some("java/lang/Object".into()),
                binary_interfaces: vec![],
                signature: None,
                major: 61,
                minor: 0,
                is_interface: false,
                is_final: false,
                access: Access::Public,
                fields: vec![],
                methods: vec![],
            },
            inner_classes: groups,
            has_enclosing_method: false,
        })
    }
    fn index(parent: Vec<Vec<InnerClassEntry>>, children: Vec<Arc<ClassFileData>>) -> Index {
        let mut p = Project::new();
        p.insert_class_signature(data("parent.class", "p/Owner", parent));
        for child in children {
            p.insert_class_signature(child);
        }
        Index::build(&p.files, &p.binaries)
    }
    fn owner(index: &Index) -> &ClassRec {
        index.binaries.iter().find(|c| c.path == "parent.class").unwrap()
    }
    #[test]
    fn source_actual_owner_ranges_and_competitors_are_preserved() {
        let mut p = Project::new();
        p.insert("C.java", "class C {class Inner {} class Inner {}} class C {class Inner {}}");
        let i = Index::build(&p.files, &p.binaries);
        let outer: Vec<_> = i.files[0].classes.iter().filter(|c| c.nesting.len() == 1).collect();
        let TypeLookup::Many(first) = i.direct_nested_type(outer[0], "Inner") else { panic!("lost duplicate") };
        assert_eq!(first.len(), 2);
        let TypeLookup::One(second) = i.direct_nested_type(outer[1], "Inner") else {
            panic!("mixed distinct outer ranges")
        };
        assert!(second.range_start > outer[1].range_start);
        assert!(first.iter().all(|c| c.range_end < outer[0].range_end));
    }
    #[test]
    fn binary_metadata_not_dollar_spelling_proves_relationship() {
        let e = entry("p/UnusualBinary", "Inner", 2);
        let i = index(vec![vec![e.clone()]], vec![data("child.class", &e.binary_name, vec![vec![e.clone()]])]);
        let TypeLookup::One(child) = i.direct_nested_type(owner(&i), "Inner") else {
            panic!("missing metadata relationship")
        };
        assert_eq!(child.path, "child.class");
        let i = index(vec![], vec![data("fake.class", "p/Owner$Inner", vec![])]);
        assert!(matches!(i.direct_nested_type(owner(&i), "Inner"), TypeLookup::None));
    }
    #[test]
    fn binary_distinct_candidates_remain_ambiguous_even_with_private_access() {
        let a = entry("p/A", "Inner", 1);
        let b = entry("p/B", "Inner", 2);
        let i = index(
            vec![vec![a.clone(), b.clone()]],
            vec![data("a", "p/A", vec![vec![a]]), data("b", "p/B", vec![vec![b]])],
        );
        let TypeLookup::Many(all) = i.direct_nested_type(owner(&i), "Inner") else { panic!("selected first") };
        assert_eq!(all.len(), 2);
    }
    #[test]
    fn binary_missing_duplicate_conflicting_and_unlisted_claims_refuse() {
        let e = entry("p/Child", "Inner", 1);
        let good = data("child", "p/Child", vec![vec![e.clone()]]);
        let mut bad_flags = e.clone();
        bad_flags.raw_access_flags = 0x8001;
        let mut wrong_outer = e.clone();
        wrong_outer.outer_binary_name = Some("q/Other".into());
        let mut local = (*good).clone();
        local.has_enclosing_method = true;
        let cases = vec![
            (vec![vec![e.clone()]], vec![]),
            (vec![vec![e.clone(), e.clone()]], vec![good.clone()]),
            (vec![vec![e.clone()], vec![]], vec![good.clone()]),
            (vec![vec![e.clone()]], vec![data("child", "p/Child", vec![])]),
            (vec![vec![e.clone()]], vec![data("child", "p/Child", vec![vec![e.clone(), e.clone()]])]),
            (vec![vec![e.clone()]], vec![data("child", "p/Child", vec![vec![wrong_outer]])]),
            (vec![vec![bad_flags.clone()]], vec![data("child", "p/Child", vec![vec![bad_flags]])]),
            (vec![vec![e.clone()]], vec![Arc::new(local)]),
            (vec![], vec![good.clone()]),
            (vec![vec![]], vec![good]),
        ];
        for (n, (parent, children)) in cases.into_iter().enumerate() {
            let i = index(parent, children);
            assert!(matches!(i.direct_nested_type(owner(&i), "Inner"), TypeLookup::MissingImport), "case {n}");
        }
    }
    #[test]
    fn binary_flags_and_hidden_claimants_cannot_remove_competitors() {
        for flags in [0x0003, 0x0410, 0x0201, 0x2001, 0x0011, 0x8001] {
            let e = entry("p/Child", "Inner", flags);
            let i = index(vec![vec![e.clone()]], vec![data("child", "p/Child", vec![vec![e]])]);
            assert!(matches!(i.direct_nested_type(owner(&i), "Inner"), TypeLookup::MissingImport), "flags {flags:x}");
        }
        let a = entry("p/A", "Inner", 1);
        let b = entry("p/B", "Inner", 1);
        let i = index(vec![vec![a.clone()]], vec![data("a", "p/A", vec![vec![a]]), data("b", "p/B", vec![vec![b]])]);
        assert!(matches!(i.direct_nested_type(owner(&i), "Inner"), TypeLookup::MissingImport));
        let e = entry("p/Child", "Inner", 1);
        let i = index(
            vec![vec![e.clone()]],
            vec![data("one", "p/Child", vec![vec![e.clone()]]), data("two", "p/Child", vec![vec![e]])],
        );
        let TypeLookup::Many(found) = i.direct_nested_type(owner(&i), "Inner") else { panic!("collapsed origins") };
        assert_eq!(found.len(), 2);
    }
    #[test]
    fn only_visible_source_jvm_identity_blocks_binary_selection() {
        let e = entry("p/Child", "Inner", 1);
        let mut binary = Project::new();
        binary.insert_class_signature(data("parent.class", "p/Owner", vec![vec![e.clone()]]));
        binary.insert_class_signature(data("child.class", "p/Child", vec![vec![e]]));
        let mut source = Project::new();
        source.insert("Child.java", "package p; class Child {}");
        let isolated = Index::scoped(&[&binary, &source], &[BTreeSet::from([0]), BTreeSet::from([1])]);
        assert!(matches!(isolated.direct_nested_type(owner(&isolated), "Inner"), TypeLookup::One(_)));
        let visible = Index::scoped(&[&binary, &source], &[BTreeSet::from([0, 1]), BTreeSet::from([1])]);
        assert!(matches!(visible.direct_nested_type(owner(&visible), "Inner"), TypeLookup::MissingImport));
        binary.insert("Child.java", "package p; class Child {}");
        let same = Index::build(&binary.files, &binary.binaries);
        assert!(matches!(same.direct_nested_type(owner(&same), "Inner"), TypeLookup::MissingImport));
    }
    #[test]
    fn signature_path_checks_each_source_owner_before_descending() {
        for (source, expected) in [
            ("class Owner {class Mid {class Leaf {}}}", true),
            ("class Owner {class Mid {class Leaf {}} class Mid {}}", false),
            ("class Owner {} class Owner {class Mid {class Leaf {}}}", false),
        ] {
            let mut project = Project::new();
            project.insert("Owner.java", source);
            let i = Index::build(&project.files, &project.binaries);
            let root = &i.files[0].classes[0];
            let result = i.signature_nested(0, root, &["Mid".into(), "Leaf".into()]);
            assert_eq!(result.is_some(), expected, "{source}");
            if let Some(c) = result {
                assert_eq!(c.dotted(), "Owner.Mid.Leaf");
            }
        }
    }
    #[test]
    fn signature_path_rejects_a_dollar_named_top_level_and_missing_middle() {
        let i = index(vec![], vec![data("top.class", "p/Owner$Inner", vec![])]);
        assert!(i.signature_nested(0, owner(&i), &["Inner".into()]).is_none());
        let mid = entry("p/Owner$Mid", "Mid", 1);
        let mut leaf = entry("p/Owner$Mid$Leaf", "Leaf", 1);
        leaf.outer_binary_name = Some("p/Owner$Mid".into());
        let i = index(vec![vec![mid]], vec![data("leaf.class", "p/Owner$Mid$Leaf", vec![vec![leaf]])]);
        assert!(i.signature_nested(0, owner(&i), &["Mid".into(), "Leaf".into()]).is_none());
    }
    #[test]
    fn signature_path_accepts_exact_binary_metadata_without_name_concatenation() {
        let mid = entry("p/UnusualMiddle", "Mid", 1);
        let mut leaf = entry("p/UnusualLeaf", "Leaf", 1);
        leaf.outer_binary_name = Some(mid.binary_name.clone());
        let i = index(
            vec![vec![mid.clone()]],
            vec![
                data("mid.class", &mid.binary_name, vec![vec![mid.clone(), leaf.clone()]]),
                data("leaf.class", &leaf.binary_name, vec![vec![leaf.clone()]]),
            ],
        );
        let selected = i.signature_nested(0, owner(&i), &["Mid".into(), "Leaf".into()]).unwrap();
        assert_eq!(selected.path, "leaf.class");
    }
    #[test]
    fn signature_path_keeps_query_scope_jvm_identity_competitors() {
        let mut source = Project::new();
        source.insert("Outer.java", "package p; class Outer {class Inner {}}");
        let mut binary = Project::new();
        binary.insert_class_signature(data("binary.class", "p/Outer$Inner", vec![]));
        let isolated = Index::scoped(&[&binary, &source], &[BTreeSet::from([0]), BTreeSet::from([1])]);
        let outer = &isolated.files[0].classes[0];
        assert!(isolated.signature_nested(1, outer, &["Inner".into()]).is_some());
        let visible = Index::scoped(&[&binary, &source], &[BTreeSet::from([0, 1]), BTreeSet::from([1])]);
        let outer = &visible.files[0].classes[0];
        assert!(visible.signature_nested(0, outer, &["Inner".into()]).is_none());
        source.insert_class_signature(data("binary.class", "p/Outer$Inner", vec![]));
        let same = Index::build(&source.files, &source.binaries);
        assert!(same.signature_nested(0, &same.files[0].classes[0], &["Inner".into()]).is_none());
    }
    #[test]
    fn signature_path_refuses_cyclic_binary_ownership() {
        let child = entry("p/Child", "Inner", 1);
        let mut back = entry("p/Owner", "Back", 1);
        back.outer_binary_name = Some("p/Child".into());
        let i = index(
            vec![vec![child.clone(), back.clone()]],
            vec![data("child.class", "p/Child", vec![vec![child, back]])],
        );
        assert!(i.signature_nested(0, owner(&i), &["Inner".into(), "Back".into()]).is_none());
    }
}
