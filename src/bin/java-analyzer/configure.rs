//! Explicit-input configuration only: no build discovery, execution or file writes.
use super::{
    Error, MAX_DOCUMENT_BYTES,
    config::{self, BinaryEntry, Compilation, Id, Manifest, PlatformInput},
};
use java_engine::{JarOptions, WorkspaceSnapshot};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
    path::Path,
};

pub(super) const COMMAND: &str = "configure";

/// Include argument separators so empty arguments also consume the input budget.
pub(super) fn check_argument_bytes(args: &[String]) -> Result<(), Error> {
    args.iter()
        .try_fold(0usize, |total, arg| {
            total
                .checked_add(arg.len())
                .and_then(|n| n.checked_add(1))
                .filter(|&n| n <= MAX_DOCUMENT_BYTES as usize)
                .ok_or_else(|| Error::new("document_limit", "configure arguments exceed 1 MiB"))
        })
        .map(|_| ())
}

struct Group {
    compilation: Compilation,
    binary_only: bool,
    no_platform: bool,
    release_seen: bool,
}
impl Group {
    fn new(id: Id) -> Result<Self, Error> {
        id.validate()?;
        Ok(Self {
            compilation: Compilation {
                id,
                source_roots: vec![],
                source_exclusions: vec![],
                dependencies: vec![],
                classpath: vec![],
                platform: vec![],
                target_release: JarOptions::default().target_release,
                limits: Default::default(),
                jar_limits: Default::default(),
                jmod_limits: Default::default(),
            },
            binary_only: false,
            no_platform: false,
            release_seen: false,
        })
    }
    fn finish(self) -> Result<Compilation, Error> {
        if self.compilation.source_roots.is_empty() && !self.binary_only {
            return Err(Error::new("usage", "each compilation needs --source-root or explicit --binary-only"));
        }
        if self.compilation.platform.is_empty() && !self.no_platform {
            return Err(Error::new("usage", "each compilation needs explicit platform inputs or --no-platform"));
        }
        Ok(self.compilation)
    }
}
fn value<'a>(iter: &mut std::slice::Iter<'a, String>, flag: &str) -> Result<&'a str, Error> {
    iter.next().map(String::as_str).ok_or_else(|| Error::new("usage", format!("missing value for {flag}")))
}
fn id(iter: &mut std::slice::Iter<'_, String>, flag: &str) -> Result<Id, Error> {
    let result = Id { project: value(iter, flag)?.into(), source_set: value(iter, flag)?.into() };
    result.validate()?;
    Ok(result)
}
fn path(base: &Path, value: &str) -> Result<String, Error> {
    // Joining, rather than canonicalizing or lexically simplifying, leaves the
    // original leaf and symlink traversal semantics visible to the existing loader.
    let path = config::local_path(base, value)?;
    if !path.is_absolute() {
        return Err(Error::new("invalid_path", "configure paths must resolve to absolute paths").at(&path));
    }
    path.to_str().map(str::to_owned).ok_or_else(|| Error::new("invalid_path", "paths must be UTF-8").at(&path))
}
fn parse(args: &[String], base: &Path) -> Result<Manifest, Error> {
    check_argument_bytes(args)?;
    let mut manifest = Manifest { schema_version: 1, compilations: vec![], limits: Default::default() };
    let mut current: Option<Group> = None;
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        if flag == "--compilation" {
            if let Some(group) = current.take() {
                manifest.compilations.push(group.finish()?);
            }
            current = Some(Group::new(id(&mut iter, flag)?)?);
            continue;
        }
        let group = current
            .as_mut()
            .ok_or_else(|| Error::new("usage", "options must follow --compilation PROJECT SOURCE_SET"))?;
        match flag.as_str() {
            "--source-root" => {
                if group.binary_only {
                    return Err(Error::new("usage", "--source-root conflicts with --binary-only"));
                }
                group.compilation.source_roots.push(path(base, value(&mut iter, flag)?)?);
            }
            "--exclude-source" => group.compilation.source_exclusions.push(value(&mut iter, flag)?.into()),
            "--binary-only" => {
                if group.binary_only || !group.compilation.source_roots.is_empty() {
                    return Err(Error::new("usage", "duplicate --binary-only or conflict with --source-root"));
                }
                group.binary_only = true;
            }
            "--depends-on" => group.compilation.dependencies.push(id(&mut iter, flag)?),
            "--classpath-jar" | "--classpath-dir" => {
                let path = path(base, value(&mut iter, flag)?)?;
                group.compilation.classpath.push(if flag == "--classpath-jar" {
                    BinaryEntry::Jar { path }
                } else {
                    BinaryEntry::Directory { path }
                });
            }
            "--platform-jmod" | "--platform-jar" => {
                if group.no_platform {
                    return Err(Error::new("usage", "platform inputs conflict with --no-platform"));
                }
                let path = path(base, value(&mut iter, flag)?)?;
                group.compilation.platform.push(if flag == "--platform-jmod" {
                    PlatformInput::Jmod { path }
                } else {
                    PlatformInput::Jar { path }
                });
            }
            "--no-platform" => {
                if group.no_platform || !group.compilation.platform.is_empty() {
                    return Err(Error::new("usage", "duplicate --no-platform or conflict with platform inputs"));
                }
                group.no_platform = true;
            }
            "--target-release" => {
                if group.release_seen {
                    return Err(Error::new("usage", "duplicate option --target-release"));
                }
                group.release_seen = true;
                let release = value(&mut iter, flag)?;
                group.compilation.target_release = release
                    .parse::<u16>()
                    .ok()
                    .filter(|n| *n != 0 && !release.is_empty() && release.bytes().all(|b| b.is_ascii_digit()))
                    .ok_or_else(|| {
                        Error::new("invalid_configuration", "--target-release must be decimal digits in 1..65535")
                    })?;
            }
            _ => return Err(Error::new("usage", format!("unknown configure option {flag}"))),
        }
    }
    let group = current.ok_or_else(|| Error::new("usage", "configure needs --compilation PROJECT SOURCE_SET"))?;
    manifest.compilations.push(group.finish()?);
    Ok(manifest)
}

struct Document(Vec<u8>);
impl Write for Document {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > (MAX_DOCUMENT_BYTES as usize).saturating_sub(self.0.len()) {
            return Err(io::Error::other("generated manifest exceeds 1 MiB including newline"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn encode(manifest: &Manifest) -> Result<Vec<u8>, Error> {
    let mut output = Document(vec![]);
    serde_json::to_writer(&mut output, manifest).map_err(|e| Error::new("output_limit", e.to_string()))?;
    output.write_all(b"\n").map_err(|e| Error::new("output_limit", e.to_string()))?;
    Ok(output.0)
}

pub(super) fn run(args: &[String]) -> Result<Vec<u8>, Error> {
    let base = std::env::current_dir().map_err(|e| Error::new("io", e.to_string()))?;
    let manifest = parse(args, &base)?;
    let config = manifest.core(&base)?;
    let bytes = encode(&manifest)?;
    WorkspaceSnapshot::load(&config).map_err(|e| {
        let mut error = Error::new("load_failed", e.to_string());
        error.path = e.input.map(|input| input.origin);
        error
    })?;
    // Only the successful native worker publishes bytes. The supervisor validates
    // the entire typed document before writing any of it to the caller's stdout.
    Ok(bytes)
}

/// Validate the native worker's distinct success protocol without filesystem I/O.
/// Keep strict typed deserialization: Value would erase duplicate object fields.
pub(super) fn valid_output(bytes: &[u8]) -> bool {
    if bytes.len() > MAX_DOCUMENT_BYTES as usize {
        return false;
    }
    let Ok(manifest) = serde_json::from_slice::<Manifest>(bytes) else { return false };
    let Ok(config) = manifest.core(Path::new("")) else { return false };
    if config.compilations.len() > config.max_compilations {
        return false;
    }
    for c in &manifest.compilations {
        if !c.source_roots.iter().all(|p| Path::new(p).is_absolute())
            || !c.classpath.iter().all(|e| match e {
                BinaryEntry::Directory { path } | BinaryEntry::Jar { path } => Path::new(path).is_absolute(),
            })
            || !c.platform.iter().all(|e| match e {
                PlatformInput::Jmod { path } | PlatformInput::Jar { path } => Path::new(path).is_absolute(),
            })
        {
            return false;
        }
    }
    // Bound and check the graph structurally. Never reread inputs outside the
    // supervised worker, including paths that changed after its successful load.
    let mut ids = BTreeMap::new();
    for (index, c) in config.compilations.iter().enumerate() {
        if ids.insert(&c.id, index).is_some() {
            return false;
        }
    }
    let mut graph = Vec::new();
    let mut edge_count = 0usize;
    for c in &config.compilations {
        let mut edges = BTreeSet::new();
        for target in &c.dependencies {
            let Some(&index) = ids.get(target) else { return false };
            if !edges.insert(index) {
                return false;
            }
        }
        edge_count += edges.len();
        if edge_count > config.max_dependency_edges {
            return false;
        }
        graph.push(edges);
    }
    for via in 0..graph.len() {
        let reachable = graph[via].clone();
        for edges in &mut graph {
            if edges.contains(&via) {
                edges.extend(&reachable);
            }
        }
    }
    graph.iter().enumerate().all(|(index, edges)| !edges.contains(&index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_argument_and_document_limits_include_separators_and_final_newline() {
        let limit = MAX_DOCUMENT_BYTES as usize;
        assert!(check_argument_bytes(&["a".repeat(limit - 1)]).is_ok());
        assert_eq!(check_argument_bytes(&["a".repeat(limit)]).unwrap_err().code, "document_limit");
        assert_eq!(check_argument_bytes(&["a".repeat(limit - 1), String::new()]).unwrap_err().code, "document_limit");
        let mut doc = Document(vec![b' '; limit - 1]);
        doc.write_all(b"\n").unwrap();
        assert_eq!(doc.0.len(), limit);
        assert!(doc.write_all(b"\n").is_err());
        assert_eq!(doc.0.len(), limit);
        let mut manifest = parse(
            &["--compilation", "app", "main", "--binary-only", "--no-platform"].map(str::to_owned),
            Path::new(""),
        )
        .unwrap();
        let size = encode(&manifest).unwrap().len();
        manifest.compilations[0].id.project.push_str(&"a".repeat(limit - size));
        assert_eq!(encode(&manifest).unwrap().len(), limit);
        manifest.compilations[0].id.project.push('a');
        assert_eq!(encode(&manifest).unwrap_err().code, "output_limit");
    }
}

#[cfg(test)]
mod source_exclusion_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_exclusions_worker_success_validates_rules_without_io() {
        let root = std::env::temp_dir().join("absent-source-selection-worker-root");
        let base = json!({"schema_version":1,"compilations":[{
            "id":{"project":"app","source_set":"main"},"source_roots":[root],
            "source_exclusions":["module-info.java", "中文 !", "--cancel-on-stdin-close"]
        }]});
        assert!(valid_output((base.to_string() + "\n").as_bytes()));
        for rules in [
            json!([""]),
            json!(["same", "same"]),
            json!(["a/../b"]),
            json!(["*.java"]),
            json!(["a\u{7f}"]),
            json!(["a\u{85}"]),
            json!(["a".repeat(4097)]),
            json!(vec!["a"; 1025]),
            json!(null),
            json!("a"),
        ] {
            let mut changed = base.clone();
            changed["compilations"][0]["source_exclusions"] = rules;
            assert!(!valid_output((changed.to_string() + "\n").as_bytes()), "{changed}");
        }
        let mut no_roots = base;
        no_roots["compilations"][0]["source_roots"] = json!([]);
        assert!(!valid_output((no_roots.to_string() + "\n").as_bytes()));
    }
}
