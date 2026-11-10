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
use std::sync::Arc;

use crate::jar::{ArchiveReads, SharedArchiveRead, read_jar_with_usage, read_jmod_with_usage};
use crate::{ClassSignature, JarOptions, JmodOptions, Project, ResolveResult, parse_class_bytes};

/// One entry on the ordered, flat binary classpath. No discovery or downloads occur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClasspathEntry {
    Directory(PathBuf),
    Jar(PathBuf),
}

/// Explicit platform class libraries, read only as data. No JDK is discovered.
/// Entries are searched before the application classpath; JPMS is not enforced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlatformEntry {
    Jmod(PathBuf),
    /// A legacy platform JAR such as rt.jar, with normal JAR selection rules.
    Jar(PathBuf),
}

/// Filesystem loading limits, in addition to [`JarOptions`] and [`JmodOptions`] limits.
#[derive(Clone, Debug)]
pub struct ProjectLimits {
    /// Maximum entries visited across directory walks and all archive entries.
    pub max_entries: usize,
    /// Maximum combined number of platform and application binary entries.
    pub max_classpath_entries: usize,
    /// Maximum bytes in one source or directory class file.
    pub max_file_bytes: u64,
    /// Sum of source/class bytes read and compressed archive file lengths.
    /// Decompressed archive bytes are also bounded separately per archive.
    pub max_input_bytes: u64,
    /// Sum of declared uncompressed entry sizes across all JARs/JMODs, including resources.
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
    /// Literal slash-separated paths relative to every source root independently.
    /// An exact regular file is omitted; an exact real directory is pruned.
    /// Empty by default. See [Self::validate_source_exclusions] for syntax and bounds.
    pub source_exclusions: Vec<String>,
    /// First entry wins for duplicate binary names. Discarded origins are reported.
    /// Duplicate source types, including source/binary collisions, remain ambiguous.
    pub classpath: Vec<ClasspathEntry>,
    /// Ordered platform binaries, taking precedence over application binaries.
    pub platform: Vec<PlatformEntry>,
    pub jmod_options: JmodOptions,
    pub jar_options: JarOptions,
    pub limits: ProjectLimits,
}

impl ProjectConfig {
    /// Validate source-selection syntax and bounds without filesystem I/O.
    ///
    /// At most 1,024 distinct rules of at most 4,096 UTF-8 bytes each are accepted.
    /// Rules use literal, case-sensitive components separated by single slashes.
    /// Empty, dot and dot-dot components, backslashes, colons, glob metacharacters
    /// and Unicode Cc controls are rejected. No normalization, globbing or negation
    /// is performed. Missing matches and redundant ancestor/descendant rules are
    /// allowed. Nonempty exclusions require at least one configured source root.
    pub fn validate_source_exclusions(&self) -> Result<(), ProjectLoadError> {
        let invalid = |detail| error("source_exclusions", ProjectLoadErrorKind::InvalidPath, detail);
        if self.source_exclusions.len() > 1024 {
            return Err(error(
                "source_exclusions",
                ProjectLoadErrorKind::LimitExceeded,
                "at most 1024 source exclusion rules are allowed",
            ));
        }
        if !self.source_exclusions.is_empty() && self.source_roots.is_empty() {
            return Err(invalid("source exclusions require a source root"));
        }
        let mut seen = BTreeSet::new();
        for rule in &self.source_exclusions {
            if rule.len() > 4096 {
                return Err(error(
                    "source_exclusions",
                    ProjectLoadErrorKind::LimitExceeded,
                    "source exclusion rules must not exceed 4096 UTF-8 bytes",
                ));
            }
            if rule.split('/').any(|part| part.is_empty() || part == "." || part == "..")
                || rule.chars().any(|c| c.is_control() || matches!(c, '\\' | ':' | '*' | '?' | '[' | ']' | '{' | '}'))
            {
                return Err(invalid("source exclusions must be literal root-relative slash-separated paths"));
            }
            if !seen.insert(rule) {
                return Err(invalid("duplicate source exclusion rule"));
            }
        }
        Ok(())
    }
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
    Jmod,
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

/// A class ignored because an earlier platform or application classpath entry supplied the same binary name.
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
/// ID or incremental query database. Legacy resolution rebuilds the binder index
/// per query; `snapshot.project().prepare()` retains one index for repeated queries.
#[derive(Clone, Debug)]
pub struct ProjectSnapshot {
    project: Project,
    source_paths: Vec<String>,
    class_origins: Vec<String>,
    shadowed_classes: Vec<ShadowedClass>,
    pub(crate) usage: (usize, u64, u64),
    // Original compilation limits, not a workspace loader's temporary allowance.
    pub(crate) limits: ProjectLimits,
}

impl ProjectSnapshot {
    /// Read all explicit inputs in deterministic order. Incomplete source syntax
    /// is retained for editor use; I/O, invalid UTF-8, and bad class/JAR/JMOD inputs
    /// fail the whole load. Directory symlinks and symlink entries are rejected.
    ///
    /// This is an owned snapshot after loading, not an atomic filesystem view:
    /// callers should avoid modifying inputs concurrently with a load. Bounds
    /// limit I/O and archive expansion; they are not a parser CPU/stack budget.
    pub fn load(config: &ProjectConfig) -> Result<Self, ProjectLoadError> {
        Self::load_with_archives(config, None)
    }

    pub(crate) fn load_with_archives(
        config: &ProjectConfig,
        mut archives: Option<&mut ArchiveReads>,
    ) -> Result<Self, ProjectLoadError> {
        config.validate_source_exclusions()?;
        if config.platform.len().saturating_add(config.classpath.len()) > config.limits.max_classpath_entries {
            return Err(error(
                "classpath",
                ProjectLoadErrorKind::LimitExceeded,
                "too many platform and application classpath entries",
            ));
        }
        let mut budget = Budget { limits: &config.limits, entries: 0, bytes: 0 };
        let mut expanded_bytes = 0_u64;
        let mut project = Project::new();
        let mut source_paths = BTreeSet::new();
        for root in &config.source_roots {
            let root = checked_path(root, true)?;
            // Rebase independently for each root: a nested root may select a file
            // pruned from an outer root. This does not relax duplicate ownership.
            let exclusions = config.source_exclusions.iter().map(|rule| root.join(rule)).collect();
            let files = walk(&root, "java", 0, &mut budget, Some(&exclusions))?;
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
        let mut observed_origins = BTreeMap::<String, String>::new();
        let mut class_origins = Vec::new();
        let mut shadowed_classes = Vec::new();
        let inputs = config
            .platform
            .iter()
            .map(|entry| match entry {
                PlatformEntry::Jmod(path) => BinaryInput::Jmod(path),
                PlatformEntry::Jar(path) => BinaryInput::Jar(path),
            })
            .chain(config.classpath.iter().map(|entry| match entry {
                ClasspathEntry::Directory(path) => BinaryInput::Directory(path),
                ClasspathEntry::Jar(path) => BinaryInput::Jar(path),
            }));
        for entry in inputs {
            let signatures = match entry {
                BinaryInput::Directory(root) => {
                    let root = checked_path(root, true)?;
                    let mut signatures = Vec::new();
                    for path in walk(&root, "class", 0, &mut budget, None)? {
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
                        signatures.push(Arc::new(signature));
                    }
                    signatures
                }
                BinaryInput::Jar(path) | BinaryInput::Jmod(path) => {
                    let path = checked_path(path, false)?;
                    let origin = path_string(&path)?;
                    let remaining_bytes = config.limits.max_input_bytes.saturating_sub(budget.bytes);
                    let remaining_entries = config.limits.max_entries.saturating_sub(budget.entries);
                    let remaining_expansion = config.limits.max_archive_expanded_bytes.saturating_sub(expanded_bytes);
                    let loaded = if matches!(entry, BinaryInput::Jmod(_)) {
                        let mut options = config.jmod_options.clone();
                        options.max_archive_bytes = options.max_archive_bytes.min(remaining_bytes);
                        options.max_entries = options.max_entries.min(remaining_entries);
                        options.max_total_uncompressed_bytes =
                            options.max_total_uncompressed_bytes.min(remaining_expansion);
                        match archives.as_deref_mut() {
                            Some(archives) => archives.read_jmod(&path, &options),
                            None => read_jmod_with_usage(&path, &options).map(SharedArchiveRead::from),
                        }
                        .map_err(|err| error(&origin, ProjectLoadErrorKind::Jmod, &err.to_string()))?
                    } else {
                        let mut options = config.jar_options.clone();
                        options.max_archive_bytes = options.max_archive_bytes.min(remaining_bytes);
                        options.max_entries = options.max_entries.min(remaining_entries);
                        options.max_total_uncompressed_bytes =
                            options.max_total_uncompressed_bytes.min(remaining_expansion);
                        match archives.as_deref_mut() {
                            Some(archives) => archives.read_jar(&path, &options),
                            None => read_jar_with_usage(&path, &options).map(SharedArchiveRead::from),
                        }
                        .map_err(|err| error(&origin, ProjectLoadErrorKind::Jar, &err.to_string()))?
                    };
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
                if observed_origins
                    .insert(signature.origin.clone(), signature.binary_name.clone())
                    .is_some_and(|prior| prior != signature.binary_name)
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
        Ok(Self {
            project,
            source_paths: source_paths.into_iter().collect(),
            class_origins,
            shadowed_classes,
            usage: (budget.entries, budget.bytes, expanded_bytes),
            limits: config.limits.clone(),
        })
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

    /// The workspace validates ownership and the entire replacement budget first.
    pub(crate) fn replace_source_text(&mut self, path: &str, text: &str) {
        self.project.insert(path, text);
    }
}

#[derive(Clone, Copy)]
enum BinaryInput<'a> {
    Directory(&'a Path),
    Jar(&'a Path),
    Jmod(&'a Path),
}

fn register_class(
    project: &mut Project,
    signature: Arc<ClassSignature>,
    classes: &mut BTreeMap<String, String>,
    origins: &mut Vec<String>,
    shadowed: &mut Vec<ShadowedClass>,
) {
    if let Some(selected) = classes.get(&signature.binary_name) {
        shadowed.push(ShadowedClass {
            binary_name: signature.binary_name.clone(),
            selected_origin: selected.clone(),
            shadowed_origin: signature.origin.clone(),
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

fn walk(
    dir: &Path,
    extension: &str,
    depth: usize,
    budget: &mut Budget<'_>,
    exclusions: Option<&BTreeSet<PathBuf>>,
) -> Result<Vec<PathBuf>, ProjectLoadError> {
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
        // Encountered entries are already charged and symlinks already refused.
        // Pruned descendants are never visited, typechecked or read.
        if exclusions.is_some_and(|paths| paths.contains(&path)) {
            validate_excluded_entry(&path, kind)?;
            continue;
        }
        if kind.is_dir() {
            files.extend(walk(&path, extension, depth + 1, budget, exclusions)?);
        } else if kind.is_file() && path.extension().is_some_and(|value| value == extension) {
            path_string(&path)?;
            files.push(path);
        }
    }
    Ok(files)
}

fn validate_excluded_entry(path: &Path, kind: fs::FileType) -> Result<(), ProjectLoadError> {
    path_string(path)?;
    if !kind.is_file() && !kind.is_dir() {
        return Err(error_path(
            path,
            ProjectLoadErrorKind::InvalidPath,
            "excluded entries must be regular files or real directories",
        ));
    }
    Ok(())
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

#[cfg(all(test, unix))]
mod source_exclusion_metadata_tests {
    use super::*;

    #[test]
    fn source_exclusions_reject_matched_special_file_metadata() {
        // Metadata-only regression for the shared matched-entry check, not a
        // whole-walker socket fixture. No special file or permissions are created.
        let kind = fs::symlink_metadata("/dev/null").unwrap().file_type();
        assert!(!kind.is_file() && !kind.is_dir() && !kind.is_symlink());
        let error = validate_excluded_entry(Path::new("/dev/null"), kind).unwrap_err();
        assert_eq!(error.kind, ProjectLoadErrorKind::InvalidPath);
    }

    #[test]
    fn source_exclusions_matched_entry_check_rejects_non_utf8_paths() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let kind = fs::symlink_metadata(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap().file_type();
        let path = PathBuf::from(OsString::from_vec(b"invalid\xff.java".to_vec()));
        assert_eq!(validate_excluded_entry(&path, kind).unwrap_err().kind, ProjectLoadErrorKind::InvalidPath);
    }
}
