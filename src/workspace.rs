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
/// every load: there is no cross-compilation parse/archive cache.
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

/// Owned, immutable compilation inputs and visibility closures. Fresh loads replace
/// edges/inputs; old snapshots retain their exact contexts. This is neither an
/// atomic filesystem snapshot nor incremental invalidation/caching.
#[derive(Clone, Debug)]
pub struct WorkspaceSnapshot {
    ids: Vec<CompilationId>,
    inputs: Vec<ProjectSnapshot>,
    visibility: Vec<BTreeSet<usize>>,
}
impl WorkspaceSnapshot {
    /// Validate the complete graph before I/O, then load in lexicographic ID order.
    /// Duplicate IDs/edges, unknown targets, cycles (including self-edges), and
    /// canonical source paths owned by multiple compilations are rejected.
    /// Source declarations always bind in their owner's context, even when queried
    /// from a dependent view. Binary hierarchy links use their own compilation's
    /// selected inputs; different versions can coexist in distinct contexts.
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
        let mut inputs = Vec::new();
        let mut owners = BTreeSet::new();
        let (mut entries, mut bytes, mut expanded) = (0, 0, 0);
        for compilation in configs {
            let mut bounded = compilation.inputs.clone();
            bounded.limits.max_entries = bounded.limits.max_entries.min(config.max_entries - entries);
            bounded.limits.max_input_bytes = bounded.limits.max_input_bytes.min(config.max_input_bytes - bytes);
            bounded.limits.max_archive_expanded_bytes =
                bounded.limits.max_archive_expanded_bytes.min(config.max_archive_expanded_bytes - expanded);
            let loaded = ProjectSnapshot::load(&bounded).map_err(|input| WorkspaceLoadError {
                kind: K::Input,
                detail: format!("{:?}: {input}", compilation.id),
                input: Some(input),
            })?;
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
        Ok(Self { ids, inputs, visibility })
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
