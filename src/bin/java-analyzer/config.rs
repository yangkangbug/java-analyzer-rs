//! JSON adapter types. The analysis library does not depend on this schema.
use java_engine::{
    ClasspathEntry, CompilationConfig, CompilationId, JarOptions, JmodOptions, PlatformEntry, ProjectConfig,
    ProjectLimits, WorkspaceConfig,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::Error;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Id {
    pub project: String,
    pub source_set: String,
}
impl Id {
    pub fn core(&self) -> CompilationId {
        CompilationId::new(&self.project, &self.source_set)
    }
    pub fn validate(&self) -> Result<(), Error> {
        if [&self.project, &self.source_set].iter().any(|s| s.is_empty() || s.chars().any(char::is_whitespace)) {
            return Err(Error::new(
                "invalid_configuration",
                "compilation ID parts must be nonempty and whitespace-free",
            ));
        }
        Ok(())
    }
}
impl From<&CompilationId> for Id {
    fn from(id: &CompilationId) -> Self {
        Self { project: id.project.clone(), source_set: id.source_set.clone() }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub compilations: Vec<Compilation>,
    #[serde(default)]
    pub limits: WorkspaceLimits,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Compilation {
    pub id: Id,
    pub source_roots: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<Id>,
    #[serde(default)]
    pub classpath: Vec<BinaryEntry>,
    #[serde(default)]
    pub platform: Vec<PlatformInput>,
    #[serde(default = "default_release")]
    pub target_release: u16,
    #[serde(default)]
    pub limits: InputLimits,
    #[serde(default)]
    pub jar_limits: ArchiveLimits,
    #[serde(default)]
    pub jmod_limits: JmodLimits,
}
fn default_release() -> u16 {
    JarOptions::default().target_release
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BinaryEntry {
    Directory { path: String },
    Jar { path: String },
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlatformInput {
    Jmod { path: String },
    Jar { path: String },
}
#[derive(Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspaceLimits {
    pub max_compilations: usize,
    pub max_dependency_edges: usize,
    pub max_entries: usize,
    pub max_input_bytes: u64,
    pub max_archive_expanded_bytes: u64,
}
impl Default for WorkspaceLimits {
    fn default() -> Self {
        let v = WorkspaceConfig::default();
        Self {
            max_compilations: v.max_compilations,
            max_dependency_edges: v.max_dependency_edges,
            max_entries: v.max_entries,
            max_input_bytes: v.max_input_bytes,
            max_archive_expanded_bytes: v.max_archive_expanded_bytes,
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct InputLimits {
    pub max_entries: usize,
    pub max_classpath_entries: usize,
    pub max_file_bytes: u64,
    pub max_input_bytes: u64,
    pub max_archive_expanded_bytes: u64,
    pub max_directory_depth: usize,
}
impl Default for InputLimits {
    fn default() -> Self {
        let v = ProjectLimits::default();
        Self {
            max_entries: v.max_entries,
            max_classpath_entries: v.max_classpath_entries,
            max_file_bytes: v.max_file_bytes,
            max_input_bytes: v.max_input_bytes,
            max_archive_expanded_bytes: v.max_archive_expanded_bytes,
            max_directory_depth: v.max_directory_depth,
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ArchiveLimits {
    pub max_archive_bytes: u64,
    pub max_entries: usize,
    pub max_entry_bytes: u64,
    pub max_total_uncompressed_bytes: u64,
}
impl Default for ArchiveLimits {
    fn default() -> Self {
        let v = JarOptions::default();
        Self {
            max_archive_bytes: v.max_archive_bytes,
            max_entries: v.max_entries,
            max_entry_bytes: v.max_entry_bytes,
            max_total_uncompressed_bytes: v.max_total_uncompressed_bytes,
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct JmodLimits {
    pub max_archive_bytes: u64,
    pub max_entries: usize,
    pub max_entry_bytes: u64,
    pub max_total_uncompressed_bytes: u64,
}
impl Default for JmodLimits {
    fn default() -> Self {
        let v = JmodOptions::default();
        Self {
            max_archive_bytes: v.max_archive_bytes,
            max_entries: v.max_entries,
            max_entry_bytes: v.max_entry_bytes,
            max_total_uncompressed_bytes: v.max_total_uncompressed_bytes,
        }
    }
}
impl JmodLimits {
    fn validate(&self) -> Result<(), Error> {
        ArchiveLimits {
            max_archive_bytes: self.max_archive_bytes,
            max_entries: self.max_entries,
            max_entry_bytes: self.max_entry_bytes,
            max_total_uncompressed_bytes: self.max_total_uncompressed_bytes,
        }
        .validate()
    }
}

fn positive(field: &str, value: u64) -> Result<(), Error> {
    if value == 0 { Err(Error::new("invalid_configuration", format!("{field} must be positive"))) } else { Ok(()) }
}
impl ArchiveLimits {
    fn validate(&self) -> Result<(), Error> {
        positive("archive.max_archive_bytes", self.max_archive_bytes)?;
        positive("archive.max_entries", self.max_entries as u64)?;
        positive("archive.max_entry_bytes", self.max_entry_bytes)?;
        positive("archive.max_total_uncompressed_bytes", self.max_total_uncompressed_bytes)
    }
}
/// No canonicalization here: the loader must see symlinks and reject them itself.
pub fn local_path(base: &Path, path: &str) -> Result<PathBuf, Error> {
    if path.is_empty() || path.contains('\0') {
        return Err(Error::new("invalid_path", "paths must be nonempty and contain no NUL").at(path));
    }
    Ok(base.join(path))
}
impl Manifest {
    pub fn core(&self, base: &Path) -> Result<WorkspaceConfig, Error> {
        super::version(self.schema_version)?;
        if self.compilations.is_empty() {
            return Err(Error::new("invalid_configuration", "compilations must not be empty"));
        }
        positive("limits.max_compilations", self.limits.max_compilations as u64)?;
        if self.limits.max_compilations > 128 || self.limits.max_dependency_edges > 1024 {
            return Err(Error::new(
                "invalid_configuration",
                "CLI graph ceilings are 128 compilations and 1024 dependency edges",
            ));
        }
        positive("limits.max_entries", self.limits.max_entries as u64)?;
        positive("limits.max_input_bytes", self.limits.max_input_bytes)?;
        positive("limits.max_archive_expanded_bytes", self.limits.max_archive_expanded_bytes)?;
        let mut compilations = Vec::new();
        for c in &self.compilations {
            c.id.validate()?;
            for id in &c.dependencies {
                id.validate()?;
            }
            positive("target_release", c.target_release.into())?;
            let l = &c.limits;
            positive("limits.max_entries", l.max_entries as u64)?;
            positive("limits.max_file_bytes", l.max_file_bytes)?;
            if l.max_file_bytes > u32::MAX.into() {
                return Err(Error::new(
                    "invalid_configuration",
                    "max_file_bytes exceeds the engine's u32 source positions",
                ));
            }
            positive("limits.max_input_bytes", l.max_input_bytes)?;
            positive("limits.max_archive_expanded_bytes", l.max_archive_expanded_bytes)?;
            c.jar_limits.validate()?;
            c.jmod_limits.validate()?;
            let source_roots = c.source_roots.iter().map(|p| local_path(base, p)).collect::<Result<_, _>>()?;
            let classpath = c
                .classpath
                .iter()
                .map(|e| match e {
                    BinaryEntry::Directory { path } => local_path(base, path).map(ClasspathEntry::Directory),
                    BinaryEntry::Jar { path } => local_path(base, path).map(ClasspathEntry::Jar),
                })
                .collect::<Result<_, _>>()?;
            let platform = c
                .platform
                .iter()
                .map(|e| match e {
                    PlatformInput::Jmod { path } => local_path(base, path).map(PlatformEntry::Jmod),
                    PlatformInput::Jar { path } => local_path(base, path).map(PlatformEntry::Jar),
                })
                .collect::<Result<_, _>>()?;
            let jar = &c.jar_limits;
            let jmod = &c.jmod_limits;
            compilations.push(CompilationConfig {
                id: c.id.core(),
                dependencies: c.dependencies.iter().map(Id::core).collect(),
                inputs: ProjectConfig {
                    source_roots,
                    classpath,
                    platform,
                    limits: ProjectLimits {
                        max_entries: l.max_entries,
                        max_classpath_entries: l.max_classpath_entries,
                        max_file_bytes: l.max_file_bytes,
                        max_input_bytes: l.max_input_bytes,
                        max_archive_expanded_bytes: l.max_archive_expanded_bytes,
                        max_directory_depth: l.max_directory_depth,
                    },
                    jar_options: JarOptions {
                        target_release: c.target_release,
                        max_archive_bytes: jar.max_archive_bytes,
                        max_entries: jar.max_entries,
                        max_entry_bytes: jar.max_entry_bytes,
                        max_total_uncompressed_bytes: jar.max_total_uncompressed_bytes,
                    },
                    jmod_options: JmodOptions {
                        max_archive_bytes: jmod.max_archive_bytes,
                        max_entries: jmod.max_entries,
                        max_entry_bytes: jmod.max_entry_bytes,
                        max_total_uncompressed_bytes: jmod.max_total_uncompressed_bytes,
                    },
                },
            });
        }
        Ok(WorkspaceConfig {
            compilations,
            max_compilations: self.limits.max_compilations,
            max_dependency_edges: self.limits.max_dependency_edges,
            max_entries: self.limits.max_entries,
            max_input_bytes: self.limits.max_input_bytes,
            max_archive_expanded_bytes: self.limits.max_archive_expanded_bytes,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requests {
    pub schema_version: u32,
    pub queries: Vec<Query>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub compilation: Id,
    pub file: String,
    pub position: Position,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "encoding", rename_all = "snake_case", deny_unknown_fields)]
pub enum Position {
    Utf8 { offset: u32 },
    Utf16 { offset: u32 },
    Utf16Line { line: u32, character: u32 },
}
impl Requests {
    pub fn validate(&self) -> Result<(), Error> {
        super::version(self.schema_version)?;
        if self.queries.is_empty() || self.queries.len() > super::MAX_QUERIES {
            return Err(Error::new(
                "request_limit",
                format!("queries must contain 1..={} entries", super::MAX_QUERIES),
            ));
        }
        for (index, q) in self.queries.iter().enumerate() {
            q.compilation.validate().map_err(|e| e.query(index))?;
            local_path(Path::new(""), &q.file).map_err(|e| e.query(index))?;
        }
        Ok(())
    }
}
