//! Explicit project/source-set graph, without Maven, Gradle, or a JVM.
use java_engine::{CompilationConfig, CompilationId, ProjectConfig, ResolveStatus, WorkspaceConfig, WorkspaceSnapshot};
use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scopes");
    let id = |project, set| CompilationId::new(project, set);
    let compilation = |project, set, dependencies| CompilationConfig {
        id: id(project, set),
        inputs: ProjectConfig { source_roots: vec![root.join(project).join(set)], ..Default::default() },
        dependencies,
    };
    let config = WorkspaceConfig {
        compilations: vec![
            compilation("app", "test", vec![id("app", "main")]),
            compilation("app", "main", vec![id("lib", "main")]),
            compilation("lib", "main", vec![]),
            compilation("lib", "test", vec![id("lib", "main")]),
            compilation("unrelated", "main", vec![]),
        ],
        ..Default::default()
    };
    let snapshot = WorkspaceSnapshot::load(&config)?;
    let scope = snapshot.scope(&id("app", "test")).expect("configured scope");
    let path = root.join("app/test/app/AppTest.java").canonicalize()?;
    let path = path.to_str().ok_or("non-UTF-8 fixture path")?;
    let text = std::fs::read_to_string(path)?;
    for (name, expected) in [
        ("value", ResolveStatus::Resolved),
        ("LibTest", ResolveStatus::Unresolved),
        ("Other", ResolveStatus::Unresolved),
    ] {
        let offset = text.rfind(name).ok_or("fixture reference missing")? as u32;
        let result = scope.resolve_at(path, offset);
        assert_eq!(result.status, expected);
        println!("app/test {name}: {:?}", result.status);
    }
    println!("Visible source files: {}", scope.source_paths().len());
    Ok(())
}
