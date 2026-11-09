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
overload applicability/selection, generic substitution, references, rename,
completion, semantic diagnostics, and an editor/LSP adapter. A local variable
sharing a method's name can still capture an unqualified call. The parser also
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

## M2: semantic types and method applicability

Introduce types separate from spelling: primitives, arrays, declared types,
type variables, wildcards, substitutions, null, and incomplete/error types.
Implement namespace/access rules, expression types, conversions, constructors,
overload applicability and specificity, and generic inference in testable steps.
Normalize source and class-file signatures against resolved type identity.

Gate: positive/negative fixtures cover each rule and incomplete editor input.
Optional javac/JDT LS differential checks can validate development fixtures,
but neither becomes a runtime dependency. Fix known small soundness defects
as part of these capabilities, not as a substitute for the project model.

## M3: incremental query engine and cancellation

Cache parsed units, symbol stubs, dependency edges, and semantic queries with
explicit invalidation. Expose immutable analysis snapshots and cancellation;
keep expensive work away from the UI. Design stable identities in M1 and start
measuring while M2 grows, rather than bolting caching onto a completed resolver.

Gate: full recomputation agrees with incremental results after file/import/JAR
edits and deletions. Record cold/warm p50/p95 latency, peak memory, and edit-to-result
latency on versioned small/medium/large Java projects. Set budgets from those
measurements; the existing one-sample timing test is not a substitute.

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
