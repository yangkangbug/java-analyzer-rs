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

use crate::classfile::{
    Access, ClassError, ClassSignature, MemberKind, MemberSignature, parse_class_bytes, read_class_directory,
};
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
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
    binaries: BTreeMap<String, ClassSignature>,
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
        self.binaries.insert(origin, signature);
        Ok(())
    }

    /// Register every `*.class` file under `root`. One bad file fails the call
    /// and leaves previously registered classes unchanged.
    pub fn add_class_directory(&mut self, root: &Path) -> Result<Vec<String>, ClassError> {
        let parsed = read_class_directory(root)?;
        let origins = parsed.iter().map(|class| class.origin.clone()).collect();
        for class in parsed {
            self.binaries.insert(class.origin.clone(), class);
        }
        Ok(origins)
    }

    pub(crate) fn insert_class_signature(&mut self, signature: ClassSignature) {
        self.binaries.insert(signature.origin.clone(), signature);
    }

    pub fn remove_class(&mut self, origin: &str) -> bool {
        self.binaries.remove(origin).is_some()
    }

    pub fn class_signature(&self, origin: &str) -> Option<&ClassSignature> {
        self.binaries.get(origin)
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
    name: String,
    kind: TargetKind,
    start: u32,
    end: u32,
    declared: Option<QualName>,
    access: Access,
    shape: Shape,
    source_call: Option<SourceCall>,
}

#[derive(Clone, Debug)]
struct ClassRec {
    scope: usize,
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
    is_interface: bool,
    type_variables: Vec<String>,
    primitive_owner: bool,
    binary_root: bool,
    fields: Vec<Member>,
    methods: Vec<Member>,
}

impl ClassRec {
    fn dotted(&self) -> String {
        let nested = self.nesting.join(".");
        if self.package.is_empty() { nested } else { format!("{}.{}", self.package, nested) }
    }

    fn simple(&self) -> &str {
        self.nesting.last().map(String::as_str).unwrap_or("")
    }

    fn target(&self) -> Target {
        Target {
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
    path: String,
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
    call_inventory_valid: bool,
}

#[cfg(test)]
thread_local! {
    static INDEX_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct Index {
    files: Vec<FileRec>,
    binaries: Vec<ClassRec>,
    visibility: Vec<BTreeSet<usize>>,
}

impl Index {
    fn build(sources: &BTreeMap<String, ParsedFile>, binaries: &BTreeMap<String, ClassSignature>) -> Self {
        #[cfg(test)]
        INDEX_BUILDS.with(|count| count.set(count.get() + 1));
        let files = sources.iter().map(|(path, file)| index_file(path, file)).collect();
        let binaries = binaries.values().map(class_from_signature).collect();
        Self { files, binaries, visibility: vec![BTreeSet::from([0])] }
    }

    fn scoped(projects: &[&Project], visibility: &[BTreeSet<usize>]) -> Self {
        #[cfg(test)]
        INDEX_BUILDS.with(|count| count.set(count.get() + 1));
        let mut files = Vec::new();
        let mut binaries = Vec::new();
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
                let mut class = class_from_signature(signature);
                class.scope = scope;
                binaries.push(class);
            }
        }
        Self { files, binaries, visibility: visibility.to_vec() }
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
            let found = match self.collect(class, name, true, file, byte) {
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
                    let primitive_mismatch = caller.is_some_and(|(caller, start, end)| {
                        if !file.call_inventory_valid || source_call(caller, start).is_none() {
                            return false;
                        }
                        actuals.as_ref().is_some_and(|args| {
                            let Shape::Source { parameters, .. } = &candidate.member.shape else { return false };
                            // One independently known contradiction is enough;
                            // unrelated unknown formals cannot erase it.
                            !source.varargs
                                && args.len() == parameters.len()
                                && args.iter().zip(parameters).any(|((arg, at), formal)| {
                                    formal.primitive.filter(|_| !formal.unsupported).is_some_and(|primitive| {
                                        primitive_expression(file, arg, *at, start, end).is_some_and(|actual| {
                                            !actual.converts_to(PrimitiveValue { primitive, rank: formal.array_rank })
                                        })
                                    })
                                })
                        })
                    });
                    if fixed_arity_mismatch || primitive_mismatch {
                        result.status = ResolveStatus::CallNotResolved;
                        return result;
                    }
                }
            }
            let Some((caller_node, caller_start, caller_end)) = caller else { return result };
            if !self.closed_primitive_owner(file, class, name)
                || !file.call_inventory_valid
                || !caller_node.child_nodes().any(|node| node.kind() == SyntaxKind::Block)
                || !source_call(caller_node, caller_start).is_some_and(|call| call.is_static)
                || child_kind(caller_node, caller_start, SyntaxKind::TypeParamList).is_some()
            {
                return result;
            }
            if !matches!(result.status, ResolveStatus::Resolved | ResolveStatus::OverloadNotResolved) {
                return result;
            }
            // Every same-name declaration is considered before access filtering.
            // No unsupported competitor may be dropped to manufacture a winner.
            let members: Vec<_> = class.methods.iter().filter(|member| member.name == name).collect();
            let selected = actuals.as_ref().and_then(|args| {
                let actuals = args
                    .iter()
                    .map(|(arg, at)| primitive_expression(file, arg, *at, caller_start, caller_end))
                    .collect::<Option<Vec<_>>>()?;
                let formals = members
                    .iter()
                    .map(|member| {
                        member.source_call.as_ref().filter(|call| call.is_static)?;
                        primitive_formals(member)
                    })
                    .collect::<Option<Vec<_>>>()?;
                // Duplicate declarations invalidate the inventory even if that
                // signature would not be applicable to these particular actuals.
                for (i, left) in formals.iter().enumerate() {
                    if formals[i + 1..].contains(left) {
                        return None;
                    }
                }
                let applicable: Vec<_> = formals
                    .iter()
                    .enumerate()
                    .filter(|(_, parameters)| {
                        actuals.len() == parameters.len()
                            && actuals.iter().zip(parameters.iter()).all(|(actual, formal)| actual.converts_to(*formal))
                    })
                    .map(|(i, _)| i)
                    .collect();
                let most_specific: Vec<_> = applicable
                    .iter()
                    .copied()
                    .filter(|&i| {
                        applicable
                            .iter()
                            .all(|&j| formals[i].iter().zip(&formals[j]).all(|(left, right)| left.converts_to(*right)))
                    })
                    .collect();
                match most_specific.as_slice() {
                    [one] => Some(members[*one]),
                    _ => None,
                }
            });
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

    fn closed_primitive_owner(&self, file: &FileRec, class: &ClassRec, name: &str) -> bool {
        if !class.primitive_owner
            || !matches!(self.class_named(class.scope, &class.dotted()), Named::One(found) if same_class(found, class))
        {
            return false;
        }
        let Some(parent) = &class.super_parts else { return false };
        match self.resolve_edge(class, parent) {
            Named::One(object) => {
                // Source Object implicitly extends itself in this index. A
                // supplied binary root is the bounded, verifiable closure.
                object.binary_root
                    && object.scope == file.scope
                    && object.methods.iter().all(|member| member.name != name)
            }
            _ => false,
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
        let locals: Vec<&LocalRec> = file
            .locals
            .iter()
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
        if let Some(member) = member_type_here(file, byte, name) {
            return TypeLookup::One(member);
        }
        if let Some((_, full)) = file.singles.iter().find(|(simple, _)| simple == name) {
            let imported = self.classes(file.scope).filter(|class| class.dotted() == *full).collect::<Vec<_>>();
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
        self.qualify(file, &prefix, false, byte)
    }

    fn resolve_field(&self, file: &FileRec, _parsed: &ParsedFile, hit: Hit<'_>, byte: u32) -> ResolveResult {
        // Super lookup is not implemented. Do not let field_chain discard an
        // unsupported receiver and reinterpret its selector as a simple name.
        if has_super_receiver(hit.node) {
            return ResolveResult::none(ResolveStatus::ReceiverNotBound);
        }
        let chain = field_chain(hit.node, hit.start);
        let Some(index) = chain.iter().position(|segment| segment.start <= byte && byte < segment.end) else {
            return ResolveResult::none(ResolveStatus::Unresolved);
        };
        self.resolve_chain(file, &chain[..=index], hit.call, byte)
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
        for (index, segment) in chain.iter().enumerate().skip(1) {
            let last = index + 1 == chain.len();
            let next = self.member_from_base(file, &current, &segment.name, call && last, byte);
            if matches!(next.status, ResolveStatus::Unresolved | ResolveStatus::ReceiverNotBound) {
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
            ResolveStatus::Resolved | ResolveStatus::Ambiguous if base.targets.len() == 1 => {
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
        if parts.is_empty() {
            return ResolveResult::none(ResolveStatus::Unresolved);
        }
        for length in (1..=parts.len()).rev() {
            let found = class_by_parts(self.classes(file.scope), &parts[..length]);
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
        if parts.len() == 1 {
            return self.simple_type(file, &parts[0], byte);
        }
        let found = class_by_parts(self.classes(file.scope), parts);
        match found.len() {
            0 => TypeLookup::None,
            1 => TypeLookup::One(found[0]),
            _ => TypeLookup::Many(found),
        }
    }

    fn method_signature(&self, owner: &ClassRec, member: &Member) -> Option<MethodIdentity> {
        match &member.shape {
            Shape::Unknown => None,
            Shape::Binary { descriptor, generic } => {
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
        let found = match self.collect(class, name, false, site, byte) {
            Ok(found) => found,
            Err(err) => return walk_error(err),
        };
        select_fields(found)
    }

    fn lookup_methods(&self, class: &ClassRec, name: &str, site: &FileRec, byte: u32) -> ResolveResult {
        let found = match self.collect(class, name, true, site, byte) {
            Ok(found) => found,
            Err(err) => return walk_error(err),
        };
        select_methods(found)
    }

    fn collect<'a>(
        &'a self,
        class: &'a ClassRec,
        name: &str,
        call: bool,
        site: &FileRec,
        byte: u32,
    ) -> Result<Vec<Candidate<'a>>, WalkError> {
        let steps = self.hierarchy(class)?;
        let mut found = Vec::new();
        for step in steps {
            let members: Vec<&Member> = if call {
                step.class.methods.iter().filter(|member| member.name == name).collect()
            } else {
                step.class.fields.iter().filter(|member| member.name == name).collect()
            };
            for member in members {
                if !self.can_see(member, step.class, class, site, byte)? {
                    continue;
                }
                found.push(Candidate {
                    member,
                    target: member_target(step.class, member),
                    depth: step.depth,
                    signature: if call { self.method_signature(step.class, member) } else { None },
                    declaring: ClassIdentity::of(step.class),
                    ancestors: if call {
                        self.method_hierarchy(step.class)?
                            .into_iter()
                            .skip(1)
                            .map(|s| ClassIdentity::of(s.class))
                            .collect()
                    } else {
                        BTreeSet::new()
                    },
                    unspecified: member.access == Access::Unspecified,
                });
            }
        }
        Ok(found)
    }

    fn hierarchy<'a>(&'a self, start: &'a ClassRec) -> Result<Vec<Step<'a>>, WalkError> {
        let mut out = Vec::new();
        let mut gray = Vec::new();
        let mut black = BTreeSet::new();
        self.walk(start, 0, false, &mut gray, &mut black, &mut out)?;
        Ok(out)
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
    ) -> Result<(), WalkError> {
        let name = (class.scope, class.path.clone(), class.dotted());
        if gray.iter().any(|seen| seen == &name) {
            return Err(WalkError::Cycle);
        }
        if !black.insert(name.clone()) {
            return Ok(());
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
        for edge in edges {
            match self.resolve_edge(class, &edge) {
                Named::None => {}
                Named::Many(classes) => {
                    return Err(WalkError::Ambiguous(classes.into_iter().map(ClassRec::target).collect()));
                }
                Named::One(next) => self.walk(next, depth + 1, method_inheritance, gray, black, out)?,
            }
        }
        gray.pop();
        Ok(())
    }

    fn can_see(
        &self,
        member: &Member,
        declaring: &ClassRec,
        start: &ClassRec,
        site: &FileRec,
        byte: u32,
    ) -> Result<bool, WalkError> {
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

    fn resolve_edge<'a>(&'a self, class: &ClassRec, parts: &[String]) -> Named<'a> {
        if class.binary {
            return self.class_named(class.scope, &parts.join("."));
        }
        let Some(file) = self.files.iter().find(|file| file.path == class.path) else {
            return Named::None;
        };
        match self.lookup_type_name(file, parts, class.name_start) {
            TypeLookup::One(found) => Named::One(found),
            TypeLookup::Many(found) => Named::Many(found),
            TypeLookup::None | TypeLookup::MissingImport => Named::None,
        }
    }

    fn class_named<'a>(&'a self, scope: usize, dotted: &str) -> Named<'a> {
        let found: Vec<_> = self.classes(scope).filter(|class| class.dotted() == dotted).collect();
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

fn select_fields(found: Vec<Candidate<'_>>) -> ResolveResult {
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
    let min = found.iter().map(|candidate| candidate.depth).min().unwrap();
    let nearest: Vec<_> =
        found.into_iter().filter(|candidate| candidate.depth == min).map(|candidate| candidate.target).collect();
    match nearest.len() {
        0 => ResolveResult::none(ResolveStatus::Unresolved),
        1 => ResolveResult::many(ResolveStatus::Resolved, nearest),
        _ => ResolveResult::many(ResolveStatus::Ambiguous, nearest),
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
    name: Vec<String>,
    primitive: Option<Primitive>,
    array_rank: usize,
    unsupported: bool,
}
#[derive(Clone, Debug)]
enum Shape {
    Unknown,
    Source { parameters: Vec<ParameterType>, variables: Vec<String> },
    Binary { descriptor: String, generic: bool },
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

#[derive(Clone)]
struct Candidate<'a> {
    member: &'a Member,
    target: Target,
    depth: u32,
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
        path: class.path.clone(),
        kind: member.kind,
        name: member.name.clone(),
        owner: class.dotted(),
        start_byte: member.start,
        end_byte: member.end,
    }
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

fn member_type_here<'a>(file: &'a FileRec, byte: u32, name: &str) -> Option<&'a ClassRec> {
    for class in enclosing_classes(file, byte) {
        if let Some(inner) = file.classes.iter().find(|inner| {
            inner.nesting.len() == class.nesting.len() + 1
                && inner.nesting[..class.nesting.len()] == class.nesting[..]
                && inner.simple() == name
        }) {
            return Some(inner);
        }
    }
    None
}

fn class_by_parts<'a>(classes: impl Iterator<Item = &'a ClassRec>, parts: &[String]) -> Vec<&'a ClassRec> {
    let dotted = parts.join(".");
    classes.filter(|class| class.dotted() == dotted).collect()
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
    if byte < start || byte >= end {
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
        for member in class.fields.iter().chain(class.methods.iter()) {
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
    let mut file = FileRec {
        scope: 0,
        path: path.to_string(),
        package: String::new(),
        singles: Vec::new(),
        stars: Vec::new(),
        static_names: Vec::new(),
        classes: Vec::new(),
        locals: Vec::new(),
        call_inventory_valid: parsed.errors().is_empty() && valid_call_inventory(parsed.root(), 0),
    };
    index_node(parsed.root(), 0, &mut IndexCtx { file: &mut file, class_stack: Vec::new(), scope_stack: Vec::new() });
    file
}

struct IndexCtx<'a> {
    file: &'a mut FileRec,
    class_stack: Vec<usize>,
    scope_stack: Vec<(u32, u32, ScopeKind)>,
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
    match node.kind() {
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
    let mut class = ClassRec {
        scope: 0,
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
        is_interface: node.kind() == SyntaxKind::Interface,
        type_variables: type_variables(node, start),
        primitive_owner: node.kind() == SyntaxKind::Class
            && ctx.class_stack.is_empty()
            && child_kind(node, start, SyntaxKind::TypeParamList).is_none()
            && !tokens(node, start).iter().any(|t| matches!(t.0, SyntaxKind::Extends | SyntaxKind::Implements))
            && valid_modifiers(node, start, false),
        binary_root: false,
        fields: Vec::new(),
        methods: Vec::new(),
    };
    let (super_parts, interface_parts) = declared_supers(node, start);
    class.super_parts = super_parts;
    class.interface_parts = interface_parts;
    let index = ctx.file.classes.len();
    ctx.file.classes.push(class);
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

fn index_components(params: &GreenNode, start: u32, ctx: &mut IndexCtx<'_>, class_index: usize) {
    let mut offset = start;
    for child in params.children() {
        if let Some(child) = child.as_node()
            && child.kind() == SyntaxKind::Parameter
            && let Some((name, name_start, name_end)) = direct_ident(child, offset)
        {
            ctx.file.classes[class_index].fields.push(Member {
                name,
                kind: TargetKind::Field,
                start: name_start,
                end: name_end,
                declared: read_type(child, offset),
                access: read_access(child, offset),
                shape: source_type_shape(child, offset),
                source_call: None,
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
            name,
            kind,
            start: name_start,
            end: name_end,
            declared,
            access: read_access(node, start),
            shape: if kind == TargetKind::Method {
                source_method_shape(node, start)
            } else {
                source_type_shape(node, start)
            },
            source_call: (node.kind() == SyntaxKind::Method).then(|| source_call(node, start)).flatten(),
        };
        if kind == TargetKind::Field {
            ctx.file.classes[class_index].fields.push(member);
        } else {
            ctx.file.classes[class_index].methods.push(member);
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
        SyntaxKind::Block | SyntaxKind::ForStmt => ScopeKind::Other,
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
                name,
                primitive,
                array_rank: prefix + suffix,
                unsupported: contains_kind(ty, SyntaxKind::TypeArgList),
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

// Parse the entire JVMS method descriptor, including the discarded return.
// This is independent of cafebabe's permissiveness and never compares raw text.
fn descriptor_parameters(descriptor: &str) -> Option<Vec<ParameterType>> {
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
            name.replace(['/', '$'], ".").split('.').map(str::to_string).collect()
        } else {
            return None;
        };
        Some(ParameterType {
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
    if bytes.get(at) == Some(&b'V') {
        at += 1;
    } else {
        field(bytes, &mut at)?;
    }
    (at == bytes.len()).then_some(parameters)
}

fn class_from_signature(sig: &ClassSignature) -> ClassRec {
    ClassRec {
        scope: 0,
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
        is_interface: sig.is_interface,
        type_variables: Vec::new(),
        primitive_owner: false,
        binary_root: sig.dotted_name == "java.lang.Object"
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
    }
}

fn member_from_signature(member: &MemberSignature, kind: TargetKind) -> Member {
    Member {
        name: member.name.clone(),
        kind,
        start: 0,
        end: 0,
        declared: None,
        access: member.access,
        shape: Shape::Binary { descriptor: member.descriptor.clone(), generic: member.signature.is_some() },
        source_call: None,
    }
}

#[derive(Clone, Debug)]
struct SourceCall {
    is_static: bool,
    varargs: bool,
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
    Some(SourceCall { is_static: mods.iter().any(|t| t.0 == SyntaxKind::Static), varargs })
}

fn valid_call_inventory(node: &GreenNode, start: u32) -> bool {
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

// Only a File -> Class -> ClassBody -> Method path owns modeled method locals.
// Lambdas and anonymous/local/nested classes must not borrow an outer variable.
fn direct_caller(root: &GreenNode, byte: u32) -> Option<(&GreenNode, u32, u32)> {
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
        || path[3].0.kind() != SyntaxKind::Method
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
    Some((method, start, start + method.utf8_len()))
}

fn primitive_expression(
    file: &FileRec,
    node: &GreenNode,
    start: u32,
    caller_start: u32,
    caller_end: u32,
) -> Option<PrimitiveValue> {
    match node.kind() {
        SyntaxKind::ReferenceExpr => {
            let name = token_name(node);
            let mut locals = file.locals.iter().filter(|local| {
                local.name == name
                    && local.start >= caller_start
                    && local.end <= caller_end
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
            primitive_expression(file, expression, *at, caller_start, caller_end)
        }
        SyntaxKind::IndexExpr => {
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
                    if (nodes.is_empty() && !kinds.is_empty()) || (nodes.len() == 1 && kinds != [SyntaxKind::LBracket])
                    {
                        return None;
                    }
                    nodes.push((child.as_ref(), at));
                }
                at += child.utf8_len();
            }
            let [(base, base_at), (index, index_at)] = nodes.as_slice() else { return None };
            if kinds != [SyntaxKind::LBracket, SyntaxKind::RBracket] {
                return None;
            }
            let mut base = primitive_expression(file, base, *base_at, caller_start, caller_end)?;
            let index = primitive_expression(file, index, *index_at, caller_start, caller_end)?;
            if base.rank == 0
                || index.rank != 0
                || !matches!(index.primitive, Primitive::Byte | Primitive::Short | Primitive::Char | Primitive::Int)
            {
                return None;
            }
            base.rank -= 1;
            Some(base)
        }
        _ => None,
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
) -> ResolveResult {
    let parsed = visibility[scope].iter().find_map(|index| projects[*index].file(path));
    let Some(parsed) = parsed else {
        return ResolveResult::none(ResolveStatus::Unresolved);
    };
    Index::scoped(projects, visibility).resolve(path, parsed, byte)
}

/// Validated workspace inputs, indexed once with declaration-owner contexts.
pub(crate) struct PreparedScopedIndex<'a> {
    projects: Vec<&'a Project>,
    index: Index,
}
impl<'a> PreparedScopedIndex<'a> {
    pub(crate) fn new(projects: Vec<&'a Project>, visibility: &[BTreeSet<usize>]) -> Self {
        let index = Index::scoped(&projects, visibility);
        Self { projects, index }
    }
    pub(crate) fn resolve_at(&self, scope: usize, path: &str, byte: u32) -> ResolveResult {
        let parsed = self.index.visibility[scope].iter().find_map(|index| self.projects[*index].file(path));
        let Some(parsed) = parsed else {
            return ResolveResult::none(ResolveStatus::Unresolved);
        };
        self.index.resolve(path, parsed, byte)
    }
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
