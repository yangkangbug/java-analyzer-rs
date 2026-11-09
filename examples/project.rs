//! Load a complete source-to-JAR fixture without registering individual files.
use java_engine::{ClasspathEntry, ProjectConfig, ProjectSnapshot, ResolveStatus};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project");
    let config = ProjectConfig {
        source_roots: vec![root.join("src")],
        classpath: vec![ClasspathEntry::Jar(root.join("deps/library.jar"))],
        ..Default::default()
    };
    let snapshot = ProjectSnapshot::load(&config)?;
    let path = root.join("src/app/Use.java").canonicalize()?;
    let path = path.to_str().ok_or("non-UTF-8 fixture path")?;
    let source = snapshot.project().file(path).ok_or("missing fixture source")?;
    let byte = source.text().find("run()").ok_or("missing fixture call")? as u32;
    let result = snapshot.resolve_at(path, byte);
    assert_eq!(result.status, ResolveStatus::Resolved);
    assert_eq!(result.targets[0].owner, "lib.Lib");
    assert_eq!(snapshot.source_paths().len(), 2);
    println!("{} sources, {} library classes", snapshot.source_paths().len(), snapshot.class_origins().len());
    println!("app.Use -> app.Bridge -> {}::{}", result.targets[0].owner, result.targets[0].name);
    println!("{}", result.targets[0].path);
    Ok(())
}
