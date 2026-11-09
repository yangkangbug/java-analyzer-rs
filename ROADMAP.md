# Native Java analysis roadmap

## Goal and boundary

Build a reusable Rust Java project-analysis engine, then integrate it with
OpenIDEA to replace JDT LS analysis capabilities incrementally. The core must
not start a JVM. Java compilation, application execution, and debugging still
use a JVM/JDK. Maven/Gradle model-import adapters may invoke their existing
JVM tools; their output must cross a clear boundary into the native core.

IntelliJ's separation of syntax, symbols/stubs, indexes, and project services
is a design reference, not a claim that its PSI or type system has been ported.

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
omits switch/pattern/module support and some parameter/type-annotation syntax.

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

## M2: semantic types and method applicability

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

The measured [single-load archive reuse](docs/ARCHIVE_REUSE.md) prerequisite is implemented:
sharing immutable parsed signatures across compilation scopes after freshly reading
and exactly comparing their archive bytes. Per-scope contexts and logical input
budgets remain separate. This targets duplicated decoding and snapshot cloning,
not persistent snapshots or incremental semantic invalidation. A true importer
also needs a model for source exclusions, generated inputs and build conditions;
guessing these from Maven/Gradle directory names would not close M1.

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
