# Native Java analysis roadmap

## Goal and boundary

Build a reusable, standalone Rust Java project-analysis engine. Editor and
build-tool integrations remain optional adapters, not goals or dependencies of
the analysis core. Historical extraction provenance is recorded in NOTICE and
PROVENANCE.md. The core must not start a JVM. Java compilation, application execution, and debugging still
use a JVM/JDK. Maven/Gradle model-import adapters may invoke their existing
JVM tools; their output must cross a clear boundary into the native core.

IntelliJ's separation of syntax, symbols/stubs, indexes, and project services
is a design reference, not a claim that its PSI or type system has been ported.

## Bounded binary invocation composition

Selected nongeneric binary method results can supply primitive scalar or plain
reference types to existing qualified call chains and arguments. Selection and
return typing remain separate, with exact declaration linking context and shared
proof limits. Arrays, generic substitution and arbitrary expression inference
remain deferred. See [binary call results](docs/BINARY_CALL_RETURNS.md).

## Exact-name directory reuse

Four legacy exact-name lookup paths reuse the existing immutable scope-qualified
name directory rather than scanning all source/platform classes. Candidate order,
duplicates and visibility remain unchanged; no additional Java semantics or
incremental cache is introduced. See [boundary and evidence](docs/EXACT_NAME_DIRECTORY.md).

## Source enum constant navigation

Explicit source constants are fields of their enum; anonymous constant-specific
bodies remain opaque rather than leaking members. Owner visibility is bounded,
with cross-package protected nested types deferred. Enum constructors, synthetic
methods and case-label semantics remain unsupported. See [scope and checks](docs/SOURCE_ENUM_CONSTANTS.md).

## Current measured baseline

The source started as a Java syntax/name-binding subset, not a language server.
It has source parsing and block reparsing, source/class-file registration,
limited name lookup, and supertype member walks. Legacy resolve queries rebuild
the index; explicitly prepared handles reuse one immutable index. The suite includes receiver provenance and array/varargs signature regressions,
plus bounded native JAR/JMOD and immutable project-loading acceptance tests. It has no real-project performance budget.

Missing foundations include build-tool/JDK discovery, JPMS rules, stable symbol
IDs, type inference,
general overload applicability/selection, generic substitution, references, rename,
completion and semantic diagnostics. A narrow definition-only LSP adapter is
available, with the limitations described below. Unqualified invocation
names now use the method namespace independently of
same-spelled local/parameter/field values. The parser still
supports traditional colon-label switch structure and shared lexical scope, but
omits switch expressions, arrow rules, patterns, switch semantic validation, module
support and some parameter/type-annotation syntax. See [switch boundaries](docs/TRADITIONAL_SWITCH.md).
Local type declarations have an opaque containment boundary; their bindings
remain unsupported rather than being indexed as ordinary members.

### Return declaration inventory separation

A bounded return-Type AST proof now keeps complete nested-generic/wildcard
headers in declaration inventory independently from strict applicability and
return propagation. Three previously deferred real Commons ClassUtils queries
pass their original exact targets. Generic methods/parameters remain complete
unsupported competitors; ancestor grammar and parser behavior are unchanged.
See [scope, refusals and IDEA reference](docs/DECLARATION_INVENTORY.md).

### Effective binary declarations

The shared strict selector now validates complete same-name binary candidate
sets before proving exact-descriptor ancestor overrides. It supports inherited
class declarations and public abstract interface declarations, while preserving
default/bridge/generic/mixed-source refusal boundaries. See
[scope and frozen evidence](docs/EFFECTIVE_BINARY_METHODS.md).

## M0: reproducible independent project

Gate: independent manifest/lockfile/toolchain; preserved licensing/provenance;
source/test equivalence with the migration snapshot; tests, lint, docs, example,
and root CI definition. Do not remove the original OpenIDEA copy before the
independent remote and CI exist and their exact revision is verified.

## M1: project and classpath model

Define stable file/symbol identities and an immutable project snapshot containing
source roots, test/generated roots, language level, module graph, dependencies,
and JDK selection. Add native JAR/class-directory/JDK module metadata loading,
archive limits, multi-release JAR selection, and explicit duplicate precedence.
Keep Maven/Gradle discovery in adapters that emit the same project model.

Gate: checked-in multi-file and multi-module fixture projects resolve JDK and
third-party symbols without manually adding each class. Change/remove a source,
JAR, or module edge and verify the next snapshot has no stale targets. Unsupported
build models fail explicitly rather than silently borrowing JDT LS results.

### First M1 slice implemented

Explicit UTF-8 source roots and ordered local class-directory/JAR inputs load into
an owned read-only snapshot. Native ZIP/DEFLATE reads selected class signatures;
multi-release JARs select by target release. Bounds cover directory traversal,
input bytes, archive entries and expansion. First binary classpath entry wins,
with shadowed origins reported; source collisions remain ambiguous. Reloads
replace input sets without mutating older snapshots. A checked-in two-source/JAR
fixture resolves through inheritance, and a real Commons Lang JAR was analyzed
with no Java executable on PATH; see docs/VALIDATION.md.

This does not close M1: canonical path identities are not persistent symbol IDs;
no JDK discovery/JPMS or build-tool model exists yet. Explicit compilation-scope
relationships are now supported below. Stable identities and scoped query
invalidation remain the next foundation. The current snapshot rebuilds from disk,
and the legacy binder rebuilds its index per query. Prepared handles now retain
one derived index; neither path implements incremental invalidation.

### Explicit platform input slice implemented

`ProjectConfig.platform` accepts caller-supplied JMOD 1.0 or legacy platform JAR
paths before application binaries. Native archive loading selects `classes/`
signatures from JMOD data, ignores module descriptors and nonclass sections, and
shares project entry/input/expansion budgets with application dependencies.
Duplicate precedence, provenance, source collisions, and fresh-snapshot semantics
remain explicit. Tests resolve implicit `java.lang` types and Object members with
platform data and leave them unresolved when absent. No JDK binary is bundled,
discovered, started, linked, or embedded. This is a flat class-library input,
not JPMS resolution, jimage/ct.sym reading, or a `--release` API model.

### Explicit compilation/source-set graph implemented

`WorkspaceConfig` identifies each compilation by project and source-set labels,
with explicit directed source dependencies. Reachable sources are visible
transitively; test or unrelated roots never leak through an implicit reverse edge.
Each compilation owns its platform/classpath inputs, with no automatic binary
export. Source declarations and binary hierarchy instances retain their original
compilation context, including when one physical JAR links against different
superclass versions in different scopes.

Graph validation, canonical source ownership, deterministic views, aggregate
workspace budgets, and edge-change/old-snapshot tests now cover a checked-in
main/test + library fixture. This is an explicit native project model, not a
build-system importer or JPMS graph. Public targets still use path/range provenance;
persistent symbol identity and incremental scoped queries remain unimplemented.

### Explicit configuration bootstrap implemented

The native [configuration bootstrap](docs/CLI_CONFIGURE.md) now emits the same
manifest model from explicit multi-compilation command-line inputs after bounded
loading. This removes manual JSON construction, not explicit configuration:
build discovery, generated-source production and semantic completeness are still
separate gates.

### Explicit source selection implemented

Each compilation can provide literal root-relative source exclusions, through
the core API, schema-v1 manifests or repeatable configure options. Exact files
are omitted and real directories pruned independently beneath each source root.
Shared no-I/O validation precedes loading, while encountered symlinks, entry
budgets, canonical ownership and immutable snapshots retain their boundaries.
See [source selection](docs/SOURCE_SELECTION.md) for syntax, limits and restart
requirements. Explicitly omitting a module descriptor does not implement JPMS,
automatic build import, generated sources or a new flat-classpath mode.

### Prepared analysis lifetime implemented

Flat projects and compilation workspaces can explicitly prepare a reusable index.
Prepared scope views share the workspace index and preserve declaration contexts.
Legacy rebuild queries remain an independent differential path. Shared borrowing
prevents input mutation while a prepared handle is live; fresh snapshots require
fresh preparation. This removes repeated index construction, not collection scans,
archive reloads, or semantic limitations. Real-source acceptance records parser
gaps and repeated-query observations separately from correctness tests.

[Identity boundaries](docs/PREPARED_ANALYSIS.md) remain explicit: public targets are
provenance, not persistent symbol IDs, even when their path/range values are equal.
Stable input/declaration identities, scoped invalidation, cancellation, and measured
project-scale memory/latency budgets remain open work.

### Real-source-driven catch recovery implemented

The pinned Commons Lang source workload identified catch-parameter `final` as a
large recovery discontinuity. Catch-specific modifier/type/annotation parsing now
preserves following declarations and supplies bounded error recovery. Across the
unmodified 249-file corpus, parser error nodes decreased from 8,187 to 149; this
counts recovery cascades, not independent semantic features. The exact inventory
and query-target changes are versioned. Throwable legality, union least-upper-
bound semantics, remaining parser gaps and general project support are still open.

The next source-grounded slice implements basic-for expression lists and prevents
partial incremental fragments from losing source coverage. The complete corpus
now reports 85 diagnostics, with every cause retained and the unrelated contextual-
`var` recovery increase disclosed. [Validation boundaries](docs/ACCEPTANCE_PLAN.md)
separate default native CI from fixed-input extended CI and the optional
independent compiler oracle. Neither prepared/rebuild agreement nor error counts prove Java semantics.

### Source-grounded primitive class-literal syntax implemented

Primitive/array and scalar void `.class` syntax now has a distinct lossless node.
The fixed Gson workload drops from 331 to 166 parser errors without narrowing
its 211 files or changing fixed navigation/refusal checks; the Commons workload
stays unchanged. This adds no reflection type inference or generic applicability.
See [class-literal scope and evidence](docs/CLASS_LITERALS.md).

### Independent acceptance gate implemented

A separate trusted-main/manual workflow verifies pinned upstream artifacts and
runs the native real-source workload, then a development-only javac declaration
oracle. Fifty-nine supported bindings and four explicit refusal boundaries are checked
independently; a further mixed-input suite checks five source overrides against
compiled dependencies and four refusal boundaries. The oracle exposed a shadowed-
super field error now conservatively refused. This does not implement full super semantics or close project/type gates.
The default native test path and production library remain JVM-free. Broader
projects, diagnostics, overload/generic semantics and required-gate policy remain
open work; see the validation boundary document.

### Multiple declarator identity implemented

Every sibling field in a comma declaration now receives its own source target.
Field/local declarators retain the shared type and their own postfix array rank;
initializer names and nested commas are not declarations. Arrays remain explicit
unsupported receivers instead of borrowing scalar element members. Existing
local declaration ordering, source-owner imports and compilation visibility stay
covered, including independent compiler-selected targets. This is a declaration
correctness foundation for later symbol/type services, not persistent IDs,
array-expression typing, definite assignment or complete Java scope rules.
See [declaration boundaries](docs/DECLARATORS.md).

### Local-type containment and surrounding navigation

The second fixed project exposed a correctness problem beyond parser counts:
method-local classes could disrupt the enclosing method and make later variable
uses look like fields. An opaque local-type boundary now preserves surrounding
statement structure, excludes unsupported descendants from the symbol index,
and blocks type-name fallback within the declaration's lexical scope. Surrounding
locals retain their own declaration identity. See [the precise boundary and
acceptance evidence](docs/LOCAL_TYPES.md).

This is not local-class binding, capture analysis, or record synthesis. A single
unsupported boundary can replace many cascading reports without analyzing its
body. Keep that distinction separate from actual repaired navigation. The next
semantic work still includes qualified instance-call applicability and reference
types: Gson's generic fromJson, overloaded add, and chained-call cases remain
outside this slice.

## M2: semantic types and method applicability

### Bounded reference instance calls implemented

Direct calls on explicitly typed local/parameter source receivers now have a
separate proof for scalar reference arguments, strict reference widening and a
unique most-specific direct public source method. Declaration-context identity,
complete bounded hierarchy/member inventories and access checks are prerequisites.
A narrow extension admits plain `new Type()` arguments only with an explicit
public zero-argument constructor and complete concrete source-class evidence;
implicit defaults, additional constructors, throws and general construction stay
unsupported. Generic/inherited competitors and unknown evidence remain refused.
The two earlier Gson `add(a)` selections remain fixed. An additive gate now
corrects original `a.add(new JsonObject())` to `add(JsonElement)`, preserving the
other 15 original rows, historical golden files, source bytes and parser records.
See [the exact boundary](docs/REFERENCE_CALLS.md) and
[real-Gson construction acceptance](examples/second_project/CONSTRUCTOR_CALLS.md).
Ordinary string-literal arguments now add exact supplied binary String identity
without lexical-name inference. Real Gson get/remove/has and mixed add calls are
covered by an [additive gate](examples/second_project/STRING_CALLS.md); unknown
binary generic edges, text blocks, Unicode preprocessing and other literal types
remain refused. This does not enable String/Object overload selection or general
reference conversion through generic class-file signatures.
Validated scalar primitive formals no longer block a proved strict reference
winner: reference actuals cannot unbox in that first phase. The
[Gson strict-phase gate](examples/second_project/STRICT_REFERENCE_PHASE.md) selects
`remove(JsonElement)` for an explicit `JsonPrimitive` argument while retaining
integer/mixed-argument and chained-call refusals. Complete candidate validation,
primitive signature identity and all existing evidence limits remain required.
A later [single-primitive-argument instance-call slice](docs/QUALIFIED_PRIMITIVE_CALLS.md)
adds identity/widening selection for explicit scalar locals/parameters and bounded
nonnegative decimal `int` literals. The additive frozen Gson plan changes only
`remove(1)` to its exact `remove(int)` declaration, while mixed-argument and
chained-result controls remain refused. IDEA Community mechanisms inform the
independent Rust design, not a PSI dependency or full overload-resolution claim.

The [mixed strict-invocation slice](docs/MIXED_STRICT_CALLS.md) combines the existing
reference and scalar-primitive argument evidence position by position. A unique
winner must dominate every other applicable complete formal vector; no aggregate
conversion score is used. The additive Gson target is now `set(0,b)`, while outer
call-result receiver inference remains outside scope.

The later [one-layer call-result receiver proof](docs/CALL_RESULT_RECEIVERS.md)
composes those strict calls through an explicit nongeneric reference return,
with declaration-context lookup and one shared budget. That historical one-layer
boundary is now extended by [composable source invocation values](docs/COMPOSED_SOURCE_INVOCATIONS.md)
to bounded nested arguments and receiver chains; general expression inference remains unsupported.
The subsequent [plain-null strict-argument proof](docs/NULL_STRICT_CALLS.md)
adds the null invocation conversion to validated reference formals only. Formal
specificity and complete competitor checks remain unchanged; unknown arguments,
arrays, generics, boxing and additional expression inference are not enabled.
That phase's sole new real-Gson success was `set(0, null)`; its predicted
`add(propertyName, null)` target was explicitly deferred rather than counted as
success. The later [exact binary argument compatibility proof](docs/BINARY_ARGUMENT_COMPATIBILITY.md)
addresses the ordinary binary-root cross-scope gap using selected-origin and
complete modeled inheritance-closure evidence, without global type unification
or cross-scope subtype widening. Independent signature comparison is budgeted;
small complete closures support both shared and independent parses, while shared
signatures can allow proofs that oversized independent copies must refuse.

This does not complete M2 or make every legacy Resolved result an applicability
proof. The independent oracle preserves original evidence and records one
no-platform singleton tightening. Missing Object is not silently treated as an
empty ancestor. Generic inference, arbitrary expression typing, inherited/binary
selection and the common semantic type model remain open foundations.

The current priority is the standalone JVM-free Java engine: accurate supported
call selection, explicit refusal boundaries and real-source regression evidence.
Editor integration is separate future adapter work, not the current development
objective. It must not bring IDE, JDT, build or debug dependencies into the core.

### Cross-cutting experimental CLI entry

The optional native executable exposes an explicit versioned compilation manifest,
project inventory, prepared declaration-query batches and labeled parser reports.
It does not infer Maven/Gradle models or fetch dependencies. Source targets retain
their owning compilation and ranges; binary targets do not invent source locations.
This provides a narrow end-to-end entry point for real-project validation, not a
completed IDE preview. See [CLI contract](docs/CLI.md) and
[preview acceptance gates](docs/PREVIEW_PLAN.md). Native worker supervision now
adds execution deadlines and crash/cancellation reporting; see M3 below. Automatic
reload, broad editor capabilities, interactive performance budgets and distribution
remain needed, alongside the semantic work below. A separate
[delivery-evaluation protocol](docs/CLI_PREVIEW.md) now fixes a second real
core/extras main/test project, local Linux packaging and honest resource
measurements. It retains substantial parser gaps and does not mark the preview
gates complete.

### Type-system direction

Introduce types separate from spelling: primitives, arrays, declared types,
type variables, wildcards, substitutions, null, and incomplete/error types.
Implement namespace/access rules, expression types, conversions, constructors,
overload applicability and specificity, and generic inference in testable steps.
Normalize source and class-file signatures against resolved type identity.

Gate: positive/negative fixtures cover each rule and incomplete editor input.
Optional javac/JDT LS differential checks can validate development fixtures,
but neither becomes a runtime dependency. Fix known small soundness defects
as part of these capabilities, not as a substitute for the project model.

### First semantic identity slice implemented

Supported source and class-file method parameters now normalize to primitive,
array and resolved declared-type identity in their owning compilation context.
Returns do not create overloads; equal signatures require a justified declaring-
type relationship to hide ancestors. Missing/ambiguous types, generic contexts,
inherited member-type gaps and bridge/default-method conflicts remain explicit.
Private identities are snapshot-local, not persistent public symbol IDs.
See [signature model and API compatibility](docs/METHOD_SIGNATURES.md).

Seven real StrBuilder.length query sites now resolve to the source override of
CharSequence.length. ObjectUtils.clone(T) and Object.clone() remain distinct
candidates, without argument-based selection. Two generic comparisons become
more conservative rather than retaining unsupported spelling assumptions. This
is a project-navigation capability, not a reduction in parser error counts.

A first bounded source-call slice now proves strict primitive identity/widening
and fixed-array applicability in closed same-owner static contexts. Four real
IEEE754rUtils calls select compiler-verified source declarations; known unique
source arity/primitive contradictions refuse instead of choosing a wrong target.
Unknown expressions and unsupported competitors remain explicit. See the
[precise contract and API change](docs/PRIMITIVE_CALLS.md). General literal,
reference, boxing, generic, inherited and binary call selection remain open.

Nested already-proved direct source calls now propagate explicit primitive scalar
returns. Four additional real outer max/min calls are compiler-checked; the ten
previously refused fixed queries remain refused. Parameter specificity still
selects the callee before its return type is used. See
[compositional proof and bounded work](docs/CALL_RETURNS.md).

The next M2 gate broadens expression/type evidence without weakening these
refusals. Couple that with stable declaration/query identity and a small real-project definition/
hover acceptance surface before claiming IDE usability. Generic inference,
incremental invalidation, completion, diagnostics, references and safe refactoring
remain required work; do not replace that route with unrelated syntax cleanup.

### Owned prepared analysis lifecycle

`AnalysisSnapshot` now owns immutable inputs and their shared prepared index,
with atomic validated source-overlay derivation and explicit fresh loading.
Unchanged batches share the same state after validation; real changes still
rebuild the complete index. Prior handles preserve old results. This prepares a
long-lived embedding API, not fine-grained invalidation or a persistent editor
worker. See [lifecycle and remaining integration gates](docs/ANALYSIS_SNAPSHOTS.md).

## M3: incremental query engine and cancellation

Cache parsed units, symbol stubs, dependency edges, and semantic queries with
explicit invalidation. Expose immutable analysis snapshots and cancellation;
keep expensive work away from the UI. Design stable identities in M1 and start
measuring while M2 grows, rather than bolting caching onto a completed resolver.

Gate: full recomputation agrees with incremental results after file/import/JAR
edits and deletions. Record cold/warm p50/p95 latency, peak memory, and edit-to-result
latency on versioned small/medium/large Java projects. Set budgets from those
measurements; the existing one-sample timing test is not a substitute.

### First CLI execution boundary implemented

The public CLI runs analysis in a supervised instance of its own native executable.
Wall-clock deadlines, bounded output capture, protocol checks and optional
stdin-EOF cancellation convert worker failures into explicit errors. A stdin
lease lets the worker exit when its supervisor disappears. This is coarse process
cancellation, not incremental or cooperative analysis cancellation. Core parsing
and binding APIs are unchanged, and no JVM or external analysis tool is started.
The [execution contract](docs/CLI_EXECUTION.md) excludes memory quotas, arbitrary
descendant containment, caller-output backpressure and hard real-time guarantees.
The next interactive gates still need snapshot/reload correctness and measurement,
alongside method applicability and the semantic coverage described in M2.

### Bounded editor navigation slice

The definition-only stdio LSP adapter keeps protocol and full-text buffer state in
a broker while native one-shot jobs load and analyze complete snapshots. Frozen
configuration and source ownership, whole-workspace revisions, exact snapshot
target positions and explicit cancellation prevent unsupported stale navigation.
The Neovim companion also guards results already in transit, after flushing all
attached buffers. See [the scope and acceptance contract](docs/LSP_PREVIEW.md).

This is a usable adapter for the existing narrow resolver, not a persistent
incremental engine. Every query still loads and prepares the project anew;
configuration/source-inventory changes require restart. No general Java semantic
coverage, automatic build import, UI latency budget or replacement of JDT LS is
implied. Next gates must improve measured repeated-query behavior and practical
semantic coverage without losing cross-buffer correctness or refusal boundaries.

The client-side [explicit refresh lifecycle](docs/EDITOR_REFRESH.md) serializes
retirement, replacement initialization and full unsaved-buffer replay. It settles
obsolete callbacks promptly, avoids restart-storm process overlap, and exposes
initialization/cleanup failures. This improves the existing restart boundary;
it is not transactional in-process reload, an automatic file watcher, or an
incremental semantic snapshot engine.

The measured [single-load archive reuse](docs/ARCHIVE_REUSE.md) prerequisite is implemented:
sharing immutable parsed signatures across compilation scopes after freshly reading
and exactly comparing their archive bytes. Per-scope contexts and logical input
budgets remain separate. This targets duplicated decoding and snapshot cloning,
not persistent snapshots or incremental semantic invalidation. A true importer
still needs generated inputs and build conditions beyond the explicit source
exclusions above; guessing these from Maven/Gradle directory names would not
close M1.

## M4: usable analysis capabilities

Build definition/hover, semantic diagnostics, completion/signature help,
reference search, then rename with conflict checks and previewable edits.
Use stable API types, UTF-16 boundaries, incomplete-input handling, cancellation,
and tests for stale snapshots. Never claim rename safety before references and
symbol identity are reliable.

Gate: each capability has end-to-end fixture tests and explicit unsupported
results. References and rename include sources, inheritance, imports, and library
boundaries without silently editing an unrelated symbol.

## M5: OpenIDEA integration and staged cutover

Choose a thin native-library or LSP adapter after the analysis API is stable.
Route capabilities behind a switch, compare results on real projects, and keep
an explicit fallback during rollout. Separate JDT LS's current build/classpath
and java-debug responsibilities before removing any startup path.

Gate: completion/navigation/references/diagnostics/refactoring acceptance suites,
crash/cancellation tests, and measured latency pass for the supported project
matrix. Removing JDT LS analysis does not promise JVM-free Java run/debug.

### Unsaved-buffer and native Mac gate slice

Immutable source-only replacements preserve ownership, visibility and original
file/compilation/workspace byte budgets without rereading binary inputs. Strict
UTF-16 line/character conversion and one-shot CLI overlays allow a caller to query
unsaved source text. Caller revision labels are not stale-result enforcement; a
persistent adapter, asynchronous lifecycle and transactional project reload remain
open. Snapshot cloning now shares immutable binary signatures; fresh prepared
indexes are still required and this is not incremental caching.
See [the buffer contract](docs/CLI_OVERLAYS.md).

A separate [macOS source-build/runtime gate](docs/MACOS_NATIVE.md) targets declared
native ARM64 and Intel hosted runners. No user computer is used, no binary release
is uploaded, and workflow configuration alone does not establish compatibility.

### Explicit cast-null strict arguments

The [bounded cast-null argument proof](docs/CAST_NULL_ARGUMENTS.md) preserves the
explicit plain reference type of `(Type)null` during strict source calls. Real
Gson coverage adds Number and JsonElement cases only; eighteen other cast-null
probes, including String/Boolean/Character overload sets, remain conservative
refusals. No general casts, receivers, generic conversion or cross-scope widening
are enabled.

### Declaring-aware inherited source calls

The [bounded inherited-source proof](docs/INHERITED_SOURCE_CALLS.md) collects
plain source-parent candidates and applies existing strict invocation selection.
Unique inherited methods and different-signature parent/child overloads retain
the actual declaring class and its type context. The additive source override
proof below handles a bounded same-signature subset; interface/default methods
and binary/generic parent selection remain unsupported.

### Plain source override proof

See [source override selection](docs/SOURCE_OVERRIDE_CALLS.md) for the additive
public instance same-signature capability and conservative annotation/return
boundaries. Three real Gson deepCopy queries now select their source subclass.
The fourth predicted JsonNull query remains deferred because its annotated local
receiver has no supported type evidence; the original forecast and failure are
preserved alongside an explicit V2 contract.

### Composable source invocation evidence

[Bounded source invocation values](docs/COMPOSED_SOURCE_INVOCATIONS.md) unify
receiver chains and nested actual arguments through the same strict selector,
with declared reference/primitive results and shared work/path-depth limits.
Direct binary method selection and descriptor identity are covered by the
separate bounded slice below; neither slice introduces generic or general
expression inference.


### Direct binary instance selection

The shared strict selector supports bounded direct binary instance declarations
with complete JVM method descriptors and explicit declaration contexts. That initial slice did not add binary override selection or return propagation;
subsequent bounded effective-method and binary-result proofs are described above. Unsupported ancestors
and flags fail closed, including deliberate contractions of legacy name-only
successes. [Scope and CLI opt-in details](docs/BINARY_METHOD_CALLS.md) describe the
three revised StringUtils selection targets, three ClassUtils deferrals and
three conservative controls; original
historical records remain intact.
