//! Resolve a UTF-8 byte offset in a source root with explicit local JARs.
//! Usage: inspect_project SOURCE_ROOT RELATIVE_JAVA_FILE BYTE_OFFSET [JAR ...]
use java_engine::{ClasspathEntry, ProjectConfig, ProjectSnapshot};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() < 3 {
        return Err("usage: inspect_project SOURCE_ROOT RELATIVE_JAVA_FILE BYTE_OFFSET [JAR ...]".into());
    }
    let root = PathBuf::from(&args[0]);
    let source = root.join(&args[1]).canonicalize()?;
    let byte: u32 = args[2].to_str().ok_or("byte offset is not UTF-8")?.parse()?;
    let config = ProjectConfig {
        source_roots: vec![root],
        classpath: args[3..].iter().map(|arg| ClasspathEntry::Jar(PathBuf::from(arg))).collect(),
        ..Default::default()
    };
    let snapshot = ProjectSnapshot::load(&config)?;
    let source = source.to_str().ok_or("source path is not UTF-8")?;
    let file = snapshot.project().file(source).ok_or("source is not in the configured root")?;
    if byte as usize >= file.text().len() || !file.text().is_char_boundary(byte as usize) {
        return Err("byte offset is outside the source or inside a UTF-8 character".into());
    }
    let result = snapshot.resolve_at(source, byte);
    println!(
        "{} sources, {} library classes, {} shadowed classes",
        snapshot.source_paths().len(),
        snapshot.class_origins().len(),
        snapshot.shadowed_classes().len()
    );
    println!("{:?}", result.status);
    for target in result.targets {
        println!("{}::{} at {} [{}..{}]", target.owner, target.name, target.path, target.start_byte, target.end_byte);
    }
    Ok(())
}
