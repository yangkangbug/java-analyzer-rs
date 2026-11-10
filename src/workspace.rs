//! Explicit compilation graphs. Edges export sources transitively, never binaries.
//! This is not a build-tool importer or JPMS graph.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::{ProjectConfig, ProjectLoadError, ProjectSnapshot, ResolveResult};

/// Caller-assigned identity. Both components must be nonempty and whitespace-free.
/// No main/test, generated-source, or project relation is inferred from these labels.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CompilationId {
    pub project: String,
    pub source_set: String,
}
impl CompilationId {
    pub fn new(project: impl Into<String>, source_set: impl Into<String>) -> Self {
        Self { project: project.into(), source_set: source_set.into() }
    }
}

/// Local inputs and outgoing source-visibility edges for one compilation.
#[derive(Clone, Debug)]
pub struct CompilationConfig {
    pub id: CompilationId,
    /// Binaries/platform belong exclusively to this compilation's linking context.
    pub inputs: ProjectConfig,
    /// Every reachable compilation's sources are visible. No binary input is exported.
    pub dependencies: Vec<CompilationId>,
}

/// Graph and aggregate loading budgets. Repeated physical inputs are charged on
/// every occurrence, even when equal archive bytes reuse parsed signatures within
/// one load. Every occurrence still reads and checks the archive's current bytes.
#[derive(Clone, Debug)]
pub struct WorkspaceConfig {
    pub compilations: Vec<CompilationConfig>,
    pub max_compilations: usize,
    pub max_dependency_edges: usize,
    pub max_entries: usize,
    pub max_input_bytes: u64,
    pub max_archive_expanded_bytes: u64,
}
impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            compilations: Vec::new(),
            max_compilations: 128,
            max_dependency_edges: 1024,
            max_entries: 200_000,
            max_input_bytes: 512 * 1024 * 1024,
            max_archive_expanded_bytes: 1024 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceLoadErrorKind {
    InvalidId,
    DuplicateCompilation,
    UnknownDependency,
    DuplicateDependency,
    Cycle,
    DuplicateSource,
    LimitExceeded,
    Input,
}
#[derive(Clone, Debug)]
pub struct WorkspaceLoadError {
    pub kind: WorkspaceLoadErrorKind,
    pub detail: String,
    /// The underlying filesystem/archive error for an input failure.
    pub input: Option<ProjectLoadError>,
}
impl fmt::Display for WorkspaceLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.detail)
    }
}
impl std::error::Error for WorkspaceLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.input.as_ref().map(|error| error as &dyn std::error::Error)
    }
}
fn error(kind: WorkspaceLoadErrorKind, detail: impl Into<String>) -> WorkspaceLoadError {
    WorkspaceLoadError { kind, detail: detail.into(), input: None }
}

/// A whole-buffer replacement for an existing, exactly identified source file.
///
/// `path` must be its loaded canonical source path, not an alias or a new file.
/// `owner` is its owning compilation, not a consumer that can see the source.
/// Revisions and open/close state belong to the caller, not this value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceOverlay {
    pub owner: CompilationId,
    pub path: String,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceOverlayErrorKind {
    UnknownCompilation,
    UnknownSource,
    OwnerMismatch,
    DuplicateSource,
    LimitExceeded,
}

/// Rejected overlays leave the source snapshot and every prepared view unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceOverlayError {
    pub kind: SourceOverlayErrorKind,
    /// Offending source when the failure can be assigned to one file.
    pub path: Option<String>,
    pub detail: String,
}
impl fmt::Display for SourceOverlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.path {
            write!(f, "{path}: ")?;
        }
        write!(f, "{:?}: {}", self.kind, self.detail)
    }
}
impl std::error::Error for SourceOverlayError {}

fn overlay_error(kind: SourceOverlayErrorKind, path: Option<&str>, detail: impl Into<String>) -> SourceOverlayError {
    SourceOverlayError { kind, path: path.map(str::to_owned), detail: detail.into() }
}

/// Owned, immutable compilation inputs and visibility closures. Fresh loads replace
/// edges/inputs; old snapshots retain their exact contexts. This is neither an
/// atomic filesystem snapshot nor incremental invalidation/caching.
#[derive(Clone, Debug)]
pub struct WorkspaceSnapshot {
    ids: Vec<CompilationId>,
    inputs: Vec<ProjectSnapshot>,
    visibility: Vec<BTreeSet<usize>>,
    max_input_bytes: u64,
}
impl WorkspaceSnapshot {
    /// Validate the complete graph before I/O, then load in lexicographic ID order.
    /// Duplicate IDs/edges, unknown targets, cycles (including self-edges), and
    /// canonical source paths owned by multiple compilations are rejected.
    /// Source declarations always bind in their owner's context, even when queried
    /// from a dependent view. Binary hierarchy links use their own compilation's
    /// selected inputs; different versions can coexist in distinct contexts.
    ///
    /// Within this call, equal archive bytes at a checked canonical path may reuse
    /// parsed signatures for the same format and exact JAR target release, provided
    /// all current limits pass. Every occurrence still captures fresh bytes and is
    /// charged to every budget. A bounded private FIFO retains at most 16 archives
    /// and 64 MiB of raw bytes; eviction or oversized inputs only cause more decoding.
    /// Signatures remain immutable and shared, but each scope builds its own binding
    /// records. Nothing is retained for the next load. This is not an atomic view of
    /// concurrently modified files or a persistent/incremental query cache.
    pub fn load(config: &WorkspaceConfig) -> Result<Self, WorkspaceLoadError> {
        use WorkspaceLoadErrorKind as K;
        if config.compilations.len() > config.max_compilations {
            return Err(error(K::LimitExceeded, "too many compilations"));
        }
        let mut ordered = BTreeMap::new();
        let mut edge_count = 0usize;
        for compilation in &config.compilations {
            if [&compilation.id.project, &compilation.id.source_set]
                .iter()
                .any(|part| part.is_empty() || part.chars().any(char::is_whitespace))
            {
                return Err(error(K::InvalidId, "compilation ID components must be nonempty and whitespace-free"));
            }
            if ordered.insert(compilation.id.clone(), compilation).is_some() {
                return Err(error(K::DuplicateCompilation, format!("duplicate compilation {:?}", compilation.id)));
            }
            edge_count = edge_count
                .checked_add(compilation.dependencies.len())
                .filter(|count| *count <= config.max_dependency_edges)
                .ok_or_else(|| error(K::LimitExceeded, "too many dependency edges"))?;
        }
        let ids: Vec<_> = ordered.keys().cloned().collect();
        let configs: Vec<_> = ordered.values().copied().collect();
        let mut visibility = Vec::new();
        for (index, compilation) in configs.iter().enumerate() {
            let mut edges = BTreeSet::new();
            for target in &compilation.dependencies {
                let target_index = ids
                    .binary_search(target)
                    .map_err(|_| error(K::UnknownDependency, format!("unknown dependency {target:?}")))?;
                if !edges.insert(target_index) {
                    return Err(error(K::DuplicateDependency, format!("duplicate dependency {target:?}")));
                }
            }
            if edges.contains(&index) {
                return Err(error(K::Cycle, "self dependency"));
            }
            visibility.push(edges);
        }
        // Iterative transitive closure avoids recursive graph traversal on caller-raised limits.
        for via in 0..ids.len() {
            let reachable = visibility[via].clone();
            for edges in &mut visibility {
                if edges.contains(&via) {
                    edges.extend(&reachable);
                }
            }
        }
        for (index, edges) in visibility.iter_mut().enumerate() {
            if edges.contains(&index) {
                return Err(error(K::Cycle, "dependency cycle"));
            }
            edges.insert(index);
        }
        // Validate every selection before any compilation can touch its inputs.
        for compilation in &configs {
            compilation.inputs.validate_source_exclusions().map_err(|input| WorkspaceLoadError {
                kind: K::Input,
                detail: format!("{:?}: {input}", compilation.id),
                input: Some(input),
            })?;
        }
        let mut inputs = Vec::new();
        let mut archives = crate::jar::ArchiveReads::default();
        let mut owners = BTreeSet::new();
        let (mut entries, mut bytes, mut expanded) = (0, 0, 0);
        for compilation in configs {
            let mut bounded = compilation.inputs.clone();
            bounded.limits.max_entries = bounded.limits.max_entries.min(config.max_entries - entries);
            bounded.limits.max_input_bytes = bounded.limits.max_input_bytes.min(config.max_input_bytes - bytes);
            bounded.limits.max_archive_expanded_bytes =
                bounded.limits.max_archive_expanded_bytes.min(config.max_archive_expanded_bytes - expanded);
            let mut loaded = ProjectSnapshot::load_with_archives(&bounded, Some(&mut archives)).map_err(|input| {
                WorkspaceLoadError {
                    kind: K::Input,
                    detail: format!("{:?}: {input}", compilation.id),
                    input: Some(input),
                }
            })?;
            // The remaining workspace allowance constrains this load, but is not
            // the compilation's limit for a later whole-batch source replacement.
            loaded.limits = compilation.inputs.limits.clone();
            entries += loaded.usage.0;
            bytes += loaded.usage.1;
            expanded += loaded.usage.2;
            for path in loaded.source_paths() {
                if !owners.insert(path.clone()) {
                    return Err(error(K::DuplicateSource, format!("source owned by multiple compilations: {path}")));
                }
            }
            inputs.push(loaded);
        }
        Ok(Self { ids, inputs, visibility, max_input_bytes: config.max_input_bytes })
    }

    /// Derive an immutable snapshot by replacing complete, already loaded sources.
    ///
    /// All identities, owners and final source/input byte budgets are validated
    /// before cloning or parsing. The complete batch is atomic: an error returns
    /// no new snapshot. Omitted files retain this snapshot's text, so an empty
    /// batch is a clone, not a close/reset operation. To discard overlays, derive
    /// again from the retained disk-loaded base, or explicitly load a fresh base.
    ///
    /// No filesystem reads, path canonicalization, archive decoding or Java
    /// execution occur. Sources cannot be added/removed and the compilation graph
    /// and binary contexts cannot be changed. Immutable parsed binary signatures
    /// are shared, not reread or deeply cloned. Other snapshot/index allocations
    /// remain; this is not zero-copy storage or an incremental semantic index.
    /// A new prepared view still indexes the derived snapshot independently. Old
    /// snapshots and their prepared views remain valid. Revisions, cancellation
    /// and stale-result rejection remain the caller's responsibility.
    pub fn with_source_overlays(&self, overlays: &[SourceOverlay]) -> Result<Self, SourceOverlayError> {
        use SourceOverlayErrorKind as K;
        let mut seen = BTreeSet::new();
        let mut replacements = Vec::with_capacity(overlays.len());
        for overlay in overlays {
            let owner = self.ids.binary_search(&overlay.owner).map_err(|_| {
                overlay_error(K::UnknownCompilation, Some(&overlay.path), "overlay owner is not configured")
            })?;
            let actual_owner = self
                .inputs
                .iter()
                .position(|input| input.project().file(&overlay.path).is_some())
                .ok_or_else(|| {
                    overlay_error(K::UnknownSource, Some(&overlay.path), "overlay path is not a loaded source identity")
                })?;
            if owner != actual_owner {
                return Err(overlay_error(
                    K::OwnerMismatch,
                    Some(&overlay.path),
                    "overlay compilation does not own this source",
                ));
            }
            if !seen.insert(overlay.path.as_str()) {
                return Err(overlay_error(K::DuplicateSource, Some(&overlay.path), "duplicate overlay source"));
            }
            let new_bytes = u64::try_from(overlay.text.len()).map_err(|_| {
                overlay_error(K::LimitExceeded, Some(&overlay.path), "source byte length is not representable")
            })?;
            if new_bytes > u64::from(u32::MAX) || new_bytes > self.inputs[owner].limits.max_file_bytes {
                return Err(overlay_error(
                    K::LimitExceeded,
                    Some(&overlay.path),
                    "replacement exceeds the source file byte limit or u32 source range",
                ));
            }
            let old_bytes = self.inputs[owner].project().file(&overlay.path).expect("validated source").text().len();
            replacements.push((owner, old_bytes as u64, new_bytes, overlay));
        }

        let mut bytes: Vec<_> = self.inputs.iter().map(|input| input.usage.1).collect();
        // Subtract every old source first. A grow-before-shrink batch must have
        // the same final allowance as the reversed order, without transient caps.
        for (owner, old_bytes, _, overlay) in &replacements {
            bytes[*owner] = bytes[*owner].checked_sub(*old_bytes).ok_or_else(|| {
                overlay_error(K::LimitExceeded, Some(&overlay.path), "source byte accounting underflow")
            })?;
        }
        for (owner, _, new_bytes, overlay) in &replacements {
            bytes[*owner] = bytes[*owner].checked_add(*new_bytes).ok_or_else(|| {
                overlay_error(K::LimitExceeded, Some(&overlay.path), "source byte accounting overflow")
            })?;
        }
        for (owner, &total) in bytes.iter().enumerate() {
            if total > self.inputs[owner].limits.max_input_bytes {
                return Err(overlay_error(
                    K::LimitExceeded,
                    None,
                    format!("replacement inputs exceed compilation {:?} byte limit", self.ids[owner]),
                ));
            }
        }
        let total = bytes.iter().try_fold(0_u64, |sum, value| sum.checked_add(*value));
        if total.is_none_or(|total| total > self.max_input_bytes) {
            return Err(overlay_error(K::LimitExceeded, None, "replacement inputs exceed workspace byte limit"));
        }

        let mut next = self.clone();
        for (owner, _, _, overlay) in replacements {
            next.inputs[owner].replace_source_text(&overlay.path, &overlay.text);
        }
        for (input, total) in next.inputs.iter_mut().zip(bytes) {
            input.usage.1 = total;
        }
        Ok(next)
    }
    /// Build one reusable index shared by all prepared compilation views.
    /// This borrows this exact immutable snapshot; reloads require preparing the
    /// new snapshot separately and do not change existing prepared views.
    pub fn prepare(&self) -> PreparedWorkspace<'_> {
        let projects = self.inputs.iter().map(ProjectSnapshot::project).collect();
        PreparedWorkspace { workspace: self, index: crate::bind::PreparedScopedIndex::new(projects, &self.visibility) }
    }

    /// IDs are returned in lexicographic project/source-set order.
    pub fn compilation_ids(&self) -> &[CompilationId] {
        &self.ids
    }
    pub fn scope(&self, id: &CompilationId) -> Option<CompilationScope<'_>> {
        self.ids.binary_search(id).ok().map(|index| CompilationScope { workspace: self, index })
    }
}

/// Read-only view of a compilation. Borrowing the snapshot prevents mutation.
#[derive(Clone, Copy, Debug)]
pub struct CompilationScope<'a> {
    workspace: &'a WorkspaceSnapshot,
    index: usize,
}
impl<'a> CompilationScope<'a> {
    pub fn id(&self) -> &'a CompilationId {
        &self.workspace.ids[self.index]
    }
    /// Only this compilation's loaded inputs, origins, and shadowed binaries.
    /// `inputs().resolve_at` is the legacy flat resolver, not this graph view.
    /// Use this view's `resolve_at` for dependency-aware analysis.
    pub fn inputs(&self) -> &'a ProjectSnapshot {
        &self.workspace.inputs[self.index]
    }
    /// Visible source paths in canonical lexicographic order, including dependencies.
    pub fn source_paths(&self) -> Vec<&'a str> {
        let mut paths: Vec<_> = self.workspace.visibility[self.index]
            .iter()
            .flat_map(|index| self.workspace.inputs[*index].source_paths().iter().map(String::as_str))
            .collect();
        paths.sort_unstable();
        paths
    }
    /// Resolve visible source using its owner's context. An invisible/missing
    /// path returns Unresolved, even if another workspace scope owns it.
    /// Indexes are rebuilt on each query; no performance guarantee is implied.
    pub fn resolve_at(&self, path: &str, byte: u32) -> ResolveResult {
        let projects: Vec<_> = self.workspace.inputs.iter().map(ProjectSnapshot::project).collect();
        crate::bind::resolve_scoped(&projects, &self.workspace.visibility, self.index, path, byte)
    }
}

/// A workspace-wide owned binding index borrowing immutable snapshot inputs.
///
/// Preparing indexes all compilations once, including their separate binary
/// linking contexts. Creating scope views and querying them reuses that index.
/// This is not incremental invalidation, result caching, or persistent identity.
pub struct PreparedWorkspace<'a> {
    workspace: &'a WorkspaceSnapshot,
    index: crate::bind::PreparedScopedIndex<'a>,
}
impl PreparedWorkspace<'_> {
    /// Borrow a view of the retained index; unknown compilation IDs return None.
    pub fn scope(&self, id: &CompilationId) -> Option<PreparedCompilationScope<'_>> {
        self.workspace.ids.binary_search(id).ok().map(|index| PreparedCompilationScope {
            scope: CompilationScope { workspace: self.workspace, index },
            index: &self.index,
        })
    }
}

/// Lightweight compilation view borrowing a prepared workspace's shared index.
#[derive(Clone, Copy)]
pub struct PreparedCompilationScope<'a> {
    scope: CompilationScope<'a>,
    index: &'a crate::bind::PreparedScopedIndex<'a>,
}
impl<'a> PreparedCompilationScope<'a> {
    pub fn id(&self) -> &'a CompilationId {
        self.scope.id()
    }
    /// This compilation's own inputs, without dependency sources.
    /// `inputs().resolve_at` is the flat, rebuilding resolver, not this prepared
    /// graph view. Use this view's `resolve_at` for retained contextual analysis.
    pub fn inputs(&self) -> &'a ProjectSnapshot {
        self.scope.inputs()
    }
    /// Visible source paths in canonical lexicographic order.
    pub fn source_paths(&self) -> Vec<&'a str> {
        self.scope.source_paths()
    }
    /// Resolve visible source in its declaration owner's context without rebuilding.
    /// Invisible/missing paths return Unresolved, just as in [`CompilationScope`].
    pub fn resolve_at(&self, path: &str, byte: u32) -> ResolveResult {
        self.index.resolve_at(self.scope.index, path, byte)
    }
}

#[cfg(test)]
mod archive_reuse_tests;
