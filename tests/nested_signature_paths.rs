use java_engine::{Project, ResolveStatus};

fn project(source: &str) -> Project {
    let mut p = Project::new();
    p.insert("Use.java", source);
    assert!(p.file("Use.java").unwrap().errors().is_empty());
    let mut object = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61, 0, 3, 1, 0, 16];
    object.extend(b"java/lang/Object");
    object.extend([7, 0, 1, 0, 0x21, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    p.add_class_bytes("Object.class", &object).unwrap();
    p
}
#[test]
fn an_override_needs_every_parameter_type_owner_to_be_unique() {
    for (types, expected) in [
        ("class Outer {static class Mid {static class Leaf {}}}", ResolveStatus::Resolved),
        (
            "class Outer {static class Mid {static class Leaf {}} static class Mid {}}",
            ResolveStatus::SignatureNotCompared,
        ),
        ("class Outer {} class Outer$Mid$Leaf {}", ResolveStatus::SignatureNotCompared),
    ] {
        let source = format!(
            "{types} class Base {{void pick(Outer.Mid.Leaf x){{}}}} class Use extends Base {{void pick(Outer.Mid.Leaf x){{}} void run(){{pick(null);}}}}"
        );
        let p = project(&source);
        let at = source.rfind("pick(null)").unwrap() as u32;
        let result = p.resolve_at("Use.java", at);
        assert_eq!(result, p.prepare().resolve_at("Use.java", at));
        assert_eq!(result.status, expected, "{types}: {result:?}");
        if expected == ResolveStatus::Resolved {
            assert_eq!(result.targets.len(), 1);
            assert_eq!(result.targets[0].owner, "Use");
        } else {
            assert_eq!(result.targets.len(), 2);
            assert!(result.targets.iter().any(|t| t.owner == "Base"));
            assert!(result.targets.iter().any(|t| t.owner == "Use"));
        }
    }
}
#[test]
fn a_dollar_is_valid_in_a_top_level_name_but_does_not_create_an_owner() {
    let source = "class Dollar$Name {} class Base {void pick(Dollar$Name x){}} class Use extends Base {void pick(Dollar$Name x){} void run(){pick(null);}}";
    let p = project(source);
    let result = p.resolve_at("Use.java", source.rfind("pick(null)").unwrap() as u32);
    assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
    assert_eq!(result.targets.len(), 1);
    assert_eq!(result.targets[0].owner, "Use");
}
