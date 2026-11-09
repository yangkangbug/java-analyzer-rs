//! Optional oracle's native endpoint. This executable never starts Java.
use java_engine::Project;
use std::{env, fs, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if !(3..=4).contains(&args.len()) {
        return Err("usage: oracle_bindings SOURCE_ROOT REQUESTS_TSV [CLASS_DIRECTORY]".into());
    }
    let mut project = Project::new();
    let requests = fs::read_to_string(&args[2])?;
    for line in requests.lines() {
        let cells: Vec<_> = line.split('\t').collect();
        if let ["F", path] = cells.as_slice() {
            project.insert(*path, &fs::read_to_string(Path::new(&args[1]).join(path))?);
        }
    }
    if let Some(root) = args.get(3) {
        project.add_class_directory(Path::new(root))?;
    }
    let prepared = project.prepare();
    for line in requests.lines() {
        let cells: Vec<_> = line.split('\t').collect();
        match cells.as_slice() {
            ["F", _] => (),
            ["Q", id, path, offset] => {
                let result = prepared.resolve_at(path, offset.parse()?);
                print!("{id}\t{:?}\t{}", result.status, result.targets.len());
                for target in result.targets {
                    print!(
                        "\t{}|{:?}|{}|{}|{}|{}",
                        target.path, target.kind, target.owner, target.name, target.start_byte, target.end_byte
                    );
                }
                println!();
            }
            _ => return Err(format!("malformed request: {line}").into()),
        }
    }
    Ok(())
}
