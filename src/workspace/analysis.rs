//! Caller-owned immutable prepared analysis. No process or filesystem cache.
use super::*;
use std::sync::Arc;

/// An owned, immutable workspace and its prepared declaration index.
///
/// Clones retain the same state without rebuilding. Source updates produce a new
/// state only after complete ownership and budget validation. Previous handles
/// keep their exact inputs and results; real updates rebuild the whole index.
///
/// This is not a filesystem watcher, persistent symbol identity, fine-grained
/// invalidation or a semantic result cache. Existing LSP jobs are unchanged.
#[derive(Clone)]
pub struct AnalysisSnapshot {
    state: Arc<AnalysisState>,
}
struct AnalysisState {
    workspace: WorkspaceSnapshot,
    index: crate::bind::OwnedScopedIndex,
}
impl AnalysisSnapshot {
    /// Prepare owned immutable inputs once. No filesystem reads are performed.
    pub fn new(workspace: WorkspaceSnapshot) -> Self {
        let projects = workspace.inputs.iter().map(ProjectSnapshot::project).collect::<Vec<_>>();
        let index =
            crate::bind::OwnedScopedIndex::new(&projects, &workspace.visibility).with_compilation_ids(&workspace.ids);
        Self { state: Arc::new(AnalysisState { workspace, index }) }
    }

    /// Load and prepare a new root. Nothing is reused from an earlier load.
    /// Source inventory, dependencies, archives and graph changes use this path.
    pub fn load(config: &WorkspaceConfig) -> Result<Self, WorkspaceLoadError> {
        WorkspaceSnapshot::load(config).map(Self::new)
    }

    /// Exact immutable inputs belonging to this index, never a newer workspace.
    pub fn inputs(&self) -> &WorkspaceSnapshot {
        &self.state.workspace
    }

    /// Derive an analysis version from whole-buffer replacements.
    ///
    /// Empty and byte-identical replacements share the existing state, but only
    /// after the entire batch passes the usual validation. Every real change
    /// builds one new index; errors never publish a partial state. An empty batch
    /// does not revert earlier overlays. Loading disk state requires a fresh load.
    pub fn with_source_overlays(&self, overlays: &[SourceOverlay]) -> Result<Self, SourceOverlayError> {
        let next = self.inputs().with_source_overlays(overlays)?;
        // The existing transaction already proved each exact path and owner.
        let unchanged = overlays.iter().all(|overlay| {
            let scope = self.inputs().scope(&overlay.owner).expect("validated overlay owner");
            scope.inputs().project().file(&overlay.path).expect("validated overlay path").text() == overlay.text
        });
        if unchanged { Ok(self.clone()) } else { Ok(Self::new(next)) }
    }

    /// Borrow a compilation view. Unknown IDs do not create an implicit scope.
    pub fn scope(&self, id: &CompilationId) -> Option<AnalysisScope<'_>> {
        self.inputs().ids.binary_search(id).ok().map(|index| AnalysisScope { state: &self.state, index })
    }
}

/// A lightweight view of an owned analysis version's exact compilation context.
#[derive(Clone, Copy)]
pub struct AnalysisScope<'a> {
    state: &'a AnalysisState,
    index: usize,
}
impl<'a> AnalysisScope<'a> {
    fn input_scope(&self) -> CompilationScope<'a> {
        CompilationScope { workspace: &self.state.workspace, index: self.index }
    }
    /// Caller-assigned compilation identity.
    pub fn id(&self) -> &'a CompilationId {
        self.input_scope().id()
    }
    /// This compilation's own immutable inputs, without dependency sources.
    /// Its flat resolver is not the graph-aware resolver on this view.
    pub fn inputs(&self) -> &'a ProjectSnapshot {
        self.input_scope().inputs()
    }
    /// Visible source paths, including dependency sources, in canonical order.
    pub fn source_paths(&self) -> Vec<&'a str> {
        self.input_scope().source_paths()
    }
    /// Query without rebuilding, retaining declaration-owner linking context.
    pub fn resolve_at(&self, path: &str, byte: u32) -> ResolveResult {
        self.state.index.resolve_at(self.index, path, byte, |index| self.state.workspace.inputs[index].project())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_state_build_counts_and_validated_noops() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scopes/lib/main");
        let id = CompilationId::new("lib", "main");
        let config = WorkspaceConfig {
            compilations: vec![CompilationConfig {
                id: id.clone(),
                inputs: crate::ProjectConfig { source_roots: vec![root], ..Default::default() },
                dependencies: vec![],
            }],
            ..Default::default()
        };
        let before = crate::bind::observed_index_builds();
        let snapshot = AnalysisSnapshot::load(&config).unwrap();
        assert_eq!(crate::bind::observed_index_builds(), before + 1);
        let path = snapshot.scope(&id).unwrap().source_paths()[0].to_owned();
        let text = snapshot.scope(&id).unwrap().inputs().project().file(&path).unwrap().text().to_owned();
        let overlay = SourceOverlay { owner: id.clone(), path: path.clone(), text };
        let clone = snapshot.clone();
        let empty = clone.with_source_overlays(&[]).unwrap();
        let same = empty.with_source_overlays(std::slice::from_ref(&overlay)).unwrap();
        assert!(Arc::ptr_eq(&snapshot.state, &same.state));
        for _ in 0..10 {
            same.scope(&id).unwrap().resolve_at(&path, 0);
            same.scope(&id).unwrap().resolve_at("absent.java", 0);
        }
        assert_eq!(crate::bind::observed_index_builds(), before + 1);
        assert_eq!(
            same.with_source_overlays(&[overlay.clone(), overlay.clone()]).err().unwrap().kind,
            SourceOverlayErrorKind::DuplicateSource
        );
        let mut wrong = overlay.clone();
        wrong.owner = CompilationId::new("unknown", "main");
        assert_eq!(
            same.with_source_overlays(&[overlay.clone(), wrong]).err().unwrap().kind,
            SourceOverlayErrorKind::UnknownCompilation
        );
        assert_eq!(crate::bind::observed_index_builds(), before + 1);
        let mut changed = overlay;
        changed.text.push_str("\n// changed\n");
        let next = same.with_source_overlays(&[changed]).unwrap();
        assert!(!Arc::ptr_eq(&same.state, &next.state));
        assert_eq!(crate::bind::observed_index_builds(), before + 2);
        assert!(Arc::ptr_eq(&next.state, &next.with_source_overlays(&[]).unwrap().state));
        assert_eq!(crate::bind::observed_index_builds(), before + 2);
    }
}
