//! Experimental, one-shot native project CLI. No protocol or runtime integration
//! is added to the library. See docs/CLI.md for the versioned wire contract.
mod config;
mod execution;
mod overlays;

use config::{Id, Manifest, Position, Query, Requests};
use java_engine::{
    CompilationId, LineIndex, ParsedFile, ResolveStatus, Target, TargetKind, Utf16Position, WorkspaceSnapshot,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_DOCUMENT_BYTES: u64 = 1024 * 1024;
const MAX_QUERIES: usize = 1024;
const MAX_RESULT_ITEMS: usize = 10_000;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Serialize)]
struct Error {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_reaped: Option<bool>,
}
impl Error {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), path: None, query_index: None, worker_reaped: None }
    }
    fn at(mut self, path: impl AsRef<Path>) -> Self {
        self.path = Some(path.as_ref().to_string_lossy().into_owned());
        self
    }
    fn query(mut self, index: usize) -> Self {
        self.query_index = Some(index);
        self
    }
}
fn version(value: u32) -> Result<(), Error> {
    if value == 1 {
        Ok(())
    } else {
        Err(Error::new("unsupported_version", format!("schema_version {value} is not supported; expected 1")))
    }
}
fn read_document<T: DeserializeOwned>(path: &Path) -> Result<(T, PathBuf), Error> {
    let meta = fs::symlink_metadata(path).map_err(|e| Error::new("io", e.to_string()).at(path))?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(Error::new("invalid_path", "JSON document must be a regular file, not a symlink").at(path));
    }
    if meta.len() > MAX_DOCUMENT_BYTES {
        return Err(Error::new("document_limit", "JSON document exceeds 1 MiB").at(path));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(MAX_DOCUMENT_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| Error::new("io", e.to_string()).at(path))?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(Error::new("document_limit", "JSON document exceeds 1 MiB").at(path));
    }
    let parsed = serde_json::from_slice(&bytes).map_err(|e| Error::new("invalid_json", e.to_string()).at(path))?;
    let canonical = path.canonicalize().map_err(|e| Error::new("io", e.to_string()).at(path))?;
    if canonical.to_str().is_none() {
        return Err(Error::new("invalid_path", "document path must be UTF-8").at(path));
    }
    Ok((parsed, canonical))
}

struct Arguments {
    command: String,
    manifest: String,
    overlays: Option<overlays::Overlays>,
    queries: Option<Requests>,
    filter: Option<Id>,
    details: bool,
}
fn arguments(args: &[String]) -> Result<Option<Arguments>, Error> {
    if args == ["--help"] || args == ["--version"] {
        return Ok(None);
    }
    let Some(command) = args.first() else {
        return Err(Error::new("usage", "expected inspect, declaration, parser-report, --help or --version"));
    };
    if !["inspect", "declaration", "parser-report"].contains(&command.as_str()) {
        return Err(Error::new("usage", format!("unknown command {command}")));
    }
    let mut flags = BTreeMap::new();
    let mut details = false;
    let mut iter = args[1..].iter();
    while let Some(flag) = iter.next() {
        if flag == "--details" {
            if details {
                return Err(Error::new("usage", "duplicate option --details"));
            }
            details = true;
            continue;
        }
        if ![
            "--manifest",
            "--project",
            "--source-set",
            "--file",
            "--byte",
            "--utf16",
            "--requests",
            "--overlays",
            "--line",
            "--character",
        ]
        .contains(&flag.as_str())
        {
            return Err(Error::new("usage", format!("unknown option {flag}")));
        }
        let value = iter.next().ok_or_else(|| Error::new("usage", format!("missing value for {flag}")))?;
        if flags.insert(flag.as_str(), value.clone()).is_some() {
            return Err(Error::new("usage", format!("duplicate option {flag}")));
        }
    }
    if details && command != "inspect" {
        return Err(Error::new("usage", "--details is supported only by inspect"));
    }
    let manifest = flags.remove("--manifest").ok_or_else(|| Error::new("usage", "--manifest is required"))?;
    let overlays = flags
        .remove("--overlays")
        .map(|path| {
            let (document, _): (overlays::Overlays, _) = read_document(Path::new(&path))?;
            document.validate().map_err(|e| e.at(&path))?;
            Ok::<_, Error>(document)
        })
        .transpose()?;
    let project = flags.remove("--project");
    let source_set = flags.remove("--source-set");
    let filter = match (project, source_set) {
        (None, None) => None,
        (Some(project), Some(source_set)) => Some(Id { project, source_set }),
        _ => return Err(Error::new("usage", "--project and --source-set must be supplied together")),
    };
    if let Some(id) = &filter {
        id.validate()?;
    }
    let mut queries = None;
    let mut filter = filter;
    if command == "declaration" {
        if let Some(requests) = flags.remove("--requests") {
            if filter.is_some() || !flags.is_empty() {
                return Err(Error::new("usage", "--requests cannot be combined with single-query options"));
            }
            let (document, _): (Requests, _) = read_document(Path::new(&requests))?;
            document.validate().map_err(|e| e.at(&requests))?;
            queries = Some(document);
        } else {
            let compilation = filter
                .take()
                .ok_or_else(|| Error::new("usage", "declaration needs --project and --source-set or --requests"))?;
            let file = flags.remove("--file").ok_or_else(|| Error::new("usage", "--file is required"))?;
            let parse_position = |value: String| {
                value
                    .parse::<u32>()
                    .map_err(|_| Error::new("invalid_position", "offset must be a nonnegative u32 integer"))
            };
            let parse_line_position = |value: String| {
                value
                    .parse::<u32>()
                    .ok()
                    .filter(|_| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
                    .ok_or_else(|| {
                        Error::new("invalid_position", "line and character must be decimal digits in 0..4294967295")
                    })
            };
            let position = match (
                flags.remove("--byte"),
                flags.remove("--utf16"),
                flags.remove("--line"),
                flags.remove("--character"),
            ) {
                (Some(offset), None, None, None) => Position::Utf8 { offset: parse_position(offset)? },
                (None, Some(offset), None, None) => Position::Utf16 { offset: parse_position(offset)? },
                (None, None, Some(line), Some(character)) => {
                    Position::Utf16Line { line: parse_line_position(line)?, character: parse_line_position(character)? }
                }
                _ => {
                    return Err(Error::new(
                        "usage",
                        "supply exactly one of --byte, --utf16, or --line with --character",
                    ));
                }
            };
            queries = Some(Requests { schema_version: 1, queries: vec![Query { compilation, file, position }] });
        }
    } else if command == "inspect" && filter.is_some() {
        return Err(Error::new("usage", "inspect does not accept a compilation filter"));
    }
    if !flags.is_empty() {
        return Err(Error::new("usage", "option is not supported for this command"));
    }
    if let Some(queries) = &queries {
        queries.validate()?;
    }
    Ok(Some(Arguments { command: command.clone(), manifest, overlays, queries, filter, details }))
}

/// Includes source files, class origins, shadowed records, visible-path references,
/// parser gaps and returned candidates. Count before expanding response objects.
struct OutputBudget(usize);
impl OutputBudget {
    fn charge(&mut self, count: usize) -> Result<(), Error> {
        self.0 = self.0.checked_add(count).filter(|n| *n <= MAX_RESULT_ITEMS).ok_or_else(|| {
            Error::new(
                "output_limit",
                "response exceeds 10000 result items; narrow the project, parser-report filter or query batch",
            )
        })?;
        Ok(())
    }
}
struct BoundedOutput(Vec<u8>);
impl Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_RESPONSE_BYTES - self.0.len() {
            return Err(io::Error::other("JSON response exceeds 16 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn encode(value: &Value) -> Result<Vec<u8>, Error> {
    let mut out = BoundedOutput(Vec::new());
    serde_json::to_writer(&mut out, value).map_err(|e| Error::new("output_limit", e.to_string()))?;
    out.write_all(b"\n").map_err(|e| Error::new("output_limit", e.to_string()))?;
    Ok(out.0)
}
fn capabilities() -> Value {
    json!({
        "experimental": true,
        "declaration_navigation": "limited_name_binding",
        "method_argument_applicability": false,
        "limited_call_selection": {
            "source_primitive_strict_phase": true,
            "primitive_source_call_returns": true,
            "single_source_contradiction_guard": true,
            "general_applicability": false
        },
        "semantic_diagnostics": false,
        "parser_reports": "parser_limit_or_invalid_syntax",
        "runtime": "native_rust_no_jvm",
        "build_tool_import": false,
        "implicit_platform": false,
        "persistent_server_reload_overlays": false,
        "process_timeout_isolation": true,
        "execution": execution::capabilities(),
        "persistent_symbol_identity": false,
        "limits": {"document_bytes":MAX_DOCUMENT_BYTES,"batch_queries":MAX_QUERIES,
            "result_items":MAX_RESULT_ITEMS,"response_bytes":MAX_RESPONSE_BYTES},
        "resource_caveat": "Worker wall-clock timeout and crash isolation are not CPU quotas, memory limits or a security sandbox. Use trusted inputs; consume output concurrently."
    })
}
fn help() -> Value {
    json!({"schema_version":1,"version":env!("CARGO_PKG_VERSION"),"capabilities":capabilities(),"usage":[
        "java-analyzer inspect --manifest FILE [--details] [--overlays FILE]",
        "java-analyzer declaration --manifest FILE --project ID --source-set ID --file PATH (--byte N|--utf16 N|--line N --character N) [--overlays FILE]",
        "java-analyzer declaration --manifest FILE --requests FILE [--overlays FILE]",
        "java-analyzer parser-report --manifest FILE [--project ID --source-set ID] [--overlays FILE]"
    ],"execution_options":["--timeout-ms 1..300000 (default 30000)","--cancel-on-stdin-close"],"documentation":"docs/CLI.md"})
}

struct Source<'a> {
    id: &'a CompilationId,
    file: &'a ParsedFile,
}
fn sources(workspace: &WorkspaceSnapshot) -> BTreeMap<&str, Source<'_>> {
    let mut out = BTreeMap::new();
    for id in workspace.compilation_ids() {
        let input = workspace.scope(id).expect("known compilation").inputs();
        for path in input.source_paths() {
            out.insert(path.as_str(), Source { id, file: input.project().file(path).expect("loaded source") });
        }
    }
    out
}
fn inspect(
    workspace: &WorkspaceSnapshot,
    manifest: &Manifest,
    budget: &mut OutputBudget,
    details: bool,
) -> Result<Value, Error> {
    let mut compilations = Vec::new();
    for id in workspace.compilation_ids() {
        let scope = workspace.scope(id).expect("known compilation");
        let input = scope.inputs();
        let visible = scope.source_paths();
        budget.charge(
            1 + input.source_paths().len()
                + visible.len()
                + if details { input.class_origins().len() } else { 0 }
                + input.shadowed_classes().len(),
        )?;
        let configured = manifest.compilations.iter().find(|c| c.id.core() == *id).expect("validated configuration");
        let shadowed: Vec<_> = input
            .shadowed_classes()
            .iter()
            .map(|c| {
                json!({
                    "binary_name":c.binary_name,"selected_origin":c.selected_origin,"shadowed_origin":c.shadowed_origin
                })
            })
            .collect();
        compilations.push(json!({"id":Id::from(id),"configuration":configured,
            "source_paths":input.source_paths(),"visible_source_paths":visible,
            "class_count":input.class_origins().len(),"shadowed_classes":shadowed}));
        if details {
            compilations.last_mut().expect("just inserted")["class_origins"] = json!(input.class_origins());
        }
    }
    Ok(
        json!({"detail_mode":if details { "full" } else { "summary" },"workspace_limits":manifest.limits,"compilations":compilations}),
    )
}
fn range(file: &ParsedFile, start: u32, end: u32) -> Result<Value, Error> {
    let start_utf16 = file
        .byte_to_utf16(start)
        .ok_or_else(|| Error::new("invalid_target", "target start is not a UTF-8 boundary"))?;
    let end_utf16 =
        file.byte_to_utf16(end).ok_or_else(|| Error::new("invalid_target", "target end is not a UTF-8 boundary"))?;
    Ok(json!({"start_byte":start,"end_byte":end,"start_utf16":start_utf16,"end_utf16":end_utf16}))
}
fn kind_name(kind: TargetKind) -> &'static str {
    match kind {
        TargetKind::Class => "Class",
        TargetKind::Field => "Field",
        TargetKind::Method => "Method",
        TargetKind::Local => "Local",
        TargetKind::Parameter => "Parameter",
        TargetKind::Resource => "Resource",
    }
}
fn status_name(status: ResolveStatus) -> &'static str {
    match status {
        ResolveStatus::Resolved => "Resolved",
        ResolveStatus::Unresolved => "Unresolved",
        ResolveStatus::Ambiguous => "Ambiguous",
        ResolveStatus::OverloadNotResolved => "OverloadNotResolved",
        ResolveStatus::CallNotResolved => "CallNotResolved",
        ResolveStatus::ReceiverNotBound => "ReceiverNotBound",
        ResolveStatus::StaticImportNotApplied => "StaticImportNotApplied",
        ResolveStatus::Package => "Package",
        ResolveStatus::ConstructorRefNotBound => "ConstructorRefNotBound",
        ResolveStatus::Cycle => "Cycle",
        ResolveStatus::SignatureNotCompared => "SignatureNotCompared",
        ResolveStatus::NotDetermined => "NotDetermined",
    }
}
fn target_json(
    workspace: &WorkspaceSnapshot,
    sources: &BTreeMap<&str, Source<'_>>,
    target: &Target,
    status: ResolveStatus,
) -> Result<Value, Error> {
    let common = json!({"kind":kind_name(target.kind),"name":target.name,"owner":target.owner});
    let mut out = common.as_object().expect("object").clone();
    if status == ResolveStatus::Package && target.path.is_empty() {
        out.insert("origin".into(), json!("package"));
        out.insert("source_available".into(), json!(false));
    } else if let Some(source) = sources.get(target.path.as_str()) {
        if source.file.text().get(target.start_byte as usize..target.end_byte as usize) != Some(target.name.as_str()) {
            return Err(Error::new("invalid_target", "source target does not cover its exact declaration name")
                .at(&target.path));
        }
        out.extend([
            ("origin".into(), json!("source")),
            ("source_available".into(), json!(true)),
            ("path".into(), json!(target.path)),
            ("compilation".into(), json!(Id::from(source.id))),
            ("range".into(), range(source.file, target.start_byte, target.end_byte)?),
        ]);
    } else {
        // Public Target omits binary linking context. These are exact locations
        // where this origin was loaded, NOT guessed contexts used by resolution.
        let mut loaded_contexts = Vec::new();
        let mut binary_name = None;
        for id in workspace.compilation_ids() {
            if let Some(signature) =
                workspace.scope(id).expect("known compilation").inputs().project().class_signature(&target.path)
            {
                loaded_contexts.push(Id::from(id));
                binary_name = Some(signature.binary_name.clone());
            }
        }
        let binary_name = binary_name.ok_or_else(|| {
            Error::new("invalid_target", "target has no loaded source or binary origin").at(&target.path)
        })?;
        out.extend([
            ("origin".into(), json!("binary")),
            ("source_available".into(), json!(false)),
            ("path".into(), json!(target.path)),
            ("binary_name".into(), json!(binary_name)),
            ("loaded_contexts".into(), json!(loaded_contexts)),
            ("linking_context".into(), Value::Null),
            ("identity_precision".into(), json!("origin_kind_owner_name_only")),
        ]);
    }
    Ok(Value::Object(out))
}
fn declarations(
    workspace: &WorkspaceSnapshot,
    base: &Path,
    requests: &Requests,
    budget: &mut OutputBudget,
) -> Result<Value, Error> {
    let sources = sources(workspace);
    // Check every path/position before preparing or returning any query result.
    let mut validated = Vec::new();
    for (index, query) in requests.queries.iter().enumerate() {
        let validate = || -> Result<_, Error> {
            let id = query.compilation.core();
            let scope = workspace
                .scope(&id)
                .ok_or_else(|| Error::new("unknown_compilation", "query compilation is not configured"))?;
            let requested = config::local_path(base, &query.file)?;
            // Query aliases (including symlinks) are allowed only if they resolve
            // to a source already loaded by the strict source-input loader.
            let canonical = requested.canonicalize().map_err(|e| Error::new("io", e.to_string()).at(&requested))?;
            let path = canonical
                .to_str()
                .ok_or_else(|| Error::new("invalid_path", "source path must be UTF-8").at(&requested))?;
            if scope.source_paths().binary_search(&path).is_err() {
                return Err(Error::new("invisible_source", "source is not visible in this compilation").at(path));
            }
            let source = sources.get(path).expect("visible source is loaded");
            let byte = match query.position {
                Position::Utf8 { offset } => source.file.byte_to_utf16(offset).map(|_| offset),
                Position::Utf16 { offset } => source.file.utf16_to_byte(offset),
                Position::Utf16Line { line, character } => {
                    LineIndex::new(source.file.text()).position_to_byte(Utf16Position { line, character })
                }
            }
            .filter(|byte| (*byte as usize) < source.file.text().len())
            .ok_or_else(|| {
                Error::new(
                    "invalid_position",
                    "position is outside source, at EOF, inside UTF-8 bytes or inside a UTF-16 surrogate pair",
                )
                .at(path)
            })?;
            Ok((id, path.to_owned(), byte, source.id))
        };
        validated.push(validate().map_err(|e| e.query(index))?);
    }
    let prepared = workspace.prepare();
    let mut queries = Vec::new();
    budget.charge(requests.queries.len())?;
    for (index, (query, (id, path, byte, owner))) in requests.queries.iter().zip(validated).enumerate() {
        let result = prepared.scope(&id).expect("validated scope").resolve_at(&path, byte);
        budget.charge(result.targets.len()).map_err(|e| e.query(index))?;
        let targets = result
            .targets
            .iter()
            .map(|target| target_json(workspace, &sources, target, result.status))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.query(index))?;
        queries.push(json!({"query_index":index,"compilation":query.compilation,"source_compilation":Id::from(owner),
            "file":path,"position":query.position,"byte_offset":byte,"status":status_name(result.status),"targets":targets}));
    }
    Ok(json!({"index_builds":1,"queries":queries}))
}
fn parser_report(
    workspace: &WorkspaceSnapshot,
    filter: Option<&Id>,
    budget: &mut OutputBudget,
) -> Result<Value, Error> {
    let ids = if let Some(filter) = filter {
        let id = filter.core();
        if workspace.scope(&id).is_none() {
            return Err(Error::new("unknown_compilation", "parser-report compilation is not configured"));
        }
        vec![id]
    } else {
        workspace.compilation_ids().to_vec()
    };
    let (mut source_count, mut files_with_reports, mut report_count) = (0, 0, 0);
    let mut files = Vec::new();
    for id in &ids {
        let inputs = workspace.scope(id).expect("known compilation").inputs();
        for path in inputs.source_paths() {
            budget.charge(1)?;
            let file = inputs.project().file(path).expect("loaded source");
            let errors = file.errors();
            budget.charge(errors.len()).map_err(|e| e.at(path))?;
            source_count += 1;
            if !errors.is_empty() {
                files_with_reports += 1;
            }
            report_count += errors.len();
            let reports: Vec<_> = errors
                .into_iter()
                .map(|e| {
                    json!({"classification":"parser_limit_or_invalid_syntax",
                "message":e.message,"range":{"start_byte":e.start_byte,"end_byte":e.end_byte,
                    "start_utf16":e.start_utf16,"end_utf16":e.end_utf16}})
                })
                .collect();
            files.push(json!({"path":path,"compilation":Id::from(id),"reports":reports}));
        }
    }
    Ok(json!({"classification":"parser_limit_or_invalid_syntax","java_semantic_diagnostics":false,
        "scope":"owned_sources_only","source_count":source_count,"files_with_reports":files_with_reports,
        "report_count":report_count,"files":files}))
}
fn run(raw_args: &[String]) -> Result<Value, Error> {
    let Some(args) = arguments(raw_args)? else {
        return Ok(help());
    };
    let (manifest, path): (Manifest, _) = read_document(Path::new(&args.manifest))?;
    let base = path.parent().expect("absolute manifest parent");
    let config = manifest.core(base).map_err(|e| if e.path.is_some() { e } else { e.at(&path) })?;
    // Cheap request validation, including IDs, precedes source/archive loading.
    if let Some(requests) = &args.queries {
        for (index, query) in requests.queries.iter().enumerate() {
            if !config.compilations.iter().any(|c| c.id == query.compilation.core()) {
                return Err(Error::new("unknown_compilation", "query compilation is not configured").query(index));
            }
        }
    }
    if let Some(filter) = &args.filter
        && !config.compilations.iter().any(|c| c.id == filter.core())
    {
        return Err(Error::new("unknown_compilation", "parser-report compilation is not configured"));
    }
    if let Some(overlays) = &args.overlays {
        overlays.validate_compilations(&config)?;
    }
    let workspace = WorkspaceSnapshot::load(&config).map_err(|e| {
        let mut error = Error::new("load_failed", e.to_string());
        error.path = Some(e.input.map_or_else(|| path.to_string_lossy().into_owned(), |input| input.origin));
        error
    })?;
    let mut budget = OutputBudget(0);
    let (workspace, overlay_state) = if let Some(overlays) = &args.overlays {
        let (updated, state) = overlays.apply(&workspace, base, &mut budget)?;
        (updated, Some(state))
    } else {
        (workspace, None)
    };
    let mut result = match args.command.as_str() {
        "inspect" => inspect(&workspace, &manifest, &mut budget, args.details)?,
        "declaration" => {
            declarations(&workspace, base, args.queries.as_ref().expect("validated queries"), &mut budget)?
        }
        "parser-report" => parser_report(&workspace, args.filter.as_ref(), &mut budget)?,
        _ => unreachable!("validated command"),
    };
    if let Some(state) = overlay_state {
        result["overlay_state"] = state;
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    Ok(json!({"schema_version":1,"command":args.command,"snapshot":{
        "id":format!("{}-{now}",std::process::id()),"identity":"invocation_local","manifest":path.to_str()},
        "capabilities":capabilities(),"result":result}))
}
fn main() {
    let args = std::env::args_os()
        .skip(1)
        .map(|arg| arg.into_string().map_err(|_| Error::new("usage", "arguments must be UTF-8")))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|error| emit_error(error));
    if args.first().is_some_and(|arg| arg == execution::WORKER_ARGUMENT) {
        execution::watch_parent().unwrap_or_else(|error| emit_error(error));
        let bytes = run(&args[1..]).and_then(|value| encode(&value)).unwrap_or_else(|error| emit_error(error));
        if let Err(error) = io::stdout().lock().write_all(&bytes) {
            emit_error(Error::new("output_io", error.to_string()));
        }
        return;
    }
    match execution::run(args) {
        Ok(response) => {
            let result = if response.success {
                io::stdout().lock().write_all(&response.bytes)
            } else {
                io::stderr().lock().write_all(&response.bytes)
            };
            if let Err(error) = result {
                emit_error(Error::new("output_io", error.to_string()));
            }
            if !response.success {
                std::process::exit(2);
            }
        }
        Err(error) => emit_error(error),
    }
}
fn emit_error(error: Error) -> ! {
    let mut stderr = io::stderr().lock();
    let _ = serde_json::to_writer(&mut stderr, &json!({"schema_version":1,"error":error}));
    let _ = stderr.write_all(b"\n");
    std::process::exit(2)
}
