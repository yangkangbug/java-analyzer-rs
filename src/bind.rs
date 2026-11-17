//! Name binding for source files and class files registered on one project.
//!
//! The search order for a simple name follows JLS 6.5: a local or parameter
//! in the innermost scope, then a field of a lexically enclosing type, then
//! a single-type import, then a top-level type in the same compilation unit,
//! then the same package, then on-demand imports. `java.lang` is an on-demand
//! import only for types that are actually in this project. A call does not
//! fall through to a type.
//!
//! IntelliJ's `ResolveClassUtil.resolveClass` (intellij-community
//! `840d3be11dfdeada533d05052008b48816b6cc4e`) returns a class only when the
//! scope walk produced exactly one candidate. This module uses that same
//! one-candidate rule. It does not copy the resolver.
//!
//! Field and method lookup walks `extends` and `implements`, including class
//! files registered on [`Project`]. A missing supertype contributes nothing.
//! A cycle is [`ResolveStatus::Cycle`]. Two members that share a name stay
//! ambiguous or [`ResolveStatus::OverloadNotResolved`] unless the bounded
//! primitive source-call proof selects a unique most-specific method. Unsupported
//! singleton calls may retain a candidate as [`ResolveStatus::CallNotResolved`].
//! The first candidate is never chosen arbitrarily. Method parameters are compared by resolved type identity in their
//! declaration context; unsupported comparisons stay
//! [`ResolveStatus::SignatureNotCompared`]. Legacy queries rebuild the index;
//! [`Project::prepare`] retains it for repeated queries. There is no JDK and no classpath beyond what the caller added.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use crate::classfile::{
    Access, ClassError, ClassSignature, MemberKind, MemberSignature, parse_class_bytes, read_class_directory,
};
mod instantiation;

use crate::green::GreenNode;
use crate::kind::SyntaxKind;
use crate::{EditResult, ParsedFile, TextEdit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolveStatus {
    Resolved,
    Unresolved,
    Ambiguous,
    /// No supported unique overload selection. The original candidates are retained.
    OverloadNotResolved,
    /// A single source invocation was not proved applicable. Retained targets are
    /// candidates, not a selection. This is not a Java semantic diagnostic.
    CallNotResolved,
    /// The qualifier is `super` or a value whose declared type is not a source class.
    ReceiverNotBound,
    /// A static import names this identifier, and static imports are not applied.
    StaticImportNotApplied,
    /// The name is a package prefix of a source class, not a type or member.
    Package,
    /// `Type::new`. Constructors are not chosen.
    ConstructorRefNotBound,
    /// `extends` / `implements` visits a type that is already on the path.
    Cycle,
    /// A malformed signature or an unsupported/unresolved parameter comparison
    /// prevents distinguishing same-signature inheritance from overloads.
    SignatureNotCompared,
    /// The only candidates have contradictory access flags.
    NotDetermined,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    Class,
    Field,
    Method,
    Local,
    Parameter,
    Resource,
}

/// Linking context of a declaration in this snapshot, not a persistent or runtime identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BinaryLinkingContext {
    Standalone,
    Compilation(crate::workspace::CompilationId),
}

/// Exact modeled binary method declaration. The origin is `Target::path`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinaryTargetIdentity {
    pub binary_name: String,
    /// Full JVM method descriptor, including its return descriptor.
    pub descriptor: String,
    pub linking_context: BinaryLinkingContext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// Present only for binary methods; source/class/field targets remain unrefined.
    pub binary: Option<BinaryTargetIdentity>,
    pub path: String,
    pub kind: TargetKind,
    pub name: String,
    /// Dotted package plus nesting, such as `demo.Outer.Inner`.
    pub owner: String,
    pub start_byte: u32,
    pub end_byte: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveResult {
    pub status: ResolveStatus,
    pub targets: Vec<Target>,
}

impl ResolveResult {
    fn one(status: ResolveStatus, target: Target) -> Self {
        Self { status, targets: vec![target] }
    }

    fn none(status: ResolveStatus) -> Self {
        Self { status, targets: Vec::new() }
    }

    fn many(status: ResolveStatus, targets: Vec<Target>) -> Self {
        Self { status, targets }
    }
}

/// Parsed sources keyed by a caller-chosen path. The index is rebuilt from
/// these trees on each query, so [`Project::edit`] drops stale bindings.
#[derive(Clone, Debug, Default)]
pub struct Project {
    files: BTreeMap<String, ParsedFile>,
    binaries: BTreeMap<String, Arc<ClassSignature>>,
}

impl Project {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: impl Into<String>, text: &str) {
        self.files.insert(path.into(), ParsedFile::parse(text));
    }

    pub fn file(&self, path: &str) -> Option<&ParsedFile> {
        self.files.get(path)
    }

    /// Replace `path` by applying `edit` to its current text.
    pub fn edit(&mut self, path: &str, edit: TextEdit) -> Option<EditResult> {
        let parsed = self.files.get(path)?;
        let result = parsed.apply_edit(edit);
        self.files.insert(path.to_string(), result.file.clone());
        Some(result)
    }

    /// Parse `bytes` and register them under `origin`. A later call with the
    /// same origin replaces the class. The next [`Project::resolve_at`] rebuilds
    /// the index from the current map.
    pub fn add_class_bytes(&mut self, origin: impl Into<String>, bytes: &[u8]) -> Result<(), ClassError> {
        let origin = origin.into();
        let signature = parse_class_bytes(&origin, bytes)?;
        self.binaries.insert(origin, Arc::new(signature));
        Ok(())
    }

    /// Register every `*.class` file under `root`. One bad file fails the call
    /// and leaves previously registered classes unchanged.
    pub fn add_class_directory(&mut self, root: &Path) -> Result<Vec<String>, ClassError> {
        let parsed = read_class_directory(root)?;
        let origins = parsed.iter().map(|class| class.origin.clone()).collect();
        for class in parsed {
            self.binaries.insert(class.origin.clone(), Arc::new(class));
        }
        Ok(origins)
    }

    pub(crate) fn insert_class_signature(&mut self, signature: Arc<ClassSignature>) {
        self.binaries.insert(signature.origin.clone(), signature);
    }

    pub fn remove_class(&mut self, origin: &str) -> bool {
        self.binaries.remove(origin).is_some()
    }

    pub fn class_signature(&self, origin: &str) -> Option<&ClassSignature> {
        self.binaries.get(origin).map(Arc::as_ref)
    }

    /// Prepare one immutable index for repeated half-open byte-offset queries.
    /// The borrow prevents edits until the prepared handle is no longer used.
    /// Targets retain path/range provenance, not stable cross-edit identities.
    pub fn prepare(&self) -> PreparedProject<'_> {
        PreparedProject { project: self, index: Index::build(&self.files, &self.binaries) }
    }

    /// Bind against the current inputs, rebuilding the index on each call.
    pub fn resolve_at(&self, path: &str, byte: u32) -> ResolveResult {
        let Some(parsed) = self.files.get(path) else {
            return ResolveResult::none(ResolveStatus::Unresolved);
        };
        let index = Index::build(&self.files, &self.binaries);
        index.resolve(path, parsed, byte)
    }
}

/// A reusable index borrowing the exact project inputs it describes.
///
/// Preparing allocates owned index records once. Queries do not rebuild the index
/// or cache results. Drop the handle before mutating the project and prepare again.
/// No target or internal index position is a persistent symbol identity.
///
/// ```compile_fail
/// use java_engine::Project;
/// let mut project = Project::new();
/// project.insert("A.java", "class A {}");
/// let prepared = project.prepare();
/// project.insert("A.java", "class B {}");
/// prepared.resolve_at("A.java", 6);
/// ```
pub struct PreparedProject<'a> {
    project: &'a Project,
    index: Index,
}

impl PreparedProject<'_> {
    /// Bind the name containing `byte` using the retained index.
    /// Missing paths and bytes outside a name return Unresolved.
    pub fn resolve_at(&self, path: &str, byte: u32) -> ResolveResult {
        let Some(parsed) = self.project.file(path) else {
            return ResolveResult::none(ResolveStatus::Unresolved);
        };
        self.index.resolve(path, parsed, byte)
    }
}

#[derive(Clone, Debug)]
struct QualName {
    parts: Vec<String>,
    array_rank: usize,
}

#[derive(Clone, Debug)]
struct Member {
    source_enum_constant: bool,
    // Navigation-only declaration fact, independent of strict call eligibility.
    source_interface_declared_only: bool,
    name: String,
    kind: TargetKind,
    start: u32,
    end: u32,
    declared: Option<QualName>,
    access: Access,
    shape: Shape,
    source_call: Option<SourceCall>,
    // Keep unused source-only metadata out of every binary member payload.
    source_field: Option<Box<SourceField>>,
}

#[derive(Clone, Debug)]
struct ClassRec {
    scope: usize,
    context: BinaryLinkingContext,
    data: ClassData,
}

#[derive(Clone, Debug)]
enum ClassData {
    Source(Box<ClassPayload>),
    Binary(Arc<ClassPayload>),
}

// Only scope-independent syntax/signature data lives here. Resolved parents,
// parameter identities, access decisions and ClassIdentity remain contextual.
#[derive(Clone, Debug)]
struct ClassPayload {
    path: String,
    package: String,
    nesting: Vec<String>,
    name_start: u32,
    name_end: u32,
    range_start: u32,
    range_end: u32,
    super_parts: Option<Vec<String>>,
    interface_parts: Vec<Vec<String>>,
    binary: bool,
    binary_signature: Option<Arc<ClassSignature>>,
    is_interface: bool,
    type_variables: Vec<String>,
    primitive_owner: bool,
    access: Access,
    reference_inventory: bool,
    reference_generic: bool,
    reference_final: bool,
    // Syntax-only eligibility, never inferred from a same-named method.
    reference_constructor_work: Option<usize>,
    reference_edges: Option<Vec<(Vec<String>, bool)>>,
    // Scope-independent JVM edge spellings; None marks a dotted-name collision.
    binary_ancestor_names: BTreeMap<String, Option<String>>,
    binary_root: bool,
    fields: Vec<Member>,
    methods: Vec<Member>,
    // Source constructor declarations are not ordinary or inherited methods.
    constructors: Vec<Member>,
}

impl std::ops::Deref for ClassRec {
    type Target = ClassPayload;

    fn deref(&self) -> &Self::Target {
        match &self.data {
            ClassData::Source(data) => data,
            ClassData::Binary(data) => data,
        }
    }
}

impl ClassRec {
    fn source_data_mut(&mut self) -> &mut ClassPayload {
        match &mut self.data {
            ClassData::Source(data) => data,
            ClassData::Binary(_) => unreachable!("binary payloads are immutable"),
        }
    }

    fn dotted(&self) -> String {
        let nested = self.nesting.join(".");
        if self.package.is_empty() { nested } else { format!("{}.{}", self.package, nested) }
    }

    fn simple(&self) -> &str {
        self.nesting.last().map(String::as_str).unwrap_or("")
    }

    fn target(&self) -> Target {
        Target {
            binary: None,
            path: self.path.clone(),
            kind: TargetKind::Class,
            name: self.simple().to_string(),
            owner: self.dotted(),
            start_byte: self.name_start,
            end_byte: self.name_end,
        }
    }
}

#[derive(Clone, Debug)]
struct LocalRec {
    name: String,
    kind: TargetKind,
    start: u32,
    end: u32,
    scope_start: u32,
    scope_end: u32,
    visible_from: u32,
    declared: Option<QualName>,
    primitive: Option<PrimitiveValue>,
    postfix_after: Option<u32>,
    catch_origin: bool,
    // Strict plain-reference syntax, with dimensions; unlike legacy declared metadata.
    reference: Option<ReferenceDeclaration>,
    path: String,
}

#[derive(Clone, Debug)]
enum ReferenceDeclaration {
    Plain(QualName),
    // None is an explicit unbounded wildcard, never erased Object evidence.
    Applied { name: Vec<String>, arguments: Vec<Option<Vec<String>>> },
}
impl ReferenceDeclaration {
    fn plain(&self) -> Option<&QualName> {
        match self {
            Self::Plain(ty) => Some(ty),
            Self::Applied { .. } => None,
        }
    }
}

#[derive(Clone, Debug)]
struct LocalTypeRec {
    name: Option<String>,
    start: u32,
    end: u32,
    scope_end: u32,
}

fn local_type_shadows(file: &FileRec, name: &str, byte: u32) -> bool {
    file.local_types
        .iter()
        .any(|local| local.name.as_deref() == Some(name) && local.start <= byte && byte < local.scope_end)
}

#[derive(Clone, Debug)]
struct FileRec {
    scope: usize,
    path: String,
    package: String,
    singles: Vec<(String, String)>,
    stars: Vec<String>,
    static_names: Vec<String>,
    classes: Vec<ClassRec>,
    locals: Vec<LocalRec>,
    local_types: Vec<LocalTypeRec>,
    opaque_enum_bodies: Vec<(u32, u32)>,
    syntax_valid: bool,
    call_inventory_valid: bool,
}

#[cfg(test)]
thread_local! {
    static LEGACY_NAME_BUCKET_ENTRIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static INDEX_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static BINARY_PAYLOAD_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static BINARY_MEMBER_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static BINARY_PAYLOAD_CACHE_LOOKUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static BINARY_PAYLOAD_CACHE_MAX_ENTRIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct Index {
    files: Vec<FileRec>,
    binaries: Vec<ClassRec>,
    visibility: Vec<BTreeSet<usize>>,
    reference_names: BTreeMap<(usize, String), Vec<(usize, usize)>>,
}

impl Index {
    fn build(sources: &BTreeMap<String, ParsedFile>, binaries: &BTreeMap<String, Arc<ClassSignature>>) -> Self {
        #[cfg(test)]
        INDEX_BUILDS.with(|count| count.set(count.get() + 1));
        let files = sources.iter().map(|(path, file)| index_file(path, file)).collect();
        let binaries = binaries
            .values()
            .map(|signature| ClassRec {
                scope: 0,
                context: BinaryLinkingContext::Standalone,
                data: ClassData::Binary(Arc::new(class_from_signature(signature))),
            })
            .collect();
        Self::with_reference_names(files, binaries, vec![BTreeSet::from([0])])
    }

    fn scoped(projects: &[&Project], visibility: &[BTreeSet<usize>]) -> Self {
        #[cfg(test)]
        INDEX_BUILDS.with(|count| count.set(count.get() + 1));
        let mut files = Vec::new();
        let mut binaries = Vec::new();
        // One preparation only, bounded by its distinct input signature allocations.
        // Each value retains binary_signature, a strong Arc owning its pointer key.
        // Equal names/origins (or even equal bytes) never substitute for identity.
        let mut payloads: BTreeMap<*const ClassSignature, Arc<ClassPayload>> = BTreeMap::new();
        for (scope, project) in projects.iter().enumerate() {
            for (path, parsed) in &project.files {
                let mut file = index_file(path, parsed);
                file.scope = scope;
                for class in &mut file.classes {
                    class.scope = scope;
                }
                files.push(file);
            }
            for signature in project.binaries.values() {
                let payload = if projects.len() == 1 {
                    // Each project's origin-keyed map can contain a signature
                    // allocation only once, so a single scope cannot reuse it.
                    Arc::new(class_from_signature(signature))
                } else {
                    #[cfg(test)]
                    BINARY_PAYLOAD_CACHE_LOOKUPS.with(|count| count.set(count.get() + 1));
                    let payload = payloads
                        .entry(Arc::as_ptr(signature))
                        .or_insert_with(|| Arc::new(class_from_signature(signature)));
                    let payload = Arc::clone(payload);
                    #[cfg(test)]
                    BINARY_PAYLOAD_CACHE_MAX_ENTRIES.with(|count| count.set(count.get().max(payloads.len())));
                    payload
                };
                binaries.push(ClassRec {
                    scope,
                    context: BinaryLinkingContext::Standalone,
                    data: ClassData::Binary(payload),
                });
            }
        }
        Self::with_reference_names(files, binaries, visibility.to_vec())
    }

    fn with_reference_names(files: Vec<FileRec>, binaries: Vec<ClassRec>, visibility: Vec<BTreeSet<usize>>) -> Self {
        let mut reference_names: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for (i, file) in files.iter().enumerate() {
            for (j, class) in file.classes.iter().enumerate() {
                reference_names.entry((class.scope, class.dotted())).or_default().push((i, j));
            }
        }
        for (j, class) in binaries.iter().enumerate() {
            reference_names.entry((class.scope, class.dotted())).or_default().push((usize::MAX, j));
        }
        Self { files, binaries, visibility, reference_names }
    }

    fn with_compilation_ids(mut self, ids: &[crate::workspace::CompilationId]) -> Self {
        assert_eq!(ids.len(), self.visibility.len());
        for class in &mut self.binaries {
            class.context = BinaryLinkingContext::Compilation(ids[class.scope].clone());
        }
        self
    }

    fn reference_named(&self, scope: usize, name: &str) -> Vec<&ClassRec> {
        let mut out = Vec::new();
        for visible in &self.visibility[scope] {
            if let Some(locations) = self.reference_names.get(&(*visible, name.to_owned())) {
                for &(file, class) in locations {
                    if file != usize::MAX {
                        out.push(&self.files[file].classes[class]);
                    } else if *visible == scope {
                        out.push(&self.binaries[class]);
                    }
                }
            }
        }
        out
    }

    fn legacy_named(&self, scope: usize, name: &str) -> Vec<&ClassRec> {
        // Reuse immutable name buckets, but preserve the legacy global order:
        // all visible source declarations first, then this scope's binaries.
        // reference_named has a different scope-interleaved order and stays separate.
        let mut locations = Vec::new();
        let mut key = (scope, name.to_owned());
        for visible in &self.visibility[scope] {
            key.0 = *visible;
            if let Some(bucket) = self.reference_names.get(&key) {
                for &(file, class) in bucket {
                    #[cfg(test)]
                    LEGACY_NAME_BUCKET_ENTRIES.with(|count| count.set(count.get() + 1));
                    if file != usize::MAX || *visible == scope {
                        locations.push((file, class));
                    }
                }
            }
        }
        // Legacy classes() always includes own binaries, even when a private
        // Index caller supplies visibility without its own scope.
        if !self.visibility[scope].contains(&scope) {
            key.0 = scope;
            if let Some(bucket) = self.reference_names.get(&key) {
                for &(file, class) in bucket {
                    #[cfg(test)]
                    LEGACY_NAME_BUCKET_ENTRIES.with(|count| count.set(count.get() + 1));
                    if file == usize::MAX {
                        locations.push((file, class));
                    }
                }
            }
        }
        locations.sort_unstable();
        locations
            .into_iter()
            .map(
                |(file, class)| {
                    if file == usize::MAX { &self.binaries[class] } else { &self.files[file].classes[class] }
                },
            )
            .collect()
    }

    fn classes(&self, scope: usize) -> impl Iterator<Item = &ClassRec> {
        self.files
            .iter()
            .filter(move |file| self.visibility[scope].contains(&file.scope))
            .flat_map(|file| file.classes.iter())
            .chain(self.binaries.iter().filter(move |class| class.scope == scope))
    }

    fn resolve(&self, path: &str, parsed: &ParsedFile, byte: u32) -> ResolveResult {
        let Some(file) = self.files.iter().find(|file| file.path == path) else {
            return ResolveResult::none(ResolveStatus::Unresolved);
        };
        if file.opaque_enum_bodies.iter().any(|&(start, end)| start <= byte && byte < end)
            || file.local_types.iter().any(|local| local.start <= byte && byte < local.end)
        {
            return ResolveResult::none(ResolveStatus::Unresolved);
        }
        if let Some(hit) = innermost_ref(parsed.root(), 0, byte, false) {
            return self.resolve_hit(file, parsed, byte, hit);
        }
        if let Some(target) = declaration_at(file, byte) {
            return ResolveResult::one(ResolveStatus::Resolved, target);
        }
        ResolveResult::none(ResolveStatus::Unresolved)
    }

    fn resolve_hit(&self, file: &FileRec, parsed: &ParsedFile, byte: u32, hit: Hit<'_>) -> ResolveResult {
        match hit.node.kind() {
            SyntaxKind::ReferenceExpr => {
                let name = token_name(hit.node);
                if hit.call {
                    self.simple_call(file, parsed, byte, &name, hit.arguments)
                } else {
                    self.simple_name(file, byte, &name, false)
                }
            }
            SyntaxKind::Name => self.resolve_type_name(file, parsed, hit, byte),
            SyntaxKind::FieldExpr => self.resolve_field(file, parsed, hit, byte),
            SyntaxKind::MethodRefExpr => self.resolve_method_ref(file, parsed, hit, byte),
            _ => ResolveResult::none(ResolveStatus::Unresolved),
        }
    }

    // Name navigation remains the fallback. Only this direct-callee path has
    // argument syntax; receiver/member-reference lookup does not infer calls.
    fn simple_call(
        &self,
        file: &FileRec,
        parsed: &ParsedFile,
        byte: u32,
        name: &str,
        arguments: Option<(&GreenNode, u32)>,
    ) -> ResolveResult {
        for class in enclosing_classes(file, byte) {
            let found = match self.collect(class, name, file, byte) {
                Ok(found) => found,
                Err(err) => return walk_error(err),
            };
            let mut result = select_methods(found.clone());
            if result.status == ResolveStatus::Unresolved {
                continue;
            }
            let Some((args, args_at)) = arguments else { return result };
            let actuals = delimited_nodes(args, args_at);
            let caller = direct_caller(parsed.root(), byte);
            // A negative guard needs no Object inventory. An arity mismatch
            // does not type a literal or discard variable-arity possibilities.
            if result.status == ResolveStatus::Resolved && found.len() == 1 {
                let candidate = &found[0];
                if let Some(source) = &candidate.member.source_call {
                    let fixed_arity_mismatch = !source.varargs
                        && actuals.as_ref().is_some_and(|args| {
                            candidate.signature.as_ref().is_some_and(|sig| args.len() != sig.parameters.len())
                        });
                    let primitive_mismatch = caller.is_some_and(|DirectCaller { node: caller, start, end, .. }| {
                        if !file.call_inventory_valid || source_call(caller, start).is_none() {
                            return false;
                        }
                        let mut proof = PrimitiveProof::new(file, start, end, None);
                        let mismatch = actuals.as_ref().is_some_and(|args| {
                            let Shape::Source { parameters, .. } = &candidate.member.shape else { return false };
                            // One independently known contradiction is enough;
                            // unrelated unknown formals cannot erase it.
                            !source.varargs
                                && args.len() == parameters.len()
                                && args.iter().zip(parameters).any(|((arg, at), formal)| {
                                    formal.primitive.filter(|_| !formal.unsupported).is_some_and(|primitive| {
                                        proof.expression(arg, *at, 0).is_some_and(|actual| {
                                            !actual.converts_to(PrimitiveValue { primitive, rank: formal.array_rank })
                                        })
                                    })
                                })
                        });
                        // A bounded guard must not turn an exhausted known-
                        // contradiction check into unique-name success when
                        // the strict positive context (e.g. Object) is absent.
                        mismatch || proof.exhausted
                    });
                    if fixed_arity_mismatch || primitive_mismatch {
                        result.status = ResolveStatus::CallNotResolved;
                        return result;
                    }
                }
            }
            let Some(DirectCaller { node: caller_node, start: caller_start, end: caller_end, .. }) = caller else {
                return result;
            };
            // Bare calls have a method namespace, not a value/type qualifier.
            // Reuse the strict invocation proof before the older primitive path.
            if reference_method_header(caller_node, caller_start)
                && child_kind(caller_node, caller_start, SyntaxKind::TypeParamList).is_none()
                && !contains_kind(caller_node, SyntaxKind::LocalType)
                && crate::green::collect_errors(caller_node).is_empty()
            {
                let mut proof = ReferenceCallProof::new(self, file, caller);
                if let Some(owner) = proof.implicit_instance_owner(byte)
                    && same_class(owner, class)
                    && let Some(selected) =
                        proof.select_mode(owner, name, args, args_at, 0, InvocationMode::OwnInstance)
                    && !proof.exhausted
                {
                    return ResolveResult::one(
                        ResolveStatus::Resolved,
                        member_target(selected.declaring, selected.member),
                    );
                }
            }
            let Some(object) = self.closed_primitive_owner(file, class, name) else { return result };
            if !file.call_inventory_valid
                || !caller_node.child_nodes().any(|node| node.kind() == SyntaxKind::Block)
                || !source_call(caller_node, caller_start).is_some_and(|call| call.is_static)
                || child_kind(caller_node, caller_start, SyntaxKind::TypeParamList).is_some()
            {
                return result;
            }
            if !matches!(result.status, ResolveStatus::Resolved | ResolveStatus::OverloadNotResolved) {
                return result;
            }
            let mut proof = PrimitiveProof::new(file, caller_start, caller_end, Some((class, object)));
            let selected = proof.call(name, args, args_at, 0);
            if let Some(member) = selected {
                return ResolveResult::one(ResolveStatus::Resolved, member_target(class, member));
            }
            if result.status == ResolveStatus::Resolved {
                result.status = ResolveStatus::CallNotResolved;
            }
            return result;
        }
        if file.static_names.iter().any(|imported| imported == name) {
            ResolveResult::none(ResolveStatus::StaticImportNotApplied)
        } else {
            ResolveResult::none(ResolveStatus::Unresolved)
        }
    }

    fn closed_primitive_owner<'a>(&'a self, file: &FileRec, class: &ClassRec, name: &str) -> Option<&'a ClassRec> {
        if !class.primitive_owner
            || !matches!(self.class_named(class.scope, &class.dotted()), Named::One(found) if same_class(found, class))
        {
            return None;
        }
        let parent = class.super_parts.as_ref()?;
        match self.resolve_edge(class, parent) {
            Named::One(object) => {
                // Source Object implicitly extends itself in this index. A
                // supplied binary root is the bounded, verifiable closure.
                (object.binary_root
                    && object.scope == file.scope
                    && object.methods.iter().all(|member| member.name != name))
                .then_some(object)
            }
            _ => None,
        }
    }

    fn simple_name(&self, file: &FileRec, byte: u32, name: &str, call: bool) -> ResolveResult {
        // An invocation name occupies the method namespace. Lexical values
        // still win for ordinary references and qualified receiver prefixes.
        if call {
            let methods = self.methods_here(file, byte, name);
            if methods.status != ResolveStatus::Unresolved {
                return methods;
            }
            if file.static_names.iter().any(|imported| imported == name) {
                return ResolveResult::none(ResolveStatus::StaticImportNotApplied);
            }
            return ResolveResult::none(ResolveStatus::Unresolved);
        }
        if let Some(result) = self.locals_named(file, byte, name) {
            return result;
        }
        if let Some(result) = self.fields_named(file, byte, name) {
            return result;
        }
        match self.simple_type(file, name, byte) {
            TypeLookup::One(class) => ResolveResult::one(ResolveStatus::Resolved, class.target()),
            TypeLookup::Many(classes) => {
                ResolveResult::many(ResolveStatus::Ambiguous, classes.into_iter().map(|c| c.target()).collect())
            }
            TypeLookup::MissingImport => ResolveResult::none(ResolveStatus::Unresolved),
            TypeLookup::None => {
                if file.static_names.iter().any(|imported| imported == name) {
                    ResolveResult::none(ResolveStatus::StaticImportNotApplied)
                } else {
                    ResolveResult::none(ResolveStatus::Unresolved)
                }
            }
        }
    }

    fn locals_named(&self, file: &FileRec, byte: u32, name: &str) -> Option<ResolveResult> {
        local_resolution(file.locals.iter(), byte, name)
    }

    fn fields_named(&self, file: &FileRec, byte: u32, name: &str) -> Option<ResolveResult> {
        for class in enclosing_classes(file, byte) {
            let result = self.lookup_fields(class, name, file, byte);
            if result.status != ResolveStatus::Unresolved {
                return Some(result);
            }
        }
        None
    }

    fn methods_here(&self, file: &FileRec, byte: u32, name: &str) -> ResolveResult {
        for class in enclosing_classes(file, byte) {
            let result = self.lookup_methods(class, name, file, byte);
            if result.status != ResolveStatus::Unresolved {
                return result;
            }
        }
        ResolveResult::none(ResolveStatus::Unresolved)
    }

    fn simple_type<'a>(&'a self, file: &'a FileRec, name: &str, byte: u32) -> TypeLookup<'a> {
        if local_type_shadows(file, name, byte) {
            return TypeLookup::MissingImport;
        }
        match member_type_here(file, byte, name) {
            TypeLookup::None => {}
            member => return member,
        }
        let imports: BTreeSet<_> =
            file.singles.iter().filter(|(simple, _)| simple == name).map(|(_, full)| full).collect();
        // A missing or invisible import is still a competing declaration. Do
        // not choose whichever import was indexed first or happens to be loaded.
        // Exact repeated imports contribute only one name, as in strict lookup.
        if imports.len() > 1 {
            return TypeLookup::MissingImport;
        }
        if let Some(full) = imports.first() {
            let imported = self.legacy_named(file.scope, full);
            let same_unit = file
                .classes
                .iter()
                .filter(|class| class.nesting.len() == 1 && class.simple() == name)
                .collect::<Vec<_>>();
            if imported.len() == 1 && same_unit.is_empty() {
                return TypeLookup::One(imported[0]);
            }
            if imported.is_empty() {
                return TypeLookup::MissingImport;
            }
            let mut all = imported;
            all.extend(same_unit);
            return if all.len() == 1 { TypeLookup::One(all[0]) } else { TypeLookup::Many(all) };
        }
        let same_package: Vec<_> = self
            .classes(file.scope)
            .filter(|class| class.package == file.package && class.nesting.len() == 1 && class.simple() == name)
            .collect();
        if same_package.len() == 1 {
            return TypeLookup::One(same_package[0]);
        }
        if same_package.len() > 1 {
            return TypeLookup::Many(same_package);
        }
        let mut stars = file.stars.clone();
        if !stars.iter().any(|package| package == "java.lang") {
            stars.push("java.lang".to_string());
        }
        let demanded: Vec<_> = self
            .classes(file.scope)
            .filter(|class| stars.contains(&class.package) && class.nesting.len() == 1 && class.simple() == name)
            .collect();
        match demanded.len() {
            0 => TypeLookup::None,
            1 => TypeLookup::One(demanded[0]),
            _ => TypeLookup::Many(demanded),
        }
    }

    fn resolve_type_name(&self, file: &FileRec, _parsed: &ParsedFile, hit: Hit<'_>, byte: u32) -> ResolveResult {
        let (parts, _) = name_pieces(hit.node, hit.start);
        let Some(index) = parts.iter().position(|(_, start, end)| *start <= byte && byte < *end) else {
            return ResolveResult::none(ResolveStatus::Unresolved);
        };
        let prefix = parts[..=index].iter().map(|(name, _, _)| name.clone()).collect::<Vec<_>>();
        if prefix.len() == 1 {
            return match self.simple_type(file, &prefix[0], byte) {
                TypeLookup::One(class) => ResolveResult::one(ResolveStatus::Resolved, class.target()),
                TypeLookup::Many(classes) => {
                    ResolveResult::many(ResolveStatus::Ambiguous, classes.into_iter().map(ClassRec::target).collect())
                }
                TypeLookup::None | TypeLookup::MissingImport => ResolveResult::none(ResolveStatus::Unresolved),
            };
        }
        // A contested lexical type prefix cannot be reinterpreted as a package
        // merely because an identically spelled qualified class is available.
        if matches!(member_type_here(file, byte, &prefix[0]), TypeLookup::Many(_)) {
            return ResolveResult::none(ResolveStatus::Ambiguous);
        }
        self.qualify(file, &prefix, false, byte)
    }

    fn resolve_field(&self, file: &FileRec, parsed: &ParsedFile, hit: Hit<'_>, byte: u32) -> ResolveResult {
        // Super and class-literal reflection lookup are not implemented. Do not
        // discard an unsupported receiver and reinterpret its selector as a name.
        if has_super_receiver(hit.node) || has_class_literal_receiver(hit.node) {
            return ResolveResult::none(ResolveStatus::ReceiverNotBound);
        }
        if has_call_result_receiver(hit.node) {
            return if hit.call {
                self.reference_call(file, parsed, &hit, byte)
                    .unwrap_or_else(|| ResolveResult::none(ResolveStatus::ReceiverNotBound))
            } else {
                ResolveResult::none(ResolveStatus::ReceiverNotBound)
            };
        }
        let chain = field_chain(hit.node, hit.start);
        let Some(index) = chain.iter().position(|segment| segment.start <= byte && byte < segment.end) else {
            return ResolveResult::none(ResolveStatus::Unresolved);
        };
        if hit.call
            && index + 1 == chain.len()
            && let Some(result) = self.reference_call(file, parsed, &hit, byte)
        {
            return result;
        }
        self.resolve_chain(file, &chain[..=index], hit.call, byte)
    }

    fn reference_call(&self, file: &FileRec, parsed: &ParsedFile, hit: &Hit<'_>, byte: u32) -> Option<ResolveResult> {
        let (args, args_at) = hit.arguments?;
        let mut nodes = Vec::new();
        let mut selector = None;
        let mut at = hit.start;
        let mut state = 0;
        for child in hit.node.children() {
            if let Some(node) = child.as_node() {
                if state != 0 {
                    return None;
                }
                nodes.push((node.as_ref(), at));
                state = 1;
            } else if let Some(token) = child.as_token()
                && !token.kind().is_trivia()
            {
                state = match (state, token.kind()) {
                    (1, SyntaxKind::Dot) => 2,
                    (2, SyntaxKind::Ident) => {
                        selector = Some(token.text());
                        3
                    }
                    _ => return None,
                };
            }
            at += child.utf8_len();
        }
        let [(receiver, receiver_at)] = nodes.as_slice() else { return None };
        if state != 3 {
            return None;
        }
        let (mut plain_receiver, mut plain_at) = (*receiver, *receiver_at);
        let mut parens = 0;
        while plain_receiver.kind() == SyntaxKind::ParenExpr {
            if parens >= REFERENCE_PROOF_DEPTH {
                return Some(ResolveResult::none(ResolveStatus::ReceiverNotBound));
            }
            let Some(inner) = delimited_nodes(plain_receiver, plain_at) else {
                return Some(ResolveResult::none(ResolveStatus::ReceiverNotBound));
            };
            let [(node, at)] = inner.as_slice() else {
                return Some(ResolveResult::none(ResolveStatus::ReceiverNotBound));
            };
            plain_receiver = node;
            plain_at = *at;
            parens += 1;
        }
        // Only the new exact this.field form enters strict field typing.
        // Preserve unrelated legacy navigation for arbitrary object.field chains.
        if plain_receiver.kind() == SyntaxKind::FieldExpr
            && plain_receiver.child_nodes().next().is_none_or(|base| base.kind() != SyntaxKind::ThisExpr)
        {
            return None;
        }
        if plain_receiver.kind() == SyntaxKind::FieldExpr {
            let context = direct_caller(parsed.root(), byte)?;
            let DirectCaller { node: caller, start: caller_start, .. } = context;
            if !reference_method_header(caller, caller_start)
                || child_kind(caller, caller_start, SyntaxKind::TypeParamList).is_some()
                || contains_kind(caller, SyntaxKind::LocalType)
                || !crate::green::collect_errors(caller).is_empty()
            {
                return None;
            }
            let mut proof = ReferenceCallProof::new(self, file, Some(context));
            let receiver = proof.receiver(plain_receiver, plain_at, parens + 1)?;
            let member = proof.select_receiver(&receiver, selector?, args, args_at, 0)?;
            // Failure leaves legacy field-chain navigation intact. Only this
            // separately proved success supplies a strict selected target.
            return (!proof.exhausted)
                .then(|| ResolveResult::one(ResolveStatus::Resolved, member_target(member.declaring, member.member)));
        }
        if matches!(plain_receiver.kind(), SyntaxKind::CallExpr | SyntaxKind::IndexExpr | SyntaxKind::LiteralExpr) {
            let refused = || ResolveResult::none(ResolveStatus::ReceiverNotBound);
            let context = direct_caller(parsed.root(), byte);
            let Some(DirectCaller { node: caller, start: caller_start, .. }) = context else {
                return Some(refused());
            };
            if !reference_method_header(caller, caller_start)
                || child_kind(caller, caller_start, SyntaxKind::TypeParamList).is_some()
                || contains_kind(caller, SyntaxKind::LocalType)
                || !crate::green::collect_errors(caller).is_empty()
            {
                return Some(refused());
            }
            let mut proof = ReferenceCallProof::new(self, file, context);
            let value = if plain_receiver.kind() == SyntaxKind::CallExpr {
                match proof.invocation_value(plain_receiver, plain_at, parens + 1) {
                    Some(StrictActual::Reference(value)) => value,
                    _ => return Some(refused()),
                }
            } else {
                let Some(value) = proof.expression(plain_receiver, plain_at, parens + 1) else {
                    return Some(refused());
                };
                value
            };
            return Some(match proof.select(value.class, selector?, args, args_at, 0) {
                Some(member) if !proof.exhausted => {
                    ResolveResult::one(ResolveStatus::Resolved, member_target(member.declaring, member.member))
                }
                _ => refused(),
            });
        }
        if plain_receiver.kind() != SyntaxKind::ReferenceExpr {
            return None;
        }
        let name = selector?;
        let receiver_name = token_name(plain_receiver);
        let caller = direct_caller(parsed.root(), byte);
        let (caller_start, caller_end) = caller.map_or((0, 0), |c| (c.start, c.end));
        let mut proof = ReferenceCallProof::new(self, file, caller);
        if proof.charge(file.locals.len().max(1)).is_none() {
            return Some(ResolveResult::none(ResolveStatus::ReceiverNotBound));
        }
        // This complete scan already inspects every declaration. Reuse only
        // its fixed caller subset; ordinary name hints still use their existing
        // innermost-scope policy, while strict expression proof checks overlaps.
        let mut caller_locals = Vec::new();
        let locals = local_resolution(
            file.locals.iter().inspect(|local| {
                if local.start >= caller_start && local.end <= caller_end {
                    caller_locals.push(*local);
                }
            }),
            *receiver_at,
            &receiver_name,
        );
        proof.caller_locals = Some(caller_locals);
        let is_value = locals.is_some();
        if !is_value
            && let Some(DirectCaller { node: caller_node, .. }) = caller
            && reference_method_header(caller_node, caller_start)
            && child_kind(caller_node, caller_start, SyntaxKind::TypeParamList).is_none()
            && !contains_kind(caller_node, SyntaxKind::LocalType)
            && crate::green::collect_errors(caller_node).is_empty()
            && let Some(receiver) = proof.receiver(plain_receiver, plain_at, 0)
            && let Some(member) = proof.select_receiver(&receiver, name, args, args_at, 0)
            && !proof.exhausted
        {
            return Some(ResolveResult::one(ResolveStatus::Resolved, member_target(member.declaring, member.member)));
        }
        let owner = if let Some(local) = locals {
            if local.targets.len() != 1 || local.status != ResolveStatus::Resolved {
                return Some(ResolveResult::none(ResolveStatus::ReceiverNotBound));
            }
            let target = &local.targets[0];
            let declared = declared_of(file, target);
            let Some(owner) =
                declared.filter(|q| q.array_rank == 0).and_then(|q| proof.raw_type(file, &q.parts, target.start_byte))
            else {
                return Some(ResolveResult::none(ResolveStatus::ReceiverNotBound));
            };
            owner
        } else {
            // Fields and other nonlocal values are outside this proof. Never
            // reinterpret a bound value as a same-spelled class.
            for enclosing in enclosing_classes(file, *receiver_at) {
                match proof.has_field(enclosing, &receiver_name, 0, &mut BTreeSet::new()) {
                    Some(true) if !proof.exhausted => return None,
                    Some(true) => return Some(ResolveResult::none(ResolveStatus::ReceiverNotBound)),
                    Some(false) => {}
                    None => return Some(ResolveResult::none(ResolveStatus::ReceiverNotBound)),
                }
            }
            if caller.is_some_and(|c| type_variables(c.node, c.start).contains(&receiver_name)) {
                return Some(ResolveResult::none(ResolveStatus::ReceiverNotBound));
            }
            let Some(owner) = proof.raw_type(file, std::slice::from_ref(&receiver_name), *receiver_at) else {
                return Some(ResolveResult::none(ResolveStatus::ReceiverNotBound));
            };
            owner
        };
        if !is_value {
            if proof.exhausted {
                return Some(ResolveResult::none(ResolveStatus::ReceiverNotBound));
            }
            if let Some(DirectCaller { node: caller_node, .. }) = caller
                && parens == 0
                && reference_method_header(caller_node, caller_start)
                && child_kind(caller_node, caller_start, SyntaxKind::TypeParamList).is_none()
                && !contains_kind(caller_node, SyntaxKind::LocalType)
                && crate::green::collect_errors(caller_node).is_empty()
                && proof.static_type_owner(&receiver_name, *receiver_at).is_some_and(|c| same_class(c, owner))
                && let Some(selected) = proof.select_mode(owner, name, args, args_at, 0, InvocationMode::TypeStatic)
                && !proof.exhausted
            {
                return Some(ResolveResult::one(
                    ResolveStatus::Resolved,
                    member_target(selected.declaring, selected.member),
                ));
            }
            if owner.binary {
                if proof.exhausted {
                    return Some(ResolveResult::none(ResolveStatus::ReceiverNotBound));
                }
                // Preserve effective legacy hints, never expression evidence.
                let mut result = self.lookup_methods(owner, name, file, byte);
                if result.status == ResolveStatus::Resolved {
                    result.status = ResolveStatus::CallNotResolved;
                }
                return Some(result);
            }
            // Unsupported source type-qualified calls retain legacy navigation.
            // Only the strict success above may supply nested invocation values.
        }
        // Nested binary receiver lookup remains outside this proof. Raw dotted
        // name hints must not turn an unsupported receiver into a bound value.
        if owner.binary && owner.nesting.len() > 1 {
            return Some(ResolveResult::none(ResolveStatus::ReceiverNotBound));
        }
        let mut members = proof.hints(owner, name);
        if owner.binary {
            // Preserve legacy caller-visible failure hints, but never remove
            // declarations from the strict selector's competitor inventory.
            let mut visible = Vec::new();
            for (declaring, member) in members {
                match proof.binary_hint_visible(member, declaring, owner, byte) {
                    Some(true) => visible.push((declaring, member)),
                    Some(false) => {}
                    None => return Some(ResolveResult::none(ResolveStatus::ReceiverNotBound)),
                }
            }
            members = visible;
        }
        if let Some(failure) = proof.failure.take() {
            return Some(failure);
        }
        if members.is_empty() {
            return Some(ResolveResult::none(ResolveStatus::Unresolved));
        }
        let targets: Vec<_> = members.iter().map(|(owner, m)| member_target(owner, m)).collect();
        let mut result = if proof.charge(members.len().saturating_mul(members.len())).is_some() {
            let candidates = members
                .iter()
                .map(|(declaring, member)| Candidate {
                    member,
                    target: member_target(declaring, member),
                    signature: proof.hint_signature(declaring, member),
                    declaring: ClassIdentity::of(declaring),
                    ancestors: BTreeSet::new(),
                    unspecified: false,
                })
                .collect();
            select_methods(candidates)
        } else {
            ResolveResult::many(
                if members.len() == 1 { ResolveStatus::CallNotResolved } else { ResolveStatus::OverloadNotResolved },
                targets.clone(),
            )
        };
        result.targets = targets;
        // An independent fixed-arity contradiction needs neither a complete
        // hierarchy nor an argument type. It also guards type-qualified calls.
        if result.status == ResolveStatus::Resolved && members.len() == 1 {
            let member = members[0].1;
            if member.source_call.as_ref().is_some_and(|call| !call.varargs)
                && let Shape::Source { parameters, .. } = &member.shape
                && delimited_nodes(args, args_at).is_some_and(|args| args.len() != parameters.len())
            {
                result.status = ResolveStatus::CallNotResolved;
            }
        }
        if !is_value {
            if (parens != 0 || members.iter().any(|(_, m)| m.source_call.as_ref().is_none_or(|call| !call.is_static)))
                && result.status == ResolveStatus::Resolved
            {
                result.status = ResolveStatus::CallNotResolved;
            }
            // Static type-qualified names retain legacy navigation, but a
            // known inherited member type must not be replaced by a different
            // top-level class. Missing edges alone are not an applicability
            // proof or a new reason to accept/reject legacy static navigation.
            if result.status == ResolveStatus::Resolved {
                for enclosing in enclosing_classes(file, *receiver_at) {
                    if proof.known_type_shadow(enclosing, &receiver_name, 0, &mut BTreeSet::new()) != Some(false) {
                        result.status = ResolveStatus::CallNotResolved;
                        break;
                    }
                }
            }
            if proof.exhausted && result.status == ResolveStatus::Resolved {
                result.status = ResolveStatus::CallNotResolved;
            }
            return Some(result);
        }
        if result.status == ResolveStatus::Resolved {
            result.status = ResolveStatus::CallNotResolved;
        }
        let Some(DirectCaller { node: caller_node, .. }) = caller else { return Some(result) };
        if !reference_method_header(caller_node, caller_start)
            || child_kind(caller_node, caller_start, SyntaxKind::TypeParamList).is_some()
            || contains_kind(caller_node, SyntaxKind::LocalType)
            || !crate::green::collect_errors(caller_node).is_empty()
        {
            return Some(result);
        }
        // Exact local declaration evidence is separate from hint lookup above.
        let Some(value) = proof.receiver(receiver, *receiver_at, 0) else { return Some(result) };
        if !same_class(value.class, owner) {
            return Some(result);
        }
        if let Some(member) = proof.select_receiver(&value, name, args, args_at, 0)
            && !proof.exhausted
        {
            return Some(ResolveResult::one(ResolveStatus::Resolved, member_target(member.declaring, member.member)));
        }
        Some(result)
    }

    fn resolve_chain(&self, file: &FileRec, chain: &[Segment], call: bool, byte: u32) -> ResolveResult {
        if chain.is_empty() {
            return ResolveResult::none(ResolveStatus::Unresolved);
        }
        if chain[0].this {
            let mut current = self.member_of_enclosing(
                file,
                chain[0].start,
                &chain.get(1).map(|segment| segment.name.clone()).unwrap_or_default(),
                call && chain.len() == 2,
            );
            if chain.len() <= 2 {
                return current;
            }
            for (index, segment) in chain.iter().enumerate().skip(2) {
                current = self.member_from_base(file, &current, &segment.name, call && index + 1 == chain.len(), byte);
            }
            return current;
        }
        let mut current = self.simple_name(file, chain[0].start, &chain[0].name, false);
        if matches!(current.status, ResolveStatus::Unresolved | ResolveStatus::StaticImportNotApplied) {
            current = self.qualify(file, &[chain[0].name.clone()], false, byte);
        }
        let mut value_receiver = false;
        for (index, segment) in chain.iter().enumerate().skip(1) {
            value_receiver |= current.targets.iter().any(|target| {
                matches!(
                    target.kind,
                    TargetKind::Local | TargetKind::Parameter | TargetKind::Resource | TargetKind::Field
                )
            });
            let last = index + 1 == chain.len();
            let next = self.member_from_base(file, &current, &segment.name, call && last, byte);
            if !value_receiver && matches!(next.status, ResolveStatus::Unresolved | ResolveStatus::ReceiverNotBound) {
                let parts = chain[..=index].iter().map(|segment| segment.name.clone()).collect::<Vec<_>>();
                let qualified = self.qualify(file, &parts, call && last, byte);
                current = if qualified.status == ResolveStatus::Unresolved { next } else { qualified };
            } else {
                current = next;
            }
        }
        current
    }

    fn resolve_method_ref(&self, file: &FileRec, parsed: &ParsedFile, hit: Hit<'_>, byte: u32) -> ResolveResult {
        if token_kind_covering(hit.node, hit.start, byte) == Some(SyntaxKind::New) {
            return ResolveResult::none(ResolveStatus::ConstructorRefNotBound);
        }
        self.resolve_field(file, parsed, hit, byte)
    }

    fn member_of_enclosing(&self, file: &FileRec, byte: u32, name: &str, call: bool) -> ResolveResult {
        let Some(class) = enclosing_classes(file, byte).into_iter().next() else {
            return ResolveResult::none(ResolveStatus::Unresolved);
        };
        self.lookup_member(class, name, call, file, byte)
    }

    fn member_from_base(
        &self,
        file: &FileRec,
        base: &ResolveResult,
        name: &str,
        call: bool,
        byte: u32,
    ) -> ResolveResult {
        match base.status {
            // A competing lexical base is not a package prefix. Keep ambiguity
            // terminal across every following selector instead of qualifying a
            // spelling taken from an arbitrary first candidate.
            ResolveStatus::Ambiguous => ResolveResult::none(ResolveStatus::Ambiguous),
            ResolveStatus::Resolved if base.targets.len() == 1 => {
                let target = &base.targets[0];
                match target.kind {
                    TargetKind::Class => {
                        let Some(class) = self
                            .classes(file.scope)
                            .find(|class| class.dotted() == target.owner && class.path == target.path)
                        else {
                            return ResolveResult::none(ResolveStatus::Unresolved);
                        };
                        self.lookup_member(class, name, call, file, byte)
                    }
                    TargetKind::Local | TargetKind::Parameter | TargetKind::Resource | TargetKind::Field => {
                        let Some(declaration_file) = self.files.iter().find(|source| source.path == target.path) else {
                            return ResolveResult::none(ResolveStatus::ReceiverNotBound);
                        };
                        let Some(qual) = declared_of(declaration_file, target) else {
                            return ResolveResult::none(ResolveStatus::ReceiverNotBound);
                        };
                        if qual.array_rank != 0 {
                            return ResolveResult::none(ResolveStatus::ReceiverNotBound);
                        }
                        // Resolve the receiver type where it was declared, but check member access at the use site.
                        match self.lookup_type_name(declaration_file, &qual.parts, target.start_byte) {
                            TypeLookup::One(class) => self.lookup_member(class, name, call, file, byte),
                            TypeLookup::Many(_) => ResolveResult::none(ResolveStatus::Ambiguous),
                            TypeLookup::MissingImport | TypeLookup::None => {
                                ResolveResult::none(ResolveStatus::ReceiverNotBound)
                            }
                        }
                    }
                    TargetKind::Method => ResolveResult::none(ResolveStatus::ReceiverNotBound),
                }
            }
            ResolveStatus::Package => {
                let mut parts = Vec::new();
                if let Some(owner) = base.targets.first() {
                    parts.extend(owner.owner.split('.').map(str::to_string));
                }
                parts.push(name.to_string());
                self.qualify(file, &parts, call, byte)
            }
            _ => {
                let mut parts = Vec::new();
                if let Some(first) = base.targets.first() {
                    parts.push(first.name.clone());
                }
                parts.push(name.to_string());
                if parts.len() >= 2 {
                    self.qualify(file, &parts, call, byte)
                } else {
                    ResolveResult::none(ResolveStatus::ReceiverNotBound)
                }
            }
        }
    }

    fn qualify(&self, file: &FileRec, parts: &[String], call: bool, byte: u32) -> ResolveResult {
        if parts.is_empty() || local_type_shadows(file, &parts[0], byte) {
            return ResolveResult::none(ResolveStatus::Unresolved);
        }
        for length in (1..=parts.len()).rev() {
            let found = self.legacy_named(file.scope, &parts[..length].join("."));
            if found.is_empty() {
                continue;
            }
            if found.len() > 1 {
                return ResolveResult::many(
                    ResolveStatus::Ambiguous,
                    found.into_iter().map(ClassRec::target).collect(),
                );
            }
            let class = found[0];
            if length == parts.len() {
                return ResolveResult::one(ResolveStatus::Resolved, class.target());
            }
            if length + 1 == parts.len() {
                return self.lookup_member(class, &parts[length], call, file, byte);
            }
            let member = self.lookup_member(class, &parts[length], false, file, byte);
            if member.status != ResolveStatus::Resolved || member.targets.len() != 1 {
                return member;
            }
            let mut rest = member;
            for next in &parts[length + 1..] {
                rest = self.member_from_base(file, &rest, next, false, byte);
            }
            return rest;
        }
        if is_package_prefix(self.classes(file.scope), parts) {
            let dotted = parts.join(".");
            return ResolveResult::one(
                ResolveStatus::Package,
                Target {
                    binary: None,
                    path: String::new(),
                    kind: TargetKind::Class,
                    name: parts.last().cloned().unwrap_or_default(),
                    owner: dotted,
                    start_byte: 0,
                    end_byte: 0,
                },
            );
        }
        ResolveResult::none(ResolveStatus::Unresolved)
    }

    fn lookup_type_name<'a>(&'a self, file: &'a FileRec, parts: &[String], byte: u32) -> TypeLookup<'a> {
        if parts.first().is_some_and(|name| local_type_shadows(file, name, byte)) {
            return TypeLookup::MissingImport;
        }
        if parts.len() == 1 {
            return self.simple_type(file, &parts[0], byte);
        }
        if let Some(first) = parts.first()
            && let ambiguity @ TypeLookup::Many(_) = member_type_here(file, byte, first)
        {
            return ambiguity;
        }
        let found = self.legacy_named(file.scope, &parts.join("."));
        match found.len() {
            0 => TypeLookup::None,
            1 => TypeLookup::One(found[0]),
            _ => TypeLookup::Many(found),
        }
    }

    fn method_signature(&self, owner: &ClassRec, member: &Member) -> Option<MethodIdentity> {
        match &member.shape {
            Shape::Unknown => None,
            Shape::Binary { descriptor, generic, .. } => {
                let parameters = descriptor_parameters(descriptor)?;
                Some(MethodIdentity {
                    generic: *generic,
                    parameters: parameters
                        .into_iter()
                        .map(|parameter| {
                            if *generic {
                                return None;
                            }
                            let base = parameter.primitive.map(TypeBase::Primitive);
                            let base =
                                base.or_else(|| match self.class_named(owner.scope, &parameter.name.join(".")) {
                                    Named::One(class) => Some(TypeBase::Declared(ClassIdentity::of(class))),
                                    _ => None,
                                })?;
                            Some(SemanticType { base, array_rank: parameter.array_rank })
                        })
                        .collect(),
                })
            }
            Shape::Source { parameters, variables } => {
                let file = self.files.iter().find(|file| file.scope == owner.scope && file.path == owner.path)?;
                Some(MethodIdentity {
                    generic: !variables.is_empty(),
                    parameters: parameters
                        .iter()
                        .map(|parameter| {
                            if parameter.unsupported
                                || variables.contains(&parameter.name[0])
                                || enclosing_classes(file, member.start)
                                    .iter()
                                    .any(|class| class.type_variables.contains(&parameter.name[0]))
                            {
                                return None;
                            }
                            let base = parameter.primitive.map(TypeBase::Primitive);
                            let base = base.or_else(|| {
                                self.signature_type(file, &parameter.name, member.start)
                                    .map(|class| TypeBase::Declared(ClassIdentity::of(class)))
                            })?;
                            Some(SemanticType { base, array_rank: parameter.array_rank })
                        })
                        .collect(),
                })
            }
        }
    }

    // Resolve the leading type in its declaration scope, then member nesting.
    // Do not reinterpret a missing explicit import as a fully qualified name.
    fn signature_type<'a>(&'a self, file: &'a FileRec, parts: &[String], byte: u32) -> Option<&'a ClassRec> {
        let first = parts.first()?;
        if local_type_shadows(file, first, byte) {
            return None;
        }
        for outer in enclosing_classes(file, byte) {
            let nested: Vec<_> = file
                .classes
                .iter()
                .filter(|inner| {
                    inner.nesting.len() == outer.nesting.len() + 1
                        && inner.nesting[..outer.nesting.len()] == outer.nesting[..]
                        && inner.simple() == first
                })
                .collect();
            if !nested.is_empty() {
                return if nested.len() == 1 {
                    self.signature_nested(file.scope, nested[0], &parts[1..])
                } else {
                    None
                };
            }
            if outer.simple() == first {
                return self.signature_nested(file.scope, outer, &parts[1..]);
            }
            // Inherited member-type lookup is not implemented. Its presence
            // blocks fallback to a same-spelled package/import type.
            let inherited = self.hierarchy(outer).ok()?;
            if inherited.iter().skip(1).any(|step| {
                self.classes(step.class.scope).any(|inner| {
                    inner.package == step.class.package
                        && inner.nesting.len() == step.class.nesting.len() + 1
                        && inner.nesting[..step.class.nesting.len()] == step.class.nesting[..]
                        && inner.simple() == first
                })
            }) {
                return None;
            }
        }
        let imports: BTreeSet<_> =
            file.singles.iter().filter(|(simple, _)| simple == first).map(|(_, full)| full).collect();
        if imports.len() > 1 {
            return None;
        }
        match self.simple_type(file, first, byte) {
            TypeLookup::One(class) => self.signature_nested(file.scope, class, &parts[1..]),
            TypeLookup::None if parts.len() > 1 => match self.class_named(file.scope, &parts.join(".")) {
                Named::One(class) => Some(class),
                _ => None,
            },
            _ => None,
        }
    }
    fn signature_nested<'a>(&'a self, scope: usize, class: &'a ClassRec, rest: &[String]) -> Option<&'a ClassRec> {
        if rest.is_empty() {
            return Some(class);
        }
        match self.class_named(scope, &format!("{}.{}", class.dotted(), rest.join("."))) {
            Named::One(class) => Some(class),
            _ => None,
        }
    }

    fn lookup_member(&self, class: &ClassRec, name: &str, call: bool, site: &FileRec, byte: u32) -> ResolveResult {
        if call { self.lookup_methods(class, name, site, byte) } else { self.lookup_fields(class, name, site, byte) }
    }

    fn lookup_fields(&self, class: &ClassRec, name: &str, site: &FileRec, byte: u32) -> ResolveResult {
        let members = match self.field_members(class, name, &mut BTreeSet::new(), &mut BTreeMap::new()) {
            Ok(members) => members,
            Err(err) => return walk_error(err),
        };
        let mut targets = Vec::new();
        let mut uncertain = false;
        for field in members {
            match self.can_see(field.member, field.owner, class, site, byte) {
                Ok(true) => {
                    match self.protected_field_receiver(field.member, field.owner, class, site, byte) {
                        Ok(false) => continue,
                        Ok(true) => {}
                        Err(err) => return walk_error(err),
                    }
                    uncertain |= field.member.access == Access::Unspecified;
                    targets.push(member_target(field.owner, field.member));
                }
                Ok(false) => {}
                Err(err) => return walk_error(err),
            }
        }
        let status = if uncertain {
            ResolveStatus::NotDetermined
        } else {
            match targets.len() {
                0 => ResolveStatus::Unresolved,
                1 => ResolveStatus::Resolved,
                _ => ResolveStatus::Ambiguous,
            }
        };
        ResolveResult::many(status, targets)
    }

    // Additional negative guard for ordinary field navigation only. Unknown
    // declaration facts or incomplete ancestry must retain competitors, not
    // manufacture a unique target. This is not strict field type evidence.
    fn protected_field_receiver(
        &self,
        member: &Member,
        declaring: &ClassRec,
        receiver: &ClassRec,
        site: &FileRec,
        byte: u32,
    ) -> Result<bool, WalkError> {
        if member.access != Access::Protected || site.package == declaring.package {
            return Ok(true);
        }
        let is_static = if let Shape::Binary { flags, .. } = member.shape {
            // Known legal field modifiers only; contradictory/unknown flags
            // cannot authorize removal of a navigation candidate.
            (flags & 7 == 4 && flags & !0x10dc == 0 && flags & 0x50 != 0x50).then_some(flags & 8 != 0)
        } else {
            member.source_field.as_ref().map(|field| field.is_static)
        };
        if is_static != Some(false) {
            return Ok(true);
        }
        let (receiver_graph, receiver_complete) = self.hierarchy_complete(receiver)?;
        let mut unknown = false;
        for caller in enclosing_classes(site, byte) {
            let (caller_graph, caller_complete) = self.hierarchy_complete(caller)?;
            let subclass = caller_graph.iter().any(|s| same_class(s.class, declaring));
            let qualifying = receiver_graph.iter().any(|s| same_class(s.class, caller));
            if subclass && qualifying {
                return Ok(true);
            }
            // Three-valued AND for this enclosing class, then existential OR:
            // one proven No defeats this pair; otherwise retain an unknown pair.
            if (!receiver_complete || qualifying) && (!caller_complete || subclass) {
                unknown = true;
            }
        }
        Ok(unknown)
    }

    // JLS 8.3 / 9.3: inheritance is a direct-edge operation, not a nearest
    // declaration or global ancestry ranking. A hidden field may still arrive
    // through another parent. Cache member sets before caller access checks.
    fn field_members<'a>(
        &'a self,
        class: &'a ClassRec,
        name: &str,
        active: &mut BTreeSet<ClassIdentity>,
        memo: &mut BTreeMap<ClassIdentity, Vec<FieldMember<'a>>>,
    ) -> Result<Vec<FieldMember<'a>>, WalkError> {
        let identity = ClassIdentity::of(class);
        if let Some(members) = memo.get(&identity) {
            return Ok(members.clone());
        }
        if !active.insert(identity.clone()) {
            return Err(WalkError::Cycle);
        }
        let mut inherited = BTreeMap::new();
        // Interfaces do not inherit Object fields, even in class files whose
        // super_class entry is Object. Traverse all real parents even when an
        // own declaration exists, preserving cycle/ambiguous-edge reporting.
        let edges = class.super_parts.iter().filter(|_| !class.is_interface).chain(&class.interface_parts);
        for edge in edges {
            let parent = match self.resolve_edge(class, edge) {
                Named::None => continue,
                Named::Many(classes) => {
                    return Err(WalkError::Ambiguous(classes.into_iter().map(ClassRec::target).collect()));
                }
                Named::One(parent) => parent,
            };
            for field in self.field_members(parent, name, active, memo)? {
                let eligible = match field.member.access {
                    Access::Public | Access::Protected | Access::Unspecified => true,
                    Access::Package => field.owner.package == class.package,
                    Access::Private => false,
                };
                if eligible {
                    inherited.insert((ClassIdentity::of(field.owner), field.ordinal), field);
                }
            }
        }
        let own: Vec<_> = class
            .fields
            .iter()
            .enumerate()
            .filter(|(_, member)| member.name == name)
            .map(|(ordinal, member)| FieldMember { owner: class, member, ordinal })
            .collect();
        let members = if own.is_empty() { inherited.into_values().collect() } else { own };
        active.remove(&identity);
        memo.insert(identity, members.clone());
        Ok(members)
    }

    fn lookup_methods(&self, class: &ClassRec, name: &str, site: &FileRec, byte: u32) -> ResolveResult {
        let found = match self.collect(class, name, site, byte) {
            Ok(found) => found,
            Err(err) => return walk_error(err),
        };
        select_methods(found)
    }

    fn collect<'a>(
        &'a self,
        class: &'a ClassRec,
        name: &str,
        site: &FileRec,
        byte: u32,
    ) -> Result<Vec<Candidate<'a>>, WalkError> {
        let steps = self.hierarchy(class)?;
        let mut found = Vec::new();
        for step in steps {
            let members = step.class.methods.iter().filter(|member| member.name == name);
            for member in members {
                if step.depth > 0 && !navigation_method_inherited(step.class, member) {
                    continue;
                }
                if !self.can_see(member, step.class, class, site, byte)? {
                    continue;
                }
                found.push(Candidate {
                    member,
                    target: member_target(step.class, member),
                    signature: self.method_signature(step.class, member),
                    declaring: ClassIdentity::of(step.class),
                    ancestors: self
                        .method_hierarchy(step.class)?
                        .into_iter()
                        .skip(1)
                        .map(|s| ClassIdentity::of(s.class))
                        .collect(),
                    unspecified: member.access == Access::Unspecified,
                });
            }
        }
        Ok(found)
    }

    fn hierarchy<'a>(&'a self, start: &'a ClassRec) -> Result<Vec<Step<'a>>, WalkError> {
        self.hierarchy_complete(start).map(|(steps, _)| steps)
    }

    fn hierarchy_complete<'a>(&'a self, start: &'a ClassRec) -> Result<(Vec<Step<'a>>, bool), WalkError> {
        let mut out = Vec::new();
        let mut gray = Vec::new();
        let mut black = BTreeSet::new();
        let complete = self.walk(start, 0, false, &mut gray, &mut black, &mut out)?;
        Ok((out, complete))
    }

    fn method_hierarchy<'a>(&'a self, start: &'a ClassRec) -> Result<Vec<Step<'a>>, WalkError> {
        let mut out = Vec::new();
        self.walk(start, 0, true, &mut Vec::new(), &mut BTreeSet::new(), &mut out)?;
        Ok(out)
    }

    fn walk<'a>(
        &'a self,
        class: &'a ClassRec,
        depth: u32,
        method_inheritance: bool,
        gray: &mut Vec<(usize, String, String)>,
        black: &mut BTreeSet<(usize, String, String)>,
        out: &mut Vec<Step<'a>>,
    ) -> Result<bool, WalkError> {
        let name = (class.scope, class.path.clone(), class.dotted());
        if gray.iter().any(|seen| seen == &name) {
            return Err(WalkError::Cycle);
        }
        if !black.insert(name.clone()) {
            return Ok(true);
        }
        gray.push(name);
        out.push(Step { class, depth });
        let mut edges = Vec::new();
        if let Some(super_parts) = &class.super_parts
            && !(method_inheritance && class.is_interface)
        {
            edges.push(super_parts.clone());
        }
        edges.extend(class.interface_parts.iter().cloned());
        let mut complete = true;
        for edge in edges {
            match self.resolve_edge(class, &edge) {
                Named::None => complete = false,
                Named::Many(classes) => {
                    return Err(WalkError::Ambiguous(classes.into_iter().map(ClassRec::target).collect()));
                }
                Named::One(next) => {
                    complete &= self.walk(next, depth + 1, method_inheritance, gray, black, out)?;
                }
            }
        }
        gray.pop();
        Ok(complete)
    }

    fn can_see(
        &self,
        member: &Member,
        declaring: &ClassRec,
        start: &ClassRec,
        site: &FileRec,
        byte: u32,
    ) -> Result<bool, WalkError> {
        if member.source_enum_constant && !self.enum_owner_accessible(declaring, site, byte) {
            return Ok(false);
        }
        match member.access {
            Access::Public | Access::Unspecified => Ok(true),
            Access::Package => Ok(site.package == declaring.package),
            Access::Private => Ok(declaring.dotted() == start.dotted()
                && declaring.path == start.path
                && enclosing_classes(site, byte).iter().any(|class| same_class(class, declaring))),
            Access::Protected => {
                if site.package == declaring.package
                    || enclosing_classes(site, byte).iter().any(|class| same_class(class, start))
                {
                    return Ok(true);
                }
                for class in enclosing_classes(site, byte) {
                    let steps = self.hierarchy(class)?;
                    if steps.iter().any(|step| same_class(step.class, declaring)) {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }

    // The new implicit public fields must not bypass inaccessible enclosing
    // source types. This is deliberately scoped to enum constants: legacy
    // member lookup and general protected nested-type rules are unchanged.
    fn enum_owner_accessible(&self, owner: &ClassRec, site: &FileRec, byte: u32) -> bool {
        let Some(file) = self.files.iter().find(|f| f.scope == owner.scope && f.path == owner.path) else {
            return false;
        };
        let mut parent_interface = false;
        for depth in 1..=owner.nesting.len() {
            let mut matches = file.classes.iter().filter(|c| c.nesting == owner.nesting[..depth]);
            let Some(class) = matches.next() else { return false };
            if matches.next().is_some() {
                return false;
            }
            let allowed = match class.access {
                Access::Public => true,
                Access::Package => parent_interface || class.package == site.package,
                Access::Protected => class.package == site.package,
                Access::Private => {
                    site.scope == owner.scope
                        && site.path == owner.path
                        && enclosing_classes(site, byte)
                            .iter()
                            .any(|caller| caller.nesting.first() == owner.nesting.first())
                }
                Access::Unspecified => false,
            };
            if !allowed {
                return false;
            }
            parent_interface = class.is_interface;
        }
        true
    }

    fn resolve_edge<'a>(&'a self, class: &ClassRec, parts: &[String]) -> Named<'a> {
        if class.binary {
            return self.class_named(class.scope, &parts.join("."));
        }
        let Some(file) = self.files.iter().find(|file| file.scope == class.scope && file.path == class.path) else {
            return Named::None;
        };
        match self.lookup_type_name(file, parts, class.name_start) {
            TypeLookup::One(found) => Named::One(found),
            TypeLookup::Many(found) => Named::Many(found),
            TypeLookup::None | TypeLookup::MissingImport => Named::None,
        }
    }

    fn class_named<'a>(&'a self, scope: usize, dotted: &str) -> Named<'a> {
        let found = self.legacy_named(scope, dotted);
        match found.len() {
            0 => Named::None,
            1 => Named::One(found[0]),
            _ => Named::Many(found),
        }
    }
}

enum Named<'a> {
    None,
    One(&'a ClassRec),
    Many(Vec<&'a ClassRec>),
}

fn walk_error(err: WalkError) -> ResolveResult {
    match err {
        WalkError::Cycle => ResolveResult::none(ResolveStatus::Cycle),
        WalkError::Ambiguous(targets) => ResolveResult::many(ResolveStatus::Ambiguous, targets),
    }
}

fn select_methods(found: Vec<Candidate<'_>>) -> ResolveResult {
    if found.is_empty() {
        return ResolveResult::none(ResolveStatus::Unresolved);
    }
    if found.iter().all(|candidate| candidate.unspecified) {
        return ResolveResult::many(
            ResolveStatus::NotDetermined,
            found.into_iter().map(|candidate| candidate.target).collect(),
        );
    }
    let found: Vec<_> = found.into_iter().filter(|candidate| !candidate.unspecified).collect();
    if found.iter().any(|candidate| candidate.signature.is_none()) {
        return ResolveResult::many(
            ResolveStatus::SignatureNotCompared,
            found.into_iter().map(|candidate| candidate.target).collect(),
        );
    }
    // A unique name is navigation, not proof of applicability or a complete type.
    // Across declarations, only proven equality permits hiding; unknown types
    // can still prove distinctness via arity or a different concrete parameter.
    let mut hidden = vec![false; found.len()];
    let mut different = false;
    for (i, left) in found.iter().enumerate() {
        for (j, right) in found.iter().enumerate().skip(i + 1) {
            match compare_signatures(left.signature.as_ref().unwrap(), right.signature.as_ref().unwrap()) {
                Comparison::Unknown => {
                    return ResolveResult::many(
                        ResolveStatus::SignatureNotCompared,
                        found.into_iter().map(|candidate| candidate.target).collect(),
                    );
                }
                Comparison::Different => different = true,
                Comparison::Equal => {
                    // Traversal distance is not an override relation: unrelated
                    // interfaces can be reached at different depths.
                    if left.ancestors.contains(&right.declaring) {
                        hidden[j] = true;
                    }
                    if right.ancestors.contains(&left.declaring) {
                        hidden[i] = true;
                    }
                }
            }
        }
    }
    let kept: Vec<_> = found.into_iter().zip(hidden).filter_map(|(c, hidden)| (!hidden).then_some(c.target)).collect();
    let status = if kept.len() == 1 {
        ResolveStatus::Resolved
    } else if different {
        ResolveStatus::OverloadNotResolved
    } else {
        ResolveStatus::Ambiguous
    };
    ResolveResult::many(status, kept)
}

enum TypeLookup<'a> {
    One(&'a ClassRec),
    Many(Vec<&'a ClassRec>),
    None,
    MissingImport,
}

// Snapshot-local declaration identity, never a persistent/public symbol ID.
// Scope distinguishes identical binary inputs linked against different classpaths.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ClassIdentity {
    scope: usize,
    path: String,
    name: String,
    start: u32,
    end: u32,
}
impl ClassIdentity {
    fn of(class: &ClassRec) -> Self {
        Self {
            scope: class.scope,
            path: class.path.clone(),
            name: class.dotted(),
            start: class.name_start,
            end: class.name_end,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Primitive {
    Boolean,
    Byte,
    Char,
    Short,
    Int,
    Long,
    Float,
    Double,
}
impl Primitive {
    fn source(name: &str) -> Option<Self> {
        Some(match name {
            "boolean" => Self::Boolean,
            "byte" => Self::Byte,
            "char" => Self::Char,
            "short" => Self::Short,
            "int" => Self::Int,
            "long" => Self::Long,
            "float" => Self::Float,
            "double" => Self::Double,
            _ => return None,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum TypeBase {
    Primitive(Primitive),
    Declared(ClassIdentity),
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct SemanticType {
    base: TypeBase,
    array_rank: usize,
}
#[derive(Clone, Debug)]
struct ParameterType {
    binary_name: Option<String>,
    name: Vec<String>,
    primitive: Option<Primitive>,
    array_rank: usize,
    unsupported: bool,
}
#[derive(Clone, Debug)]
enum Shape {
    Unknown,
    Source { parameters: Vec<ParameterType>, variables: Vec<String> },
    Binary { descriptor: String, generic: bool, flags: u16, signature: Option<String> },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Comparison {
    Equal,
    Different,
    Unknown,
}
#[derive(Clone)]
struct MethodIdentity {
    parameters: Vec<Option<SemanticType>>,
    generic: bool,
}
fn compare_signatures(left: &MethodIdentity, right: &MethodIdentity) -> Comparison {
    if left.parameters.len() != right.parameters.len() {
        return Comparison::Different;
    }
    let mut unknown = left.generic || right.generic;
    for (left, right) in left.parameters.iter().zip(&right.parameters) {
        match (left, right) {
            (Some(left), Some(right)) if left != right => return Comparison::Different,
            (Some(_), Some(_)) => {}
            _ => unknown = true,
        }
    }
    if unknown { Comparison::Unknown } else { Comparison::Equal }
}

fn same_class(left: &ClassRec, right: &ClassRec) -> bool {
    ClassIdentity::of(left) == ClassIdentity::of(right)
}

#[derive(Clone, Copy)]
struct FieldMember<'a> {
    owner: &'a ClassRec,
    member: &'a Member,
    // Snapshot-local member ordinal keeps malformed duplicate declarations
    // distinct while coalescing the same field reached along a diamond.
    ordinal: usize,
}

#[derive(Clone)]
struct Candidate<'a> {
    member: &'a Member,
    target: Target,
    signature: Option<MethodIdentity>,
    declaring: ClassIdentity,
    ancestors: BTreeSet<ClassIdentity>,
    unspecified: bool,
}

enum WalkError {
    Cycle,
    Ambiguous(Vec<Target>),
}

struct Step<'a> {
    class: &'a ClassRec,
    depth: u32,
}

fn member_target(class: &ClassRec, member: &Member) -> Target {
    Target {
        binary: if class.binary && member.kind == TargetKind::Method {
            if let Shape::Binary { descriptor, .. } = &member.shape {
                class.binary_signature.as_ref().map(|sig| BinaryTargetIdentity {
                    binary_name: sig.binary_name.clone(),
                    descriptor: descriptor.clone(),
                    linking_context: class.context.clone(),
                })
            } else {
                None
            }
        } else {
            None
        },
        path: class.path.clone(),
        kind: member.kind,
        name: member.name.clone(),
        owner: class.dotted(),
        start_byte: member.start,
        end_byte: member.end,
    }
}

fn local_resolution<'a>(locals: impl Iterator<Item = &'a LocalRec>, byte: u32, name: &str) -> Option<ResolveResult> {
    let locals: Vec<&LocalRec> = locals
        .filter(|local| {
            local.name == name && local.scope_start <= byte && byte < local.scope_end && local.visible_from <= byte
        })
        .collect();
    if locals.is_empty() {
        return None;
    }
    let min_span = locals.iter().map(|local| local.scope_end - local.scope_start).min().unwrap();
    let targets = locals
        .into_iter()
        .filter(|local| local.scope_end - local.scope_start == min_span)
        .map(LocalRec::target)
        .collect::<Vec<_>>();
    let status = if targets.len() == 1 { ResolveStatus::Resolved } else { ResolveStatus::Ambiguous };
    Some(ResolveResult::many(status, targets))
}

fn declared_of<'a>(file: &'a FileRec, target: &Target) -> Option<&'a QualName> {
    if file.path != target.path {
        return None;
    }
    match target.kind {
        TargetKind::Local | TargetKind::Parameter | TargetKind::Resource => file
            .locals
            .iter()
            .find(|local| {
                local.kind == target.kind
                    && local.start == target.start_byte
                    && local.end == target.end_byte
                    && local.name == target.name
            })
            .and_then(|local| local.declared.as_ref()),
        TargetKind::Field => file
            .classes
            .iter()
            .find(|class| class.dotted() == target.owner)
            .and_then(|class| {
                class.fields.iter().find(|field| {
                    field.start == target.start_byte && field.end == target.end_byte && field.name == target.name
                })
            })
            .and_then(|field| field.declared.as_ref()),
        TargetKind::Class | TargetKind::Method => None,
    }
}

fn enclosing_classes(file: &FileRec, byte: u32) -> Vec<&ClassRec> {
    let mut classes: Vec<_> =
        file.classes.iter().filter(|class| class.range_start <= byte && byte < class.range_end).collect();
    classes.sort_by_key(|class| class.range_end - class.range_start);
    classes
}

fn member_type_here<'a>(file: &'a FileRec, byte: u32, name: &str) -> TypeLookup<'a> {
    for class in enclosing_classes(file, byte) {
        let members: Vec<_> = file
            .classes
            .iter()
            .filter(|inner| {
                inner.nesting.len() == class.nesting.len() + 1
                    && inner.nesting[..class.nesting.len()] == class.nesting[..]
                    && inner.simple() == name
            })
            .collect();
        match members.len() {
            0 => {}
            1 => return TypeLookup::One(members[0]),
            _ => return TypeLookup::Many(members),
        }
    }
    TypeLookup::None
}

fn is_package_prefix<'a>(classes: impl Iterator<Item = &'a ClassRec>, parts: &[String]) -> bool {
    let prefix = parts.join(".");
    classes.into_iter().any(|class| {
        class.package == prefix
            || class
                .package
                .split('.')
                .collect::<Vec<_>>()
                .starts_with(&parts.iter().map(String::as_str).collect::<Vec<_>>())
    })
}

struct Hit<'a> {
    node: &'a GreenNode,
    start: u32,
    call: bool,
    arguments: Option<(&'a GreenNode, u32)>,
}

fn innermost_ref(node: &GreenNode, start: u32, byte: u32, called: bool) -> Option<Hit<'_>> {
    let end = start + node.utf8_len();
    if node.kind() == SyntaxKind::SwitchLabel || byte < start || byte >= end {
        return None;
    }
    let mut child_at = start;
    let mut saw_node = false;
    for child in node.children() {
        if let Some(child_node) = child.as_node() {
            let child_called = node.kind() == SyntaxKind::CallExpr && !saw_node;
            saw_node = true;
            if let Some(mut hit) = innermost_ref(child_node, child_at, byte, child_called) {
                if child_called && hit.call && std::ptr::eq(hit.node, child_node.as_ref()) {
                    hit.arguments = child_kind(node, start, SyntaxKind::ArgList);
                }
                return Some(hit);
            }
        }
        child_at += child.utf8_len();
    }
    if matches!(
        node.kind(),
        SyntaxKind::ReferenceExpr | SyntaxKind::Name | SyntaxKind::FieldExpr | SyntaxKind::MethodRefExpr
    ) {
        Some(Hit { node, start, call: called, arguments: None })
    } else {
        None
    }
}

struct Segment {
    name: String,
    start: u32,
    end: u32,
    this: bool,
}

// Follow only the receiver/callee spine, never call arguments or index operands.
// Qualified `Outer.super` is a FieldExpr with a Super token, whereas plain
// `super` is a SuperExpr. Inspect syntax, not source text (comments may say super).
fn has_super_receiver(node: &GreenNode) -> bool {
    match node.kind() {
        SyntaxKind::SuperExpr => true,
        SyntaxKind::FieldExpr
        | SyntaxKind::MethodRefExpr
        | SyntaxKind::CallExpr
        | SyntaxKind::IndexExpr
        | SyntaxKind::ParenExpr => {
            (node.kind() == SyntaxKind::FieldExpr
                && node
                    .children()
                    .iter()
                    .any(|child| child.as_token().is_some_and(|token| token.kind() == SyntaxKind::Super)))
                || node.children().iter().find_map(|child| child.as_node()).is_some_and(|base| has_super_receiver(base))
        }
        _ => false,
    }
}

// Only inspect the receiver/callee spine, not arguments or index operands.
fn has_class_literal_receiver(node: &GreenNode) -> bool {
    match node.kind() {
        SyntaxKind::ClassLiteralExpr => true,
        SyntaxKind::FieldExpr
        | SyntaxKind::MethodRefExpr
        | SyntaxKind::CallExpr
        | SyntaxKind::IndexExpr
        | SyntaxKind::ParenExpr => node.child_nodes().next().is_some_and(|base| has_class_literal_receiver(base)),
        // A cast starts with its Type child; the operand is the last node.
        SyntaxKind::CastExpr => node.child_nodes().last().is_some_and(|operand| has_class_literal_receiver(operand)),
        SyntaxKind::BinaryExpr => {
            let operator = node
                .children()
                .iter()
                .filter_map(|child| child.as_token())
                .find(|token| !token.kind().is_trivia())
                .map(|token| token.kind());
            match operator {
                // The condition and assignment target do not determine the result.
                Some(SyntaxKind::Question) => {
                    node.child_nodes().skip(1).any(|branch| has_class_literal_receiver(branch))
                }
                Some(SyntaxKind::Eq) => {
                    node.child_nodes().nth(1).is_some_and(|value| has_class_literal_receiver(value))
                }
                _ => false,
            }
        }
        _ => false,
    }
}

fn has_call_result_receiver(node: &GreenNode) -> bool {
    match node.kind() {
        SyntaxKind::CallExpr => true,
        SyntaxKind::FieldExpr
        | SyntaxKind::MethodRefExpr
        | SyntaxKind::IndexExpr
        | SyntaxKind::ParenExpr
        | SyntaxKind::UnaryExpr => node.child_nodes().next().is_some_and(|base| has_call_result_receiver(base)),
        // A cast starts with its Type child; the operand is the last node.
        SyntaxKind::CastExpr => node.child_nodes().last().is_some_and(|operand| has_call_result_receiver(operand)),
        SyntaxKind::BinaryExpr => {
            let operator = node
                .children()
                .iter()
                .filter_map(|child| child.as_token())
                .find(|token| !token.kind().is_trivia())
                .map(|token| token.kind());
            match operator {
                // Ignore the condition and plain assignment target.
                Some(SyntaxKind::Question) => node.child_nodes().skip(1).any(|branch| has_call_result_receiver(branch)),
                Some(SyntaxKind::Eq) => node.child_nodes().nth(1).is_some_and(|value| has_call_result_receiver(value)),
                // Other binary results, including compound assignments, depend
                // on their operands. This is refusal provenance, not typing.
                _ => node.child_nodes().any(|operand| has_call_result_receiver(operand)),
            }
        }
        _ => false,
    }
}

fn field_chain(node: &GreenNode, start: u32) -> Vec<Segment> {
    if node.kind() == SyntaxKind::ThisExpr {
        return vec![Segment { name: "this".into(), start, end: start + node.utf8_len(), this: true }];
    }
    if node.kind() == SyntaxKind::ReferenceExpr {
        let mut offset = start;
        for child in node.children() {
            if let Some(token) = child.as_token()
                && token.kind() == SyntaxKind::Ident
            {
                return vec![Segment {
                    name: token.text().to_string(),
                    start: offset,
                    end: offset + token.utf8_len(),
                    this: false,
                }];
            }
            offset += child.utf8_len();
        }
    }
    if matches!(node.kind(), SyntaxKind::FieldExpr | SyntaxKind::MethodRefExpr) {
        let mut offset = start;
        let mut base = None;
        let mut selector = None;
        for child in node.children() {
            if base.is_none()
                && let Some(child_node) = child.as_node()
            {
                base = Some(field_chain(child_node, offset));
            }
            if let Some(token) = child.as_token()
                && token.kind() == SyntaxKind::Ident
            {
                selector = Some(Segment {
                    name: token.text().to_string(),
                    start: offset,
                    end: offset + token.utf8_len(),
                    this: false,
                });
            }
            offset += child.utf8_len();
        }
        let mut chain = base.unwrap_or_default();
        if let Some(selector) = selector {
            chain.push(selector);
        }
        return chain;
    }
    Vec::new()
}

fn token_kind_covering(node: &GreenNode, start: u32, byte: u32) -> Option<SyntaxKind> {
    let mut offset = start;
    for child in node.children() {
        let end = offset + child.utf8_len();
        if let Some(token) = child.as_token()
            && offset <= byte
            && byte < end
        {
            return Some(token.kind());
        }
        offset = end;
    }
    None
}

fn token_name(node: &GreenNode) -> String {
    node.children()
        .iter()
        .find_map(|child| {
            child.as_token().filter(|token| token.kind() == SyntaxKind::Ident).map(|token| token.text().to_string())
        })
        .unwrap_or_default()
}

fn declaration_at(file: &FileRec, byte: u32) -> Option<Target> {
    if let Some(local) = file.locals.iter().find(|local| local.start <= byte && byte < local.end) {
        return Some(local.target());
    }
    for class in &file.classes {
        if class.name_start <= byte && byte < class.name_end {
            return Some(class.target());
        }
        for member in class.fields.iter().chain(&class.methods).chain(&class.constructors) {
            if member.start <= byte && byte < member.end {
                return Some(member_target(class, member));
            }
        }
    }
    None
}

impl LocalRec {
    fn target(&self) -> Target {
        Target {
            binary: None,
            path: self.path.clone(),
            kind: self.kind,
            name: self.name.clone(),
            owner: String::new(),
            start_byte: self.start,
            end_byte: self.end,
        }
    }
}

fn index_file(path: &str, parsed: &ParsedFile) -> FileRec {
    let syntax_valid = parsed.errors().is_empty();
    let mut file = FileRec {
        scope: 0,
        path: path.to_string(),
        package: String::new(),
        singles: Vec::new(),
        stars: Vec::new(),
        static_names: Vec::new(),
        classes: Vec::new(),
        locals: Vec::new(),
        local_types: Vec::new(),
        opaque_enum_bodies: Vec::new(),
        syntax_valid,
        call_inventory_valid: syntax_valid && valid_call_inventory(parsed.root(), 0),
    };
    index_node(
        parsed.root(),
        0,
        &mut IndexCtx { file: &mut file, class_stack: Vec::new(), scope_stack: Vec::new(), enum_constant_depth: 0 },
    );
    file
}

struct IndexCtx<'a> {
    file: &'a mut FileRec,
    class_stack: Vec<usize>,
    scope_stack: Vec<(u32, u32, ScopeKind)>,
    enum_constant_depth: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    Other,
    Method,
    Lambda,
    Catch,
    Try,
}

fn index_node(node: &GreenNode, start: u32, ctx: &mut IndexCtx<'_>) {
    // Invalid direct labeled declarations and bounded-chain recovery cannot
    // introduce variables into the enclosing block. Valid labels add no scope.
    if node.kind() == SyntaxKind::LabeledStmt && node.message().is_some() {
        return;
    }
    // Include anonymous bodies nested in constructor arguments, without
    // changing general NewExpr behavior outside enum constants.
    if ctx.enum_constant_depth > 0 && node.kind() == SyntaxKind::ClassBody {
        ctx.file.opaque_enum_bodies.push((start, start + node.utf8_len()));
        return;
    }
    match node.kind() {
        // Labels have no modeled enum/constant semantics or declarations.
        SyntaxKind::SwitchLabel => return,
        SyntaxKind::EnumConstant => {
            index_enum_constant(node, start, ctx);
            return;
        }
        SyntaxKind::LocalType => {
            let mut header =
                node.children().iter().filter_map(|child| child.as_token()).filter(|token| !token.kind().is_trivia());
            header.next(); // declaration keyword; annotations/modifiers are child nodes
            let name =
                header.next().filter(|token| token.kind() == SyntaxKind::Ident).map(|token| token.text().to_string());
            ctx.file.local_types.push(LocalTypeRec {
                name,
                start,
                end: start + node.utf8_len(),
                scope_end: ctx.scope_stack.last().map_or(start + node.utf8_len(), |scope| scope.1),
            });
            return;
        }
        SyntaxKind::PackageDecl => {
            if let Some(name) = child_kind(node, start, SyntaxKind::Name) {
                ctx.file.package = name_strings(name.0, name.1).join(".");
            }
        }
        SyntaxKind::ImportDecl => index_import(node, start, ctx),
        SyntaxKind::Class | SyntaxKind::Interface | SyntaxKind::Enum | SyntaxKind::Record => {
            index_class(node, start, ctx);
            return;
        }
        SyntaxKind::Field if !ctx.class_stack.is_empty() => {
            let class_index = *ctx.class_stack.last().unwrap();
            index_members(node, start, ctx, class_index, TargetKind::Field);
            return;
        }
        SyntaxKind::Method | SyntaxKind::Constructor if !ctx.class_stack.is_empty() => {
            let class_index = *ctx.class_stack.last().unwrap();
            index_members(node, start, ctx, class_index, TargetKind::Method);
            return;
        }
        _ => {}
    }
    let pushed = scope_of(node, start);
    if let Some(scope) = pushed {
        ctx.scope_stack.push(scope);
    }
    if matches!(node.kind(), SyntaxKind::LocalVar | SyntaxKind::Resource) {
        index_local(
            node,
            start,
            ctx,
            if node.kind() == SyntaxKind::Resource { TargetKind::Resource } else { TargetKind::Local },
        );
    }
    if node.kind() == SyntaxKind::Parameter {
        index_parameter(node, start, ctx);
    }
    if node.kind() == SyntaxKind::CatchClause {
        index_catch(node, start, ctx);
    }
    let mut offset = start;
    for child in node.children() {
        if let Some(child) = child.as_node() {
            index_node(child, offset, ctx);
        }
        offset += child.utf8_len();
    }
    if pushed.is_some() {
        ctx.scope_stack.pop();
    }
}

fn index_import(node: &GreenNode, start: u32, ctx: &mut IndexCtx<'_>) {
    let is_static = tokens(node, start).iter().any(|token| token.0 == SyntaxKind::Static);
    let Some((name_node, name_at)) = child_kind(node, start, SyntaxKind::Name) else { return };
    let (parts, star) = name_pieces(name_node, name_at);
    if parts.is_empty() {
        return;
    }
    if is_static {
        if let Some((simple, _, _)) = parts.last() {
            ctx.file.static_names.push(simple.clone());
        }
        return;
    }
    if star {
        let package = parts.iter().map(|(name, _, _)| name.clone()).collect::<Vec<_>>().join(".");
        ctx.file.stars.push(package);
    } else if let Some((simple, _, _)) = parts.last() {
        let full = parts.iter().map(|(name, _, _)| name.clone()).collect::<Vec<_>>().join(".");
        ctx.file.singles.push((simple.clone(), full));
    }
}

fn index_class(node: &GreenNode, start: u32, ctx: &mut IndexCtx<'_>) {
    let Some((name, name_start, name_end)) = direct_ident(node, start) else {
        return;
    };
    let mut nesting =
        ctx.class_stack.iter().map(|index| ctx.file.classes[*index].simple().to_string()).collect::<Vec<_>>();
    nesting.push(name);
    let mut data = ClassPayload {
        path: ctx.file.path.clone(),
        package: ctx.file.package.clone(),
        nesting,
        name_start,
        name_end,
        range_start: start,
        range_end: start + node.utf8_len(),
        super_parts: None,
        interface_parts: Vec::new(),
        binary: false,
        binary_signature: None,
        is_interface: node.kind() == SyntaxKind::Interface,
        type_variables: type_variables(node, start),
        primitive_owner: node.kind() == SyntaxKind::Class
            && ctx.class_stack.is_empty()
            && child_kind(node, start, SyntaxKind::TypeParamList).is_none()
            && !tokens(node, start).iter().any(|t| matches!(t.0, SyntaxKind::Extends | SyntaxKind::Implements))
            && valid_modifiers(node, start, false),
        access: read_access(node, start),
        reference_inventory: reference_class_inventory(node, start),
        reference_generic: child_kind(node, start, SyntaxKind::TypeParamList).is_some(),
        reference_final: child_kind(node, start, SyntaxKind::ModifierList)
            .is_some_and(|(n, _)| tokens(n, 0).iter().any(|t| t.0 == SyntaxKind::Final)),
        reference_constructor_work: reference_constructor_work(node, start),
        reference_edges: reference_source_edges(node, start),
        binary_ancestor_names: BTreeMap::new(),
        binary_root: false,
        fields: Vec::new(),
        methods: Vec::new(),
        constructors: Vec::new(),
    };
    let (super_parts, interface_parts) = declared_supers(node, start);
    data.super_parts = super_parts;
    data.interface_parts = interface_parts;
    let index = ctx.file.classes.len();
    ctx.file.classes.push(ClassRec {
        scope: 0,
        context: BinaryLinkingContext::Standalone,
        data: ClassData::Source(Box::new(data)),
    });
    ctx.class_stack.push(index);
    if node.kind() == SyntaxKind::Record
        && let Some((params, params_at)) = child_kind(node, start, SyntaxKind::ParamList)
    {
        index_components(params, params_at, ctx, index);
    }
    let mut offset = start;
    for child in node.children() {
        if let Some(child) = child.as_node() {
            index_node(child, offset, ctx);
        }
        offset += child.utf8_len();
    }
    ctx.class_stack.pop();
}

// A constant denotes a public field of its enum, not the members of its
// anonymous constant-specific subclass. Those bodies are deliberately opaque
// for both indexing and queries until anonymous-subclass semantics are modeled.
fn index_enum_constant(node: &GreenNode, start: u32, ctx: &mut IndexCtx<'_>) {
    if let Some(&class_index) = ctx.class_stack.last()
        && crate::green::collect_errors(node).is_empty()
        && let Some((name, name_start, name_end)) = direct_ident(node, start)
    {
        let declared = QualName {
            parts: ctx.file.classes[class_index].dotted().split('.').map(str::to_owned).collect(),
            array_rank: 0,
        };
        ctx.file.classes[class_index].source_data_mut().fields.push(Member {
            source_enum_constant: true,
            source_interface_declared_only: false,
            name,
            kind: TargetKind::Field,
            start: name_start,
            end: name_end,
            declared: Some(declared),
            access: Access::Public,
            shape: Shape::Unknown,
            source_call: None,
            source_field: None,
        });
    }
    ctx.enum_constant_depth += 1;
    let mut at = start;
    for child in node.children() {
        if let Some(child) = child.as_node() {
            index_node(child, at, ctx);
        }
        at += child.utf8_len();
    }
    ctx.enum_constant_depth -= 1;
}

fn index_components(params: &GreenNode, start: u32, ctx: &mut IndexCtx<'_>, class_index: usize) {
    let mut offset = start;
    for child in params.children() {
        if let Some(child) = child.as_node()
            && child.kind() == SyntaxKind::Parameter
            && let Some((name, name_start, name_end)) = direct_ident(child, offset)
        {
            ctx.file.classes[class_index].source_data_mut().fields.push(Member {
                source_enum_constant: false,
                source_interface_declared_only: false,
                name,
                kind: TargetKind::Field,
                start: name_start,
                end: name_end,
                declared: read_type(child, offset),
                access: read_access(child, offset),
                shape: source_type_shape(child, offset),
                source_call: None,
                source_field: None,
            });
        }
        offset += child.utf8_len();
    }
}

fn index_members(node: &GreenNode, start: u32, ctx: &mut IndexCtx<'_>, class_index: usize, kind: TargetKind) {
    let declarations = if kind == TargetKind::Field {
        variable_declarators(node, start)
    } else {
        direct_ident(node, start).map(|(name, at, end)| (name, at, end, read_type(node, start))).into_iter().collect()
    };
    for (name, name_start, name_end, declared) in declarations {
        let member = Member {
            source_enum_constant: false,
            source_interface_declared_only: ctx.file.classes[class_index].is_interface
                && interface_declared_only(node, start),
            name,
            kind,
            start: name_start,
            end: name_end,
            declared,
            access: match read_access(node, start) {
                Access::Package if kind == TargetKind::Field && ctx.file.classes[class_index].is_interface => {
                    Access::Public
                }
                access => access,
            },
            shape: if kind == TargetKind::Method {
                source_method_shape(node, start)
            } else {
                source_type_shape(node, start)
            },
            source_call: (node.kind() == SyntaxKind::Method).then(|| source_call(node, start)).flatten(),
            source_field: (node.kind() == SyntaxKind::Field)
                .then(|| source_field(node, start, name_start))
                .flatten()
                .map(Box::new),
        };
        if kind == TargetKind::Field {
            ctx.file.classes[class_index].source_data_mut().fields.push(member);
        } else if node.kind() == SyntaxKind::Constructor {
            ctx.file.classes[class_index].source_data_mut().constructors.push(member);
        } else {
            ctx.file.classes[class_index].source_data_mut().methods.push(member);
        }
    }
    let scope = (start, start + node.utf8_len(), ScopeKind::Method);
    ctx.scope_stack.push(scope);
    let mut offset = start;
    for child in node.children() {
        if let Some(child) = child.as_node()
            && !matches!(child.kind(), SyntaxKind::Field | SyntaxKind::Method | SyntaxKind::Constructor)
        {
            index_node(child, offset, ctx);
        }
        offset += child.utf8_len();
    }
    ctx.scope_stack.pop();
}

// Declaration names and suffix brackets are direct children; initializer names,
// commas, and array accesses are nested expressions and must never become symbols.
// Keep the existing lossless CST layout and reset suffix rank at each comma.
fn variable_declarators(node: &GreenNode, start: u32) -> Vec<(String, u32, u32, Option<QualName>)> {
    let shared = child_kind(node, start, SyntaxKind::Type).and_then(|(ty, at)| {
        let parts = type_parts(ty, at)?;
        let array_rank = tokens(ty, at).iter().filter(|token| token.0 == SyntaxKind::LBracket).count();
        Some(QualName { parts, array_rank })
    });
    let mut result = Vec::new();
    let mut current: Option<(String, u32, u32, Option<QualName>)> = None;
    let mut in_initializer = false;
    let mut offset = start;
    for child in node.children() {
        if let Some(token) = child.as_token() {
            match token.kind() {
                SyntaxKind::Ident if current.is_none() && !in_initializer => {
                    current = Some((token.text().to_string(), offset, offset + token.utf8_len(), shared.clone()));
                }
                SyntaxKind::LBracket if !in_initializer => {
                    if let Some((_, _, _, Some(ty))) = current.as_mut() {
                        ty.array_rank += 1;
                    }
                }
                SyntaxKind::Eq => in_initializer = true,
                SyntaxKind::Comma => {
                    if let Some(declaration) = current.take() {
                        result.push(declaration);
                    }
                    in_initializer = false;
                }
                _ => {}
            }
        }
        offset += child.utf8_len();
    }
    if let Some(declaration) = current {
        result.push(declaration);
    }
    result
}

fn index_local(node: &GreenNode, start: u32, ctx: &mut IndexCtx<'_>, kind: TargetKind) {
    let Some((scope_start, scope_end, _)) = ctx.scope_stack.last().copied() else { return };
    for (name, name_start, name_end, declared) in variable_declarators(node, start) {
        ctx.file.locals.push(LocalRec {
            name,
            kind,
            start: name_start,
            end: name_end,
            scope_start,
            scope_end,
            visible_from: name_end,
            declared,
            primitive: primitive_declaration(node, start, name_start),
            postfix_after: (kind == TargetKind::Local)
                .then(|| postfix_declaration_end(node, start, name_start))
                .flatten(),
            catch_origin: false,
            reference: reference_declaration(node, start, name_start),
            path: ctx.file.path.clone(),
        });
    }
}

fn index_catch(node: &GreenNode, start: u32, ctx: &mut IndexCtx<'_>) {
    let Some((scope_start, scope_end, _)) = ctx.scope_stack.last().copied() else { return };
    let Some((name, name_start, name_end)) = direct_ident(node, start) else { return };
    ctx.file.locals.push(LocalRec {
        name,
        kind: TargetKind::Parameter,
        start: name_start,
        end: name_end,
        scope_start,
        scope_end,
        visible_from: name_end,
        declared: read_type(node, start),
        primitive: None,
        postfix_after: None,
        catch_origin: true,
        reference: catch_reference_declaration(node, start, name_start)
            .map(|parts| ReferenceDeclaration::Plain(QualName { parts, array_rank: 0 })),
        path: ctx.file.path.clone(),
    });
}

fn index_parameter(node: &GreenNode, start: u32, ctx: &mut IndexCtx<'_>) {
    let Some((scope_start, scope_end, kind)) = ctx.scope_stack.last().copied() else { return };
    if !matches!(kind, ScopeKind::Method | ScopeKind::Lambda | ScopeKind::Catch) {
        return;
    }
    if let Some((name, name_start, name_end)) = direct_ident(node, start) {
        ctx.file.locals.push(LocalRec {
            name,
            kind: TargetKind::Parameter,
            start: name_start,
            end: name_end,
            scope_start,
            scope_end,
            visible_from: name_end,
            declared: read_type(node, start),
            primitive: primitive_declaration(node, start, name_start),
            postfix_after: (kind == ScopeKind::Method)
                .then(|| postfix_declaration_end(node, start, name_start))
                .flatten(),
            catch_origin: false,
            reference: reference_declaration(node, start, name_start),
            path: ctx.file.path.clone(),
        });
    }
}

fn scope_of(node: &GreenNode, start: u32) -> Option<(u32, u32, ScopeKind)> {
    if node.kind() == SyntaxKind::TryStmt
        && let Some(body_end) = try_body_end(node, start)
    {
        // The header through the try block, and not the catch or finally.
        // That span is wider than the block, so a local declared in the block
        // shadows a resource of the same name.
        return Some((start, body_end, ScopeKind::Try));
    }
    let kind = match node.kind() {
        SyntaxKind::Block | SyntaxKind::SwitchBody | SyntaxKind::ForStmt => ScopeKind::Other,
        SyntaxKind::LambdaExpr => ScopeKind::Lambda,
        SyntaxKind::CatchClause => ScopeKind::Catch,
        SyntaxKind::TryStmt => ScopeKind::Try,
        _ => return None,
    };
    Some((start, start + node.utf8_len(), kind))
}

fn try_body_end(node: &GreenNode, start: u32) -> Option<u32> {
    let mut offset = start;
    for child in node.children() {
        if let Some(token) = child.as_token()
            && token.kind() == SyntaxKind::Finally
        {
            return None;
        }
        if let Some(child) = child.as_node() {
            if child.kind() == SyntaxKind::CatchClause {
                return None;
            }
            if child.kind() == SyntaxKind::Block {
                return Some(offset + child.utf8_len());
            }
        }
        offset += child.utf8_len();
    }
    None
}

fn declared_supers(node: &GreenNode, start: u32) -> (Option<Vec<String>>, Vec<Vec<String>>) {
    let mut phase = 0u8;
    let mut extends = Vec::new();
    let mut implements = Vec::new();
    let mut offset = start;
    for child in node.children() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::Extends {
                phase = 1;
            } else if token.kind() == SyntaxKind::Implements {
                phase = 2;
            }
        }
        if let Some(child) = child.as_node()
            && child.kind() == SyntaxKind::Type
            && let Some(name) = type_parts(child, offset)
        {
            if phase == 1 {
                extends.push(name);
            } else if phase == 2 {
                implements.push(name);
            }
        }
        offset += child.utf8_len();
    }
    let object = vec!["java".to_string(), "lang".to_string(), "Object".to_string()];
    match node.kind() {
        SyntaxKind::Interface => (Some(object), extends),
        SyntaxKind::Enum => (Some(vec!["java".to_string(), "lang".to_string(), "Enum".to_string()]), implements),
        SyntaxKind::Record => (Some(vec!["java".to_string(), "lang".to_string(), "Record".to_string()]), implements),
        _ => (Some(extends.into_iter().next().unwrap_or(object)), implements),
    }
}

fn type_parts(node: &GreenNode, start: u32) -> Option<Vec<String>> {
    let (ty, at) =
        if node.kind() == SyntaxKind::Type { (node, start) } else { child_kind(node, start, SyntaxKind::Type)? };
    let (name, name_at) = child_kind(ty, at, SyntaxKind::Name)?;
    let parts = name_strings(name, name_at);
    if parts.is_empty() { None } else { Some(parts) }
}

// Interface declaration modifiers classify navigation even for signatures
// outside the strict selector's subset. Unknown/contradictory modifiers do not
// authorize exclusions. Annotations are separate syntax, not modifier tokens.
fn interface_declared_only(node: &GreenNode, start: u32) -> bool {
    if node.kind() != SyntaxKind::Method || child_kind(node, start, SyntaxKind::Block).is_none() {
        return false;
    }
    let Some((mods, _)) = child_kind(node, start, SyntaxKind::ModifierList) else { return false };
    if !crate::green::collect_errors(mods).is_empty() {
        return false;
    }
    let mut seen = Vec::new();
    for child in mods.children() {
        if let Some(node) = child.as_node() {
            if node.kind() != SyntaxKind::Annotation {
                return false;
            }
        } else if let Some(token) = child.as_token() {
            let kind = token.kind();
            if kind.is_trivia() {
                continue;
            }
            if !matches!(
                kind,
                SyntaxKind::Public
                    | SyntaxKind::Private
                    | SyntaxKind::Static
                    | SyntaxKind::Abstract
                    | SyntaxKind::Default
                    | SyntaxKind::Strictfp
            ) || seen.contains(&kind)
            {
                return false;
            }
            seen.push(kind);
        }
    }
    !(seen.contains(&SyntaxKind::Abstract)
        || seen.contains(&SyntaxKind::Default)
        || seen.contains(&SyntaxKind::Public) && seen.contains(&SyntaxKind::Private))
        && (seen.contains(&SyntaxKind::Static) || seen.contains(&SyntaxKind::Private))
}

// This view is ONLY for navigation. Keep the raw member inventory, graph,
// override relations and strict selector unchanged, including malformed entries.
fn navigation_method_inherited(owner: &ClassRec, member: &Member) -> bool {
    if !owner.is_interface {
        return true;
    }
    if !owner.binary {
        return !member.source_interface_declared_only;
    }
    let Shape::Binary { flags, .. } = &member.shape else { return true };
    let Some(signature) = &owner.binary_signature else { return true };
    let access = flags & 7;
    let declared_only = matches!(access, 1 | 2)
        && flags & (0x0008 | 0x0002) != 0
        && flags & !0x18cb == 0 // no abstract/final/native/synchronized interface declaration
        && signature.major >= 52
        && (access != 2 || signature.major >= 53)
        && (flags & 0x0800 == 0 || signature.major <= 60);
    !declared_only
}

fn read_access(node: &GreenNode, start: u32) -> Access {
    let Some((mods, _)) = child_kind(node, start, SyntaxKind::ModifierList) else {
        return Access::Package;
    };
    let mut seen = Vec::new();
    for (kind, _, _) in tokens(mods, 0) {
        match kind {
            SyntaxKind::Public => seen.push(Access::Public),
            SyntaxKind::Protected => seen.push(Access::Protected),
            SyntaxKind::Private => seen.push(Access::Private),
            _ => {}
        }
    }
    match seen.as_slice() {
        [] => Access::Package,
        [one] => *one,
        _ => Access::Unspecified,
    }
}

#[derive(Clone, Debug)]
struct SourceField {
    is_static: bool,
    reference: Option<ReferenceDeclaration>,
    primitive: Option<PrimitiveValue>,
}

// Field types are declaration evidence, not initializer inference or legacy
// Member.declared navigation. Each sibling keeps its own suffix dimensions.
fn source_field(node: &GreenNode, start: u32, name_at: u32) -> Option<SourceField> {
    if node.kind() != SyntaxKind::Field || !crate::green::collect_errors(node).is_empty() {
        return None;
    }
    let mut modifiers = Vec::new();
    if let Some((mods, _)) = child_kind(node, start, SyntaxKind::ModifierList) {
        if mods.child_nodes().next().is_some() {
            return None;
        }
        for (kind, _, _) in tokens(mods, 0).into_iter().filter(|t| !t.0.is_trivia()) {
            if !matches!(
                kind,
                SyntaxKind::Public
                    | SyntaxKind::Private
                    | SyntaxKind::Protected
                    | SyntaxKind::Static
                    | SyntaxKind::Final
                    | SyntaxKind::Transient
                    | SyntaxKind::Volatile
            ) || modifiers.contains(&kind)
            {
                return None;
            }
            modifiers.push(kind);
        }
    }
    if [SyntaxKind::Public, SyntaxKind::Private, SyntaxKind::Protected].iter().filter(|k| modifiers.contains(k)).count()
        > 1
        || (modifiers.contains(&SyntaxKind::Final) && modifiers.contains(&SyntaxKind::Volatile))
    {
        return None;
    }
    Some(SourceField {
        is_static: modifiers.contains(&SyntaxKind::Static),
        reference: reference_declaration_type(node, start, name_at),
        primitive: primitive_declaration(node, start, name_at),
    })
}

fn source_type_shape(_node: &GreenNode, _start: u32) -> Shape {
    // Field selection does not use method signatures.
    Shape::Unknown
}

fn type_variables(node: &GreenNode, start: u32) -> Vec<String> {
    let Some((params, at)) = child_kind(node, start, SyntaxKind::TypeParamList) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut offset = at;
    for child in params.children() {
        if let Some(node) = child.as_node()
            && node.kind() == SyntaxKind::TypeParam
            && let Some((name, _, _)) = direct_ident(node, offset)
        {
            out.push(name);
        }
        offset += child.utf8_len();
    }
    out
}
fn contains_kind(node: &GreenNode, kind: SyntaxKind) -> bool {
    node.kind() == kind
        || node.children().iter().filter_map(|child| child.as_node()).any(|node| contains_kind(node, kind))
}
fn source_method_shape(node: &GreenNode, start: u32) -> Shape {
    let Some((params, params_at)) = child_kind(node, start, SyntaxKind::ParamList) else {
        return Shape::Unknown;
    };
    if !crate::green::collect_errors(params).is_empty() {
        return Shape::Unknown;
    }
    let mut parameters = Vec::new();
    let mut offset = params_at;
    for child in params.children() {
        if let Some(child) = child.as_node()
            && child.kind() == SyntaxKind::Parameter
        {
            let Some(name) = type_parts(child, offset).or_else(|| primitive_type(child, offset).map(|name| vec![name]))
            else {
                return Shape::Unknown;
            };
            let Some((ty, at)) = child_kind(child, offset, SyntaxKind::Type) else {
                return Shape::Unknown;
            };
            let prefix = tokens(ty, at).iter().filter(|token| token.0 == SyntaxKind::LBracket).count();
            let suffix = tokens(child, offset)
                .iter()
                .filter(|token| matches!(token.0, SyntaxKind::LBracket | SyntaxKind::DotDotDot))
                .count();
            if name == ["void"] || prefix + suffix > 255 {
                return Shape::Unknown;
            }
            let primitive = primitive_type(child, offset).as_deref().and_then(Primitive::source);
            parameters.push(ParameterType {
                binary_name: None,
                name,
                primitive,
                array_rank: prefix + suffix,
                unsupported: contains_kind(ty, SyntaxKind::TypeArgList)
                    || (primitive.is_none() && prefix == 0 && !plain_reference_type(ty)),
            });
        }
        offset += child.utf8_len();
    }
    Shape::Source { parameters, variables: type_variables(node, start) }
}

fn primitive_type(node: &GreenNode, start: u32) -> Option<String> {
    let (ty, at) = child_kind(node, start, SyntaxKind::Type)?;
    tokens(ty, at).into_iter().find_map(|(kind, _, _)| match kind {
        SyntaxKind::IntKw => Some("int".to_string()),
        SyntaxKind::LongKw => Some("long".to_string()),
        SyntaxKind::Short => Some("short".to_string()),
        SyntaxKind::Byte => Some("byte".to_string()),
        SyntaxKind::CharKw => Some("char".to_string()),
        SyntaxKind::BooleanKw => Some("boolean".to_string()),
        SyntaxKind::FloatKw => Some("float".to_string()),
        SyntaxKind::DoubleKw => Some("double".to_string()),
        SyntaxKind::Void => Some("void".to_string()),
        _ => None,
    })
}

// Parse the entire JVMS method descriptor. Void is a validated absent result;
// scalar/reference/array results retain their own descriptor type evidence.
// This is independent of cafebabe's permissiveness and never splits on ')',
// which is legal inside a JVM class name.
fn descriptor_parts(descriptor: &str) -> Option<(Vec<ParameterType>, Option<ParameterType>)> {
    fn field(bytes: &[u8], at: &mut usize) -> Option<ParameterType> {
        let mut rank = 0;
        while bytes.get(*at) == Some(&b'[') {
            rank += 1;
            *at += 1;
        }
        if rank > 255 {
            return None;
        }
        let tag = *bytes.get(*at)?;
        *at += 1;
        let primitive = match tag {
            b'Z' => Some("boolean"),
            b'B' => Some("byte"),
            b'C' => Some("char"),
            b'S' => Some("short"),
            b'I' => Some("int"),
            b'J' => Some("long"),
            b'F' => Some("float"),
            b'D' => Some("double"),
            _ => None,
        };
        let mut binary_name = None;
        let name = if let Some(primitive) = primitive {
            vec![primitive.to_string()]
        } else if tag == b'L' {
            let start = *at;
            while *bytes.get(*at)? != b';' {
                *at += 1;
            }
            let name = std::str::from_utf8(&bytes[start..*at]).ok()?;
            // JVMS unqualified-name grammar, not Java identifier grammar.
            if name.split('/').any(|part| part.is_empty() || part.contains(['.', ';', '['])) {
                return None;
            }
            *at += 1;
            binary_name = Some(name.to_owned());
            name.replace(['/', '$'], ".").split('.').map(str::to_string).collect()
        } else {
            return None;
        };
        Some(ParameterType {
            binary_name,
            name,
            primitive: primitive.and_then(Primitive::source),
            array_rank: rank,
            unsupported: false,
        })
    }
    let bytes = descriptor.as_bytes();
    if bytes.first() != Some(&b'(') {
        return None;
    }
    let mut at = 1;
    let mut parameters = Vec::new();
    let mut slots = 0;
    while bytes.get(at) != Some(&b')') {
        let parameter = field(bytes, &mut at)?;
        slots +=
            if parameter.array_rank == 0 && matches!(parameter.primitive, Some(Primitive::Long | Primitive::Double)) {
                2
            } else {
                1
            };
        if slots > 255 {
            return None;
        }
        parameters.push(parameter);
    }
    at += 1;
    let result = if bytes.get(at) == Some(&b'V') {
        at += 1;
        None
    } else {
        Some(field(bytes, &mut at)?)
    };
    (at == bytes.len()).then_some((parameters, result))
}

fn descriptor_parameters(descriptor: &str) -> Option<Vec<ParameterType>> {
    descriptor_parts(descriptor).map(|(parameters, _)| parameters)
}

fn class_from_signature(sig: &Arc<ClassSignature>) -> ClassPayload {
    let mut binary_ancestor_names = BTreeMap::new();
    for name in sig.binary_super_class.iter().chain(&sig.binary_interfaces) {
        let dotted = name.replace(['/', '$'], ".");
        let slot = binary_ancestor_names.entry(dotted).or_insert_with(|| Some(name.clone()));
        if slot.as_ref().is_some_and(|prior| prior != name) {
            *slot = None;
        }
    }

    #[cfg(test)]
    BINARY_PAYLOAD_BUILDS.with(|count| count.set(count.get() + 1));
    ClassPayload {
        path: sig.origin.clone(),
        package: sig.package.clone(),
        nesting: sig.nesting.clone(),
        name_start: 0,
        name_end: 0,
        range_start: 0,
        range_end: 0,
        super_parts: sig.super_class.as_ref().map(|name| name.split('.').map(str::to_string).collect()),
        interface_parts: sig.interfaces.iter().map(|name| name.split('.').map(str::to_string).collect()).collect(),
        binary: true,
        binary_signature: Some(Arc::clone(sig)),
        is_interface: sig.is_interface,
        type_variables: Vec::new(),
        primitive_owner: false,
        access: sig.access,
        reference_inventory: sig.methods.iter().all(|m| descriptor_parameters(&m.descriptor).is_some())
            && sig.binary_super_class.as_ref().map(|name| name.replace(['/', '$'], ".")) == sig.super_class
            && sig
                .binary_interfaces
                .iter()
                .map(|name| name.replace(['/', '$'], "."))
                .eq(sig.interfaces.iter().cloned()),
        reference_generic: sig.signature.as_ref().is_some_and(|signature| signature.starts_with('<')),
        reference_final: sig.is_final,
        reference_constructor_work: None,
        reference_edges: Some(
            sig.super_class
                .iter()
                .chain(&sig.interfaces)
                .map(|name| (name.split('.').map(str::to_string).collect(), sig.signature.is_none()))
                .collect(),
        ),
        binary_ancestor_names,
        binary_root: sig.binary_name == "java/lang/Object"
            && sig.dotted_name == "java.lang.Object"
            && !sig.is_interface
            && sig.signature.is_none()
            && sig.super_class.is_none()
            && sig.interfaces.is_empty(),
        fields: sig.fields.iter().map(|member| member_from_signature(member, TargetKind::Field)).collect(),
        methods: sig
            .methods
            .iter()
            .filter(|member| member.name != "<init>" && member.kind == MemberKind::Method)
            .map(|member| member_from_signature(member, TargetKind::Method))
            .collect(),
        // Binary constructor selection is not modeled. Raw signatures remain
        // available for complete declaration-inventory validation above.
        constructors: Vec::new(),
    }
}

fn member_from_signature(member: &MemberSignature, kind: TargetKind) -> Member {
    #[cfg(test)]
    BINARY_MEMBER_BUILDS.with(|count| count.set(count.get() + 1));
    Member {
        source_enum_constant: false,
        source_interface_declared_only: false,
        name: member.name.clone(),
        kind,
        start: 0,
        end: 0,
        declared: None,
        access: member.access,
        shape: Shape::Binary {
            descriptor: member.descriptor.clone(),
            generic: member.signature.is_some(),
            signature: member.signature.clone(),
            flags: member.raw_access_flags,
        },
        source_call: None,
        source_field: None,
    }
}

#[derive(Clone, Debug)]
struct SourceCall {
    is_static: bool,
    is_abstract: bool,
    varargs: bool,
    override_facts: Option<OverrideFacts>,
    // Only a strictly selected source member may expose this value. It is not
    // part of Shape/MethodIdentity. Shared invocation proof exposes the declared
    // primitive only after strict selection; it is never a constant-value inference.
    scalar_return: Option<Primitive>,
    reference_return: Option<Vec<String>>,
}

// Independent evidence for legality of a source override, not invocation result typing.
#[derive(Clone, Debug)]
struct OverrideFacts {
    is_final: bool,
    is_abstract: bool,
    result: OverrideReturn,
    work: usize,
}
#[derive(Clone, Debug)]
enum OverrideReturn {
    Void,
    Primitive(Primitive),
    Reference(Vec<String>),
}

// Kept separate from QualName: retaining primitive values must not teach the
// receiver resolver to interpret a primitive or array as a declared class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PrimitiveValue {
    primitive: Primitive,
    rank: usize,
}
impl PrimitiveValue {
    fn converts_to(self, other: Self) -> bool {
        if self.rank != 0 || other.rank != 0 {
            return self == other;
        }
        use Primitive::*;
        self.primitive == other.primitive
            || matches!(
                (self.primitive, other.primitive),
                (Byte, Short | Int | Long | Float | Double)
                    | (Short | Char, Int | Long | Float | Double)
                    | (Int, Long | Float | Double)
                    | (Long, Float | Double)
                    | (Float, Double)
            )
    }
}

fn primitive_formals(member: &Member) -> Option<Vec<PrimitiveValue>> {
    let Shape::Source { parameters, variables } = &member.shape else { return None };
    if !variables.is_empty() || member.access == Access::Unspecified {
        return None;
    }
    parameters
        .iter()
        .map(|parameter| {
            if parameter.unsupported {
                return None;
            }
            Some(PrimitiveValue { primitive: parameter.primitive?, rank: parameter.array_rank })
        })
        .collect()
}

// Parameter/argument lists must alternate expressions and commas. The lossless
// parser deliberately accepts some incomplete lists without an Error node.
fn delimited_nodes(node: &GreenNode, start: u32) -> Option<Vec<(&GreenNode, u32)>> {
    let mut state = 0;
    let mut nodes = Vec::new();
    let mut at = start;
    for child in node.children() {
        if let Some(token) = child.as_token() {
            if !token.kind().is_trivia() {
                state = match (state, token.kind()) {
                    (0, SyntaxKind::LParen) => 1,
                    (1 | 2, SyntaxKind::RParen) => 4,
                    (2, SyntaxKind::Comma) => 3,
                    _ => return None,
                };
            }
        } else if let Some(child) = child.as_node() {
            if !matches!(state, 1 | 3) {
                return None;
            }
            nodes.push((child.as_ref(), at));
            state = 2;
        }
        at += child.utf8_len();
    }
    (state == 4).then_some(nodes)
}

fn valid_modifiers(node: &GreenNode, start: u32, method: bool) -> bool {
    let Some((modifiers, _)) = child_kind(node, start, SyntaxKind::ModifierList) else { return true };
    let mut seen = Vec::new();
    for (kind, _, _) in tokens(modifiers, 0).into_iter().filter(|token| !token.0.is_trivia()) {
        let allowed = if method {
            matches!(
                kind,
                SyntaxKind::Public
                    | SyntaxKind::Protected
                    | SyntaxKind::Private
                    | SyntaxKind::Static
                    | SyntaxKind::Final
                    | SyntaxKind::Abstract
                    | SyntaxKind::Synchronized
                    | SyntaxKind::Native
                    | SyntaxKind::Strictfp
            )
        } else {
            matches!(kind, SyntaxKind::Public | SyntaxKind::Final | SyntaxKind::Abstract | SyntaxKind::Strictfp)
        };
        if !allowed || seen.contains(&kind) {
            return false;
        }
        seen.push(kind);
    }
    if read_access(node, start) == Access::Unspecified {
        return false;
    }
    let abstract_conflict = seen.contains(&SyntaxKind::Abstract)
        && seen.iter().any(|kind| {
            matches!(
                kind,
                SyntaxKind::Static
                    | SyntaxKind::Final
                    | SyntaxKind::Native
                    | SyntaxKind::Private
                    | SyntaxKind::Synchronized
                    | SyntaxKind::Strictfp
            )
        });
    let native_conflict = seen.contains(&SyntaxKind::Native) && seen.contains(&SyntaxKind::Strictfp);
    !(abstract_conflict || native_conflict)
}

fn source_call(node: &GreenNode, start: u32) -> Option<SourceCall> {
    if !valid_modifiers(node, start, true) {
        return None;
    }
    let (params, at) = child_kind(node, start, SyntaxKind::ParamList)?;
    let parameters = delimited_nodes(params, at)?;
    let mut varargs = false;
    let mut names = BTreeSet::new();
    for (index, (parameter, at)) in parameters.iter().enumerate() {
        if parameter.kind() != SyntaxKind::Parameter {
            return None;
        }
        let mut kinds = Vec::new();
        let mut type_seen = false;
        for child in parameter.children() {
            if let Some(token) = child.as_token() {
                if !token.kind().is_trivia() {
                    kinds.push(token.kind());
                }
            } else if let Some(child) = child.as_node() {
                match child.kind() {
                    SyntaxKind::ModifierList if !type_seen && kinds.is_empty() => {
                        let mods: Vec<_> =
                            tokens(child, 0).into_iter().filter(|t| !t.0.is_trivia()).map(|t| t.0).collect();
                        if !matches!(mods.as_slice(), [] | [SyntaxKind::Final]) {
                            return None;
                        }
                    }
                    SyntaxKind::Type if !type_seen && kinds.is_empty() => {
                        type_seen = true;
                        if tokens(child, 0).iter().any(|t| t.0 == SyntaxKind::Void) {
                            return None;
                        }
                    }
                    _ => return None,
                }
            }
        }
        if !type_seen {
            return None;
        }
        let (name, _, _) = direct_ident(parameter, *at)?;
        if !names.insert(name) {
            return None;
        }
        if kinds.first() == Some(&SyntaxKind::DotDotDot) {
            if kinds.as_slice() != [SyntaxKind::DotDotDot, SyntaxKind::Ident] || index + 1 != parameters.len() {
                return None;
            }
            varargs = true;
        } else if kinds.first() != Some(&SyntaxKind::Ident) || bracket_rank(&kinds[1..]).is_none_or(|rank| rank > 255) {
            return None;
        }
    }
    if !matches!(source_method_shape(node, start), Shape::Source { .. }) {
        return None;
    }
    if node.kind() == SyntaxKind::Method {
        let (ty, at) = child_kind(node, start, SyntaxKind::Type)?;
        if tokens(ty, at).iter().any(|t| t.0 == SyntaxKind::Void)
            && (tokens(ty, at).iter().filter(|t| !t.0.is_trivia()).count() != 1
                || tokens(node, start).iter().any(|t| t.0 == SyntaxKind::LBracket))
        {
            return None;
        }
    }
    let mods = child_kind(node, start, SyntaxKind::ModifierList).map(|(mods, _)| tokens(mods, 0)).unwrap_or_default();
    if node.kind() == SyntaxKind::Constructor
        && mods.iter().any(|t| {
            !t.0.is_trivia() && !matches!(t.0, SyntaxKind::Public | SyntaxKind::Protected | SyntaxKind::Private)
        })
    {
        return None;
    }
    if mods.iter().any(|t| matches!(t.0, SyntaxKind::Abstract | SyntaxKind::Native))
        && child_kind(node, start, SyntaxKind::Block).is_some()
    {
        return None;
    }
    if mods.iter().any(|t| t.0 == SyntaxKind::Static)
        && child_kind(node, start, SyntaxKind::Block).is_none()
        && !mods.iter().any(|t| t.0 == SyntaxKind::Native)
    {
        return None;
    }
    Some(SourceCall {
        is_static: mods.iter().any(|t| t.0 == SyntaxKind::Static),
        is_abstract: mods.iter().any(|t| t.0 == SyntaxKind::Abstract),
        varargs,
        override_facts: source_override_facts(node, start),
        scalar_return: primitive_method_return(node),
        reference_return: reference_method_return(node),
    })
}

// Declaration annotations may be structurally present, but their names confer
// no override proof. Keep this separate from the stricter result-propagation metadata.
// Structural marker/empty annotation evidence only, not annotation type/target validation.
fn marker_override_annotation(node: &GreenNode) -> bool {
    if node.kind() != SyntaxKind::Annotation {
        return false;
    }
    let mut state = 0;
    for child in node.children() {
        if let Some(t) = child.as_token() {
            if t.kind().is_trivia() {
                continue;
            }
            state = match (state, t.kind()) {
                (0, SyntaxKind::At) => 1,
                (2, SyntaxKind::LParen) => 3,
                (3, SyntaxKind::RParen) => 4,
                _ => return false,
            };
        } else if let Some(n) = child.as_node() {
            if state != 1 || n.kind() != SyntaxKind::Name {
                return false;
            }
            let mut ident = true;
            for part in n.children() {
                let Some(t) = part.as_token() else { return false };
                if t.kind().is_trivia() {
                    continue;
                }
                if t.kind() != if ident { SyntaxKind::Ident } else { SyntaxKind::Dot } {
                    return false;
                }
                ident = !ident;
            }
            if ident {
                return false;
            }
            state = 2;
        }
    }
    matches!(state, 2 | 4)
}

fn source_override_facts(node: &GreenNode, start: u32) -> Option<OverrideFacts> {
    if node.kind() != SyntaxKind::Method || !valid_modifiers(node, start, true) {
        return None;
    }
    let mods = child_kind(node, start, SyntaxKind::ModifierList).map(|(n, _)| tokens(n, 0)).unwrap_or_default();
    if mods.iter().any(|t| t.0 == SyntaxKind::Native) {
        return None;
    }
    let is_abstract = mods.iter().any(|t| t.0 == SyntaxKind::Abstract);
    let is_final = mods.iter().any(|t| t.0 == SyntaxKind::Final);
    let mut state = 0;
    let mut result = None;
    let mut work = 0usize;
    for child in node.children() {
        if !child.as_node().is_some_and(|n| n.kind() == SyntaxKind::Block) {
            work = work.saturating_add(child.utf8_len() as usize);
        }
        if let Some(token) = child.as_token() {
            if token.kind().is_trivia() {
                continue;
            }
            state = match (state, token.kind()) {
                (1, SyntaxKind::Ident) => 2,
                (3, SyntaxKind::Semi) if is_abstract => 4,
                _ => return None, // Includes every throws clause and postfix result array.
            };
        } else if let Some(n) = child.as_node() {
            if n.kind() != SyntaxKind::Block && !crate::green::collect_errors(n).is_empty() {
                return None;
            }
            state = match (state, n.kind()) {
                (0, SyntaxKind::ModifierList) if n.child_nodes().all(|n| marker_override_annotation(n)) => 0,
                (0, SyntaxKind::Type) => {
                    result = if plain_reference_type(n) {
                        Some(OverrideReturn::Reference(type_parts(n, 0)?))
                    } else if n.child_nodes().next().is_none() {
                        let kinds: Vec<_> = n
                            .children()
                            .iter()
                            .filter_map(|c| c.as_token())
                            .filter(|t| !t.kind().is_trivia())
                            .collect();
                        let [one] = kinds.as_slice() else { return None };
                        Some(if one.kind() == SyntaxKind::Void {
                            OverrideReturn::Void
                        } else {
                            OverrideReturn::Primitive(Primitive::source(one.text())?)
                        })
                    } else {
                        return None;
                    };
                    1
                }
                (2, SyntaxKind::ParamList) => 3,
                (3, SyntaxKind::Block) if !is_abstract => 4,
                _ => return None,
            };
        }
    }
    (state == 4).then_some(OverrideFacts { is_final, is_abstract, result: result?, work })
}

// Validate the complete method header, not just the first primitive token.
// In particular, dimensions after the name/parameters cannot become a scalar,
// and a Type after `throws` is not the result type. Return bodies are not typed.
fn primitive_method_return(node: &GreenNode) -> Option<Primitive> {
    if node.kind() != SyntaxKind::Method {
        return None;
    }
    let mut state = 0;
    let mut result = None;
    for child in node.children() {
        if let Some(token) = child.as_token() {
            if token.kind().is_trivia() {
                continue;
            }
            state = match (state, token.kind()) {
                (1, SyntaxKind::Ident) => 2,
                (3, SyntaxKind::Throws) => 4,
                (5, SyntaxKind::Comma) => 4,
                (3 | 5, SyntaxKind::Semi) => 6,
                _ => return None,
            };
        } else if let Some(child) = child.as_node() {
            state = match (state, child.kind()) {
                (0, SyntaxKind::ModifierList) if child.child_nodes().next().is_none() => 0,
                (0, SyntaxKind::Type) => {
                    if child.child_nodes().next().is_some() {
                        return None;
                    }
                    let mut parts =
                        child.children().iter().filter_map(|part| part.as_token()).filter(|t| !t.kind().is_trivia());
                    result = Primitive::source(parts.next()?.text());
                    if result.is_none() || parts.next().is_some() {
                        return None;
                    }
                    1
                }
                (2, SyntaxKind::ParamList) => 3,
                (4, SyntaxKind::Type) if plain_reference_type(child) => 5,
                (3 | 5, SyntaxKind::Block) => 6,
                _ => return None,
            };
        }
    }
    (state == 6).then_some(result).flatten()
}

fn index_operands(node: &GreenNode, start: u32) -> Option<[(&GreenNode, u32); 2]> {
    let mut nodes = Vec::new();
    let mut kinds = Vec::new();
    let mut at = start;
    for child in node.children() {
        if let Some(token) = child.as_token() {
            if !token.kind().is_trivia() {
                kinds.push(token.kind());
            }
        } else if let Some(child) = child.as_node() {
            // Node positions distinguish base[index] from missing or
            // reordered operands even when the parser has no errors.
            if (nodes.is_empty() && !kinds.is_empty()) || (nodes.len() == 1 && kinds != [SyntaxKind::LBracket]) {
                return None;
            }
            nodes.push((child.as_ref(), at));
        }
        at += child.utf8_len();
    }
    let [base, index] = nodes.as_slice() else { return None };
    if kinds != [SyntaxKind::LBracket, SyntaxKind::RBracket] {
        return None;
    }
    Some([*base, *index])
}

// This checks syntax, not Throwable inheritance. Unmodeled annotation/generic
// and array forms cannot make a malformed throws header supply a return value.
// A separate, complete-header proof; never reuse lossy Member.declared.
fn reference_method_return(node: &GreenNode) -> Option<Vec<String>> {
    if node.kind() != SyntaxKind::Method {
        return None;
    }
    let mut state = 0;
    let mut result = None;
    for child in node.children() {
        if let Some(token) = child.as_token() {
            if token.kind().is_trivia() {
                continue;
            }
            state = match (state, token.kind()) {
                (1, SyntaxKind::Ident) => 2,
                (3, SyntaxKind::Throws) => 4,
                (5, SyntaxKind::Comma) => 4,
                (3 | 5, SyntaxKind::Semi) => 6,
                _ => return None,
            };
        } else if let Some(child) = child.as_node() {
            state = match (state, child.kind()) {
                (0, SyntaxKind::ModifierList) if child.child_nodes().next().is_none() => 0,
                (0, SyntaxKind::Type) => {
                    if !plain_reference_type(child) {
                        return None;
                    }
                    result = type_parts(child, 0);
                    1
                }
                (2, SyntaxKind::ParamList) => 3,
                (4, SyntaxKind::Type) if plain_reference_type(child) => 5,
                (3 | 5, SyntaxKind::Block) => 6,
                _ => return None,
            };
        }
    }
    (state == 6).then_some(result).flatten()
}

// This checks syntax, not Throwable inheritance. Unmodeled annotation/generic
// and array forms cannot make a malformed throws header supply a return value.
fn plain_reference_type(node: &GreenNode) -> bool {
    plain_reference_rank(node) == Some(0)
}

fn plain_reference_rank(node: &GreenNode) -> Option<usize> {
    let mut children = node.child_nodes();
    let name = children.next()?;
    if children.next().is_some() || name.kind() != SyntaxKind::Name || name.child_nodes().next().is_some() {
        return None;
    }
    let mut identifier = true;
    for (kind, _, _) in tokens(name, 0).into_iter().filter(|t| !t.0.is_trivia()) {
        if kind != if identifier { SyntaxKind::Ident } else { SyntaxKind::Dot } {
            return None;
        }
        identifier = !identifier;
    }
    if identifier {
        return None;
    }
    let brackets: Vec<_> = tokens(node, 0).iter().filter(|t| !t.0.is_trivia()).map(|t| t.0).collect();
    bracket_rank(&brackets).filter(|rank| *rank <= 255)
}

fn valid_call_inventory(node: &GreenNode, start: u32) -> bool {
    // Traditional switches add syntax and lexical scope, not strict call-proof
    // coverage. Preserve the previous opaque-switch inventory boundary rather
    // than enabling unrelated strict proofs just because parser errors vanish.
    if matches!(node.kind(), SyntaxKind::LocalType | SyntaxKind::SwitchStmt) {
        return false;
    }
    if matches!(node.kind(), SyntaxKind::Method | SyntaxKind::Constructor) && source_call(node, start).is_none() {
        return false;
    }
    let mut at = start;
    for child in node.children() {
        if let Some(child) = child.as_node()
            && !valid_call_inventory(child, at)
        {
            return false;
        }
        at += child.utf8_len();
    }
    true
}

fn bracket_rank(kinds: &[SyntaxKind]) -> Option<usize> {
    (kinds.len().is_multiple_of(2)
        && kinds.chunks_exact(2).all(|pair| pair == [SyntaxKind::LBracket, SyntaxKind::RBracket]))
    .then_some(kinds.len() / 2)
}

// A deliberately narrow writable-variable witness, not definite-assignment analysis.
// No modifiers (including annotations), loop headers, resources or implicit types.
// An initialized local becomes usable only after its complete declaration.
fn postfix_declaration_end(node: &GreenNode, start: u32, name_at: u32) -> Option<u32> {
    if child_kind(node, start, SyntaxKind::ModifierList).is_some() {
        return None;
    }
    let ty = primitive_declaration(node, start, name_at)?;
    if ty.rank != 0 {
        return None;
    }
    if node.kind() == SyntaxKind::Parameter {
        let direct: Vec<_> = tokens(node, start).into_iter().filter(|t| !t.0.is_trivia()).collect();
        return (node.child_nodes().count() == 1
            && node.child_nodes().all(|n| n.kind() == SyntaxKind::Type)
            && direct.len() == 1
            && direct[0].0 == SyntaxKind::Ident
            && direct[0].1 == name_at)
            .then_some(start + node.utf8_len());
    }
    if node.kind() != SyntaxKind::LocalVar
        || tokens(node, start).into_iter().rev().find(|t| !t.0.is_trivia())?.0 != SyntaxKind::Semi
    {
        return None;
    }
    let mut at = start;
    let mut state = 0;
    let mut selected = false;
    for child in node.children() {
        if let Some(token) = child.as_token() {
            if !token.kind().is_trivia() {
                match state {
                    1 if token.kind() == SyntaxKind::Ident => {
                        selected |= at == name_at;
                        state = 2;
                    }
                    2 if token.kind() == SyntaxKind::Eq => state = 3,
                    4 if token.kind() == SyntaxKind::Comma => state = 1,
                    4 if token.kind() == SyntaxKind::Semi => state = 5,
                    _ => return None,
                }
            }
        } else if let Some(inner) = child.as_node() {
            match state {
                0 if inner.kind() == SyntaxKind::Type => state = 1,
                3 if inner.kind() != SyntaxKind::Error => state = 4,
                _ => return None,
            }
        }
        at += child.utf8_len();
    }
    (state == 5 && selected).then_some(start + node.utf8_len())
}

fn primitive_declaration(node: &GreenNode, start: u32, name_at: u32) -> Option<PrimitiveValue> {
    let (ty, _) = child_kind(node, start, SyntaxKind::Type)?;
    // Annotated/parameterized/other unmodeled type trees do not borrow a token.
    if ty.child_nodes().next().is_some() {
        return None;
    }
    let primitive = primitive_type(node, start).as_deref().and_then(Primitive::source)?;
    let kinds: Vec<_> = tokens(ty, 0).into_iter().filter(|t| !t.0.is_trivia()).map(|t| t.0).collect();
    let mut rank = bracket_rank(kinds.get(1..)?)?;
    let direct: Vec<_> = tokens(node, start).into_iter().filter(|t| !t.0.is_trivia()).collect();
    let name_index = direct.iter().position(|t| t.0 == SyntaxKind::Ident && t.1 == name_at)?;
    if name_index > 0 && direct[name_index - 1].0 == SyntaxKind::DotDotDot {
        rank += 1;
    }
    let suffix: Vec<_> = direct[name_index + 1..]
        .iter()
        .take_while(|t| !matches!(t.0, SyntaxKind::Eq | SyntaxKind::Comma | SyntaxKind::Semi))
        .map(|t| t.0)
        .collect();
    rank += bracket_rank(&suffix)?;
    (rank <= 255).then_some(PrimitiveValue { primitive, rank })
}

#[derive(Clone, Copy)]
enum CallerKind {
    Method,
    Constructor,
}

#[derive(Clone, Copy)]
struct DirectCaller<'n> {
    node: &'n GreenNode,
    start: u32,
    end: u32,
    kind: CallerKind,
    work: usize,
}

// Only a File -> Class -> ClassBody -> callable path owns modeled locals.
// Lambdas and anonymous/local/nested classes must not borrow an outer variable.
fn direct_caller(root: &GreenNode, byte: u32) -> Option<DirectCaller<'_>> {
    let mut path = Vec::new();
    let mut node = root;
    let mut start = 0;
    loop {
        path.push((node, start));
        let mut at = start;
        let mut next = None;
        for child in node.children() {
            if let Some(child_node) = child.as_node()
                && at <= byte
                && byte < at + child.utf8_len()
            {
                next = Some((child_node.as_ref(), at));
                break;
            }
            at += child.utf8_len();
        }
        let Some((child, at)) = next else { break };
        node = child;
        start = at;
    }
    if path.len() < 5
        || path[0].0.kind() != SyntaxKind::File
        || path[1].0.kind() != SyntaxKind::Class
        || path[2].0.kind() != SyntaxKind::ClassBody
        || !matches!(path[3].0.kind(), SyntaxKind::Method | SyntaxKind::Constructor)
        || path[4].0.kind() != SyntaxKind::Block
        || path[4..].iter().any(|(node, _)| {
            matches!(
                node.kind(),
                SyntaxKind::Annotation
                    | SyntaxKind::LambdaExpr
                    | SyntaxKind::NewExpr
                    | SyntaxKind::Class
                    | SyntaxKind::Interface
                    | SyntaxKind::Enum
                    | SyntaxKind::Record
                    | SyntaxKind::ClassBody
                    | SyntaxKind::Method
                    | SyntaxKind::Constructor
            )
        })
    {
        return None;
    }
    // An enhanced-for variable is not in scope in its iterable expression.
    // The general name index does not model that boundary; do not borrow it.
    if path.iter().any(|(node, start)| {
        let header = tokens(node, *start);
        node.kind() == SyntaxKind::ForStmt
            && header.iter().any(|t| t.0 == SyntaxKind::Colon)
            && header.iter().find(|t| t.0 == SyntaxKind::RParen).is_none_or(|t| byte < t.2)
    }) {
        return None;
    }
    let (method, start) = path[3];
    let mut caller_work = 0;
    let kind = if method.kind() == SyntaxKind::Constructor {
        let (class, class_at) = path[1];
        if direct_ident(method, start)?.0 != direct_ident(class, class_at)?.0
            || child_kind(class, class_at, SyntaxKind::TypeParamList).is_some()
            || child_kind(method, start, SyntaxKind::TypeParamList).is_some()
            || !reference_method_header(method, start)
            || !crate::green::collect_errors(method).is_empty()
            || contains_kind(method, SyntaxKind::LocalType)
        {
            return None;
        }
        // This first slice admits only bodies with implicit super(). Explicit
        // delegation has a distinct early-construction context; never grant
        // current-instance permissions to its arguments or a newer prologue.
        let mut pending = vec![path[4].0];
        let mut work = REFERENCE_PROOF_WORK;
        while let Some(node) = pending.pop() {
            work = work.checked_sub(node.children().len().max(1))?;
            if node.kind() == SyntaxKind::CallExpr
                && node.child_nodes().next().is_some_and(|callee| {
                    matches!(callee.kind(), SyntaxKind::ThisExpr | SyntaxKind::SuperExpr)
                        || callee.kind() == SyntaxKind::FieldExpr
                            && callee
                                .children()
                                .iter()
                                .any(|c| c.as_token().is_some_and(|t| t.kind() == SyntaxKind::Super))
                })
            {
                return None;
            }
            pending.extend(node.child_nodes().map(|n| n.as_ref()));
        }
        caller_work = REFERENCE_PROOF_WORK - work;
        CallerKind::Constructor
    } else {
        CallerKind::Method
    };
    Some(DirectCaller { node: method, start, end: start + method.utf8_len(), kind, work: caller_work })
}

// One budget is shared by the complete nested proof. These are analysis bounds,
// not Java language limits. Exhaustion is unknown, never unique-name success.
const PRIMITIVE_PROOF_DEPTH: usize = 32;
const PRIMITIVE_PROOF_WORK: usize = 4096;

struct PrimitiveProof<'a> {
    file: &'a FileRec,
    caller_start: u32,
    caller_end: u32,
    // None is used only by the preexisting negative guard: local types can
    // prove a contradiction without Object, but call results cannot.
    owner: Option<(&'a ClassRec, &'a ClassRec)>,
    work_left: usize,
    exhausted: bool,
}

impl<'a> PrimitiveProof<'a> {
    fn new(file: &'a FileRec, caller_start: u32, caller_end: u32, owner: Option<(&'a ClassRec, &'a ClassRec)>) -> Self {
        Self { file, caller_start, caller_end, owner, work_left: PRIMITIVE_PROOF_WORK, exhausted: false }
    }

    fn charge(&mut self, work: usize) -> Option<()> {
        let remaining = self.work_left.checked_sub(work);
        self.work_left = remaining.unwrap_or(0);
        self.exhausted |= remaining.is_none();
        remaining.map(|_| ())
    }

    fn enter(&mut self, depth: usize) -> Option<()> {
        if depth > PRIMITIVE_PROOF_DEPTH {
            self.work_left = 0;
            self.exhausted = true;
            return None;
        }
        self.charge(1)
    }

    // The caller/context checks in simple_call must pass before owner is set.
    // Return the actual member only after full applicability and specificity
    // proof, separately from public navigation's compatible Resolved fallback.
    fn call(&mut self, name: &str, args: &GreenNode, start: u32, depth: usize) -> Option<&'a Member> {
        self.enter(depth)?;
        let (class, object) = self.owner?;
        self.charge(class.methods.len())?;
        // Every nested name has its own Object/competitor closure, even though
        // its enclosing call already proved closure for a different name.
        // The immutable owner identity/root Object edge was checked once when
        // this context was created. Only the name-dependent check is repeated.
        self.charge(object.methods.len())?;
        if object.methods.iter().any(|member| member.name == name) {
            return None;
        }
        self.charge(args.children().len())?;
        let args = delimited_nodes(args, start)?;
        let actuals = args.iter().map(|(arg, at)| self.expression(arg, *at, depth + 1)).collect::<Option<Vec<_>>>()?;
        // Every same-name declaration is considered before access filtering.
        // No unsupported competitor may be dropped to manufacture a winner.
        let members: Vec<_> = class.methods.iter().filter(|member| member.name == name).collect();
        let mut formals = Vec::new();
        for member in &members {
            member.source_call.as_ref().filter(|call| call.is_static)?;
            let Shape::Source { parameters, .. } = &member.shape else { return None };
            self.charge(parameters.len().max(1))?;
            formals.push(primitive_formals(member)?);
        }
        // Duplicate signatures invalidate even inapplicable declarations;
        // different return types never distinguish otherwise equal signatures.
        for (i, left) in formals.iter().enumerate() {
            for right in &formals[i + 1..] {
                self.charge(left.len().max(1))?;
                if left == right {
                    return None;
                }
            }
        }
        let mut applicable = Vec::new();
        for (i, parameters) in formals.iter().enumerate() {
            self.charge(parameters.len().max(1))?;
            if actuals.len() == parameters.len()
                && actuals.iter().zip(parameters).all(|(actual, formal)| actual.converts_to(*formal))
            {
                applicable.push(i);
            }
        }
        let mut selected = None;
        for &i in &applicable {
            let mut most_specific = true;
            for &j in &applicable {
                self.charge(formals[i].len().max(1))?;
                if !formals[i].iter().zip(&formals[j]).all(|(left, right)| left.converts_to(*right)) {
                    most_specific = false;
                    break;
                }
            }
            if most_specific {
                if selected.is_some() {
                    return None;
                }
                selected = Some(members[i]);
            }
        }
        selected
    }

    fn expression(&mut self, node: &GreenNode, start: u32, depth: usize) -> Option<PrimitiveValue> {
        self.enter(depth)?;
        self.charge(node.children().len())?;
        match node.kind() {
            SyntaxKind::ReferenceExpr => {
                self.charge(self.file.locals.len())?;
                let name = token_name(node);
                let mut locals = self.file.locals.iter().filter(|local| {
                    local.name == name
                        && local.start >= self.caller_start
                        && local.end <= self.caller_end
                        && local.scope_start <= start
                        && start < local.scope_end
                        && local.visible_from <= start
                });
                let one = locals.next()?;
                if locals.next().is_some() {
                    return None;
                }
                if !matches!(one.kind, TargetKind::Local | TargetKind::Parameter) {
                    return None;
                }
                one.primitive
            }
            SyntaxKind::ParenExpr => {
                let expressions = delimited_nodes(node, start)?;
                let [(expression, at)] = expressions.as_slice() else { return None };
                self.expression(expression, *at, depth + 1)
            }
            SyntaxKind::IndexExpr => {
                let [base, index] = index_operands(node, start)?;
                let ((base, base_at), (index, index_at)) = (&base, &index);
                let mut base = self.expression(base, *base_at, depth + 1)?;
                let index = self.expression(index, *index_at, depth + 1)?;
                if base.rank == 0
                    || index.rank != 0
                    || !matches!(index.primitive, Primitive::Byte | Primitive::Short | Primitive::Char | Primitive::Int)
                {
                    return None;
                }
                base.rank -= 1;
                Some(base)
            }
            SyntaxKind::CallExpr => {
                let mut nodes = Vec::new();
                let mut at = start;
                for child in node.children() {
                    if child.as_token().is_some_and(|token| !token.kind().is_trivia()) {
                        return None;
                    }
                    if let Some(child) = child.as_node() {
                        nodes.push((child.as_ref(), at));
                    }
                    at += child.utf8_len();
                }
                let [(callee, _), (args, args_at)] = nodes.as_slice() else { return None };
                if callee.kind() != SyntaxKind::ReferenceExpr || args.kind() != SyntaxKind::ArgList {
                    return None;
                }
                let name = token_name(callee);
                let member = self.call(&name, args, *args_at, depth)?;
                Some(PrimitiveValue { primitive: member.source_call.as_ref()?.scalar_return?, rank: 0 })
            }
            _ => None,
        }
    }
}

// This inventory checks class/member declarations, not the validity or inferred
// types of unrelated method bodies. Generic headers remain inventory entries;
// they are never silently discarded when the queried name matches.
fn reference_class_inventory(node: &GreenNode, start: u32) -> bool {
    if !matches!(node.kind(), SyntaxKind::Class | SyntaxKind::Interface)
        || !valid_modifiers(node, start, false)
        || reference_source_edges(node, start).is_none()
        || child_kind(node, start, SyntaxKind::TypeParamList)
            .is_some_and(|(n, _)| !crate::green::collect_errors(n).is_empty() || type_variables(node, start).is_empty())
    {
        return false;
    }
    let Some((body, at)) =
        child_kind(node, start, SyntaxKind::ClassBody).or_else(|| child_kind(node, start, SyntaxKind::InterfaceBody))
    else {
        return false;
    };
    let interface = node.kind() == SyntaxKind::Interface;
    let mods = child_kind(node, start, SyntaxKind::ModifierList).map(|(n, _)| tokens(n, 0)).unwrap_or_default();
    let abstract_class = mods.iter().any(|t| t.0 == SyntaxKind::Abstract);
    if mods.iter().any(|t| t.0 == SyntaxKind::Final) && (abstract_class || interface) {
        return false;
    }
    let variables = type_variables(node, start);
    if variables.iter().collect::<BTreeSet<_>>().len() != variables.len() {
        return false;
    }
    let mut offset = at;
    for child in body.children() {
        if let Some(node) = child.as_node() {
            match node.kind() {
                SyntaxKind::Method | SyntaxKind::Constructor => {
                    if !reference_method_header(node, offset) {
                        return false;
                    }
                    let mods = child_kind(node, offset, SyntaxKind::ModifierList)
                        .map(|(n, _)| tokens(n, 0))
                        .unwrap_or_default();
                    if !interface && !abstract_class && mods.iter().any(|t| t.0 == SyntaxKind::Abstract) {
                        return false;
                    }
                    if !interface
                        && node.kind() == SyntaxKind::Method
                        && child_kind(node, offset, SyntaxKind::Block).is_none()
                        && !mods.iter().any(|t| matches!(t.0, SyntaxKind::Abstract | SyntaxKind::Native))
                    {
                        return false;
                    }
                    if node.kind() == SyntaxKind::Method
                        && child_kind(node, offset, SyntaxKind::Block).is_none()
                        && !tokens(node, offset).iter().any(|t| t.0 == SyntaxKind::Semi)
                    {
                        return false;
                    }
                }
                SyntaxKind::Error => return false,
                SyntaxKind::Field if !crate::green::collect_errors(node).is_empty() => return false,
                _ => {}
            }
        } else if child.as_token().is_some_and(|t| {
            !t.kind().is_trivia() && !matches!(t.kind(), SyntaxKind::LBrace | SyntaxKind::RBrace | SyntaxKind::Semi)
        }) {
            return false;
        }
        offset += child.utf8_len();
    }
    true
}

// A deliberately narrow construction-header proof. This does not validate
// constructor bodies, implicit super calls or checked exceptions. No implicit
// default or constructor-overload inference is made from missing declarations.
fn reference_constructor_work(node: &GreenNode, start: u32) -> Option<usize> {
    if node.kind() != SyntaxKind::Class
        || child_kind(node, start, SyntaxKind::TypeParamList).is_some()
        || child_kind(node, start, SyntaxKind::ModifierList)
            .is_some_and(|(n, _)| tokens(n, 0).iter().any(|t| t.0 == SyntaxKind::Abstract))
    {
        return None;
    }
    let (body, body_at) = child_kind(node, start, SyntaxKind::ClassBody)?;
    let mut constructor = None;
    let mut at = body_at;
    for child in body.children() {
        if let Some(n) = child.as_node()
            && n.kind() == SyntaxKind::Constructor
        {
            if constructor.is_some() {
                return None;
            }
            constructor = Some((n, at));
        }
        at += child.utf8_len();
    }
    let (constructor, at) = constructor?;
    if direct_ident(constructor, at)?.0 != direct_ident(node, start)?.0
        || read_access(constructor, at) != Access::Public
        || child_kind(constructor, at, SyntaxKind::TypeParamList).is_some()
        || !reference_method_header(constructor, at)
        || tokens(constructor, at).iter().any(|t| t.0 == SyntaxKind::Throws)
        || !crate::green::collect_errors(constructor).is_empty()
    {
        return None;
    }
    let (params, params_at) = child_kind(constructor, at, SyntaxKind::ParamList)?;
    if !delimited_nodes(params, params_at)?.is_empty() {
        return None;
    }
    child_kind(constructor, at, SyntaxKind::Block)?;
    // Account for the retained constructor syntax and complete direct inventory
    // in each query, even though validation itself is snapshot-indexing work.
    Some(body.children().len().saturating_add(constructor.utf8_len() as usize))
}

fn reference_method_header(node: &GreenNode, start: u32) -> bool {
    if source_call(node, start).is_none() {
        return false;
    }
    if let Some((ty, _)) = child_kind(node, start, SyntaxKind::Type)
        && !header_type::complete_return_type(ty)
    {
        return false;
    }
    let mut state = 0;
    for child in node.children() {
        if let Some(token) = child.as_token() {
            if token.kind().is_trivia() {
                continue;
            }
            state = match (state, token.kind()) {
                (0, SyntaxKind::Ident) if node.kind() == SyntaxKind::Constructor => 2,
                (1, SyntaxKind::Ident) => 2,
                (3, SyntaxKind::Throws) => 4,
                (5, SyntaxKind::Comma) => 4,
                (3 | 5, SyntaxKind::Semi) => 6,
                // Postfix result arrays are not supported in this slice.
                _ => return false,
            };
        } else if let Some(child) = child.as_node() {
            state = match (state, child.kind()) {
                (0, SyntaxKind::ModifierList | SyntaxKind::TypeParamList) => 0,
                (0, SyntaxKind::Type) => 1,
                (2, SyntaxKind::ParamList) => 3,
                (4, SyntaxKind::Type) if plain_reference_type(child) => 5,
                (3 | 5, SyntaxKind::Block) => 6,
                _ => return false,
            };
            if child.kind() != SyntaxKind::Block && !crate::green::collect_errors(child).is_empty() {
                return false;
            }
        }
    }
    state == 6
}

fn reference_source_edges(node: &GreenNode, start: u32) -> Option<Vec<(Vec<String>, bool)>> {
    if !matches!(node.kind(), SyntaxKind::Class | SyntaxKind::Interface) {
        return None;
    }
    let mut state = 0;
    let mut seen_extends = false;
    let mut seen_implements = false;
    let mut supers = Vec::new();
    let mut at = start;
    for child in node.children() {
        if let Some(t) = child.as_token() {
            if t.kind().is_trivia() {
                at += child.utf8_len();
                continue;
            }
            state = match (state, t.kind()) {
                (0, SyntaxKind::ClassKw | SyntaxKind::InterfaceKw) => 1,
                (1, SyntaxKind::Ident) => 2,
                (2, SyntaxKind::Extends) if !seen_extends && !seen_implements => {
                    seen_extends = true;
                    3
                }
                (2 | 4, SyntaxKind::Implements) if node.kind() == SyntaxKind::Class && !seen_implements => {
                    seen_implements = true;
                    3
                }
                (4, SyntaxKind::Comma) if seen_implements || node.kind() == SyntaxKind::Interface => 3,
                _ => return None,
            };
        } else if let Some(n) = child.as_node() {
            state = match (state, n.kind()) {
                (0, SyntaxKind::ModifierList) => 0,
                (2, SyntaxKind::TypeParamList) => 2,
                (3, SyntaxKind::Type) => {
                    if !crate::green::collect_errors(n).is_empty() {
                        return None;
                    }
                    let parts = type_parts(n, at)?;
                    // Generic edges are raw inventory only, never substitutions.
                    let plain = plain_reference_type(n);
                    if !plain && !raw_reference_header_type(n, 0, false) {
                        return None;
                    }
                    supers.push((parts, plain));
                    4
                }
                (2 | 4, SyntaxKind::ClassBody | SyntaxKind::InterfaceBody) => 5,
                _ => return None,
            };
        }
        at += child.utf8_len();
    }
    if state != 5 {
        return None;
    }
    if !seen_extends || node.kind() == SyntaxKind::Interface {
        supers.insert(0, (vec!["java".into(), "lang".into(), "Object".into()], true));
    }
    Some(supers)
}

// Recognize only a complete bare ? argument, never a bound or annotation.
fn unbounded_type_argument(node: &GreenNode) -> bool {
    node.kind() == SyntaxKind::Type
        && node.child_nodes().next().is_none()
        && tokens(node, 0).into_iter().filter(|t| !t.0.is_trivia()).map(|t| t.0).collect::<Vec<_>>()
            == [SyntaxKind::Question]
}

// Raw ancestry stays wildcard-free. Receiver declarations may retain bare ?
// as unknown inventory only; this never certifies a scalar argument type.
fn raw_reference_header_type(node: &GreenNode, depth: usize, allow_unbounded: bool) -> bool {
    if depth > REFERENCE_PROOF_DEPTH {
        return false;
    }
    if plain_reference_type(node) {
        return true;
    }
    if node.kind() != SyntaxKind::Type || tokens(node, 0).iter().any(|t| !t.0.is_trivia()) {
        return false;
    }
    let nodes: Vec<_> = node.child_nodes().collect();
    let [name] = nodes.as_slice() else { return false };
    if name.kind() != SyntaxKind::Name {
        return false;
    }
    let mut state = 0;
    for child in name.children() {
        if let Some(t) = child.as_token() {
            if t.kind().is_trivia() {
                continue;
            }
            state = match (state, t.kind()) {
                (0, SyntaxKind::Ident) => 1,
                (1, SyntaxKind::Dot) => 0,
                _ => return false,
            };
        } else if let Some(args) = child.as_node() {
            if state != 1 || args.kind() != SyntaxKind::TypeArgList {
                return false;
            }
            let mut arg_state = 0;
            for arg in args.children() {
                if let Some(t) = arg.as_token() {
                    if t.kind().is_trivia() {
                        continue;
                    }
                    arg_state = match (arg_state, t.kind()) {
                        (0, SyntaxKind::Lt) => 1,
                        (2, SyntaxKind::Comma) => 1,
                        (2, SyntaxKind::Gt) => 3,
                        _ => return false,
                    };
                } else if let Some(ty) = arg.as_node() {
                    if arg_state != 1
                        || !((allow_unbounded && unbounded_type_argument(ty))
                            || raw_reference_header_type(ty, depth + 1, allow_unbounded))
                    {
                        return false;
                    }
                    arg_state = 2;
                }
            }
            if arg_state != 3 {
                return false;
            }
            state = 2;
        }
    }
    state == 2
}

// A catch parameter is not an ordinary declaration: consume the entire header
// and preserve its origin so reference use must also prove Throwable ancestry.
fn catch_reference_declaration(node: &GreenNode, start: u32, name_at: u32) -> Option<Vec<String>> {
    if node.kind() != SyntaxKind::CatchClause {
        return None;
    }
    let mut state = 0;
    let mut parts = None;
    let mut at = start;
    for child in node.children() {
        if let Some(token) = child.as_token() {
            if !token.kind().is_trivia() {
                state = match (state, token.kind()) {
                    (0, SyntaxKind::Catch) => 1,
                    (1, SyntaxKind::LParen) => 2,
                    (4, SyntaxKind::Ident) if at == name_at => 5,
                    (5, SyntaxKind::RParen) => 6,
                    _ => return None,
                };
            }
        } else if let Some(inner) = child.as_node() {
            state = match (state, inner.kind()) {
                (2, SyntaxKind::ModifierList)
                    if inner.child_nodes().next().is_none()
                        && tokens(inner, at)
                            .into_iter()
                            .filter(|t| !t.0.is_trivia())
                            .map(|t| t.0)
                            .collect::<Vec<_>>()
                            == [SyntaxKind::Final] =>
                {
                    3
                }
                (2 | 3, SyntaxKind::Type) if plain_reference_type(inner) => {
                    parts = Some(type_parts(inner, at)?);
                    4
                }
                (6, SyntaxKind::Block) => 7,
                _ => return None,
            };
        }
        at += child.utf8_len();
    }
    (state == 7).then_some(parts).flatten()
}

fn reference_declaration(node: &GreenNode, start: u32, name_at: u32) -> Option<ReferenceDeclaration> {
    if let Some((mods, _)) = child_kind(node, start, SyntaxKind::ModifierList) {
        let kinds: Vec<_> = tokens(mods, 0).into_iter().filter(|t| !t.0.is_trivia()).map(|t| t.0).collect();
        if !matches!(kinds.as_slice(), [] | [SyntaxKind::Final]) || mods.child_nodes().next().is_some() {
            return None;
        }
    }
    reference_declaration_type(node, start, name_at)
}

fn reference_declaration_type(node: &GreenNode, start: u32, name_at: u32) -> Option<ReferenceDeclaration> {
    let (ty, at) = child_kind(node, start, SyntaxKind::Type)?;
    let prefix_rank = plain_reference_rank(ty);
    let direct: Vec<_> = tokens(node, start).into_iter().filter(|t| !t.0.is_trivia()).collect();
    let name = direct.iter().position(|t| t.0 == SyntaxKind::Ident && t.1 == name_at)?;
    if name > 0 && direct[name - 1].0 == SyntaxKind::DotDotDot {
        return None;
    }
    let suffix: Vec<_> = direct[name + 1..]
        .iter()
        .take_while(|t| !matches!(t.0, SyntaxKind::Eq | SyntaxKind::Comma | SyntaxKind::Semi))
        .map(|t| t.0)
        .collect();
    let suffix_rank = bracket_rank(&suffix)?;
    if let Some(prefix_rank) = prefix_rank {
        let array_rank = prefix_rank + suffix_rank;
        return (array_rank <= 255)
            .then_some(ReferenceDeclaration::Plain(QualName { parts: type_parts(ty, at)?, array_rank }));
    }
    if suffix_rank != 0 || !raw_reference_header_type(ty, 0, true) {
        return None;
    }
    let (name, name_at) = child_kind(ty, at, SyntaxKind::Name)?;
    let (args, args_at) = child_kind(name, name_at, SyntaxKind::TypeArgList)?;
    let mut arguments = Vec::new();
    let mut offset = args_at;
    for child in args.children() {
        if let Some(arg) = child.as_node() {
            if unbounded_type_argument(arg) {
                arguments.push(None);
            } else {
                if plain_reference_rank(arg)? != 0 {
                    return None;
                }
                arguments.push(Some(type_parts(arg, offset)?));
            }
        }
        offset += child.utf8_len();
    }
    if arguments.is_empty() || arguments.len() > 32 {
        return None;
    }
    Some(ReferenceDeclaration::Applied { name: type_parts(ty, at)?, arguments })
}

// This is deliberately not Java Unicode preprocessing or text-block typing.
// Validate the complete ordinary token even though the lossless lexer accepts
// unknown escapes and escaped physical line breaks during syntax recovery.
fn ordinary_string_literal(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() < 2
        || bytes.first() != Some(&b'"')
        || bytes.last() != Some(&b'"')
        || bytes.windows(2).any(|pair| pair == b"\\u")
    {
        return false;
    }
    let end = bytes.len() - 1;
    let mut at = 1;
    while at < end {
        match bytes[at] {
            b'"' | b'\r' | b'\n' => return false,
            b'\\' => {
                at += 1;
                if at == end {
                    return false;
                }
                match bytes[at] {
                    b'b' | b't' | b'n' | b'f' | b'r' | b's' | b'"' | b'\'' | b'\\' => at += 1,
                    first @ b'0'..=b'7' => {
                        at += 1;
                        let more = if first <= b'3' { 2 } else { 1 };
                        for _ in 0..more {
                            if at < end && matches!(bytes[at], b'0'..=b'7') {
                                at += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    _ => return false,
                }
            }
            _ => at += 1,
        }
    }
    true
}

// Complete conservative subset of JVMS class signatures: no class formals,
// variables, wildcards, arrays or nested owners. Parameterized edges are parsed
// for framing/identity only, never promoted to plain inheritance.
fn conversion_signature_edges(text: &str) -> Option<Vec<(String, bool)>> {
    fn class_type(bytes: &[u8], at: &mut usize, depth: usize) -> Option<(String, bool)> {
        if depth > REFERENCE_PROOF_DEPTH || bytes.get(*at) != Some(&b'L') {
            return None;
        }
        *at += 1;
        let start = *at;
        let mut segment = false;
        while let Some(&ch) = bytes.get(*at) {
            if ch.is_ascii_alphanumeric() || ch == b'_' {
                segment = true;
            } else if ch == b'/' && segment {
                segment = false;
            } else {
                break;
            }
            *at += 1;
        }
        if !segment {
            return None;
        }
        let name = std::str::from_utf8(&bytes[start..*at]).ok()?.to_owned();
        let plain = bytes.get(*at) != Some(&b'<');
        if !plain {
            *at += 1;
            let first = *at;
            while bytes.get(*at) != Some(&b'>') {
                class_type(bytes, at, depth + 1)?;
            }
            if *at == first {
                return None;
            }
            *at += 1;
        }
        if bytes.get(*at) != Some(&b';') {
            return None;
        }
        *at += 1;
        Some((name, plain))
    }
    let mut at = 0;
    let mut edges = Vec::new();
    while at < text.len() {
        edges.push(class_type(text.as_bytes(), &mut at, 0)?);
    }
    (!edges.is_empty()).then_some(edges)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InvocationMode {
    Instance,
    TypeStatic,
    // Exact current source class in a nonstatic ordinary method; no inherited family.
    OwnInstance,
}

const REFERENCE_PROOF_DEPTH: usize = 32;
const REFERENCE_PROOF_WORK: usize = 4096;

// Unlike name-navigation results, this private proof distinguishes actual
// declared local types, complete raw inventories and justified conversions.
// One budget covers discovery, arguments, closure and pairwise specificity.
#[derive(Clone, Copy)]
struct ReferenceValue<'a> {
    class: &'a ClassRec,
}

#[derive(Clone, Copy)]
enum StrictActual<'a> {
    Null,
    Reference(ReferenceValue<'a>),
    Primitive(PrimitiveValue),
}

// Primitive and reference evidence never cross via boxing in strict invocation.
enum ReferenceFormal<'a> {
    Reference(&'a ClassRec),
    Primitive(Primitive),
}
impl ReferenceFormal<'_> {
    fn same_type(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Reference(a), Self::Reference(b)) => same_class(a, b),
            (Self::Primitive(a), Self::Primitive(b)) => a == b,
            _ => false,
        }
    }
}

struct SelectedMethod<'a> {
    declaring: &'a ClassRec,
    member: &'a Member,
    instantiated_result: Option<StrictActual<'a>>,
}

struct InvocationReceiver<'a> {
    class: &'a ClassRec,
    instantiation: Option<ClassInstantiation<'a>>,
}
#[derive(Clone)]
struct ClassInstantiation<'a> {
    owner: ClassIdentity,
    declaration: instantiation::ClassTemplate,
    arguments: Vec<Option<&'a ClassRec>>,
}

struct ReferenceCallProof<'a> {
    index: &'a Index,
    file: &'a FileRec,
    caller_start: u32,
    caller_end: u32,
    caller_kind: Option<CallerKind>,
    work_left: usize,
    exhausted: bool,
    graphs: BTreeMap<ClassIdentity, Vec<&'a ClassRec>>,
    // A bounded declaration subset for this fixed caller, not cached type evidence.
    caller_locals: Option<Vec<&'a LocalRec>>,
    failure: Option<ResolveResult>,
}
impl<'a> ReferenceCallProof<'a> {
    fn new(index: &'a Index, file: &'a FileRec, caller: Option<DirectCaller<'_>>) -> Self {
        let (caller_start, caller_end) = caller.map_or((0, 0), |c| (c.start, c.end));
        Self {
            index,
            file,
            caller_start,
            caller_end,
            caller_kind: caller.map(|c| c.kind),
            work_left: REFERENCE_PROOF_WORK - caller.map_or(0, |c| c.work),
            exhausted: false,
            graphs: BTreeMap::new(),
            caller_locals: None,
            failure: None,
        }
    }
    fn charge(&mut self, work: usize) -> Option<()> {
        if self.exhausted {
            return None;
        }
        match self.work_left.checked_sub(work) {
            Some(left) => {
                self.work_left = left;
                Some(())
            }
            None => {
                self.work_left = 0;
                self.exhausted = true;
                None
            }
        }
    }
    fn accessible(&mut self, class: &ClassRec) -> Option<()> {
        self.charge(1)?;
        // Nested access requires enclosing/InnerClasses evidence not modeled
        // here. Refuse rather than trust a public member in an inaccessible type.
        if class.nesting.len() != 1
            || !matches!(class.access, Access::Public | Access::Package)
            || (class.access == Access::Package && class.package != self.file.package)
        {
            return None;
        }
        Some(())
    }
    fn has_field(
        &mut self,
        owner: &'a ClassRec,
        name: &str,
        depth: usize,
        seen: &mut BTreeSet<ClassIdentity>,
    ) -> Option<bool> {
        if depth > REFERENCE_PROOF_DEPTH {
            self.work_left = 0;
            self.exhausted = true;
            return None;
        }
        self.charge(owner.fields.len().max(1))?;
        if !seen.insert(ClassIdentity::of(owner)) {
            return Some(false);
        }
        let matching: Vec<_> = owner.fields.iter().filter(|f| f.name == name).collect();
        if !matching.is_empty() {
            let [field] = matching.as_slice() else { return None };
            // Only an actually visible, unique value permits the existing field
            // receiver path. Inaccessible or uncertain names must not fall back
            // to a same-spelled type after legacy filtering discards them.
            return match field.access {
                Access::Public => Some(true),
                Access::Private if depth == 0 => Some(true),
                Access::Package | Access::Protected if owner.package == self.file.package => Some(true),
                _ => None,
            };
        }
        let mut found = false;
        if let Some(edges) = &owner.reference_edges {
            for (parts, _) in edges {
                if let Some(parent) = self.edge(owner, parts)
                    && self.has_field(parent, name, depth + 1, seen)?
                {
                    if found {
                        return None;
                    }
                    found = true;
                }
            }
        }
        self.charge(0)?; // A missing edge is not an exhausted lookup.
        Some(found)
    }
    fn known_type_shadow(
        &mut self,
        owner: &'a ClassRec,
        name: &str,
        depth: usize,
        seen: &mut BTreeSet<ClassIdentity>,
    ) -> Option<bool> {
        if depth > REFERENCE_PROOF_DEPTH {
            self.work_left = 0;
            self.exhausted = true;
            return None;
        }
        self.charge(1)?;
        if !seen.insert(ClassIdentity::of(owner)) {
            return Some(false);
        }
        if let Some(edges) = &owner.reference_edges {
            for (parts, _) in edges {
                if let Some(parent) = self.edge(owner, parts)
                    && (!self.named(parent.scope, &format!("{}.{}", parent.dotted(), name))?.is_empty()
                        || self.known_type_shadow(parent, name, depth + 1, seen)?)
                {
                    return Some(true);
                }
            }
        }
        self.charge(0)?;
        Some(false)
    }
    // Match legacy visibility, but keep caller inheritance in this proof's
    // shared graph/work/depth budget. Unknown access is not a visible hint.
    fn binary_hint_visible(
        &mut self,
        member: &Member,
        declaring: &'a ClassRec,
        start: &'a ClassRec,
        byte: u32,
    ) -> Option<bool> {
        self.charge(1)?;
        match member.access {
            Access::Public | Access::Unspecified => Some(true),
            Access::Package => Some(self.file.package == declaring.package),
            Access::Private => Some(
                same_class(declaring, start)
                    && enclosing_classes(self.file, byte).iter().any(|class| same_class(class, declaring)),
            ),
            Access::Protected => {
                if self.file.package == declaring.package
                    || enclosing_classes(self.file, byte).iter().any(|class| same_class(class, start))
                {
                    return Some(true);
                }
                for class in enclosing_classes(self.file, byte) {
                    if self.graph(class)?.iter().any(|parent| same_class(parent, declaring)) {
                        return Some(true);
                    }
                }
                Some(false)
            }
        }
    }

    fn hints(&mut self, owner: &'a ClassRec, name: &str) -> Vec<(&'a ClassRec, &'a Member)> {
        let mut out = Vec::new();
        let _ = self.hint_walk(owner, name, 0, &mut BTreeSet::new(), &mut BTreeSet::new(), &mut out);
        out
    }
    fn hint_walk(
        &mut self,
        owner: &'a ClassRec,
        name: &str,
        depth: usize,
        gray: &mut BTreeSet<ClassIdentity>,
        black: &mut BTreeSet<ClassIdentity>,
        out: &mut Vec<(&'a ClassRec, &'a Member)>,
    ) -> Option<()> {
        if depth > REFERENCE_PROOF_DEPTH {
            self.work_left = 0;
            self.exhausted = true;
            return None;
        }
        self.charge(owner.methods.len().max(1))?;
        let id = ClassIdentity::of(owner);
        if gray.contains(&id) {
            self.failure = Some(ResolveResult::none(ResolveStatus::Cycle));
            return None;
        }
        if !black.insert(id.clone()) {
            return Some(());
        }
        gray.insert(id.clone());
        out.extend(
            owner
                .methods
                .iter()
                .filter(|m| m.name == name && (depth == 0 || navigation_method_inherited(owner, m)))
                .map(|m| (owner, m)),
        );
        if let Some(edges) = &owner.reference_edges {
            for (parts, _) in edges {
                if let Some(parent) = self.edge(owner, parts) {
                    self.hint_walk(parent, name, depth + 1, gray, black, out)?;
                }
            }
        }
        gray.remove(&id);
        Some(())
    }
    fn hint_signature(&mut self, owner: &'a ClassRec, member: &Member) -> Option<MethodIdentity> {
        let (parameters, generic) = match &member.shape {
            Shape::Unknown => return None,
            Shape::Source { parameters, variables } => (parameters.clone(), !variables.is_empty()),
            Shape::Binary { descriptor, generic, .. } => (descriptor_parameters(descriptor)?, *generic),
        };
        self.charge(parameters.len().max(1))?;
        let mut types = Vec::new();
        for p in parameters {
            let base = if generic || p.unsupported {
                None
            } else if let Some(primitive) = p.primitive {
                Some(TypeBase::Primitive(primitive))
            } else {
                let class = if owner.binary {
                    self.unique_named(owner.scope, &p.name.join("."))
                } else {
                    let file = self.index.files.iter().find(|f| f.scope == owner.scope && f.path == owner.path)?;
                    self.raw_type(file, &p.name, member.start)
                };
                class.map(|c| TypeBase::Declared(ClassIdentity::of(c)))
            };
            types.push(base.map(|base| SemanticType { base, array_rank: p.array_rank }));
        }
        Some(MethodIdentity { parameters: types, generic })
    }
    fn named(&mut self, scope: usize, name: &str) -> Option<Vec<&'a ClassRec>> {
        self.charge(self.index.visibility[scope].len().max(1))?;
        let classes = self.index.reference_named(scope, name);
        self.charge(classes.len().max(1))?;
        Some(classes)
    }
    fn raw_type(&mut self, file: &'a FileRec, parts: &[String], at: u32) -> Option<&'a ClassRec> {
        self.charge(file.singles.len() + file.stars.len() + file.classes.len() + file.local_types.len() + 1)?;
        let first = parts.first()?;
        if first == "var"
            || local_type_shadows(file, first, at)
            || enclosing_classes(file, at).iter().any(|c| c.type_variables.contains(first))
        {
            return None;
        }
        let mut lexical = None;
        for enclosing in enclosing_classes(file, at) {
            let found = self.named(file.scope, &format!("{}.{}", enclosing.dotted(), first))?;
            if !found.is_empty() {
                lexical = Some(found);
                break;
            }
        }
        let imports: BTreeSet<_> =
            file.singles.iter().filter(|(name, _)| name == first).map(|(_, name)| name).collect();
        if imports.len() > 1 {
            return None;
        }
        let found = if let Some(lexical) = lexical {
            lexical
        } else if let Some(import) = imports.first() {
            if file.classes.iter().any(|c| c.nesting.len() == 1 && c.simple() == first) {
                return None;
            }
            self.named(file.scope, import)?
        } else {
            let package_name =
                if file.package.is_empty() { first.clone() } else { format!("{}.{}", file.package, first) };
            let same_package = self.named(file.scope, &package_name)?;
            if same_package.is_empty() {
                let mut found = Vec::new();
                let stars: BTreeSet<_> = file.stars.iter().map(String::as_str).chain(["java.lang"]).collect();
                for star in stars {
                    found.extend(self.named(file.scope, &format!("{star}.{first}"))?);
                }
                if found.is_empty() && parts.len() > 1 {
                    return self.unique_named(file.scope, &parts.join("."));
                }
                found
            } else {
                same_package
            }
        };
        let [class] = found.as_slice() else {
            if found.len() > 1 {
                self.failure =
                    Some(ResolveResult::many(ResolveStatus::Ambiguous, found.iter().map(|c| c.target()).collect()));
            }
            return None;
        };
        if parts.len() == 1 {
            Some(*class)
        } else {
            self.unique_named(file.scope, &format!("{}.{}", class.dotted(), parts[1..].join(".")))
        }
    }
    fn unique_named(&mut self, scope: usize, name: &str) -> Option<&'a ClassRec> {
        let found = self.named(scope, name)?;
        match found.as_slice() {
            [one] => Some(*one),
            _ => {
                if found.len() > 1 {
                    self.failure =
                        Some(ResolveResult::many(ResolveStatus::Ambiguous, found.iter().map(|c| c.target()).collect()));
                }
                None
            }
        }
    }
    fn graph(&mut self, class: &'a ClassRec) -> Option<Vec<&'a ClassRec>> {
        self.charge(1)?;
        let key = ClassIdentity::of(class);
        if let Some(graph) = self.graphs.get(&key) {
            return Some(graph.clone());
        }
        let mut out = Vec::new();
        self.walk(class, 0, &mut BTreeSet::new(), &mut BTreeSet::new(), &mut out)?;
        self.graphs.insert(key, out.clone());
        Some(out)
    }
    fn walk(
        &mut self,
        class: &'a ClassRec,
        depth: usize,
        gray: &mut BTreeSet<ClassIdentity>,
        black: &mut BTreeSet<ClassIdentity>,
        out: &mut Vec<&'a ClassRec>,
    ) -> Option<()> {
        if depth > REFERENCE_PROOF_DEPTH {
            self.work_left = 0;
            self.exhausted = true;
            return None;
        }
        self.charge(1)?;
        let id = ClassIdentity::of(class);
        if gray.contains(&id) {
            return None;
        }
        if black.contains(&id) {
            return Some(());
        }
        if !class.reference_inventory
            || !self.unique_named(class.scope, &class.dotted()).is_some_and(|c| same_class(c, class))
        {
            return None;
        }
        self.charge(class.methods.len().max(1))?;
        gray.insert(id.clone());
        out.push(class);
        let edges = class.reference_edges.as_ref()?;
        // Only supplied binary java.lang.Object can certify a closed root.
        if edges.is_empty() && !class.binary_root {
            return None;
        }
        let mut edge_ids = BTreeSet::new();
        for (i, (parts, _)) in edges.iter().enumerate() {
            let parent = self.edge(class, parts)?;
            if parent.reference_final || (i == 0 && parent.is_interface) || (i > 0 && !parent.is_interface) {
                return None;
            }
            if !class.binary
                && (parent.nesting.len() != 1
                    || !matches!(parent.access, Access::Public | Access::Package)
                    || (parent.access == Access::Package && parent.package != class.package))
            {
                return None;
            }
            if !edge_ids.insert(ClassIdentity::of(parent)) {
                return None;
            }
            // An interface's synthetic Object edge is valid, but a class
            // cannot use an interface in its written extends slot. Syntax
            // metadata preserves the source edge order for that check below.
            self.walk(parent, depth + 1, gray, black, out)?;
        }
        gray.remove(&id);
        black.insert(id);
        Some(())
    }
    fn edge(&mut self, owner: &'a ClassRec, parts: &[String]) -> Option<&'a ClassRec> {
        self.charge(1)?;
        if owner.binary {
            let dotted = parts.join(".");
            self.charge(dotted.len().max(1))?;
            let raw = owner.binary_ancestor_names.get(&dotted)?.as_ref()?;
            self.charge(raw.len().max(1))?;
            let target = self.unique_named(owner.scope, &dotted)?;
            if let Some(signature) = &target.binary_signature {
                self.charge(signature.binary_name.len().max(1))?;
                if signature.binary_name != *raw {
                    return None;
                }
            } else {
                self.charge(target.package.len() + target.nesting.iter().map(String::len).sum::<usize>() + 1)?;
                let mut binary = target.package.replace('.', "/");
                if !binary.is_empty() {
                    binary.push('/');
                }
                binary.push_str(&target.nesting.join("$"));
                if binary != *raw {
                    return None;
                }
            }
            Some(target)
        } else {
            let file = self.index.files.iter().find(|f| f.scope == owner.scope && f.path == owner.path)?;
            self.raw_type(file, parts, owner.name_start)
        }
    }
    fn reference_type(&mut self, file: &'a FileRec, parts: &[String], at: u32) -> Option<&'a ClassRec> {
        let class = self.raw_type(file, parts, at)?;
        if class.reference_generic {
            return None;
        }
        self.accessible(class)?;
        if class.access == Access::Package && class.package != file.package {
            return None;
        }
        self.graph(class)?;
        // A member type inherited by a lexical owner could hide a package or
        // imported type. Do not use a missing caller hierarchy as absence.
        for enclosing in enclosing_classes(file, at) {
            for ancestor in self.graph(enclosing)? {
                self.charge(1)?;
                if !self.named(ancestor.scope, &format!("{}.{}", ancestor.dotted(), parts[0]))?.is_empty() {
                    return None;
                }
            }
        }
        Some(class)
    }
    fn local_reference(&mut self, local: &'a LocalRec) -> Option<&'a ClassRec> {
        let declared = local.reference.as_ref()?.plain()?;
        if declared.array_rank != 0 {
            return None;
        }
        let class = self.reference_type(self.file, &declared.parts, local.start)?;
        if local.catch_origin {
            if class.is_interface || class.reference_generic {
                return None;
            }
            // Resolve the real platform identity in this compilation; a same-named
            // source class or an incomplete hierarchy cannot establish a catch type.
            let throwable = self.unique_named(self.file.scope, "java.lang.Throwable")?;
            if !throwable.binary
                || throwable.is_interface
                || throwable.reference_generic
                || throwable.binary_signature.as_ref()?.binary_name != "java/lang/Throwable"
            {
                return None;
            }
            self.accessible(throwable)?;
            if !self.converts(class, throwable)? {
                return None;
            }
        }
        Some(class)
    }

    // Applied references are receiver evidence only, never erased scalar actuals.
    fn receiver(&mut self, node: &GreenNode, at: u32, depth: usize) -> Option<InvocationReceiver<'a>> {
        self.check_depth(depth)?;
        self.charge(node.children().len().max(1))?;
        if node.kind() == SyntaxKind::ParenExpr {
            let children = delimited_nodes(node, at)?;
            let [(inner, inner_at)] = children.as_slice() else { return None };
            return self.receiver(inner, *inner_at, depth + 1);
        }
        if node.kind() == SyntaxKind::ReferenceExpr {
            let name = token_name(node);
            if let Some(local) = self.argument_local(&name, at)? {
                if local.catch_origin {
                    return self.local_reference(local).map(|class| InvocationReceiver { class, instantiation: None });
                }
                return self.declared_receiver(local.reference.as_ref()?, local.start);
            }
            let field = self.own_field(&name, at)?;
            return self.declared_receiver(field.source_field.as_ref()?.reference.as_ref()?, field.start);
        }
        if node.kind() == SyntaxKind::FieldExpr {
            let field = self.this_field(node, at)?;
            return self.declared_receiver(field.source_field.as_ref()?.reference.as_ref()?, field.start);
        }
        self.expression(node, at, depth).map(|value| InvocationReceiver { class: value.class, instantiation: None })
    }

    fn declared_receiver(
        &mut self,
        reference: &ReferenceDeclaration,
        declared_at: u32,
    ) -> Option<InvocationReceiver<'a>> {
        if let ReferenceDeclaration::Plain(ty) = reference {
            if ty.array_rank != 0 {
                return None;
            }
            let class = self.reference_type(self.file, &ty.parts, declared_at)?;
            return Some(InvocationReceiver { class, instantiation: None });
        }
        let ReferenceDeclaration::Applied { name, arguments } = reference else { return None };
        self.charge(
            name.iter().map(String::len).sum::<usize>()
                + arguments.iter().flatten().flatten().map(String::len).sum::<usize>(),
        )?;
        let owner = self.raw_type(self.file, name, declared_at)?;
        if !owner.binary || !owner.reference_generic || owner.nesting.len() != 1 {
            return None;
        }
        self.accessible(owner)?;
        self.graph(owner)?;
        // Same lexical hiding checks as plain reference declarations.
        for enclosing in enclosing_classes(self.file, declared_at) {
            for ancestor in self.graph(enclosing)? {
                self.charge(1)?;
                if !self.named(ancestor.scope, &format!("{}.{}", ancestor.dotted(), name[0]))?.is_empty() {
                    return None;
                }
            }
        }
        let sig = owner.binary_signature.as_ref()?;
        let text = sig.signature.as_deref()?;
        self.charge(text.len())?;
        let parents: Vec<_> =
            sig.binary_super_class.iter().chain(sig.binary_interfaces.iter()).map(String::as_str).collect();
        let declaration = instantiation::ClassTemplate::parse(text, &parents)?;
        if declaration.parameters.len() != arguments.len() {
            return None;
        }
        let mut resolved = Vec::new();
        for parts in arguments {
            resolved.push(match parts {
                Some(parts) => Some(self.reference_type(self.file, parts, declared_at)?),
                None => None,
            });
        }
        Some(InvocationReceiver {
            class: owner,
            instantiation: Some(self.class_instantiation(owner, declaration, resolved)?),
        })
    }

    fn class_instantiation(
        &mut self,
        owner: &'a ClassRec,
        declaration: instantiation::ClassTemplate,
        arguments: Vec<Option<&'a ClassRec>>,
    ) -> Option<ClassInstantiation<'a>> {
        self.charge(arguments.len().max(1))?;
        if !owner.binary
            || owner.nesting.len() != 1
            || declaration.parameters.len() != arguments.len()
            || owner.reference_generic == declaration.parameters.is_empty()
        {
            return None;
        }
        // Unknown arguments do not certify any parameterized parent edge.
        // Limit this slice to the checked Object-only declaration envelope.
        if arguments.iter().any(Option::is_none)
            && (declaration.parents.len() != 1
                || declaration.parents[0].name != "java/lang/Object"
                || !declaration.parents[0].arguments.is_empty())
        {
            return None;
        }
        self.accessible(owner)?;
        self.graph(owner)?;
        let object = self.unique_named(owner.scope, "java.lang.Object")?;
        if !object.binary_root || object.binary_signature.as_ref()?.binary_name != "java/lang/Object" {
            return None;
        }
        self.graph(object)?;
        for argument in arguments.iter().flatten() {
            // Same spelling or independently closed graphs do not prove the bound.
            if !self.converts(argument, object)? {
                return None;
            }
        }
        Some(ClassInstantiation { owner: ClassIdentity::of(owner), declaration, arguments })
    }

    fn class_template(&mut self, owner: &'a ClassRec) -> Option<instantiation::ClassTemplate> {
        let sig = owner.binary_signature.as_ref()?;
        let parents: Vec<_> =
            sig.binary_super_class.iter().chain(sig.binary_interfaces.iter()).map(String::as_str).collect();
        self.charge(
            parents
                .iter()
                .map(|p| p.len())
                .sum::<usize>()
                .saturating_add(sig.signature.as_ref().map_or(0, String::len))
                .max(1),
        )?;
        if let Some(signature) = &sig.signature {
            instantiation::ClassTemplate::parse(signature, &parents)
        } else {
            Some(instantiation::ClassTemplate {
                parameters: Vec::new(),
                parents: parents
                    .into_iter()
                    .map(|name| instantiation::ParentTemplate { name: name.to_owned(), arguments: Vec::new() })
                    .collect(),
            })
        }
    }

    // Raw graph remains inventory only. This separate certificate refuses
    // branching/multiple environments instead of treating raw-node dedup as
    // generic inheritance proof. Only empty nongeneric leaf interfaces and
    // synthetic Object edges are neutral to the single non-Object parent chain.
    fn inherited_instantiations(
        &mut self,
        owner: &'a ClassRec,
        root: &ClassInstantiation<'a>,
    ) -> Option<Vec<ClassInstantiation<'a>>> {
        self.charge(root.arguments.len() + owner.binary_signature.as_ref()?.signature.as_ref().map_or(0, String::len))?;
        // Wildcard capture/substitution across parent edges is not proved here.
        if root.arguments.iter().any(Option::is_none) {
            return None;
        }
        let mut instances = vec![root.clone()];
        let mut seen = BTreeSet::from([ClassIdentity::of(owner)]);
        let mut current = owner;
        let mut object: Option<&'a ClassRec> = None;
        for depth in 0..=REFERENCE_PROOF_DEPTH {
            self.check_depth(depth)?;
            let env = instances.last()?;
            let edges = current.reference_edges.as_ref()?;
            if edges.len() != env.declaration.parents.len() {
                return None;
            }
            self.charge(edges.len().max(1))?;
            let mut next = None;
            for ((parts, _), template) in edges.iter().zip(&env.declaration.parents) {
                let parent = self.edge(current, parts)?;
                if !parent.binary {
                    return None;
                }
                if parent.binary_root {
                    if !template.arguments.is_empty() || object.is_some_and(|prior| !same_class(prior, parent)) {
                        return None;
                    }
                    object = Some(parent);
                    continue;
                }
                self.charge(parent.methods.len().max(1))?;
                let marker_edges = parent.reference_edges.as_ref()?;
                if parent.is_interface
                    && !parent.reference_generic
                    && parent.methods.is_empty()
                    && parent.binary_signature.as_ref()?.signature.is_none()
                    && marker_edges.len() == 1
                    && self.edge(parent, &marker_edges[0].0)?.binary_root
                {
                    if !template.arguments.is_empty() {
                        return None;
                    }
                    continue;
                }
                if next.is_some() {
                    return None;
                }
                let declaration = self.class_template(parent)?;
                let mut arguments = Vec::new();
                for argument in &template.arguments {
                    let Some(StrictActual::Reference(value)) = self.template_value(current, env, argument)? else {
                        return None;
                    };
                    arguments.push(Some(value.class));
                }
                let instance = self.class_instantiation(parent, declaration, arguments)?;
                next = Some((parent, instance));
            }
            if let Some((parent, instance)) = next {
                if !seen.insert(ClassIdentity::of(parent)) {
                    return None;
                }
                current = parent;
                instances.push(instance);
            } else {
                let object = object?;
                let declaration = self.class_template(object)?;
                instances.push(self.class_instantiation(object, declaration, Vec::new())?);
                return Some(instances);
            }
        }
        self.check_depth(REFERENCE_PROOF_DEPTH + 1)?;
        None
    }

    // A declaration-level certificate, independent of concrete caller arguments.
    // Between the two declaring owners, each edge must only rename formals in
    // order. Equal closed types alone could hide an illegal erasure clash.
    fn identity_override(
        &mut self,
        instances: &[ClassInstantiation<'a>],
        left: (&'a ClassRec, &'a Member),
        right: (&'a ClassRec, &'a Member),
    ) -> Option<()> {
        self.charge(instances.len().saturating_mul(2).max(1))?;
        let a = instances.iter().position(|e| e.owner == ClassIdentity::of(left.0))?;
        let b = instances.iter().position(|e| e.owner == ClassIdentity::of(right.0))?;
        if a == b {
            return None;
        }
        for pair in instances[a.min(b)..=a.max(b)].windows(2) {
            let child = &pair[0].declaration;
            let parent = &pair[1];
            self.charge(child.parents.iter().map(|p| p.name.len() + p.arguments.len()).sum::<usize>().max(1))?;
            let edge = child.parents.iter().find(|p| p.name.replace('/', ".") == parent.owner.name)?;
            if child.parameters.len() != parent.declaration.parameters.len()
                || edge.arguments.len() != child.parameters.len()
                || !edge.arguments.iter().enumerate().all(|(i, t)| *t == instantiation::TypeTemplate::Parameter(i))
            {
                return None;
            }
        }
        let template = |member: &Member, env: &ClassInstantiation<'a>| {
            let Shape::Binary { descriptor, signature, .. } = &member.shape else { return None };
            instantiation::MethodTemplate::parse(signature.as_deref(), descriptor, &env.declaration)
        };
        for member in [left.1, right.1] {
            let Shape::Binary { descriptor, signature, .. } = &member.shape else { return None };
            self.charge(descriptor.len().saturating_add(signature.as_ref().map_or(0, String::len)).max(1))?;
        }
        // Formal indices are normalized by MethodTemplate, not their spelling.
        // Nominal equality also needs the existing same-scope/raw override proof.
        (template(left.1, &instances[a])? == template(right.1, &instances[b])?).then_some(())
    }

    fn own_field(&mut self, name: &str, at: u32) -> Option<&'a Member> {
        let owner = self.implicit_instance_owner(at)?;
        self.graph(owner)?;
        self.charge(owner.fields.len().max(1))?;
        let mut fields = owner.fields.iter().filter(|f| f.name == name);
        let field = fields.next()?;
        if fields.next().is_some() || field.source_field.as_ref()?.is_static {
            return None;
        }
        // The declaration belongs to the caller itself, including private access.
        // An own declaration hides ancestors; absent/unknown own fields never
        // borrow an inherited declaration or a legacy navigation type.
        Some(field)
    }

    fn this_field(&mut self, node: &GreenNode, at: u32) -> Option<&'a Member> {
        self.charge(node.children().len().max(1))?;
        if node.kind() != SyntaxKind::FieldExpr {
            return None;
        }
        let mut state = 0;
        let mut name = None;
        for child in node.children() {
            if let Some(base) = child.as_node() {
                if state != 0 || base.kind() != SyntaxKind::ThisExpr || base.child_nodes().next().is_some() {
                    return None;
                }
                self.charge(base.children().len().max(1))?;
                let direct: Vec<_> = tokens(base, 0).into_iter().filter(|t| !t.0.is_trivia()).map(|t| t.0).collect();
                if direct != [SyntaxKind::This] {
                    return None;
                }
                state = 1;
            } else if let Some(token) = child.as_token()
                && !token.kind().is_trivia()
            {
                state = match (state, token.kind()) {
                    (1, SyntaxKind::Dot) => 2,
                    (2, SyntaxKind::Ident) => {
                        name = Some(token.text());
                        3
                    }
                    _ => return None,
                };
            }
        }
        if state != 3 {
            return None;
        }
        self.own_field(name?, at)
    }

    fn field_actual(&mut self, field: &'a Member) -> Option<StrictActual<'a>> {
        let evidence = field.source_field.as_ref()?;
        if let Some(reference) = &evidence.reference {
            let plain = reference.plain()?;
            if plain.array_rank != 0 {
                return None;
            }
            let class = self.reference_type(self.file, &plain.parts, field.start)?;
            return Some(StrictActual::Reference(ReferenceValue { class }));
        }
        evidence.primitive.filter(|v| v.rank == 0).map(StrictActual::Primitive)
    }

    fn template_value(
        &mut self,
        owner: &'a ClassRec,
        env: &ClassInstantiation<'a>,
        ty: &instantiation::TypeTemplate,
    ) -> Option<Option<StrictActual<'a>>> {
        use instantiation::TypeTemplate;
        self.charge(1)?;
        match ty {
            TypeTemplate::Void => Some(None),
            TypeTemplate::Primitive(primitive) => {
                Some(Some(StrictActual::Primitive(PrimitiveValue { primitive: *primitive, rank: 0 })))
            }
            TypeTemplate::Parameter(index) => Some(Some(StrictActual::Reference(ReferenceValue {
                class: env.arguments.get(*index).copied().flatten()?,
            }))),
            TypeTemplate::Nominal(raw) => {
                self.charge(raw.len())?;
                let class = self.unique_named(owner.scope, &raw.replace('/', "."))?;
                let actual = if let Some(sig) = &class.binary_signature {
                    sig.binary_name.clone()
                } else {
                    format!(
                        "{}{}{}",
                        class.package.replace('.', "/"),
                        if class.package.is_empty() { "" } else { "/" },
                        class.nesting.join("$")
                    )
                };
                self.charge(actual.len())?;
                if actual != *raw || class.reference_generic {
                    return None;
                }
                self.accessible(class)?;
                self.graph(class)?;
                Some(Some(StrictActual::Reference(ReferenceValue { class })))
            }
        }
    }

    fn expression(&mut self, node: &GreenNode, at: u32, depth: usize) -> Option<ReferenceValue<'a>> {
        if depth > REFERENCE_PROOF_DEPTH {
            self.work_left = 0;
            self.exhausted = true;
            return None;
        }
        self.charge(node.children().len().max(1))?;
        if node.kind() == SyntaxKind::ParenExpr {
            let expressions = delimited_nodes(node, at)?;
            let [(node, at)] = expressions.as_slice() else { return None };
            return self.expression(node, *at, depth + 1);
        }
        if node.kind() == SyntaxKind::LiteralExpr {
            let mut literal = None;
            for child in node.children() {
                let token = child.as_token()?;
                if token.kind().is_trivia() {
                    continue;
                }
                if token.kind() != SyntaxKind::StringLiteral || literal.is_some() {
                    return None;
                }
                self.charge(token.text().len())?;
                if !ordinary_string_literal(token.text()) {
                    return None;
                }
                literal = Some(());
            }
            literal?;
            // A literal has the platform type independently of lexical String
            // or java name shadows. Source replacements cannot certify it.
            let class = self.unique_named(self.file.scope, "java.lang.String")?;
            if !class.binary
                || class.access != Access::Public
                || class.nesting.len() != 1
                || class.is_interface
                || class.reference_generic
                || !class.reference_final
            {
                return None;
            }
            self.accessible(class)?;
            self.graph(class)?;
            return Some(ReferenceValue { class });
        }
        if node.kind() == SyntaxKind::IndexExpr {
            let [(mut base, mut base_at), (index, index_at)] = index_operands(node, at)?;
            let mut depth = depth;
            while base.kind() == SyntaxKind::ParenExpr {
                self.check_depth(depth + 1)?;
                self.charge(base.children().len().max(1))?;
                let children = delimited_nodes(base, base_at)?;
                let [(inner, offset)] = children.as_slice() else { return None };
                base = inner;
                base_at = *offset;
                depth += 1;
            }
            if base.kind() != SyntaxKind::ReferenceExpr {
                return None;
            }
            let local = self.visible_argument_local(&token_name(base), base_at)?;
            let declared = local.reference.as_ref()?.plain()?;
            if declared.array_rank != 1 || local.catch_origin {
                return None;
            }
            let StrictActual::Primitive(index) = self.actual(index, index_at, depth + 1)? else { return None };
            if index.rank != 0
                || !matches!(index.primitive, Primitive::Byte | Primitive::Short | Primitive::Char | Primitive::Int)
            {
                return None;
            }
            // An array is never a scalar receiver. Only one proved index into
            // an explicitly one-dimensional declaration produces its element.
            let class = self.reference_type(self.file, &declared.parts, local.start)?;
            return Some(ReferenceValue { class });
        }
        if node.kind() == SyntaxKind::NewExpr {
            let mut state = 0;
            let mut ty = None;
            let mut offset = at;
            for child in node.children() {
                if let Some(token) = child.as_token() {
                    if !token.kind().is_trivia() {
                        state = match (state, token.kind()) {
                            (0, SyntaxKind::New) => 1,
                            _ => return None,
                        };
                    }
                } else if let Some(n) = child.as_node() {
                    self.charge(n.children().len().max(1))?;
                    state = match (state, n.kind()) {
                        (1, SyntaxKind::Type) if plain_reference_type(n) => {
                            ty = Some(type_parts(n, offset)?);
                            2
                        }
                        (2, SyntaxKind::ArgList) if delimited_nodes(n, offset)?.is_empty() => 3,
                        _ => return None,
                    };
                }
                offset += child.utf8_len();
            }
            if state != 3 {
                return None;
            }
            let class = self.reference_type(self.file, &ty?, at)?;
            self.charge(class.reference_constructor_work?)?;
            return Some(ReferenceValue { class });
        }
        if node.kind() != SyntaxKind::ReferenceExpr {
            return None;
        }
        let name = token_name(node);
        let local = self.visible_argument_local(&name, at)?;
        let class = self.local_reference(local)?;
        Some(ReferenceValue { class })
    }
    // Only an explicit plain-reference cast of null supplies new argument evidence.
    // Keep this out of general expression/receiver typing and retain one depth/work budget.
    fn cast_null_argument(&mut self, node: &GreenNode, at: u32, depth: usize) -> Option<ReferenceValue<'a>> {
        if depth > REFERENCE_PROOF_DEPTH {
            self.work_left = 0;
            self.exhausted = true;
            return None;
        }
        self.charge(node.children().len().max(1))?;
        if node.kind() == SyntaxKind::ParenExpr {
            let children = delimited_nodes(node, at)?;
            let [(inner, at)] = children.as_slice() else { return None };
            return self.cast_null_argument(inner, *at, depth + 1);
        }
        if node.kind() != SyntaxKind::CastExpr {
            return None;
        }
        let mut state = 0;
        let mut parts = None;
        let mut operand = None;
        let mut offset = at;
        for child in node.children() {
            if let Some(token) = child.as_token() {
                if !token.kind().is_trivia() {
                    state = match (state, token.kind()) {
                        (0, SyntaxKind::LParen) => 1,
                        (2, SyntaxKind::RParen) => 3,
                        _ => return None,
                    };
                }
            } else if let Some(inner) = child.as_node() {
                self.charge(inner.children().len().max(1))?;
                state = match state {
                    1 if inner.kind() == SyntaxKind::Type && plain_reference_type(inner) => {
                        parts = Some(type_parts(inner, offset)?);
                        2
                    }
                    3 => {
                        operand = Some((inner.as_ref(), offset));
                        4
                    }
                    _ => return None,
                };
            }
            offset += child.utf8_len();
        }
        if state != 4 {
            return None;
        }
        let (operand, operand_at) = operand?;
        self.null_argument(operand, operand_at, depth + 1)?;
        let class = self.reference_type(self.file, &parts?, at)?;
        Some(ReferenceValue { class })
    }

    // Null is its own strict-argument category, not Object or unknown type.
    // Keep this out of general expression/receiver typing and share the proof budget.
    fn null_argument(&mut self, node: &GreenNode, at: u32, depth: usize) -> Option<()> {
        if depth > REFERENCE_PROOF_DEPTH {
            self.work_left = 0;
            self.exhausted = true;
            return None;
        }
        self.charge(node.children().len().max(1))?;
        if node.kind() == SyntaxKind::ParenExpr {
            let children = delimited_nodes(node, at)?;
            let [(inner, at)] = children.as_slice() else { return None };
            return self.null_argument(inner, *at, depth + 1);
        }
        if node.kind() != SyntaxKind::LiteralExpr {
            return None;
        }
        let mut found = false;
        for child in node.children() {
            let token = child.as_token()?;
            if token.kind().is_trivia() {
                continue;
            }
            if found || token.kind() != SyntaxKind::Null || token.text() != "null" {
                return None;
            }
            found = true;
        }
        found.then_some(())
    }

    // A fallback after reference typing fails: explicit scalar declarations,
    // range-checked decimal int tokens and bounded integral +/- compositions.
    // No field/array operands, call-result leaves, boxing or general inference.
    fn scalar_primitive_argument(&mut self, node: &GreenNode, at: u32, depth: usize) -> Option<PrimitiveValue> {
        if depth > REFERENCE_PROOF_DEPTH {
            self.work_left = 0;
            self.exhausted = true;
            return None;
        }
        self.charge(node.children().len().max(1))?;
        if node.kind() == SyntaxKind::ParenExpr {
            let children = delimited_nodes(node, at)?;
            let [(inner, at)] = children.as_slice() else { return None };
            return self.scalar_primitive_argument(inner, *at, depth + 1);
        }
        if node.kind() == SyntaxKind::UnaryExpr {
            // Postfix only, on one direct name. Never recurse into arbitrary
            // operands: prefix, nested updates, fields and array elements refuse.
            let mut operand = None;
            let mut updated = false;
            let mut offset = at;
            for child in node.children() {
                if let Some(inner) = child.as_node() {
                    if operand.is_some() || updated || inner.kind() != SyntaxKind::ReferenceExpr {
                        return None;
                    }
                    operand = Some((inner.as_ref(), offset));
                } else if let Some(token) = child.as_token()
                    && !token.kind().is_trivia()
                {
                    if operand.is_none()
                        || updated
                        || !matches!(token.kind(), SyntaxKind::PlusPlus | SyntaxKind::MinusMinus)
                    {
                        return None;
                    }
                    updated = true;
                }
                offset += child.utf8_len();
            }
            if !updated {
                return None;
            }
            let (inner, name_at) = operand?;
            self.charge(inner.children().len().max(1))?;
            let direct: Vec<_> = tokens(inner, name_at).into_iter().filter(|t| !t.0.is_trivia()).collect();
            if inner.child_nodes().next().is_some() || direct.len() != 1 || direct[0].0 != SyntaxKind::Ident {
                return None;
            }
            let local = self.visible_argument_local(&token_name(inner), name_at)?;
            if name_at < local.postfix_after? {
                return None;
            }
            // Unlike binary addition, postfix preserves byte/short/char.
            return local.primitive.filter(|v| {
                v.rank == 0
                    && matches!(
                        v.primitive,
                        Primitive::Byte | Primitive::Short | Primitive::Char | Primitive::Int | Primitive::Long
                    )
            });
        }
        if node.kind() == SyntaxKind::BinaryExpr {
            // BinaryExpr also represents assignments/conditionals: consume the
            // complete direct shape, never search descendants for usable leaves.
            let mut state = 0;
            let mut left = None;
            let mut right = None;
            let mut offset = at;
            for child in node.children() {
                if let Some(inner) = child.as_node() {
                    match state {
                        0 => {
                            left = Some((inner.as_ref(), offset));
                            state = 1;
                        }
                        2 => {
                            right = Some((inner.as_ref(), offset));
                            state = 3;
                        }
                        _ => return None,
                    }
                } else if let Some(token) = child.as_token()
                    && !token.kind().is_trivia()
                {
                    if state != 1 || !matches!(token.kind(), SyntaxKind::Plus | SyntaxKind::Minus) {
                        return None;
                    }
                    state = 2;
                }
                offset += child.utf8_len();
            }
            if state != 3 {
                return None;
            }
            let (left, left_at) = left?;
            let (right, right_at) = right?;
            // Same proof and budget, scalar declaration/literal leaves only:
            // no invocation results, fields, array reads, boxing or evaluation.
            let left = self.scalar_primitive_argument(left, left_at, depth + 1)?;
            let right = self.scalar_primitive_argument(right, right_at, depth + 1)?;
            if [left, right].iter().any(|v| {
                v.rank != 0
                    || !matches!(
                        v.primitive,
                        Primitive::Byte | Primitive::Short | Primitive::Char | Primitive::Int | Primitive::Long
                    )
            }) {
                return None;
            }
            let primitive = if left.primitive == Primitive::Long || right.primitive == Primitive::Long {
                Primitive::Long
            } else {
                Primitive::Int
            };
            return Some(PrimitiveValue { primitive, rank: 0 });
        }
        if node.kind() == SyntaxKind::LiteralExpr {
            let mut literal = None;
            for child in node.children() {
                let token = child.as_token()?;
                if token.kind().is_trivia() {
                    continue;
                }
                if token.kind() != SyntaxKind::IntLiteral || literal.is_some() {
                    return None;
                }
                let text = token.text();
                self.charge(text.len())?;
                if text.is_empty()
                    || !text.bytes().all(|b| b.is_ascii_digit())
                    || (text.len() > 1 && text.starts_with('0'))
                    || text.parse::<u32>().ok()? > i32::MAX as u32
                {
                    return None;
                }
                literal = Some(PrimitiveValue { primitive: Primitive::Int, rank: 0 });
            }
            return literal;
        }
        if node.kind() != SyntaxKind::ReferenceExpr {
            return None;
        }
        let name = token_name(node);
        let local = self.visible_argument_local(&name, at)?;
        local.primitive.filter(|value| value.rank == 0)
    }

    // Flat-classpath compatibility for one exact binary argument root only:
    // both scopes must select identical modeled signatures/origins throughout
    // their complete inheritance closures. Not global Java type identity.
    fn binary_closure_evidence(&mut self, actual: &'a ClassRec, formal: &'a ClassRec) -> Option<()> {
        let left = self.graph(actual)?;
        let right = self.graph(formal)?;
        self.charge(left.len().max(1))?;
        if left.len() != right.len() {
            return None;
        }
        for (left, right) in left.iter().zip(&right) {
            let a = left.binary_signature.as_ref()?;
            let b = right.binary_signature.as_ref()?;
            if Arc::ptr_eq(a, b) {
                continue;
            }
            // Charge every variable-length part before comparing its contents.
            // Distinct copies too large for this proof conservatively refuse.
            for sig in [a, b] {
                self.charge(
                    sig.nesting.len()
                        + sig.interfaces.len()
                        + sig.binary_interfaces.len()
                        + sig.fields.len()
                        + sig.methods.len()
                        + 1,
                )?;
                for text in [&sig.origin, &sig.binary_name, &sig.dotted_name, &sig.package]
                    .into_iter()
                    .chain(sig.nesting.iter())
                    .chain(sig.interfaces.iter())
                    .chain(sig.super_class.iter())
                    .chain(sig.binary_interfaces.iter())
                    .chain(sig.binary_super_class.iter())
                    .chain(sig.signature.iter())
                {
                    self.charge(text.len())?;
                }
                for member in sig.fields.iter().chain(&sig.methods) {
                    self.charge(member.name.len())?;
                    self.charge(member.descriptor.len())?;
                    if let Some(signature) = &member.signature {
                        self.charge(signature.len())?;
                    }
                }
            }
            if a != b {
                return None;
            }
        }
        Some(())
    }
    // Local conversion certificate only: shared cast/override edge metadata is
    // unchanged. Unsupported signature grammar and parameterized paths stay unknown.
    fn plain_binary_parent_edges(&mut self, class: &ClassRec) -> Option<Vec<bool>> {
        if !class.binary || class.reference_generic || class.nesting.len() != 1 {
            return None;
        }
        let sig = class.binary_signature.as_ref()?;
        let text = sig.signature.as_ref()?;
        self.charge(text.len().saturating_add(1))?;
        let parsed = conversion_signature_edges(text)?;
        let names: Vec<_> = sig.binary_super_class.iter().chain(&sig.binary_interfaces).collect();
        if names.len() != parsed.len() {
            return None;
        }
        let mut plain = Vec::new();
        for ((name, unparameterized), expected) in parsed.iter().zip(names) {
            self.charge(name.len().saturating_add(expected.len()).max(1))?;
            if name != expected {
                return None;
            }
            plain.push(*unparameterized);
        }
        Some(plain)
    }

    fn converts(&mut self, actual: &'a ClassRec, formal: &'a ClassRec) -> Option<bool> {
        self.graph(actual)?;
        self.graph(formal)?;
        let mut todo = vec![(actual, 0)];
        let mut visited = BTreeSet::new();
        let mut unknown = false;
        while let Some((class, depth)) = todo.pop() {
            if depth > REFERENCE_PROOF_DEPTH {
                self.work_left = 0;
                self.exhausted = true;
                return None;
            }
            self.charge(1)?;
            if !visited.insert(ClassIdentity::of(class)) {
                continue;
            }
            if same_class(class, formal) {
                return Some(true);
            }
            let certified = if class.binary && class.binary_signature.as_ref()?.signature.is_some() {
                self.plain_binary_parent_edges(class)
            } else {
                None
            };
            for (index, (parts, plain)) in class.reference_edges.as_ref()?.iter().enumerate() {
                if !plain && !certified.as_ref().and_then(|edges| edges.get(index)).copied().unwrap_or(false) {
                    unknown = true;
                    continue;
                }
                let parent = self.edge(class, parts)?;
                if parent.reference_generic {
                    unknown = true;
                    continue;
                }
                todo.push((parent, depth + 1));
            }
        }
        if unknown { None } else { Some(false) }
    }
    fn check_depth(&mut self, depth: usize) -> Option<()> {
        if depth > REFERENCE_PROOF_DEPTH {
            self.work_left = 0;
            self.exhausted = true;
            None
        } else {
            Some(())
        }
    }

    // One argument classifier and one selector serve both call arguments and receivers.
    // One bounded lookup shared by reference and primitive local evidence.
    // No persistent cache: overlapping, expired or untyped declarations still refuse.
    fn visible_argument_local(&mut self, name: &str, at: u32) -> Option<&'a LocalRec> {
        self.argument_local(name, at)?
    }

    // Some(None) proves absence; outer None is ambiguity/unsupported/budget
    // failure. Only proved absence may fall back to a same-named own field.
    fn argument_local(&mut self, name: &str, at: u32) -> Option<Option<&'a LocalRec>> {
        let mut found = None;
        let mut count = 0;
        let mut consider = |local: &'a LocalRec| {
            if local.name == name && local.scope_start <= at && at < local.scope_end && local.visible_from <= at {
                found = Some(local);
                count += 1;
            }
        };
        if self.caller_locals.is_none() {
            self.charge(self.file.locals.len())?;
            let mut subset = Vec::new();
            for local in &self.file.locals {
                if local.start >= self.caller_start && local.end <= self.caller_end {
                    subset.push(local);
                    // The initial full scan also answers this lookup. Do not
                    // immediately scan the newly collected subset a second time.
                    consider(local);
                }
            }
            self.caller_locals = Some(subset);
        } else {
            self.charge(self.caller_locals.as_ref()?.len().max(1))?;
            for local in self.caller_locals.as_ref()? {
                consider(local);
            }
        }
        if count == 0 {
            return Some(None);
        }
        let local = found?;
        (count == 1 && matches!(local.kind, TargetKind::Local | TargetKind::Parameter)).then_some(Some(local))
    }

    fn actual(&mut self, node: &GreenNode, at: u32, depth: usize) -> Option<StrictActual<'a>> {
        self.check_depth(depth)?;
        self.charge(node.children().len().max(1))?;
        if node.kind() == SyntaxKind::ParenExpr {
            let children = delimited_nodes(node, at)?;
            let [(inner, at)] = children.as_slice() else { return None };
            return self.actual(inner, *at, depth + 1);
        }
        if node.kind() == SyntaxKind::CallExpr {
            return self.invocation_value(node, at, depth + 1);
        }
        if node.kind() == SyntaxKind::FieldExpr {
            let field = self.this_field(node, at)?;
            return self.field_actual(field);
        }
        if node.kind() == SyntaxKind::ReferenceExpr {
            let name = token_name(node);
            let Some(local) = self.argument_local(&name, at)? else {
                let field = self.own_field(&name, at)?;
                return self.field_actual(field);
            };
            // A reference declaration failing its type proof cannot become a
            // primitive guess. Each category uses only its own stored evidence.
            if local.reference.is_some() {
                let class = self.local_reference(local)?;
                return Some(StrictActual::Reference(ReferenceValue { class }));
            }
            return local.primitive.filter(|value| value.rank == 0).map(StrictActual::Primitive);
        }
        self.expression(node, at, depth)
            .map(StrictActual::Reference)
            .or_else(|| self.scalar_primitive_argument(node, at, depth).map(StrictActual::Primitive))
            .or_else(|| self.null_argument(node, at, depth).map(|()| StrictActual::Null))
            .or_else(|| self.cast_null_argument(node, at, depth).map(StrictActual::Reference))
    }

    // Shared by direct navigation and nested invocation typing. A failed value
    // proof is never sufficient evidence that the same spelling is a type.
    fn static_type_owner(&mut self, name: &str, at: u32) -> Option<&'a ClassRec> {
        if !self.file.call_inventory_valid {
            return None;
        }
        self.charge(self.file.locals.len().max(1))?;
        if self.file.locals.iter().any(|local| {
            local.name == name && local.scope_start <= at && at < local.scope_end && local.visible_from <= at
        }) {
            return None;
        }
        for enclosing in enclosing_classes(self.file, at) {
            if self.has_field(enclosing, name, 0, &mut BTreeSet::new())? {
                return None;
            }
        }
        let owner = self.reference_type(self.file, &[name.to_owned()], at)?;
        (!owner.is_interface && owner.nesting.len() == 1).then_some(owner)
    }

    fn implicit_instance_owner(&mut self, at: u32) -> Option<&'a ClassRec> {
        if !self.file.call_inventory_valid || at < self.caller_start || at >= self.caller_end {
            return None;
        }
        self.charge(self.file.classes.len().max(1))?;
        let enclosing = enclosing_classes(self.file, at);
        let [owner] = enclosing.as_slice() else { return None };
        let owner = *owner;
        if owner.binary || owner.nesting.len() != 1 || owner.is_interface || owner.reference_generic {
            return None;
        }
        let declarations = match self.caller_kind? {
            CallerKind::Method => &owner.methods,
            CallerKind::Constructor => &owner.constructors,
        };
        self.charge(declarations.len().max(1))?;
        let mut callers = declarations.iter().filter(|m| m.start >= self.caller_start && m.end <= self.caller_end);
        let caller = callers.next()?;
        if callers.next().is_some() {
            return None;
        }
        match self.caller_kind? {
            CallerKind::Method if !caller.source_call.as_ref()?.is_static => {}
            CallerKind::Constructor if caller.source_call.is_none() && owner.nesting.last()? == &caller.name => {}
            _ => return None,
        }
        let Shape::Source { variables, .. } = &caller.shape else { return None };
        variables.is_empty().then_some(owner)
    }

    fn invocation_value(&mut self, node: &GreenNode, at: u32, depth: usize) -> Option<StrictActual<'a>> {
        self.check_depth(depth)?;
        if node.kind() != SyntaxKind::CallExpr {
            return None;
        }
        self.charge(node.children().len().max(1))?;
        let (args, args_at) = child_kind(node, at, SyntaxKind::ArgList)?;
        if let Some((callee, callee_at)) = child_kind(node, at, SyntaxKind::ReferenceExpr) {
            if node.child_nodes().count() != 2
                || node.children().iter().any(|c| c.as_token().is_some_and(|t| !t.kind().is_trivia()))
            {
                return None;
            }
            let owner = self.implicit_instance_owner(callee_at)?;
            let selected =
                self.select_mode(owner, &token_name(callee), args, args_at, depth + 1, InvocationMode::OwnInstance)?;
            return self.selected_value(&selected);
        }
        let (callee, callee_at) = child_kind(node, at, SyntaxKind::FieldExpr)?;
        if node.child_nodes().count() != 2
            || node.children().iter().any(|c| c.as_token().is_some_and(|t| !t.kind().is_trivia()))
        {
            return None;
        }
        self.charge(callee.children().len().max(1))?;
        let mut offset = callee_at;
        let mut state = 0;
        let mut base = None;
        let mut name = None;
        for child in callee.children() {
            if let Some(n) = child.as_node() {
                if state != 0 {
                    return None;
                }
                base = Some((n.as_ref(), offset));
                state = 1;
            } else if let Some(t) = child.as_token()
                && !t.kind().is_trivia()
            {
                state = match (state, t.kind()) {
                    (1, SyntaxKind::Dot) => 2,
                    (2, SyntaxKind::Ident) => {
                        name = Some(t.text());
                        3
                    }
                    _ => return None,
                };
            }
            offset += child.utf8_len();
        }
        if state != 3 {
            return None;
        }
        let (mut base, mut base_at) = base?;
        let type_qualifier_allowed = base.kind() == SyntaxKind::ReferenceExpr;
        let mut depth = depth;
        while base.kind() == SyntaxKind::ParenExpr {
            self.charge(base.children().len().max(1))?;
            self.check_depth(depth + 1)?;
            let children = delimited_nodes(base, base_at)?;
            let [(inner, at)] = children.as_slice() else { return None };
            base = inner;
            base_at = *at;
            depth += 1;
        }
        if matches!(base.kind(), SyntaxKind::ReferenceExpr | SyntaxKind::FieldExpr)
            && let Some(receiver) = self.receiver(base, base_at, depth + 1)
        {
            let selected = self.select_receiver(&receiver, name?, args, args_at, depth + 1)?;
            return self.selected_value(&selected);
        }
        let (owner, mode) = match base.kind() {
            SyntaxKind::ReferenceExpr => {
                if let Some(value) = self.expression(base, base_at, depth + 1) {
                    (value.class, InvocationMode::Instance)
                } else if type_qualifier_allowed {
                    (self.static_type_owner(&token_name(base), base_at)?, InvocationMode::TypeStatic)
                } else {
                    return None;
                }
            }
            SyntaxKind::IndexExpr => (self.expression(base, base_at, depth + 1)?.class, InvocationMode::Instance),
            SyntaxKind::CallExpr => match self.invocation_value(base, base_at, depth + 1)? {
                StrictActual::Reference(value) => (value.class, InvocationMode::Instance),
                _ => return None,
            },
            _ => return None,
        };
        let selected = self.select_mode(owner, name?, args, args_at, depth + 1, mode)?;
        self.selected_value(&selected)
    }

    fn selected_value(&mut self, selected: &SelectedMethod<'a>) -> Option<StrictActual<'a>> {
        if let Some(value) = selected.instantiated_result {
            return Some(value);
        }
        if selected.declaring.binary {
            return self.binary_invocation_value(selected);
        }
        let source = selected.member.source_call.as_ref()?;
        if let Some(primitive) = source.scalar_return {
            return Some(StrictActual::Primitive(PrimitiveValue { primitive, rank: 0 }));
        }
        let parts = source.reference_return.as_ref()?;
        let declaring = selected.declaring;
        let file = self.index.files.iter().find(|f| f.scope == declaring.scope && f.path == declaring.path)?;
        // Return names belong to the selected declaration, not the consuming caller.
        let class = self.reference_type(file, parts, selected.member.start)?;
        Some(StrictActual::Reference(ReferenceValue { class }))
    }

    // A strict selected declaration, never a legacy name-navigation result,
    // supplies its return only after complete overload/override validation.
    fn binary_invocation_value(&mut self, selected: &SelectedMethod<'a>) -> Option<StrictActual<'a>> {
        let Shape::Binary { descriptor, .. } = &selected.member.shape else { return None };
        self.charge(descriptor.len().max(1))?;
        let (_, result) = descriptor_parts(descriptor)?;
        let result = result?; // void supplies no expression type
        if result.unsupported || result.array_rank != 0 {
            return None;
        }
        if let Some(primitive) = result.primitive {
            return Some(StrictActual::Primitive(PrimitiveValue { primitive, rank: 0 }));
        }
        let raw = result.binary_name.as_ref()?;
        if raw.contains('$') {
            return None; // nested/dollar return identity/access is not modeled
        }
        self.charge(raw.len().max(1))?;
        // Descriptor names are owned by the selected declaration's linking
        // context, not by caller imports, locals or a same-named caller class.
        let class = self.unique_named(selected.declaring.scope, &result.name.join("."))?;
        if let Some(signature) = &class.binary_signature {
            self.charge(signature.binary_name.len().max(1))?;
            if signature.binary_name != *raw {
                return None;
            }
        } else {
            self.charge(class.package.len() + class.nesting.iter().map(String::len).sum::<usize>() + 1)?;
            let mut binary = class.package.replace('.', "/");
            if !binary.is_empty() {
                binary.push('/');
            }
            binary.push_str(&class.nesting.join("$"));
            if binary != *raw {
                return None;
            }
        }
        if class.reference_generic {
            return None;
        }
        self.accessible(class)?;
        self.graph(class)?;
        Some(StrictActual::Reference(ReferenceValue { class }))
    }

    // Validate each ancestor pair before removing any candidate. A nearer node
    // never excuses a final or incompatible declaration elsewhere in the chain.
    fn source_override(&mut self, child: (&'a ClassRec, &'a Member), parent: (&'a ClassRec, &'a Member)) -> Option<()> {
        if same_class(child.0, parent.0) || !self.graph(child.0)?.iter().any(|ancestor| same_class(ancestor, parent.0))
        {
            return None;
        }
        let child_facts = child.1.source_call.as_ref()?.override_facts.as_ref()?;
        let parent_facts = parent.1.source_call.as_ref()?.override_facts.as_ref()?;
        self.charge(child_facts.work.saturating_add(parent_facts.work))?;
        if parent_facts.is_final || (child_facts.is_abstract && child_facts.is_final) {
            return None;
        }
        // Abstract methods are compile-time declarations too. Body legality was
        // checked when facts were collected; no runtime implementation is guessed.
        match (&child_facts.result, &parent_facts.result) {
            (OverrideReturn::Void, OverrideReturn::Void) => Some(()),
            (OverrideReturn::Primitive(a), OverrideReturn::Primitive(b)) if a == b => Some(()),
            (OverrideReturn::Reference(a), OverrideReturn::Reference(b)) => {
                let child_file =
                    self.index.files.iter().find(|f| f.scope == child.0.scope && f.path == child.0.path)?;
                let parent_file =
                    self.index.files.iter().find(|f| f.scope == parent.0.scope && f.path == parent.0.path)?;
                let a = self.reference_type(child_file, a, child.1.start)?;
                let b = self.reference_type(parent_file, b, parent.1.start)?;
                if same_class(a, b) {
                    return Some(());
                }
                // Only a plain source superclass path supplies nonidentity covariance.
                // Complete graphs were validated above; interface/generic substitution is not proof.
                if b.binary || b.is_interface || b.reference_generic {
                    return None;
                }
                let mut current = a;
                for _ in 0..=REFERENCE_PROOF_DEPTH {
                    self.charge(1)?;
                    if current.binary || current.is_interface || current.reference_generic {
                        return None;
                    }
                    if same_class(current, b) {
                        return Some(());
                    }
                    let (parts, plain) = current.reference_edges.as_ref()?.first()?;
                    if !plain {
                        return None;
                    }
                    current = self.edge(current, parts)?;
                }
                None
            }
            _ => None,
        }
    }

    // Full candidate legality and parameter identities are checked before this
    // pairwise step. Exact descriptors suffice for equal returns only within
    // one linking scope. This override check performs no covariance, substitution
    // or return propagation; a selected invocation proves its result separately.
    fn binary_override(&mut self, child: (&'a ClassRec, &'a Member), parent: (&'a ClassRec, &'a Member)) -> Option<()> {
        if !child.0.binary
            || !parent.0.binary
            || child.0.scope != parent.0.scope
            || same_class(child.0, parent.0)
            || (child.0.is_interface && !parent.0.is_interface)
            || !self.graph(child.0)?.iter().any(|ancestor| same_class(ancestor, parent.0))
        {
            return None;
        }
        let Shape::Binary { descriptor: left, .. } = &child.1.shape else { return None };
        let Shape::Binary { descriptor: right, flags, .. } = &parent.1.shape else { return None };
        self.charge(left.len().saturating_add(right.len()))?;
        (flags & 0x0010 == 0 && left == right).then_some(())
    }

    // An opt-in certificate for one binary owner's complete overload inventory.
    // It does not replace legacy hint visibility or general Java access control.
    // Raw descriptors/flags and duplicates are checked BEFORE any exclusion.
    fn foreign_package_exclusions(
        &mut self,
        owner: &'a ClassRec,
        members: &[(&'a ClassRec, &'a Member)],
    ) -> Option<Vec<bool>> {
        if !self.file.call_inventory_valid || !owner.binary || owner.is_interface || owner.nesting.len() != 1 {
            return None;
        }
        let mut parameters: Vec<(Vec<ParameterType>, usize)> = Vec::new();
        let mut excluded = Vec::new();
        for (declaring, member) in members {
            if !same_class(owner, declaring) {
                return None;
            }
            let Shape::Binary { descriptor, generic, flags, .. } = &member.shape else { return None };
            self.charge(descriptor.len().saturating_add(1))?;
            let access = flags & 7;
            if *generic
                || !matches!((access, member.access), (0, Access::Package) | (1, Access::Public))
                || flags & !0x1dff != 0
                || flags & (0x0040 | 0x0080 | 0x0400 | 0x1000) != 0
                || flags & 0x0800 != 0 && !(46..=60).contains(&declaring.binary_signature.as_ref()?.major)
            {
                // Abstract-owner consistency is not modeled here. Unknown
                // generic/bridge/synthetic/varargs/private/protected forms refuse.
                return None;
            }
            let (params, result) = descriptor_parts(descriptor)?;
            if params.iter().any(|p| p.unsupported) || result.as_ref().is_some_and(|r| r.unsupported) {
                return None;
            }
            let slots: usize = params
                .iter()
                .map(|p| {
                    if p.array_rank == 0 && matches!(p.primitive, Some(Primitive::Long | Primitive::Double)) {
                        2
                    } else {
                        1
                    }
                })
                .sum();
            if slots > if flags & 0x0008 != 0 { 255 } else { 254 } {
                return None;
            }
            for (previous, bytes) in &parameters {
                // Raw-name equality may revisit long common prefixes per pair.
                self.charge(descriptor.len().saturating_add(*bytes).max(1))?;
                if previous.len() == params.len()
                    && previous.iter().zip(&params).all(|(a, b)| {
                        a.primitive == b.primitive && a.binary_name == b.binary_name && a.array_rank == b.array_rank
                    })
                {
                    // Return type, static mode or visibility cannot distinguish
                    // Java overloads, even if one declaration would be excluded.
                    return None;
                }
            }
            parameters.push((params, descriptor.len()));
            excluded.push(access == 0 && declaring.package != self.file.package);
        }
        Some(excluded)
    }

    fn select(
        &mut self,
        owner: &'a ClassRec,
        name: &str,
        args: &GreenNode,
        at: u32,
        depth: usize,
    ) -> Option<SelectedMethod<'a>> {
        self.select_mode(owner, name, args, at, depth, InvocationMode::Instance)
    }

    fn select_receiver(
        &mut self,
        receiver: &InvocationReceiver<'a>,
        name: &str,
        args: &GreenNode,
        at: u32,
        depth: usize,
    ) -> Option<SelectedMethod<'a>> {
        self.select_with_env(
            receiver.class,
            name,
            args,
            at,
            depth,
            InvocationMode::Instance,
            receiver.instantiation.as_ref(),
        )
    }

    fn select_mode(
        &mut self,
        owner: &'a ClassRec,
        name: &str,
        args: &GreenNode,
        at: u32,
        depth: usize,
        mode: InvocationMode,
    ) -> Option<SelectedMethod<'a>> {
        self.select_with_env(owner, name, args, at, depth, mode, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn select_with_env(
        &mut self,
        owner: &'a ClassRec,
        name: &str,
        args: &GreenNode,
        at: u32,
        depth: usize,
        mode: InvocationMode,
        env: Option<&ClassInstantiation<'a>>,
    ) -> Option<SelectedMethod<'a>> {
        self.check_depth(depth)?;
        if (owner.is_interface && !owner.binary) || (owner.reference_generic && env.is_none()) {
            return None;
        }
        self.accessible(owner)?;
        let graph = self.graph(owner)?;
        let inherited = graph.iter().skip(1).any(|class| class.methods.iter().any(|m| m.name == name));
        if let Some(env) = env
            && (mode != InvocationMode::Instance || !owner.binary || env.owner != ClassIdentity::of(owner))
        {
            return None;
        }
        if mode == InvocationMode::OwnInstance && (inherited || !same_class(self.implicit_instance_owner(at)?, owner)) {
            return None;
        }
        if mode == InvocationMode::TypeStatic && (owner.is_interface || inherited) {
            return None;
        }
        // A class-file interface's synthetic Object edge is inventory, not
        // ordinary interface inheritance. Object-name semantics remain refused.
        if owner.is_interface && graph.iter().any(|c| c.binary_root && c.methods.iter().any(|m| m.name == name)) {
            return None;
        }
        if inherited && !owner.binary {
            // A new inherited path needs a plain source superclass chain. Binary
            // Object may close the graph, but cannot contribute a named method.
            for class in &graph {
                self.charge(1)?;
                if class.is_interface {
                    // The complete graph already proved every transitive
                    // interface inventory. Generic substitution cannot erase
                    // a method name, so absence is sufficient; selection is not.
                    if class.methods.iter().any(|member| member.name == name) {
                        return None;
                    }
                    continue;
                }
                if class.reference_generic
                    || class.reference_edges.as_ref()?.first().is_some_and(|(_, plain)| !plain)
                    || (class.binary && !class.binary_root)
                {
                    return None;
                }
            }
        }
        let inherited_envs = if inherited {
            match env {
                Some(env) => Some(self.inherited_instantiations(owner, env)?),
                None => None,
            }
        } else {
            None
        };
        self.charge(args.children().len())?;
        let args = delimited_nodes(args, at)?;
        let actuals = args.iter().map(|(node, at)| self.actual(node, *at, depth + 1)).collect::<Option<Vec<_>>>()?;
        let mut members = Vec::new();
        for declaring in if inherited { graph.as_slice() } else { std::slice::from_ref(&owner) } {
            if inherited {
                self.charge(declaring.methods.len().max(1))?;
            }
            for member in declaring.methods.iter().filter(|m| m.name == name) {
                if declaring.binary != owner.binary
                    || (declaring.is_interface && !owner.binary)
                    || (declaring.reference_generic && env.is_none())
                {
                    return None;
                }
                if inherited {
                    self.accessible(declaring)?;
                }
                members.push((*declaring, member));
            }
        }
        // Keep the immutable declaration inventory intact. Only this new path
        // partitions a validated own-owner family; inherited names are tested
        // against the COMPLETE graph above, including inaccessible declarations.
        let members = if owner.binary
            && mode == InvocationMode::Instance
            && !inherited
            && members
                .iter()
                .any(|(declaring, member)| member.access == Access::Package && declaring.package != self.file.package)
        {
            let excluded = self.foreign_package_exclusions(owner, &members)?;
            members.into_iter().zip(excluded).filter_map(|(member, excluded)| (!excluded).then_some(member)).collect()
        } else {
            members
        };
        let mut formals = Vec::new();
        let mut instantiated_results = Vec::new();
        let mut erased_parameters: Vec<(&ClassRec, Vec<ParameterType>, usize)> = Vec::new();
        for (declaring, member) in &members {
            let declaration_env = if let Some(instances) = &inherited_envs {
                self.charge(instances.len().max(1))?;
                let identity = ClassIdentity::of(declaring);
                Some(instances.iter().find(|instance| instance.owner == identity)?)
            } else {
                env
            };
            let mut instantiated_result = None;
            self.charge(1)?;
            let mut types = Vec::new();
            if declaring.binary {
                let Shape::Binary { descriptor, generic, flags, signature } = &member.shape else { return None };
                // Native/abstract/final/synchronized are valid invocation declarations.
                // Type-qualified selection keeps concrete instance competitors.
                // Only the final winner must be static (JLS 15.12.3). Instance
                // calls, inherited names and interfaces retain their old boundary.
                // Never silently filter unsupported same-name candidates.
                if member.access != Access::Public
                    || (declaring.is_interface && flags & 0x0400 == 0)
                    || (*generic && declaration_env.is_none())
                    || flags & !0x1dff != 0
                    || flags & 0x0800 != 0 && !(46..=60).contains(&declaring.binary_signature.as_ref()?.major)
                    || flags & 7 != 1
                    || mode == InvocationMode::Instance && flags & 0x0008 != 0
                    || mode == InvocationMode::TypeStatic && flags & 0x0400 != 0
                    || flags & (0x0040 | 0x0080 | 0x1000) != 0
                    || flags & 0x0400 != 0
                        && (flags & (0x0010 | 0x0020 | 0x0100 | 0x0800) != 0 || declaring.reference_final)
                {
                    return None;
                }
                if declaration_env.is_some() {
                    self.charge(descriptor.len() + signature.as_ref().map_or(0, String::len))?;
                }
                let parameters = descriptor_parameters(descriptor)?;
                let slots: usize = parameters
                    .iter()
                    .map(|p| {
                        if p.array_rank == 0 && matches!(p.primitive, Some(Primitive::Long | Primitive::Double)) {
                            2
                        } else {
                            1
                        }
                    })
                    .sum();
                if slots > if flags & 0x0008 != 0 { 255 } else { 254 } {
                    return None;
                } // Only an instance receiver consumes a JVM parameter slot.
                if let Some(env) = declaration_env {
                    for (prior_owner, prior, bytes) in &erased_parameters {
                        self.charge(1)?;
                        if !same_class(prior_owner, declaring) {
                            continue;
                        }
                        self.charge(descriptor.len().saturating_add(*bytes).max(1))?;
                        if prior.len() == parameters.len()
                            && prior.iter().zip(&parameters).all(|(a, b)| {
                                a.primitive == b.primitive
                                    && a.binary_name == b.binary_name
                                    && a.array_rank == b.array_rank
                            })
                        {
                            return None;
                        }
                    }
                    erased_parameters.push((declaring, parameters.clone(), descriptor.len()));
                    let method =
                        instantiation::MethodTemplate::parse(signature.as_deref(), descriptor, &env.declaration)?;
                    for parameter in &method.parameters {
                        types.push(match self.template_value(declaring, env, parameter)?? {
                            StrictActual::Primitive(value) => ReferenceFormal::Primitive(value.primitive),
                            StrictActual::Reference(value) => ReferenceFormal::Reference(value.class),
                            StrictActual::Null => return None,
                        });
                    }
                    instantiated_result = self.template_value(declaring, env, &method.result)?;
                } else {
                    for parameter in parameters {
                        self.charge(1)?;
                        if parameter.unsupported || parameter.array_rank != 0 {
                            return None;
                        }
                        types.push(match parameter.primitive {
                            Some(primitive) => ReferenceFormal::Primitive(primitive),
                            None => {
                                // Use the existing descriptor parser's raw name, not a
                                // textual ')' split: ')' is legal within a JVM class name.
                                let binary_name = parameter.binary_name.as_ref()?;
                                if binary_name.contains('$') {
                                    return None; // nested/dollar formal proof is unsupported
                                }
                                let class = self.unique_named(declaring.scope, &parameter.name.join("."))?;
                                if class.binary_signature.as_ref().is_some_and(|sig| sig.binary_name != *binary_name) {
                                    return None;
                                }
                                if class.reference_generic {
                                    return None;
                                }
                                self.accessible(class)?;
                                self.graph(class)?;
                                ReferenceFormal::Reference(class)
                            }
                        });
                    }
                }
            } else {
                let file = self.index.files.iter().find(|f| f.scope == declaring.scope && f.path == declaring.path)?;
                let source = member.source_call.as_ref()?;
                let Shape::Source { parameters, variables } = &member.shape else { return None };
                // OwnInstance proves the caller and declaration are exactly
                // the same class. Other receiver forms keep public-only access.
                if (mode != InvocationMode::OwnInstance && member.access != Access::Public)
                    || mode != InvocationMode::TypeStatic && source.is_static
                    || mode == InvocationMode::TypeStatic && source.is_abstract
                    || source.varargs
                    || !variables.is_empty()
                    || mode == InvocationMode::TypeStatic && !file.syntax_valid
                {
                    return None;
                }
                if mode != InvocationMode::Instance {
                    let slots: usize = parameters
                        .iter()
                        .map(|p| {
                            if p.array_rank == 0 && matches!(p.primitive, Some(Primitive::Long | Primitive::Double)) {
                                2
                            } else {
                                1
                            }
                        })
                        .sum();
                    if slots > if source.is_static { 255 } else { 254 } {
                        return None;
                    }
                }
                for parameter in parameters {
                    self.charge(1)?;
                    if parameter.unsupported || parameter.array_rank != 0 {
                        return None;
                    }
                    types.push(match parameter.primitive {
                        Some(primitive) => ReferenceFormal::Primitive(primitive),
                        None => ReferenceFormal::Reference(self.reference_type(file, &parameter.name, member.start)?),
                    });
                }
            }
            formals.push(types);
            instantiated_results.push(instantiated_result);
        }
        let mut hidden = vec![false; members.len()];
        for (i, left) in formals.iter().enumerate() {
            for (j, right) in formals.iter().enumerate().skip(i + 1) {
                self.charge(left.len().max(1))?;
                if inherited && env.is_some() {
                    let (_, erased_left, left_bytes) = erased_parameters.get(i)?;
                    let (_, erased_right, right_bytes) = erased_parameters.get(j)?;
                    self.charge(left_bytes.saturating_add(*right_bytes).max(1))?;
                    let same_erasure = erased_left.len() == erased_right.len()
                        && erased_left.iter().zip(erased_right).all(|(a, b)| {
                            a.primitive == b.primitive && a.binary_name == b.binary_name && a.array_rank == b.array_rank
                        });
                    // Substitution cannot turn an erasure clash into two legal
                    // overloads. Equal erased parameters need the same typed
                    // signature before the override/return proof below applies.
                    if same_erasure
                        && (left.len() != right.len() || !left.iter().zip(right).all(|(a, b)| a.same_type(b)))
                    {
                        return None;
                    }
                }
                if left.len() == right.len() {
                    if inherited {
                        // Different scope identity is not proof of a different
                        // signature: it may hide an unsupported override. One
                        // definitely different slot suffices; equal/unknown
                        // signatures both refuse rather than discard a parent.
                        let mut different = false;
                        for (a, b) in left.iter().zip(right) {
                            self.charge(1)?;
                            different |= match (a, b) {
                                (ReferenceFormal::Primitive(a), ReferenceFormal::Primitive(b)) => a != b,
                                (ReferenceFormal::Reference(a), ReferenceFormal::Reference(b)) => {
                                    if a.scope == b.scope {
                                        !same_class(a, b)
                                    } else {
                                        // Same logical name across scopes remains
                                        // unknown even when selected origins match.
                                        a.dotted() != b.dotted()
                                    }
                                }
                                _ => true,
                            };
                        }
                        if !different {
                            if !left.iter().zip(right).all(|(a, b)| a.same_type(b)) {
                                return None; // Unknown cross-scope identity is not override equality.
                            }
                            if same_class(members[i].0, members[j].0) {
                                return None;
                            }
                            if env.is_some() {
                                self.identity_override(inherited_envs.as_ref()?, members[i], members[j])?;
                                self.charge(1)?;
                                // Equal erasure is not enough: class-formal
                                // substitution may make the return incompatible.
                                let same_result = match (instantiated_results[i], instantiated_results[j]) {
                                    (None, None) => true,
                                    (Some(StrictActual::Primitive(a)), Some(StrictActual::Primitive(b))) => {
                                        a.primitive == b.primitive && a.rank == b.rank
                                    }
                                    (Some(StrictActual::Reference(a)), Some(StrictActual::Reference(b))) => {
                                        same_class(a.class, b.class)
                                    }
                                    _ => false,
                                };
                                if !same_result {
                                    return None;
                                }
                            }
                            if self.graph(members[i].0)?.iter().any(|c| same_class(c, members[j].0)) {
                                if owner.binary {
                                    self.binary_override(members[i], members[j])?;
                                } else {
                                    self.source_override(members[i], members[j])?;
                                }
                                hidden[j] = true;
                            } else {
                                if owner.binary {
                                    self.binary_override(members[j], members[i])?;
                                } else {
                                    self.source_override(members[j], members[i])?;
                                }
                                hidden[i] = true;
                            }
                        }
                    } else if left.iter().zip(right).all(|(a, b)| a.same_type(b)) {
                        return None;
                    }
                }
            }
        }
        let mut applicable = Vec::new();
        for (i, parameters) in formals.iter().enumerate() {
            if hidden[i] || actuals.len() != parameters.len() {
                continue;
            }
            let mut applies = true;
            for (actual, formal) in actuals.iter().zip(parameters) {
                applies &= match (actual, formal) {
                    (StrictActual::Null, ReferenceFormal::Reference(_)) => {
                        self.charge(1)?;
                        true
                    }
                    (StrictActual::Primitive(actual), ReferenceFormal::Primitive(formal)) => {
                        self.charge(1)?;
                        actual.converts_to(PrimitiveValue { primitive: *formal, rank: 0 })
                    }
                    (StrictActual::Reference(actual), ReferenceFormal::Reference(formal)) => {
                        if actual.class.binary
                            && formal.binary
                            && !actual.class.reference_generic
                            && !formal.reference_generic
                            && actual.class.scope != formal.scope
                            && actual.class.binary_signature.as_ref()?.binary_name
                                == formal.binary_signature.as_ref()?.binary_name
                        {
                            self.binary_closure_evidence(actual.class, formal)?;
                            true
                        } else {
                            let converts = self.converts(actual.class, formal)?;
                            if !converts {
                                // An unsupported cross-scope ancestor match is
                                // unknown, not evidence that this competitor
                                // is inapplicable. Include source replacements:
                                // refusing is not declaring these names equal.
                                let name = formal.dotted();
                                for ancestor in self.graph(actual.class)? {
                                    self.charge(1)?;
                                    if ancestor.scope != formal.scope && ancestor.dotted() == name {
                                        return None;
                                    }
                                }
                            }
                            converts
                        }
                    }
                    // Strict invocation neither boxes nor unboxes. Do not skip
                    // the other slots: they still need complete evidence.
                    _ => {
                        self.charge(1)?;
                        false
                    }
                };
            }
            if applies {
                applicable.push(i);
            }
        }
        let mut winner = None;
        for &i in &applicable {
            let mut specific = true;
            for &j in &applicable {
                for (left, right) in formals[i].iter().zip(&formals[j]) {
                    specific &= match (left, right) {
                        (ReferenceFormal::Primitive(left), ReferenceFormal::Primitive(right)) => {
                            self.charge(1)?;
                            PrimitiveValue { primitive: *left, rank: 0 }
                                .converts_to(PrimitiveValue { primitive: *right, rank: 0 })
                        }
                        (ReferenceFormal::Reference(left), ReferenceFormal::Reference(right)) => {
                            self.converts(left, right)?
                        }
                        _ => {
                            self.charge(1)?;
                            false
                        }
                    };
                }
            }
            if specific {
                if winner.is_some() {
                    return None;
                }
                winner = Some(SelectedMethod {
                    declaring: members[i].0,
                    member: members[i].1,
                    instantiated_result: instantiated_results[i],
                });
            }
        }
        let winner = winner?;
        if mode == InvocationMode::TypeStatic {
            let is_static = match &winner.member.shape {
                Shape::Binary { flags, .. } => flags & 0x0008 != 0,
                Shape::Source { .. } => winner.member.source_call.as_ref()?.is_static,
                Shape::Unknown => return None,
            };
            if !is_static {
                return None; // A more-specific instance method cannot fall back to a static overload.
            }
        }
        Some(winner)
    }
}

fn read_type(node: &GreenNode, start: u32) -> Option<QualName> {
    let (type_node, type_at) = child_kind(node, start, SyntaxKind::Type)?;
    let parts = if let Some((name, name_at)) = child_kind(type_node, type_at, SyntaxKind::Name) {
        name_pieces(name, name_at).0.into_iter().map(|(part, _, _)| part).collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if parts.is_empty() {
        None
    } else {
        let array_rank = tokens(type_node, type_at).iter().filter(|token| token.0 == SyntaxKind::LBracket).count()
            + tokens(node, start)
                .iter()
                .filter(|token| matches!(token.0, SyntaxKind::LBracket | SyntaxKind::DotDotDot))
                .count();
        Some(QualName { parts, array_rank })
    }
}

fn child_kind(node: &GreenNode, start: u32, kind: SyntaxKind) -> Option<(&GreenNode, u32)> {
    let mut offset = start;
    for child in node.children() {
        if let Some(child) = child.as_node()
            && child.kind() == kind
        {
            return Some((child, offset));
        }
        offset += child.utf8_len();
    }
    None
}

fn direct_ident(node: &GreenNode, start: u32) -> Option<(String, u32, u32)> {
    let mut offset = start;
    for child in node.children() {
        if let Some(token) = child.as_token()
            && token.kind() == SyntaxKind::Ident
        {
            return Some((token.text().to_string(), offset, offset + token.utf8_len()));
        }
        offset += child.utf8_len();
    }
    None
}

fn name_strings(node: &GreenNode, start: u32) -> Vec<String> {
    name_pieces(node, start).0.into_iter().map(|(name, _, _)| name).collect()
}

fn name_pieces(node: &GreenNode, start: u32) -> (Vec<(String, u32, u32)>, bool) {
    let mut parts = Vec::new();
    let mut star = false;
    let mut offset = start;
    for child in node.children() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::Ident {
                parts.push((token.text().to_string(), offset, offset + token.utf8_len()));
            } else if token.kind() == SyntaxKind::Star {
                star = true;
            }
        }
        offset += child.utf8_len();
    }
    (parts, star)
}

fn tokens(node: &GreenNode, start: u32) -> Vec<(SyntaxKind, u32, u32)> {
    let mut out = Vec::new();
    let mut offset = start;
    for child in node.children() {
        if let Some(token) = child.as_token() {
            out.push((token.kind(), offset, offset + token.utf8_len()));
        }
        offset += child.utf8_len();
    }
    out
}

/// Workspace-only entry point: source ownership and visibility were validated by the loader.
pub(crate) fn resolve_scoped(
    projects: &[&Project],
    visibility: &[BTreeSet<usize>],
    scope: usize,
    path: &str,
    byte: u32,
    ids: Option<&[crate::workspace::CompilationId]>,
) -> ResolveResult {
    let parsed = visibility[scope].iter().find_map(|index| projects[*index].file(path));
    let Some(parsed) = parsed else {
        return ResolveResult::none(ResolveStatus::Unresolved);
    };
    let index = Index::scoped(projects, visibility);
    let index = if let Some(ids) = ids { index.with_compilation_ids(ids) } else { index };
    index.resolve(path, parsed, byte)
}

/// Owned declaration index; source lookup is supplied by its exact snapshot.
pub(crate) struct OwnedScopedIndex {
    index: Index,
}
impl OwnedScopedIndex {
    pub(crate) fn new(projects: &[&Project], visibility: &[BTreeSet<usize>]) -> Self {
        Self { index: Index::scoped(projects, visibility) }
    }
    pub(crate) fn with_compilation_ids(mut self, ids: &[crate::workspace::CompilationId]) -> Self {
        self.index = self.index.with_compilation_ids(ids);
        self
    }
    pub(crate) fn resolve_at<'a>(
        &self,
        scope: usize,
        path: &str,
        byte: u32,
        project: impl Fn(usize) -> &'a Project,
    ) -> ResolveResult {
        let parsed = self.index.visibility[scope].iter().find_map(|index| project(*index).file(path));
        let Some(parsed) = parsed else {
            return ResolveResult::none(ResolveStatus::Unresolved);
        };
        self.index.resolve(path, parsed, byte)
    }
}

/// Validated workspace inputs, indexed once with declaration-owner contexts.
pub(crate) struct PreparedScopedIndex<'a> {
    projects: Vec<&'a Project>,
    index: OwnedScopedIndex,
}
impl<'a> PreparedScopedIndex<'a> {
    pub(crate) fn new(projects: Vec<&'a Project>, visibility: &[BTreeSet<usize>]) -> Self {
        let index = OwnedScopedIndex::new(&projects, visibility);
        Self { projects, index }
    }
    pub(crate) fn with_compilation_ids(mut self, ids: &[crate::workspace::CompilationId]) -> Self {
        self.index = self.index.with_compilation_ids(ids);
        self
    }
    pub(crate) fn resolve_at(&self, scope: usize, path: &str, byte: u32) -> ResolveResult {
        self.index.resolve_at(scope, path, byte, |index| self.projects[index])
    }
}

#[cfg(test)]
pub(crate) fn observed_index_builds() -> usize {
    INDEX_BUILDS.with(|count| count.get())
}

#[cfg(test)]
mod prepared_tests {
    use super::*;
    use crate::{CompilationConfig, CompilationId, ProjectConfig, WorkspaceConfig, WorkspaceSnapshot};

    #[test]
    fn prepared_handles_build_once_and_views_never_rebuild() {
        INDEX_BUILDS.with(|count| count.set(0));
        let mut project = Project::new();
        project.insert("A.java", "class A { int value; int read() { return value; } }");
        let prepared = project.prepare();
        assert_eq!(INDEX_BUILDS.with(|count| count.get()), 1);
        for _ in 0..10 {
            prepared.resolve_at("A.java", 41);
            prepared.resolve_at("absent.java", 0);
        }
        assert_eq!(INDEX_BUILDS.with(|count| count.get()), 1);
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scopes");
        let id = CompilationId::new("app", "main");
        let lib = CompilationId::new("lib", "main");
        let workspace = WorkspaceSnapshot::load(&WorkspaceConfig {
            compilations: vec![
                CompilationConfig {
                    id: id.clone(),
                    inputs: ProjectConfig { source_roots: vec![root.join("app/main")], ..Default::default() },
                    dependencies: vec![lib.clone()],
                },
                CompilationConfig {
                    id: lib.clone(),
                    inputs: ProjectConfig { source_roots: vec![root.join("lib/main")], ..Default::default() },
                    dependencies: vec![],
                },
            ],
            ..Default::default()
        })
        .unwrap();
        let app_path = root.join("app/main/app/App.java").canonicalize().unwrap();
        let lib_path = root.join("lib/main/lib/Lib.java").canonicalize().unwrap();
        let app_path = app_path.to_str().unwrap();
        let lib_path = lib_path.to_str().unwrap();
        let byte =
            workspace.scope(&id).unwrap().inputs().project().file(app_path).unwrap().text().rfind("value").unwrap()
                as u32;
        let prepared = workspace.prepare();
        assert_eq!(INDEX_BUILDS.with(|count| count.get()), 2);
        for _ in 0..10 {
            let view = prepared.scope(&id).unwrap();
            view.resolve_at("absent.java", 0);
            assert_eq!(view.resolve_at(app_path, byte).status, ResolveStatus::Resolved);
            let other = prepared.scope(&lib).unwrap();
            assert_eq!(other.resolve_at(lib_path, 26).status, ResolveStatus::Resolved);
            assert_eq!(other.resolve_at(app_path, byte).status, ResolveStatus::Unresolved);
            assert!(prepared.scope(&CompilationId::new("missing", "main")).is_none());
        }
        assert_eq!(INDEX_BUILDS.with(|count| count.get()), 2);
    }
}

#[cfg(test)]
mod declarator_tests {
    use super::*;

    #[test]
    fn ranks_combine_shared_and_per_name_dimensions_only() {
        let text = "class A { Box<A[]>[] one, two[][]; void m() { Box<A[]> first[], second, third[][] = make(first, second); } }";
        let parsed = ParsedFile::parse(text);
        assert!(parsed.errors().is_empty());
        let file = index_file("A.java", &parsed);
        let fields = &file.classes[0].fields;
        assert_eq!(
            fields
                .iter()
                .map(|field| (field.name.as_str(), field.declared.as_ref().unwrap().array_rank))
                .collect::<Vec<_>>(),
            vec![("one", 1), ("two", 3)]
        );
        assert_eq!(
            file.locals
                .iter()
                .map(|local| (local.name.as_str(), local.declared.as_ref().unwrap().array_rank))
                .collect::<Vec<_>>(),
            vec![("first", 1), ("second", 0), ("third", 2)]
        );
    }
}

#[cfg(test)]
mod method_descriptor_tests {
    use super::*;

    #[test]
    fn descriptor_results_are_complete_and_keep_raw_names() {
        for (tag, primitive) in [
            ('Z', Primitive::Boolean),
            ('B', Primitive::Byte),
            ('C', Primitive::Char),
            ('S', Primitive::Short),
            ('I', Primitive::Int),
            ('J', Primitive::Long),
            ('F', Primitive::Float),
            ('D', Primitive::Double),
        ] {
            let (_, result) = descriptor_parts(&format!("(){tag}")).unwrap();
            let result = result.unwrap();
            assert_eq!(result.primitive, Some(primitive));
            assert_eq!(result.array_rank, 0);
        }
        assert!(descriptor_parts("()V").unwrap().1.is_none());
        let (parameters, result) = descriptor_parts("(Llegal/)Name;)Lreturn/)Name;").unwrap();
        assert_eq!(parameters[0].binary_name.as_deref(), Some("legal/)Name"));
        assert_eq!(result.unwrap().binary_name.as_deref(), Some("return/)Name"));
        assert_eq!(descriptor_parts("()[[I").unwrap().1.unwrap().array_rank, 2);
        for invalid in ["()Vx", "()Ljava/lang/X;x", "()L;", "()Lx//Y;", "()Lx.Y;", "()[V", "()"] {
            assert!(descriptor_parts(invalid).is_none(), "{invalid}");
        }
        assert!(descriptor_parts(&format!("(){}I", "[".repeat(256))).is_none());
    }

    #[test]
    fn validates_complete_descriptors_and_preserves_primitive_tags() {
        for invalid in ["", "I", "(V)V", "([V)V", "(I)", "()Vjunk", "(L;)V", "(Lx//Y;)V", "(Lx.Y;)V", "()Q", "()[V"] {
            assert!(descriptor_parameters(invalid).is_none(), "{invalid}");
        }
        assert!(descriptor_parameters(&format!("({}I)V", "[".repeat(256))).is_none());
        assert!(descriptor_parameters(&format!("({})V", "I".repeat(256))).is_none());
        assert!(descriptor_parameters(&format!("({})V", "J".repeat(128))).is_none());
        assert!(descriptor_parameters(&format!("({}I)V", "[".repeat(255))).is_some());
        assert!(descriptor_parameters(&format!("({})V", "I".repeat(255))).is_some());
        let parsed = descriptor_parameters("(ZBCSIJFD[ILint;)Ljava/lang/Absent;").unwrap();
        assert_eq!(parsed.len(), 10);
        assert_eq!(parsed[4].primitive, Some(Primitive::Int));
        assert_eq!(parsed[8].array_rank, 1);
        assert_eq!(parsed[9].primitive, None);
        assert_eq!(parsed[9].name, ["int"]);
        assert!(descriptor_parameters("()V").unwrap().is_empty());
    }
}

#[cfg(test)]
mod binary_payload_tests;

#[cfg(test)]
mod class_literal_receiver_tests {
    use super::*;

    fn first_field(node: &GreenNode) -> Option<&GreenNode> {
        if node.kind() == SyntaxKind::FieldExpr {
            Some(node)
        } else {
            node.child_nodes().find_map(|child| first_field(child))
        }
    }

    #[test]
    fn receiver_guard_ignores_conditions_arguments_indices_and_assignment_targets() {
        for expression in [
            "(int.class == null ? box : box).field",
            "box.take(int.class).field",
            "boxes[index(int.class)].field",
            "(boxes[index(int.class)] = box).field",
        ] {
            let file = ParsedFile::parse(&format!("class C {{ void m() {{ use({expression}); }} }}"));
            assert!(file.errors().is_empty(), "{expression}");
            assert!(!has_class_literal_receiver(first_field(file.root()).unwrap()), "{expression}");
        }
    }
    #[test]
    fn call_guard_ignores_conditions_indices_and_assignment_targets() {
        for expression in [
            "(test() ? box : box).field",
            "boxes[index()].field",
            "(boxes[index()] = box).field",
            "(boxes[index()] += box).field",
        ] {
            let file = ParsedFile::parse(&format!("class C {{ void m() {{ use({expression}); }} }}"));
            assert!(file.errors().is_empty(), "{expression}");
            assert!(!has_call_result_receiver(first_field(file.root()).unwrap()), "{expression}");
        }
    }
}

#[cfg(test)]
mod binary_actual_tests;

#[cfg(test)]
mod inherited_scope_tests;

#[cfg(test)]
mod invocation_budget_tests {
    use super::*;
    fn call_at(node: &GreenNode, start: u32, wanted: u32) -> Option<(&GreenNode, u32)> {
        if start == wanted && node.kind() == SyntaxKind::CallExpr {
            return Some((node, start));
        }
        let mut at = start;
        for child in node.children() {
            if let Some(inner) = child.as_node()
                && let Some(found) = call_at(inner, at, wanted)
            {
                return Some(found);
            }
            at += child.utf8_len();
        }
        None
    }
    #[test]
    fn primitive_actual_uses_one_visible_local_scan_under_shared_budget() {
        let filler = (0..20).map(|i| format!("int unused{i};")).collect::<String>();
        let text = format!("class C {{ void run(int value) {{ {filler} take(value); }} }}");
        let mut project = Project::new();
        project.insert("C.java", &text);
        let parsed = project.file("C.java").unwrap();
        let wanted = text.rfind("take(").unwrap() as u32;
        let context = direct_caller(parsed.root(), wanted).unwrap();
        let index = Index::build(&project.files, &project.binaries);
        let file = &index.files[0];
        let (call, at) = call_at(parsed.root(), 0, wanted).unwrap();
        let (args, args_at) = child_kind(call, at, SyntaxKind::ArgList).unwrap();
        let nodes = delimited_nodes(args, args_at).unwrap();
        let (node, at) = nodes[0];
        let mut proof = ReferenceCallProof::new(&index, file, Some(context));
        proof.work_left = file.locals.len() + 8;
        assert!(matches!(
            proof.actual(node, at, 0),
            Some(StrictActual::Primitive(PrimitiveValue { primitive: Primitive::Int, rank: 0 }))
        ));
        assert!(!proof.exhausted);
        let mut exhausted = ReferenceCallProof::new(&index, file, Some(context));
        exhausted.work_left = file.locals.len() - 1;
        assert!(exhausted.actual(node, at, 0).is_none());
        assert!(exhausted.exhausted);
    }
    #[test]
    fn additive_actual_recursion_keeps_the_callers_work_and_depth_budget() {
        let text = "class C { void run(int i) { take((i + 1) - (i + 2)); } }";
        let mut project = Project::new();
        project.insert("C.java", text);
        let parsed = project.file("C.java").unwrap();
        let wanted = text.rfind("take(").unwrap() as u32;
        let context = direct_caller(parsed.root(), wanted).unwrap();
        let index = Index::build(&project.files, &project.binaries);
        let file = &index.files[0];
        let (call, at) = call_at(parsed.root(), 0, wanted).unwrap();
        let (args, args_at) = child_kind(call, at, SyntaxKind::ArgList).unwrap();
        let nodes = delimited_nodes(args, args_at).unwrap();
        let (node, at) = nodes[0];
        let mut good = ReferenceCallProof::new(&index, file, Some(context));
        assert!(matches!(
            good.scalar_primitive_argument(node, at, 0),
            Some(PrimitiveValue { primitive: Primitive::Int, rank: 0 })
        ));
        let mut work = ReferenceCallProof::new(&index, file, Some(context));
        work.work_left = 8;
        assert!(work.scalar_primitive_argument(node, at, 0).is_none());
        assert!(work.exhausted);
        assert_eq!(work.work_left, 0);
        let mut depth = ReferenceCallProof::new(&index, file, Some(context));
        assert!(depth.scalar_primitive_argument(node, at, REFERENCE_PROOF_DEPTH).is_none());
        assert!(depth.exhausted);
        assert_eq!(depth.work_left, 0);
    }
    #[test]
    fn postfix_actual_composition_keeps_the_callers_work_and_depth_budget() {
        let text = "class C { void run(int i) { take((i++) - (i--)); } }";
        let mut project = Project::new();
        project.insert("C.java", text);
        let parsed = project.file("C.java").unwrap();
        let wanted = text.rfind("take(").unwrap() as u32;
        let context = direct_caller(parsed.root(), wanted).unwrap();
        let index = Index::build(&project.files, &project.binaries);
        let file = &index.files[0];
        let (call, at) = call_at(parsed.root(), 0, wanted).unwrap();
        let (args, args_at) = child_kind(call, at, SyntaxKind::ArgList).unwrap();
        let nodes = delimited_nodes(args, args_at).unwrap();
        let (node, at) = nodes[0];
        let mut good = ReferenceCallProof::new(&index, file, Some(context));
        assert!(matches!(
            good.scalar_primitive_argument(node, at, 0),
            Some(PrimitiveValue { primitive: Primitive::Int, rank: 0 })
        ));
        let mut work = ReferenceCallProof::new(&index, file, Some(context));
        work.work_left = 8;
        assert!(work.scalar_primitive_argument(node, at, 0).is_none());
        assert!(work.exhausted);
        assert_eq!(work.work_left, 0);
        let mut depth = ReferenceCallProof::new(&index, file, Some(context));
        assert!(depth.scalar_primitive_argument(node, at, REFERENCE_PROOF_DEPTH).is_none());
        assert!(depth.exhausted);
        assert_eq!(depth.work_left, 0);
    }
    #[test]
    fn shallow_invocation_tree_reports_shared_work_exhaustion() {
        for count in [2, 200] {
            let params = (0..count).map(|i| format!("int p{i}")).collect::<Vec<_>>().join(",");
            let args = std::iter::repeat_n("b.size()", count).collect::<Vec<_>>().join(",");
            let text = format!(
                "class Box {{ public int size() {{ return 1; }} public void take({params}) {{}} }} class Calls {{ void run(Box b) {{ b.take({args}); }} }}"
            );
            let mut project = Project::new();
            project.insert("Calls.java", &text);
            let mut object = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61, 0, 3, 1, 0, 16];
            object.extend(b"java/lang/Object");
            object.extend([7, 0, 1, 0, 0x21, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            project.add_class_bytes("Object.class", &object).unwrap();
            let parsed = project.file("Calls.java").unwrap();
            let wanted = text.rfind("b.take(").unwrap() as u32;
            let context = direct_caller(parsed.root(), wanted).unwrap();
            let index = Index::build(&project.files, &project.binaries);
            let file = &index.files[0];
            let mut proof = ReferenceCallProof::new(&index, file, Some(context));
            let (node, at) = call_at(parsed.root(), 0, wanted).unwrap();
            let (args, args_at) = child_kind(node, at, SyntaxKind::ArgList).unwrap();
            let owner = file.classes.iter().find(|class| class.dotted() == "Box").unwrap();
            let selected = proof.select(owner, "take", args, args_at, 0);
            assert_eq!(selected.is_some(), count == 2);
            assert_eq!(proof.exhausted, count == 200);
            if count == 200 {
                assert_eq!(proof.work_left, 0);
            }
        }
    }
}

#[cfg(test)]
mod header_inventory_tests;

mod header_type;

#[cfg(test)]
mod legacy_name_index_tests;
