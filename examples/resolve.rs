use java_engine::{Project, ResolveStatus};

fn main() {
    let mut project = Project::new();
    project.insert("demo/Value.java", "package demo; public class Value { public int count; }");
    project.insert("demo/Box.java", "package demo; public class Box { public Value value; }");
    let source = "class Use { void run(demo.Box box) { int n = box.value.count; } }";
    project.insert("Use.java", source);
    let result = project.resolve_at("Use.java", source.find("count").unwrap() as u32);
    assert_eq!(result.status, ResolveStatus::Resolved);
    assert_eq!(result.targets.len(), 1);
    let target = &result.targets[0];
    assert_eq!(target.owner, "demo.Value");
    println!("{}::{} in {}", target.owner, target.name, target.path);
}
