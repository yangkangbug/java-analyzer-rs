//! Read a caller-supplied java.base.jmod as data; never discover or run a JDK.
use java_engine::{PlatformEntry, ProjectConfig, ProjectSnapshot, ResolveStatus};
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let platform = PathBuf::from(args.next().ok_or("usage: platform /explicit/path/to/java.base.jmod")?);
    if args.next().is_some() {
        return Err("usage: platform /explicit/path/to/java.base.jmod".into());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/platform/src");
    let config = ProjectConfig {
        source_roots: vec![root.clone()],
        platform: vec![PlatformEntry::Jmod(platform)],
        ..Default::default()
    };
    let snapshot = ProjectSnapshot::load(&config)?;
    let path = root.join("Use.java").canonicalize()?;
    let path = path.to_str().ok_or("non-UTF-8 fixture path")?;
    let source = snapshot.project().file(path).ok_or("missing fixture source")?.text();
    println!("{} sources, {} platform classes", snapshot.source_paths().len(), snapshot.class_origins().len());
    for (name, owner) in [("String", "java.lang.String"), ("toString", "java.lang.Object")] {
        let byte = source.find(name).ok_or("missing fixture name")? as u32;
        let result = snapshot.resolve_at(path, byte);
        assert_eq!(result.status, ResolveStatus::Resolved, "{result:?}");
        assert_eq!(result.targets[0].owner, owner);
        println!("{name} -> {}::{} ({})", result.targets[0].owner, result.targets[0].name, result.targets[0].path);
    }
    Ok(())
}
