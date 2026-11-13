//! Fresh input admission with caller-owned archive decoding reuse, no JVM or LSP.
use java_engine::{
    AnalysisSnapshot, ClasspathEntry, CompilationConfig, CompilationId, ProjectConfig, ResolveStatus, WorkspaceConfig,
    WorkspaceLoadSession, WorkspaceSnapshot,
};
use std::{error::Error, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project");
    let id = CompilationId::new("app", "main");
    let config = WorkspaceConfig {
        compilations: vec![CompilationConfig {
            id: id.clone(),
            inputs: ProjectConfig {
                source_roots: vec![root.join("src")],
                classpath: vec![ClasspathEntry::Jar(root.join("deps/library.jar"))],
                ..Default::default()
            },
            dependencies: vec![],
        }],
        ..Default::default()
    };
    let path = root.join("src/app/Use.java").canonicalize()?;
    let path = path.to_str().ok_or("non-UTF-8 fixture path")?;
    let mut session = WorkspaceLoadSession::default();
    let first = session.load(&config)?;
    let inventory = first.scope(&id).unwrap().source_paths();
    let second = session.load(&config)?;
    // A caller that freezes inventory must still enforce its own policy.
    assert_eq!(inventory, second.scope(&id).unwrap().source_paths());
    let fresh = WorkspaceSnapshot::load(&config)?;
    let byte = first
        .scope(&id)
        .unwrap()
        .inputs()
        .project()
        .file(path)
        .unwrap()
        .text()
        .find("run()")
        .ok_or("missing fixture call")? as u32;
    let retained = AnalysisSnapshot::new(second);
    let expected = AnalysisSnapshot::new(fresh).scope(&id).unwrap().resolve_at(path, byte);
    assert_eq!(retained.scope(&id).unwrap().resolve_at(path, byte), expected);
    assert_eq!(expected.status, ResolveStatus::Resolved);
    drop(session);
    assert_eq!(retained.scope(&id).unwrap().resolve_at(path, byte), expected);
    println!("Fresh admission, reusable archive decoding, independently owned analysis.");
    Ok(())
}
