//! Bounded one-shot full-text overlays. Labels are caller correlation data only.
use java_engine::{SourceOverlay, SourceOverlayErrorKind, WorkspaceConfig, WorkspaceSnapshot};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use super::{
    Error, OutputBudget,
    config::{Id, local_path},
};

const MAX_OVERLAYS: usize = 1024;
// Every integer must round-trip through common JSON/editor number implementations.
const MAX_LABEL: u64 = 9_007_199_254_740_991;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overlays {
    schema_version: u32,
    revision: u64,
    files: Vec<Overlay>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Overlay {
    compilation: Id,
    file: String,
    version: u64,
    text: String,
}
impl Overlays {
    pub fn validate(&self) -> Result<(), Error> {
        super::version(self.schema_version)?;
        if self.files.len() > MAX_OVERLAYS {
            return Err(Error::new("overlay_limit", "overlays must contain at most 1024 files"));
        }
        if self.revision > MAX_LABEL || self.files.iter().any(|f| f.version > MAX_LABEL) {
            return Err(Error::new(
                "invalid_overlay",
                "revision and document versions must be integers in 0..9007199254740991",
            ));
        }
        for file in &self.files {
            file.compilation.validate()?;
            local_path(Path::new(""), &file.file)?;
        }
        Ok(())
    }
    pub fn validate_compilations(&self, config: &WorkspaceConfig) -> Result<(), Error> {
        for file in &self.files {
            if !config.compilations.iter().any(|c| c.id == file.compilation.core()) {
                return Err(Error::new("unknown_compilation", "overlay owner is not configured").at(&file.file));
            }
        }
        Ok(())
    }
    pub fn apply(
        &self,
        workspace: &WorkspaceSnapshot,
        base: &Path,
        budget: &mut OutputBudget,
    ) -> Result<(WorkspaceSnapshot, Value), Error> {
        let sources = super::sources(workspace);
        let mut paths = BTreeSet::new();
        let mut validated = Vec::new();
        let mut states = Vec::new();
        budget.charge(self.files.len())?;
        // Resolve and validate the entire set before creating the replacement snapshot.
        // Aliases are accepted only for an already loaded source and its exact owner.
        for file in &self.files {
            let requested: PathBuf = local_path(base, &file.file)?;
            let canonical = requested.canonicalize().map_err(|e| Error::new("io", e.to_string()).at(&requested))?;
            let path = canonical
                .to_str()
                .ok_or_else(|| Error::new("invalid_path", "overlay source path must be UTF-8").at(&requested))?;
            let source = sources
                .get(path)
                .ok_or_else(|| Error::new("invalid_overlay", "overlay source is not already loaded").at(path))?;
            if file.compilation.core() != *source.id {
                return Err(Error::new("invalid_overlay", "overlay compilation must be the source owner").at(path));
            }
            if !paths.insert(path.to_owned()) {
                return Err(Error::new("invalid_overlay", "duplicate canonical overlay source").at(path));
            }
            validated.push(SourceOverlay {
                owner: file.compilation.core(),
                path: path.to_owned(),
                text: file.text.clone(),
            });
            states.push(json!({"compilation":file.compilation,"file":path,"version":file.version}));
        }
        let updated = workspace.with_source_overlays(&validated).map_err(|e| {
            let code =
                if e.kind == SourceOverlayErrorKind::LimitExceeded { "overlay_limit" } else { "invalid_overlay" };
            let mut error = Error::new(code, e.to_string());
            error.path = e.path;
            error
        })?;
        Ok((updated, json!({"revision":self.revision,"files":states})))
    }
}
