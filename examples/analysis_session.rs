//! Caller-owned analysis versions. No JVM, watcher or persistent worker.
use java_engine::{AnalysisSnapshot, CompilationConfig, CompilationId, ProjectConfig, SourceOverlay, WorkspaceConfig};
use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scopes");
    let lib = CompilationId::new("lib", "main");
    let app = CompilationId::new("app", "main");
    let config = WorkspaceConfig {
        compilations: vec![
            CompilationConfig {
                id: lib.clone(),
                inputs: ProjectConfig { source_roots: vec![root.join("lib/main")], ..Default::default() },
                dependencies: vec![],
            },
            CompilationConfig {
                id: app.clone(),
                inputs: ProjectConfig { source_roots: vec![root.join("app/main")], ..Default::default() },
                dependencies: vec![lib.clone()],
            },
        ],
        ..Default::default()
    };
    let first = AnalysisSnapshot::load(&config)?;
    let query = root.join("app/main/app/App.java").canonicalize()?.to_str().ok_or("non UTF-8 path")?.to_owned();
    let target = root.join("lib/main/lib/Lib.java").canonicalize()?.to_str().ok_or("non UTF-8 path")?.to_owned();
    let source = first.scope(&app).unwrap().inputs().project().file(&query).unwrap().text();
    let at = source.rfind("value").ok_or("missing query")? as u32;
    let old = first.scope(&app).unwrap().resolve_at(&query, at);
    let target_text = first.scope(&lib).unwrap().inputs().project().file(&target).unwrap().text();
    let second = first.with_source_overlays(&[SourceOverlay {
        owner: lib,
        path: target,
        text: format!("// new version\n{target_text}"),
    }])?;
    let newer = second.scope(&app).unwrap().resolve_at(&query, at);
    assert_eq!(old.targets.len(), 1);
    assert_eq!(newer.targets.len(), 1);
    assert_eq!(newer.targets[0].start_byte, old.targets[0].start_byte + 15);
    assert_eq!(first.scope(&app).unwrap().resolve_at(&query, at), old);
    let retained = second.clone();
    drop(second);
    assert_eq!(retained.scope(&app).unwrap().resolve_at(&query, at), newer);
    assert_eq!(AnalysisSnapshot::load(&config)?.scope(&app).unwrap().resolve_at(&query, at), old);
    println!("Old, derived and reloaded versions retain their own exact target ranges.");
    Ok(())
}
