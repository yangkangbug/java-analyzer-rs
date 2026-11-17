use java_engine::{Project, ResolveStatus, TextEdit};

fn at(source: &str, marker: &str) -> u32 {
    (source.find(marker).unwrap() + marker.len()) as u32
}
fn project(source: &str) -> Project {
    let mut p = Project::new();
    p.insert("a/Item.java", "package a; public class Item {public int x;}");
    p.insert("Use.java", source);
    p
}

#[test]
fn duplicate_member_types_keep_all_declarations_instead_of_the_first() {
    for declarations in [
        "class Item {int x;} class Item {int y;}",
        "class Item {int y;} class Item {int x;}",
        "class Item {int x;} interface Item {}",
    ] {
        let source = format!(
            "import a.Item; class Use {{ {declarations} /*type*/Item value; int f(){{return value./*field*/x;}} }}"
        );
        let p = project(&source);
        let result = p.resolve_at("Use.java", at(&source, "/*type*/"));
        assert_eq!(result.status, ResolveStatus::Ambiguous, "{result:?}");
        assert_eq!(result.targets.len(), 2);
        assert!(result.targets.iter().all(|t| t.owner == "Use.Item" && t.path == "Use.java"));
        assert_ne!(result.targets[0], result.targets[1]);
        assert!(result.targets[0].start_byte < result.targets[1].start_byte);
        let receiver = p.resolve_at("Use.java", at(&source, "/*field*/"));
        assert_ne!(receiver.status, ResolveStatus::Resolved, "{receiver:?}");
        assert_eq!(result, p.prepare().resolve_at("Use.java", at(&source, "/*type*/")));
    }
}

#[test]
fn nearest_member_scope_blocks_outer_and_import_fallback() {
    let source = "import a.Item; class Use {class Item {int x;} class Nested {class Item {int x;} class Item {int y;} /*inner*/Item value;} /*outer*/Item value;}";
    let p = project(source);
    let inner = p.resolve_at("Use.java", at(source, "/*inner*/"));
    assert_eq!(inner.status, ResolveStatus::Ambiguous);
    assert_eq!(inner.targets.len(), 2);
    assert!(inner.targets.iter().all(|t| t.owner == "Use.Nested.Item"));
    let outer = p.resolve_at("Use.java", at(source, "/*outer*/"));
    assert_eq!(outer.status, ResolveStatus::Resolved);
    assert_eq!(outer.targets[0].owner, "Use.Item");
}

#[test]
fn unique_nearest_member_and_outer_member_remain_exact() {
    let source = "import a.Item; class Use {class Item {int x;} class Nested {class Item {int y;} /*near*/Item value;} class Other {/*far*/Item value;} }";
    let p = project(source);
    for (marker, owner) in [("/*near*/", "Use.Nested.Item"), ("/*far*/", "Use.Item")] {
        let result = p.resolve_at("Use.java", at(source, marker));
        assert_eq!(result.status, ResolveStatus::Resolved);
        assert_eq!(result.targets.len(), 1);
        assert_eq!(result.targets[0].owner, owner);
    }
}

#[test]
fn introducing_and_removing_duplicate_members_preserves_old_prepared_views() {
    let source = "class Use {/*insert*/class Item {int x;} /*type*/Item value; int f(){return value./*field*/x;} }";
    let p = project(source);
    let old = p.prepare();
    let insert_at = at(source, "/*insert*/");
    let insert = "class Item {int y;} ";
    let mut edited = p.clone();
    edited.edit("Use.java", TextEdit { start_byte: insert_at, end_byte: insert_at, insert: insert.into() }).unwrap();
    let text = edited.file("Use.java").unwrap().text();
    let fresh = project(text);
    for marker in ["/*type*/", "/*field*/"] {
        let result = edited.resolve_at("Use.java", at(text, marker));
        assert_eq!(result, fresh.resolve_at("Use.java", at(text, marker)));
        assert_ne!(result.status, ResolveStatus::Resolved);
        assert_eq!(old.resolve_at("Use.java", at(source, marker)).status, ResolveStatus::Resolved);
    }
    edited
        .edit(
            "Use.java",
            TextEdit { start_byte: insert_at, end_byte: insert_at + insert.len() as u32, insert: String::new() },
        )
        .unwrap();
    for marker in ["/*type*/", "/*field*/"] {
        assert_eq!(edited.resolve_at("Use.java", at(source, marker)), old.resolve_at("Use.java", at(source, marker)));
    }
}

#[test]
fn ambiguous_type_prefix_is_not_reinterpreted_as_a_package() {
    let source = "class Use {class Item {} class Item {} Item./*nested*/Inner v; int f(){return Item./*selector*/x./*tail*/y + v./*receiver*/x;} }";
    let mut p = project(source);
    p.insert("Item/Inner.java", "package Item; public class Inner {public int x;}");
    p.insert("Item/x.java", "package Item; public class x {public static int y;}");
    for marker in ["/*nested*/", "/*selector*/", "/*tail*/", "/*receiver*/"] {
        let result = p.resolve_at("Use.java", at(source, marker));
        assert_eq!(result.status, ResolveStatus::Ambiguous, "{marker}: {result:?}");
        assert!(result.targets.is_empty(), "{marker}: {result:?}");
    }
}

#[test]
fn unshadowed_package_prefix_still_resolves_exactly() {
    let source = "class Use {Item./*nested*/Inner v; int f(){return Item.x./*field*/y;} }";
    let mut p = project(source);
    p.insert("Item/Inner.java", "package Item; public class Inner {public int x;}");
    p.insert("Item/x.java", "package Item; public class x {public static int y;}");
    for (marker, owner) in [("/*nested*/", "Item.Inner"), ("/*field*/", "Item.x")] {
        let result = p.resolve_at("Use.java", at(source, marker));
        assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
        assert_eq!(result.targets[0].owner, owner);
    }
}
