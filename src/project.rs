//! Explicit project inputs and filesystem-independent analysis snapshots.
//!
//! This is one flat compilation scope, not a Maven/Gradle importer or a JPMS
//! module graph. Loading never starts a JVM, fetches dependencies, or watches
//! the filesystem. Callers load a fresh snapshot and swap it only on success.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::jar::read_jar_with_usage;
use crate::{ClassSignature, JarOptions, Project, ResolveResult, parse_class_bytes};

/// One entry on the ordered, flat binary classpath. No discovery or downloads occur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClasspathEntry {
    Directory(PathBuf),
    Jar(PathBuf),
}

/// Filesystem loading limits, in addition to per-archive [`JarOptions`] limits.
#[derive(Clone, Debug)]
pub struct ProjectLimits {
    /// Maximum entries visited across directory walks and all JAR entries.
    pub max_entries: usize,
    /// Maximum number of binary classpath entries.
    pub max_classpath_entries: usize,
    /// Maximum bytes in one source or directory class file.
    pub max_file_bytes: u64,
    /// Sum of source/class bytes read and compressed archive file lengths.
    /// Decompressed archive bytes are bounded separately by `JarOptions` per JAR.
    pub max_input_bytes: u64,
    /// Sum of declared uncompressed entry sizes across all JARs, including resources.
    pub max_archive_expanded_bytes: u64,
    /// Maximum subdirectory nesting below an explicitly configured root.
    pub max_directory_depth: usize,
}

impl Default for ProjectLimits {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_classpath_entries: 128,
            max_file_bytes: 8 * 1024 * 1024,
            max_input_bytes: 256 * 1024 * 1024,
            max_archive_expanded_bytes: 512 * 1024 * 1024,
            max_directory_depth: 64,
        }
    }
}

/// An explicit build-tool-neutral description of one flat compilation scope.
///
/// Roots and entries are local paths, resolved against the current directory at
/// load time. Sources are UTF-8. No default JDK, classpath, or generated/test root
/// is inferred. Include desired source roots explicitly; separate compilation
/// scopes must be loaded separately. `jar_options.target_release` selects JAR
/// variants, not a parser language level or a `--release` API compatibility check.
#[derive(Clone, Debug, Default)]
pub struct ProjectConfig {
    pub source_roots: Vec<PathBuf>,
    /// First entry wins for duplicate binary names. Discarded origins are reported.
    /// Duplicate source types, including source/binary collisions, remain ambiguous.
    pub classpath: Vec<ClasspathEntry>,
    pub jar_options: JarOptions,
    pub limits: ProjectLimits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectLoadErrorKind {
    Io,
    InvalidPath,
    LimitExceeded,
    InvalidUtf8,
    DuplicateSource,
    ConflictingOrigin,
    UnsupportedModule,
    ClassFile,
    Jar,
}

/// A load failure never changes any previously returned snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectLoadError {
    pub origin: String,
    pub kind: ProjectLoadErrorKind,
    pub detail: String,
}
impl fmt::Display for ProjectLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {:?}: {}", self.origin, self.kind, self.detail)
    }
}
impl std::error::Error for ProjectLoadError {}

/// A class ignored because an earlier classpath entry supplied the same binary name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShadowedClass {
    pub binary_name: String,
    pub selected_origin: String,
    pub shadowed_origin: String,
}

/// Parsed inputs owned by one load. Later filesystem changes cannot mutate it.
///
/// Source identities are canonical absolute UTF-8 paths, stable across reloads
/// at the same location, not across renames. Binary identities retain their
/// directory path or `archive!/entry` origin. This is not yet a persistent symbol
/// ID or incremental query database. Resolution uses the existing binder and
/// still rebuilds its index per query.
#[derive(Clone, Debug)]
pub struct ProjectSnapshot {
    project: Project,
    source_paths: Vec<String>,
    class_origins: Vec<String>,
    shadowed_classes: Vec<ShadowedClass>,
}

impl ProjectSnapshot {
    /// Read all explicit inputs in deterministic order. Incomplete source syntax
    /// is retained for editor use; I/O, invalid UTF-8, and bad class/JAR inputs
    /// fail the whole load. Directory symlinks and symlink entries are rejected.
    ///
    /// This is an owned snapshot after loading, not an atomic filesystem view:
    /// callers should avoid modifying inputs concurrently with a load. Bounds
    /// limit I/O and archive expansion; they are not a parser CPU/stack budget.
    pub fn load(config: &ProjectConfig) -> Result<Self, ProjectLoadError> {
        if config.classpath.len() > config.limits.max_classpath_entries {
            return Err(error("classpath", ProjectLoadErrorKind::LimitExceeded, "too many classpath entries"));
        }
        let mut budget = Budget { limits: &config.limits, entries: 0, bytes: 0 };
        let mut expanded_bytes = 0_u64;
        let mut project = Project::new();
        let mut source_paths = BTreeSet::new();
        for root in &config.source_roots {
            let root = checked_path(root, true)?;
            let files = walk(&root, "java", 0, &mut budget)?;
            for path in files {
                let origin = path_string(&path)?;
                if !source_paths.insert(origin.clone()) {
                    return Err(error(&origin, ProjectLoadErrorKind::DuplicateSource, "overlapping source roots"));
                }
                if path.file_name().is_some_and(|name| name == "module-info.java") {
                    return Err(error(
                        &origin,
                        ProjectLoadErrorKind::UnsupportedModule,
                        "JPMS source modules are not supported; configure a flat compilation scope",
                    ));
                }
                let bytes = read_bounded(&path, &mut budget)?;
                let text = String::from_utf8(bytes)
                    .map_err(|err| error(&origin, ProjectLoadErrorKind::InvalidUtf8, &err.to_string()))?;
                project.insert(origin, &text);
            }
        }
        let mut classes = BTreeMap::<String, String>::new();
        let mut class_origins = Vec::new();
        let mut shadowed_classes = Vec::new();
        for entry in &config.classpath {
            let signatures = match entry {
                ClasspathEntry::Directory(root) => {
                    let root = checked_path(root, true)?;
                    let mut signatures = Vec::new();
                    for path in walk(&root, "class", 0, &mut budget)? {
                        // On a flat classpath a module descriptor does not create a module.
                        if path == root.join("module-info.class") {
                            continue;
                        }
                        let origin = path_string(&path)?;
                        let bytes = read_bounded(&path, &mut budget)?;
                        let signature = parse_class_bytes(&origin, &bytes)
                            .map_err(|err| error(&origin, ProjectLoadErrorKind::ClassFile, &err.to_string()))?;
                        let expected = path
                            .strip_prefix(&root)
                            .expect("walk stays under root")
                            .with_extension("")
                            .components()
                            .map(|part| part.as_os_str().to_str().expect("validated UTF-8 path"))
                            .collect::<Vec<_>>()
                            .join("/");
                        if signature.binary_name != expected {
                            return Err(error(
                                &origin,
                                ProjectLoadErrorKind::ClassFile,
                                "class binary name does not match classpath-relative path",
                            ));
                        }
                        signatures.push(signature);
                    }
                    signatures
                }
                ClasspathEntry::Jar(path) => {
                    let path = checked_path(path, false)?;
                    let origin = path_string(&path)?;
                    let mut options = config.jar_options.clone();
                    options.max_archive_bytes =
                        options.max_archive_bytes.min(config.limits.max_input_bytes.saturating_sub(budget.bytes));
                    options.max_entries =
                        options.max_entries.min(config.limits.max_entries.saturating_sub(budget.entries));
                    options.max_total_uncompressed_bytes = options
                        .max_total_uncompressed_bytes
                        .min(config.limits.max_archive_expanded_bytes.saturating_sub(expanded_bytes));
                    let loaded = read_jar_with_usage(&path, &options)
                        .map_err(|err| error(&origin, ProjectLoadErrorKind::Jar, &err.to_string()))?;
                    budget.add_bytes(loaded.archive_bytes, &path)?;
                    budget.add_entries(loaded.entries, &path)?;
                    expanded_bytes = expanded_bytes
                        .checked_add(loaded.uncompressed_bytes)
                        .filter(|n| *n <= config.limits.max_archive_expanded_bytes)
                        .ok_or_else(|| {
                            error(
                                &origin,
                                ProjectLoadErrorKind::LimitExceeded,
                                "total archive expansion limit exceeded",
                            )
                        })?;
                    loaded.classes
                }
            };
            for signature in signatures {
                if project
                    .class_signature(&signature.origin)
                    .is_some_and(|prior| prior.binary_name != signature.binary_name)
                {
                    return Err(error(
                        &signature.origin,
                        ProjectLoadErrorKind::ConflictingOrigin,
                        "different binary names share a directory/archive origin",
                    ));
                }
                register_class(&mut project, signature, &mut classes, &mut class_origins, &mut shadowed_classes);
            }
        }
        Ok(Self { project, source_paths: source_paths.into_iter().collect(), class_origins, shadowed_classes })
    }

    pub fn source_paths(&self) -> &[String] {
        &self.source_paths
    }
    pub fn class_origins(&self) -> &[String] {
        &self.class_origins
    }
    pub fn shadowed_classes(&self) -> &[ShadowedClass] {
        &self.shadowed_classes
    }
    /// Read-only access to the existing analysis API. The snapshot has no mutation API.
    pub fn project(&self) -> &Project {
        &self.project
    }
    pub fn resolve_at(&self, path: &str, byte: u32) -> ResolveResult {
        self.project.resolve_at(path, byte)
    }
}

fn register_class(
    project: &mut Project,
    signature: ClassSignature,
    classes: &mut BTreeMap<String, String>,
    origins: &mut Vec<String>,
    shadowed: &mut Vec<ShadowedClass>,
) {
    if let Some(selected) = classes.get(&signature.binary_name) {
        shadowed.push(ShadowedClass {
            binary_name: signature.binary_name,
            selected_origin: selected.clone(),
            shadowed_origin: signature.origin,
        });
    } else {
        classes.insert(signature.binary_name.clone(), signature.origin.clone());
        origins.push(signature.origin.clone());
        project.insert_class_signature(signature);
    }
}

struct Budget<'a> {
    limits: &'a ProjectLimits,
    entries: usize,
    bytes: u64,
}
impl Budget<'_> {
    fn add_entries(&mut self, count: usize, path: &Path) -> Result<(), ProjectLoadError> {
        self.entries = self
            .entries
            .checked_add(count)
            .filter(|n| *n <= self.limits.max_entries)
            .ok_or_else(|| error_path(path, ProjectLoadErrorKind::LimitExceeded, "too many input entries"))?;
        Ok(())
    }
    fn add_bytes(&mut self, count: u64, path: &Path) -> Result<(), ProjectLoadError> {
        self.bytes =
            self.bytes.checked_add(count).filter(|n| *n <= self.limits.max_input_bytes).ok_or_else(|| {
                error_path(path, ProjectLoadErrorKind::LimitExceeded, "total input byte limit exceeded")
            })?;
        Ok(())
    }
}

fn checked_path(path: &Path, directory: bool) -> Result<PathBuf, ProjectLoadError> {
    let meta = fs::symlink_metadata(path).map_err(|err| io_error(path, err))?;
    if meta.file_type().is_symlink() || (directory && !meta.is_dir()) || (!directory && !meta.is_file()) {
        return Err(error_path(
            path,
            ProjectLoadErrorKind::InvalidPath,
            "expected a regular file/directory, not a symlink",
        ));
    }
    let canonical = fs::canonicalize(path).map_err(|err| io_error(path, err))?;
    path_string(&canonical)?;
    Ok(canonical)
}

fn walk(dir: &Path, extension: &str, depth: usize, budget: &mut Budget<'_>) -> Result<Vec<PathBuf>, ProjectLoadError> {
    if depth > budget.limits.max_directory_depth {
        return Err(error_path(dir, ProjectLoadErrorKind::LimitExceeded, "directory nesting limit exceeded"));
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir).map_err(|err| io_error(dir, err))? {
        let entry = entry.map_err(|err| io_error(dir, err))?;
        budget.add_entries(1, &entry.path())?;
        entries.push(entry);
    }
    entries.sort_by_key(|entry| entry.file_name());
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.path();
        let kind = entry.file_type().map_err(|err| io_error(&path, err))?;
        if kind.is_symlink() {
            return Err(error_path(&path, ProjectLoadErrorKind::InvalidPath, "symlink entries are not followed"));
        }
        if kind.is_dir() {
            files.extend(walk(&path, extension, depth + 1, budget)?);
        } else if kind.is_file() && path.extension().is_some_and(|value| value == extension) {
            path_string(&path)?;
            files.push(path);
        }
    }
    Ok(files)
}

fn read_bounded(path: &Path, budget: &mut Budget<'_>) -> Result<Vec<u8>, ProjectLoadError> {
    let file = File::open(path).map_err(|err| io_error(path, err))?;
    let remaining = budget.limits.max_input_bytes.saturating_sub(budget.bytes);
    let limit = budget.limits.max_file_bytes.min(remaining);
    if file.metadata().map_err(|err| io_error(path, err))?.len() > limit {
        return Err(error_path(path, ProjectLoadErrorKind::LimitExceeded, "input file byte limit exceeded"));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes).map_err(|err| io_error(path, err))?;
    if bytes.len() as u64 > limit {
        return Err(error_path(path, ProjectLoadErrorKind::LimitExceeded, "input grew beyond byte limit"));
    }
    budget.add_bytes(bytes.len() as u64, path)?;
    Ok(bytes)
}
fn path_string(path: &Path) -> Result<String, ProjectLoadError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| error_path(path, ProjectLoadErrorKind::InvalidPath, "paths must be valid UTF-8"))
}
fn io_error(path: &Path, err: std::io::Error) -> ProjectLoadError {
    error_path(path, ProjectLoadErrorKind::Io, &err.to_string())
}
fn error_path(path: &Path, kind: ProjectLoadErrorKind, detail: &str) -> ProjectLoadError {
    error(&path.display().to_string(), kind, detail)
}
fn error(origin: &str, kind: ProjectLoadErrorKind, detail: &str) -> ProjectLoadError {
    ProjectLoadError { origin: origin.to_owned(), kind, detail: detail.to_owned() }
}
