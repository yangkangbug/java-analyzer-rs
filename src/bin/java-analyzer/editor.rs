//! Private, supervised jobs for the narrow editor broker. Not a security boundary.
//! The broker carries frozen configuration and full buffers; only this worker loads
//! and parses Java and converts target ranges from its own immutable snapshot.
use super::{
    Error, MAX_DOCUMENT_BYTES, OutputBudget, Source, capabilities,
    config::{self, Id, Manifest},
    overlays::{self, Overlays},
    read_document, read_document_bounded, status_name,
};
use java_engine::{LineIndex, ResolveStatus, Utf16Position, WorkspaceSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Component, Path},
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) const INSPECT_COMMAND: &str = "__editor-inspect-v1";
pub(super) const DEFINITION_COMMAND: &str = "__editor-definition-v1";
const MAX_JOB_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SourceOwner {
    compilation: Id,
    file: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Frozen {
    manifest_path: String,
    base: String,
    manifest: Manifest,
    sources: Vec<SourceOwner>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InspectResult {
    frozen: Frozen,
    sources: Vec<SourceOwner>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DefinitionJob {
    schema_version: u32,
    frozen: Frozen,
    generation: u64,
    overlays: Overlays,
    query: Query,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    compilation: Id,
    file: String,
    line: u32,
    character: u32,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Position {
    line: u32,
    character: u32,
}
impl From<Utf16Position> for Position {
    fn from(position: Utf16Position) -> Self {
        Self { line: position.line, character: position.character }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Range {
    start: Position,
    end: Position,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Location {
    path: String,
    range: Range,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DefinitionResult {
    generation: u64,
    status: String,
    #[serde(deserialize_with = "required_nullable")]
    refusal: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    location: Option<Location>,
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

pub(super) fn is_command(command: &str) -> bool {
    matches!(command, INSPECT_COMMAND | DEFINITION_COMMAND)
}
fn absolute_normal_path(path: &str) -> bool {
    !path.contains('\0')
        && Path::new(path).is_absolute()
        && !Path::new(path).components().any(|part| matches!(part, Component::CurDir | Component::ParentDir))
}
fn document_size(value: &impl Serialize) -> Result<(), Error> {
    let bytes = serde_json::to_vec(value).map_err(|e| Error::new("invalid_json", e.to_string()))?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        Err(Error::new("document_limit", "frozen manifest and overlays must each fit in 1 MiB"))
    } else {
        Ok(())
    }
}
impl Frozen {
    fn validate(&self) -> Result<(), Error> {
        if !absolute_normal_path(&self.manifest_path)
            || !absolute_normal_path(&self.base)
            || Path::new(&self.manifest_path).parent() != Some(Path::new(&self.base))
        {
            return Err(Error::new(
                "invalid_configuration",
                "frozen manifest path and original base must be absolute and consistent",
            ));
        }
        document_size(&self.manifest)?;
        let mut previous: Option<&str> = None;
        let mut budget = OutputBudget(0);
        budget.charge(self.sources.len())?;
        for source in &self.sources {
            source.compilation.validate()?;
            if !absolute_normal_path(&source.file) || previous.is_some_and(|path| path >= source.file.as_str()) {
                return Err(Error::new(
                    "invalid_configuration",
                    "frozen sources must have unique ordered absolute paths",
                ));
            }
            previous = Some(&source.file);
        }
        Ok(())
    }
}
impl InspectResult {
    pub(super) fn valid(&self) -> bool {
        self.frozen.validate().is_ok()
            && self.frozen.manifest.core(Path::new(&self.frozen.base)).is_ok()
            && self.sources == self.frozen.sources
    }
}
impl DefinitionResult {
    pub(super) fn valid(&self) -> bool {
        if self.generation > overlays::MAX_LABEL
            || ![
                "Resolved",
                "Unresolved",
                "Ambiguous",
                "OverloadNotResolved",
                "CallNotResolved",
                "ReceiverNotBound",
                "StaticImportNotApplied",
                "Package",
                "ConstructorRefNotBound",
                "Cycle",
                "SignatureNotCompared",
                "NotDetermined",
            ]
            .contains(&self.status.as_str())
        {
            return false;
        }
        if let Some(location) = &self.location {
            self.status == "Resolved"
                && self.refusal.is_none()
                && absolute_normal_path(&location.path)
                && (location.range.start.line, location.range.start.character)
                    < (location.range.end.line, location.range.end.character)
        } else {
            self.refusal.as_deref().is_some_and(|reason| {
                [
                    "eof",
                    "binary_source_unavailable",
                    "package_source_unavailable",
                    "not_unique_source_target",
                    "inexact_source_range",
                ]
                .contains(&reason)
            })
        }
    }
    fn refused(generation: u64, status: ResolveStatus, reason: &str) -> Self {
        Self { generation, status: status_name(status).to_owned(), refusal: Some(reason.to_owned()), location: None }
    }
}
fn unique_sources(workspace: &WorkspaceSnapshot) -> Result<BTreeMap<&str, Source<'_>>, Error> {
    let mut sources = BTreeMap::new();
    for id in workspace.compilation_ids() {
        let inputs = workspace.scope(id).expect("known compilation").inputs();
        for path in inputs.source_paths() {
            if sources
                .insert(path.as_str(), Source { id, file: inputs.project().file(path).expect("loaded source") })
                .is_some()
            {
                return Err(Error::new("invalid_configuration", "editor source must have exactly one owner").at(path));
            }
        }
    }
    Ok(sources)
}
fn inventory(sources: &BTreeMap<&str, Source<'_>>) -> Vec<SourceOwner> {
    sources
        .iter()
        .map(|(path, source)| SourceOwner { compilation: Id::from(source.id), file: (*path).to_owned() })
        .collect()
}
fn load(frozen: &Frozen) -> Result<WorkspaceSnapshot, Error> {
    let config = frozen.manifest.core(Path::new(&frozen.base))?;
    WorkspaceSnapshot::load(&config).map_err(|e| {
        let mut error = Error::new("load_failed", e.to_string());
        error.path = Some(e.input.map_or_else(|| frozen.manifest_path.clone(), |input| input.origin));
        error
    })
}
fn inspect(path: &Path) -> Result<(String, Value), Error> {
    let (manifest, path): (Manifest, _) = read_document(path)?;
    document_size(&manifest)?;
    let base = path.parent().expect("canonical manifest parent");
    let mut frozen = Frozen {
        manifest_path: path.to_str().expect("checked UTF-8").to_owned(),
        base: base.to_str().expect("checked UTF-8").to_owned(),
        manifest,
        sources: Vec::new(),
    };
    let workspace = load(&frozen)?;
    let sources = inventory(&unique_sources(&workspace)?);
    let mut budget = OutputBudget(0);
    budget.charge(frozen.manifest.compilations.len())?;
    budget.charge(sources.len())?;
    budget.charge(sources.len())?; // The inventory is also carried inside the frozen job input.
    frozen.sources = sources.clone();
    let manifest_path = frozen.manifest_path.clone();
    Ok((manifest_path, json!(InspectResult { frozen, sources })))
}
fn definition(job: DefinitionJob) -> Result<(String, Value), Error> {
    super::version(job.schema_version)?;
    job.frozen.validate()?;
    document_size(&job.overlays)?;
    job.overlays.validate()?;
    if job.generation > overlays::MAX_LABEL || job.generation != job.overlays.revision {
        return Err(Error::new(
            "invalid_overlay",
            "captured generation must equal overlay revision within the safe unsigned integer range",
        ));
    }
    job.query.compilation.validate()?;
    let base = Path::new(&job.frozen.base);
    let requested = config::local_path(base, &job.query.file)?;
    let config = job.frozen.manifest.core(base)?;
    job.overlays.validate_compilations(&config)?;
    if !config.compilations.iter().any(|c| c.id == job.query.compilation.core()) {
        return Err(Error::new("unknown_compilation", "query owner is not configured"));
    }
    let workspace = load(&job.frozen)?;
    if inventory(&unique_sources(&workspace)?) != job.frozen.sources {
        return Err(Error::new("invalid_configuration", "loaded source inventory changed; restart the editor session"));
    }
    let mut budget = OutputBudget(0);
    let (workspace, overlay_state) = job.overlays.apply(&workspace, base, &mut budget)?;
    let canonical = requested.canonicalize().map_err(|e| Error::new("io", e.to_string()).at(&requested))?;
    let path =
        canonical.to_str().ok_or_else(|| Error::new("invalid_path", "query path must be UTF-8").at(&requested))?;
    let sources = unique_sources(&workspace)?;
    let source = sources
        .get(path)
        .ok_or_else(|| Error::new("invalid_overlay", "query source must already be loaded").at(path))?;
    let owner = job.query.compilation.core();
    if *source.id != owner {
        return Err(Error::new("invalid_overlay", "query compilation must be the exact source owner").at(path));
    }
    // apply() validated all source aliases and owners against this loaded snapshot.
    if !overlay_state["files"].as_array().expect("overlay state array").iter().any(|file| file["file"] == path) {
        return Err(Error::new("invalid_overlay", "query source must be among captured open overlays").at(path));
    }
    let index = LineIndex::new(source.file.text());
    let invalid_position =
        || Error::new("invalid_position", "query line is missing or character is inside a surrogate pair").at(path);
    let length = index.line_utf16_len(job.query.line).ok_or_else(invalid_position)?;
    let byte = index
        .position_to_byte(Utf16Position { line: job.query.line, character: job.query.character.min(length) })
        .ok_or_else(invalid_position)?;
    let result = if byte as usize == source.file.text().len() {
        DefinitionResult::refused(job.generation, ResolveStatus::Unresolved, "eof")
    } else {
        let prepared = workspace.prepare();
        let resolved = prepared.scope(&owner).expect("validated owner").resolve_at(path, byte);
        budget.charge(1 + resolved.targets.len())?;
        if resolved.status == ResolveStatus::Package {
            DefinitionResult::refused(job.generation, resolved.status, "package_source_unavailable")
        } else if resolved.status != ResolveStatus::Resolved || resolved.targets.len() != 1 {
            DefinitionResult::refused(job.generation, resolved.status, "not_unique_source_target")
        } else {
            let target = &resolved.targets[0];
            if let Some(source) = sources.get(target.path.as_str()) {
                let index = LineIndex::new(source.file.text());
                let exact = source.file.text().get(target.start_byte as usize..target.end_byte as usize)
                    == Some(target.name.as_str())
                    && target.start_byte < target.end_byte;
                match (exact, index.byte_to_position(target.start_byte), index.byte_to_position(target.end_byte)) {
                    (true, Some(start), Some(end)) => DefinitionResult {
                        generation: job.generation,
                        status: status_name(resolved.status).to_owned(),
                        refusal: None,
                        location: Some(Location {
                            path: target.path.clone(),
                            range: Range { start: start.into(), end: end.into() },
                        }),
                    },
                    _ => DefinitionResult::refused(job.generation, resolved.status, "inexact_source_range"),
                }
            } else {
                // Preserve the existing binary provenance check, without fabricating a source location.
                super::target_json(&workspace, &sources, target, resolved.status)?;
                DefinitionResult::refused(job.generation, resolved.status, "binary_source_unavailable")
            }
        }
    };
    Ok((job.frozen.manifest_path, json!(result)))
}
pub(super) fn run(args: &[String]) -> Result<Value, Error> {
    let command = args.first().expect("private command selected");
    let expected_flag = if command == INSPECT_COMMAND { "--manifest" } else { "--job" };
    if args.len() != 3 || args[1] != expected_flag {
        return Err(Error::new("usage", format!("private editor job expects {expected_flag} FILE")));
    }
    let (manifest, result) = if command == INSPECT_COMMAND {
        inspect(Path::new(&args[2]))?
    } else {
        let (job, _): (DefinitionJob, _) =
            read_document_bounded(Path::new(&args[2]), MAX_JOB_BYTES, "editor job exceeds 4 MiB")?;
        definition(job)?
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    Ok(
        json!({"schema_version":1,"command":command,"snapshot":{"id":format!("{}-{now}",std::process::id()),"identity":"invocation_local","manifest":manifest},"capabilities":capabilities(),"result":result}),
    )
}
