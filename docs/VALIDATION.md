# Native project-loading validation

Validation performed on 2026-11-09 in a cloud Linux environment with Rust 1.96.0.
Baseline: `79498ebcd4e60c172158bf33d397980d7e35127f`. This record describes the
M1 project/classpath slice, not full Java conformance or project-scale performance.

## Local checks

- `cargo fmt --all --check`
- `cargo clippy --all-targets --locked --offline -- -D warnings`
- `cargo test --all-targets --locked --offline`: 97 tests pass (68 existing,
  18 JAR loader, 11 project snapshot).
- `cargo test --doc --locked --offline`: succeeds, zero doctests.
- `RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --locked --offline`
- `cargo build --examples --locked --offline`, original resolve example,
  checked-in source-to-JAR example, native runtime script.
- `cargo package --allow-dirty --locked --offline`: packaged crate builds.
- `git diff --check`.

Two source/project acceptance tests first failed with an empty loader (zero
sources and unresolved inherited method). Nine initial JAR tests first failed
with an empty loader, then passed. Independent review found origin-key collisions,
case-insensitive manifest lookup, and quadratic manifest duplicate checking;
these were corrected and reviewed again. Failure coverage includes corrupted
ZIP headers/CRC/size, traversal, duplicates, resource limits, input replacement,
removed classpath entries, old-snapshot retention, and failed-load isolation.

## Real third-party archive, no Java executable available to analysis

The separately downloaded official Apache Commons Lang 3.17.0 archive was used
only as a local validation input. It is not redistributed by this repository.

- Artifact: https://repo.maven.apache.org/maven2/org/apache/commons/commons-lang3/3.17.0/commons-lang3-3.17.0.jar
- SHA-256: `6ee731df5c8e5a2976a1ca023b6bb320ea8d3539fbe64c8a1d5cb765127c33b4`
- Loaded: 395 library classes, one source, zero shadowed classes.
- Resolved: `org.apache.commons.lang3.StringUtils.capitalize` to the actual
  `jar!/org/apache/commons/lang3/StringUtils.class` origin.

After compiling the Rust examples and obtaining that local archive, reproduce:

```sh
sh scripts/check-native-runtime.sh /path/to/commons-lang3-3.17.0.jar
```

The script checks the artifact SHA and runs the compiled analyzer under
`env -i PATH=/nonexistent`. Java home variables and the executable search path
are absent. No build tool, download, Java process, or JDT LS is involved in this
analysis command. The analysis source has no subprocess/JNI/JVM loading path;
the Linux executable's dynamic dependency list contains libc/libgcc and the
platform loader, not libjvm. Archive and class-file decoders are native Rust.

Evidence boundary: the host itself has a Java executable installed elsewhere.
The successful check removes Java from the process environment; it is not a
claim that the host has no JVM files. Stronger filesystem/network isolation and
system-call tracing were unavailable under this environment's sandbox policy.
No complete-Java or large-project benchmark claims follow from this one query.

The mandatory checked-in fixture is hand-assembled Java class data, not generated
by javac during testing. The CI runtime step uses only that fixture and no network
input. The optional real-archive check is fully offline once the JAR is supplied.

## Native platform-input follow-up

Baseline for this follow-up: `558d7c34d34b7a6f54b1e7fd1585e9c0043c641d`.
The full suite now has **114 passing tests**: 97 from the preceding slice,
11 JMOD tests, and six platform-input tests. The same formatting, strict Clippy,
rustdoc, offline build, examples, and package checks pass. Platform acceptance
first failed behaviorally with platform inputs ignored. A large ignored-native-
section regression failed under the original 16 MiB JMOD entry ceiling and passed
with the JMOD-only 64 MiB default; explicit smaller limits still reject it.
Independent review covered shared limits, platform precedence, snapshot isolation,
and provenance, including collisions involving shadowed classes.

Only the `java.base.jmod` data member was extracted from the official Eclipse
Temurin 21.0.13+9 Linux archive after verifying its published SHA-256. No JDK
executable was installed or run, and no platform artifact is redistributed here.

- Release: https://github.com/adoptium/temurin21-binaries/releases/tag/jdk-21.0.13%2B9
- Archive: `OpenJDK21U-jdk_x64_linux_hotspot_21.0.13_9.tar.gz`
- Archive SHA-256: `d7f5d5cf865a2902b70ff549b97586a7d3f234e016f041ab447ea740c7d66d1c`
- Extracted JMOD: 24,683,914 bytes.
- JMOD SHA-256: `fad51abeafe6a1ecb0c2696a88684dc417cc534b74b5d865042df7381d9d0e91`
- ZIP inventory: 7,682 entries, 59,382,714 declared uncompressed bytes.
- Loaded class signatures: 7,615, matching an independent ZIP inventory excluding
  `classes/module-info.class` and nonclass sections.

Reproduce with the local data file after building examples:

```sh
sh scripts/check-platform-runtime.sh /path/to/java.base.jmod
```

Under `env -i PATH=/nonexistent`, the example resolves:

- `String` to `java.lang.String` in `java.base.jmod!/classes/java/lang/String.class`.
- Inherited `toString()` to `java.lang.Object` in
  `java.base.jmod!/classes/java/lang/Object.class`.

The same evidence boundary applies: this shows native analysis without a Java
launch or runtime dependency, not a physically JVM-free host. JPMS export and
readability rules, platform release/API compatibility, and automatic JDK discovery
are explicitly outside this flat platform-input slice.

## Compilation/source-set context follow-up

Baseline: `16e42e1e824ef62d2f1bc7007c07aa44f29d843e`. The full repository suite
now has **124 passing tests**, including ten workspace regressions. Formatting,
strict Clippy, rustdoc, offline examples/build, package verification, and the
empty-environment runtime script pass. The runtime script now includes the
checked-in compilation graph example.

The initial behavioral regression demonstrated that a flattened consumer project
could resolve a dependency's missing superclass against a consumer-only class.
The scoped implementation prevents that result. Tests additionally cover:

- Main/test and sibling isolation, transitive and diamond source dependencies.
- Declaration-context preservation for simple/imported/qualified source types,
  inherited members, source field receivers, and contextual binary hierarchies.
- Different class/platform versions and one physical JAR loaded into different
  compilation contexts without cross-contamination.
- Edge removal, old-snapshot isolation, graph validation before filesystem reads,
  canonical source ownership, deterministic ordering, and aggregate workspace budgets.

Independent Rust-only checks also exercised qualified reverse visibility, shared
binary-superclass context, and cycle-before-I/O behavior outside the repository's
regression harness. No blocking review finding remained.

The `scopes` example reports an inherited field as resolved, library test helpers
and unrelated source types as unresolved, and exactly three visible source files
for `app/test -> app/main -> lib/main`. Its negative references are intentional
editor-input tests, not a claim that the complete fixture compiles without errors.

A separate local smoke test combined the same verified Commons Lang and Temurin
artifacts above with three compilation contexts. Both app/main and app/test
resolved inherited `StringUtils.capitalize` through the library source context,
while `String` used each compilation's explicit platform input. Main could not
query test-owned source. This process also ran under `env -i PATH=/nonexistent`.
It validates the explicit graph contract, not Maven/Gradle or javac classpath
conformance, JPMS rules, or a project-scale performance budget.

## Prepared index and real-source follow-up

Baseline: `60b7660e9c89639af988af3bbb30cda6c6e7c71b`. New prepared-project and
prepared-workspace APIs borrow immutable inputs and retain one binder index.
The initial contract tests failed to compile because those APIs did not exist;
this is an API red test, not evidence of a preexisting semantic bug. Existing
fresh-query APIs remain independent for differential checks.

Direct test-only construction counters verify one index per prepared owner and
no additional construction for repeated successful, missing, or invisible queries
through multiple prepared scope views. Every-byte checks compare ordered results
with fresh rebuilds, including all five fixture scopes, ambiguous results,
owner-context dependencies, and contextual binary versions. Independent review
rerouted 46 existing binding/class-file regressions through preparation and added
two adversarial tests for Unicode/malformed input, extreme offsets, edits, and
concurrent access. A separate compiler check rejected dropping a prepared
workspace while its view was still used. The compile-fail mutation doctest is
included in CI.

Before the implementation, one release-mode run of a synthetic 200-file,
500-query workload took 107,686 microseconds through the rebuild path. A later
single run of the same workload measured 118,580 microseconds for rebuild queries,
250 microseconds for preparation, and 6,969 microseconds for warm queries. The
five-scope checked-in fixture measured 3,184 / 3 / 753 microseconds respectively
for 500 queries. Different timing phases exclude differential-comparison work.
These are local microbenchmark observations, not statistical estimates, memory
measurements, project-scale latency budgets, or a promised speedup. Retained
index records consume memory for the prepared handle's lifetime, and lookups
still scan collections. Manual ignored tests reproduce these synthetic workloads:

```sh
cargo test --release --test prepared --locked --offline -- --ignored --nocapture
cargo test --release --test workspace --locked --offline -- --ignored --nocapture
```

Final checks passed: **130 tests**, two intentionally ignored manual measurements,
one compile-fail doctest, formatting, strict Clippy/rustdoc, all examples, the
empty-environment native runtime checks, and verified Cargo packaging. No
runtime/development dependencies were added in this slice.

### Full published Commons Lang sources

The optional [real-source harness](../examples/real_source/README.md) takes the
complete Commons Lang 3.17.0 sources JAR plus the pinned platform data, verifies
both SHA-256 hashes, extracts Java text only, and runs the prebuilt Rust analyzer
with `PATH=/nonexistent` and no other environment variables. It does not use the
Commons Lang binary JAR to compensate for source-analysis gaps. The harness is
offline and does not invoke a Java tool; Python is only its optional data-preparation
wrapper, not an analysis-core dependency.

- Sources SHA-256: `5fdcac21ad329766054a95367d7583dfcdca737d221d5e01a5f2a198c04c6b18`.
- All 249 published Java source files, 3,676,819 bytes, retain lossless token coverage.
- 63 files contain 8,187 parser error nodes. These include recovery cascades, not
  8,187 independent defects; zero errors in the other files is not a conformance proof.
- Thirteen fixed query positions check every public target field: nine resolved,
  two overload refusals, one unresolved method, and one unbound receiver. Every
  prepared result exactly matches the rebuild result, including refusal cases.
- The published archive excludes upstream tests/build inputs. This is a real
  source-library workload, not a complete repository/build-tool acceptance test.

An independent repeat of the five-round release run (65 queries per mode) measured
load/parse 470.52 ms, one index preparation 47.51 ms, rebuild-query p50/p95
53.54/62.36 ms and prepared-query p50/p95 5.68/12.75 ms. These differ from the first
run recorded with the example and reinforce the measurement limits above. No
peak-memory or edit-latency measurement is claimed.

A diagnostic experiment on temporary in-memory source copies showed that removing
only catch-parameter `final` reduced ArrayUtils parser errors from 3,041 to 54,
ClassUtils from 444 to zero, and ObjectUtils from 429 to zero. The acceptance run
always uses unmodified source bytes. Catch-parameter parsing/recovery is a concrete
next priority; `switch` and multi-expression `for` headers are additional observed
gaps. No parser behavior was silently changed as part of the index work.


## Catch-parameter parsing follow-up

Baseline: `13271ceae6fa64e61b7745decf72aa9c1ef84338`. The old compiled analyzer
and pinned source/platform inputs were retained for comparison. The same source
archive SHA-256 and all 249 Java source byte strings were verified unchanged.
The fresh acceptance command still runs only the prebuilt Rust analyzer under
an empty environment with `PATH=/nonexistent`; no Java compiler or process runs.

Exact original-corpus comparison:

- 249 files, 3,676,819 source bytes, all lossless before and after.
- Files with parser errors: 63 before, 28 after; files without reported errors:
  186 before, 221 after. Neither number is a conformance score.
- Parser error nodes: 8,187 before, 149 after. These count recovery cascades.
- Per-file counts: 44 improved, 205 unchanged, none increased.
- ArrayUtils: 3,041 → 54; ClassUtils: 444 → 0; ObjectUtils: 429 → 0.
- [Full per-file comparison](../examples/real_source/catch-parser-comparison.tsv)
  includes zero-count files, not just the improved examples.

The original fixed query positions remain unchanged. Twelve query contracts stay
the same. `ArrayUtils.isEmpty` at StringUtils byte 44,758 changes from Unresolved
to OverloadNotResolved with nine source candidates: Object[], boolean[], byte[],
char[], double[], float[], int[], long[], and short[]. Each exact declaration range
was checked against the original source; no candidate is guessed. The harness now
stores each ordered candidate separately in `targets.tsv`, rather than assuming
all candidates share one range. All thirteen queries still compare prepared and
rebuild results, including unsupported overload/receiver cases.

Five initial focused tests failed before implementation. Eight catch regressions
now cover syntax, malformed/incomplete headers, parameter scope, exact ranges,
UTF-16, ordinary formal parameters, and incremental/full equivalence. Independent
review tested 33 header forms and 165 incremental replacements. A nested generic
annotation path that bypassed bounded recovery was corrected. The documented
first-brace synchronization limit remains; see [catch boundaries](CATCH_PARAMETERS.md).

The final five-round release corpus run uses 65 samples per query mode. Its query
shape changed from zero to nine candidates for one case, so this slice makes no
before/after performance comparison or speed claim. Loading, index preparation,
and query timings remain observations emitted by the existing optional harness.

Final repository checks pass: **138 tests**, two intentionally ignored manual
measurements, one compile-fail doctest, formatting, strict Clippy/rustdoc,
examples and empty-environment native-runtime checks, and Cargo package verification.
No dependencies or JVM requirements were added. The real-source harness's final
inventory matches its updated baseline exactly.


## Basic-for lists and incremental-fragment follow-up

Baseline: `690eaa91751e75cd8cfc1a759dc5c7fc7bd9f4b0`. Retained baseline binaries
and a separately retained Rust library produced the original diagnostics/query
results; the source archive and all 249 extracted Java byte strings are unchanged.
No Java tool was used for classification, parsing, querying, or edit checks.

Every one of the remaining 149 baseline error nodes was inspected and grouped
against its source construct: for expression lists 70, switch 27, contextual
`var` identifier 23, synchronized statements 10, explicit invocation type
arguments 10, annotation declarations 5, and labels 4. Group sizes count parser
recovery diagnostics, not independent defects or language features.

All 70 for-list diagnostics disappear after the fix. The contextual-`var` group
changes from 23 to 29 because new missing-item recovery retains tokens that the
previous invalid-initializer path consumed. TypeUtils therefore increases from
26 to 32 diagnostics. Every other group's count is unchanged. The final corpus
has 85 diagnostics across 24 files: four files improve, one increases, 244 stay
unchanged, and all 249 retain lossless coverage. Exact full-file counts and every
diagnostic/range/group are versioned alongside the source acceptance harness.

The original thirteen query contracts remain unchanged. Two new real-source
queries independently reviewed against Java lexical scope point from byte 1261
(`j--` update) and byte 1376 (body argument) to the local `j` declaration at
1234..1235 in CharSequenceUtils. Both were Unresolved in the frozen baseline.
The optional harness now checks fifteen query positions. A mandatory six-case
lexical-binding golden test separately checks each resolver against manually
specified complete targets, distinguishing loop locals, fields, and enclosing
locals. It does not derive expected targets from another resolver.

Three initial for tests failed before implementation. A separate regression
proved an inherited incremental text-loss defect: parsing could stop before the
end of a balanced fragment and still splice it. A full-consumption length guard
now falls back through the existing larger-node/full-parse path. Independent
review passed 25 adversarial grammar forms, 4,212 insertion edits, 134 deletion
edits and UTF-16 emoji edits with ordinary/verified/full parse equivalence and
lossless coverage. The two focused text-loss reproductions are retained in tests.

Evidence boundaries: default CI does not run the external 249-file corpus. That
harness was run locally under an empty environment and no Java PATH. The fixed
lexical goldens and independently reviewed source positions are narrow evidence;
prepared/rebuild equality alone does not prove Java correctness. A hash-verified
real-corpus CI job and optional compiler differential remain planned, as detailed
in [ACCEPTANCE_PLAN.md](ACCEPTANCE_PLAN.md). No broader semantic or performance
claim follows from this parser slice.

Final checks pass: **148 tests**, two intentionally ignored manual measurements,
one compile-fail doctest, strict formatting/lint/docs, examples, native-runtime
checks, and Cargo package verification. The local real-source run has fifteen
queries repeated five times (75 samples per query mode), and its diagnostic
inventory matches the updated baseline. No dependency or Java-runtime requirement
was added.

## Fixed-input extended CI and independent javac follow-up

Baseline: `c05f6586f3260a8b6d05b8803e1c3f82ac4314ce`. This slice adds a separate
trusted-main/manual acceptance workflow; it does not add a JDK requirement to the
Rust library or default native tests. See [execution boundaries](ACCEPTANCE_PLAN.md).

Local frozen checks pass: **150 Rust tests**, two intentionally ignored manual
measurements, one compile-fail doctest, strict formatting/Clippy/rustdoc, examples,
empty-environment native checks and verified Cargo packaging. Twelve offline
Python fetch-security tests pass. No Rust dependencies changed.

External inputs retain the previously recorded source/JDK/JMOD hashes. A fresh,
empty-cache network run downloaded the exact official 728,980-byte Commons Lang
sources artifact and 207,858,352-byte Temurin archive, verified their hashes, and
produced only JMOD/legal data. A separately verified compiler extraction was used
for the optional oracle. Offline cache hits were reverified; missing/corrupt
cache entries failed closed. Tests and independent probes covered path traversal,
symlinks/ancestors, duplicate normalized names, special entries, compression/count
budgets, corrupt CRC/gzip, download size limits, HTTPS policy and partial cleanup.
No external archives/JDK are included in the repository or source deliverable.

The original 249-file real-source run remains unchanged at 85 parser diagnostics
across 24 files. All fifteen pinned queries pass, with lossless source coverage.
This Rust child runs with no Java environment. The comparator is a separate,
explicit development step and is the only added path that executes Java.

The handwritten three-file oracle uses pinned javac 21.0.13+9/release 21 to select
actual declaration elements, then obtains declaration source ranges independently.
Eleven supported native bindings agree, including cross-file types/inheritance,
fields, parameters, local scope and a uniquely named source method. Compiler
UTF-16 positions are converted to UTF-8; an astral character precedes query sites.
All public target fields are checked using documented owner conventions.

Two additional cases remain explicit native refusals, not semantic-equivalence
passes: an overload chosen by javac and a super-qualified field. The latter first
exposed a real bug: native lookup dropped the super receiver and resolved to a
shadowing subclass field. Failing Rust regressions reproduced it in legacy and
workspace APIs. The conservative syntax-receiver guard now refuses super fields,
calls, method references and receiver chains instead of returning that wrong
symbol; this/unqualified lookup and ordinary scope behavior remain covered. Full
super semantics are still not implemented.

Negative controls reject a wrong golden target, a corrupted compiler declaration
range, missing or malformed output, native wrong status/target, and optimized
Python. Missing-name/incomplete Java files must produce compiler errors, and the
native missing-name probe stays unresolved. Independent review additionally
altered seven pieces of evidence and exercised a separate nine-query super/this
fixture. Annotation processing and implicit source compilation are disabled;
fixture programs are analyzed, never generated or executed.

The workflow uses read-only permissions, nonpersistent checkout credentials and
verified official action commits (checkout `11d5960a326750d5838078e36cf38b85af677262`,
rust-toolchain `b725eda525362e7cf34af9a7243cac1e3f01a7ec` with Rust 1.96.0). It excludes
PR code, limits runtime, and uploads no upstream artifacts. These pins do not
constitute a hermetic runner/dependency graph. Default CI, extended CI, local
corpus checks and semantic coverage must always be reported separately.

## Multiple declarators and receiver-type boundary follow-up

Baseline: `024fe573d973c771d9d6d17e82c5eff9ae6089a5`. Focused failing tests reproduced
missing second/third field names and recovery errors for postfix array dimensions.
Multiple ordinary local names already resolved correctly; that behavior is
preserved rather than claimed as newly implemented.

Field and local declarations now share a declarator walk that retains every
name's exact range and independent suffix rank, without descending into initializer
names/commas. Shared type dimensions and per-name dimensions are kept separately.
Native internal tests verify exact ranks and exclude generic-argument brackets.
Named array receivers now refuse scalar member lookup; array element-expression
typing and `length`/`clone` remain unimplemented. See [boundaries](DECLARATORS.md).

Final local checks pass: **165 Rust tests**, two intentionally ignored manual
measurements, one compile-fail doctest, formatting, all-target Clippy, strict docs,
all contributor examples, no-Java-environment real JAR/JMOD checks, twelve offline
fetch-security tests and verified Cargo packaging. Fourteen new integration tests
cover declaration targets, nested initializer commas, local scopes, Unicode,
imports/provenance/access, malformed buffers, edits and workspace/prepared APIs.
One new internal test verifies actual array ranks, not merely receiver refusal.
Independent review checked lossless token text and insertion/deletion/replacement
at every byte boundary of eight valid/malformed cases against full reparsing.

The independent compiler fixture grew from three to seven handwritten files.
Before the fix, javac accepted the complete fixture and selected all forty query
positions, but eleven supported native probes failed. After the fix, **37 supported
bindings** match the independently selected declarations and **three unsupported
boundaries** remain explicit: overload choice, super receiver and indexed-array
receiver. The last is `Unresolved` with no target, not semantic-equivalence success.
The unchanged compiler helper still supplies actual element name/kind/owner/range.
Wrong-sibling and same-name/wrong-package perturbations fail despite overlapping
javac ranges for comma declarations. Existing invalid/incomplete compiler inputs,
corrupted evidence and optimized-Python controls also pass. This oracle checks
array variable identity, not independent array-rank/type equivalence.

The original Commons Lang source bytes remain unchanged. A syntax-tree inventory
finds six multi-local declarations and no multi-field declarations in the 249-file
corpus. Six new hand-anchored queries in StringUtils, StrBuilder and Fraction passed
on the baseline executable and still pass after the fix; they protect existing
behavior, not newly successful upstream navigation. The complete corpus retains
**85 diagnostics in 24 files**, exact lossless source coverage and all **21 query
baselines**. The release run uses five rounds, 105 samples per mode, with no timing
threshold or general performance claim. No production dependencies changed and
only the explicit development comparator executes Java.

## Method-invocation namespace follow-up

Baseline: `c8e47a99f1609940865b5619ddac2c9370916db4`. Failing native regressions and
an actual pinned-javac run showed an unqualified method invocation incorrectly
selecting a same-spelled parameter/resource/local. The production fix only moves
the existing call-specific method lookup before lexical value lookup; syntax and
call-position propagation are unchanged. Arguments and receiver prefixes retain
value lookup. See [invocation boundaries](CALL_NAMESPACES.md).

The complete fixed Commons Lang corpus has 3,030 direct unqualified invocation
names. Twenty formerly returned local/parameter targets. With the pinned platform
data, all twenty now return only method targets: **3 uniquely resolved**, **9
OverloadNotResolved**, and **8 SignatureNotCompared**. The last two are refusals,
not successful call selection. ObjectUtils.clone retains source/Object candidates;
seven StrBuilder.length sites retain source/CharSequence candidates. This exposes
source/binary method identity as the next project-navigation dependency.

Twenty new query baselines record every exact method candidate. Original source
signatures/name ranges were checked separately, including excluding the nested
FailableStream.stream from outer Streams candidates; standard-library candidates
retain JMOD provenance. The full-call negative invariant also asserts its fixed
3,030-site inventory to avoid a vacuous pass. All **41 exact queries** pass over
five release rounds (205 samples per mode), with no timing guarantee. The 249
unmodified files retain lossless bytes and **85 parser diagnostics in 24 files**.

The independent handwritten oracle has nine valid Java files and 63 queries:
**59 supported declaration bindings** agree with javac, while **4 unsupported
boundaries** stay explicit. A preserved baseline binary first failed at a parameter
named m capturing m(). Seven deliberate wrong-target checks, corrupted ranges,
optimized-Python rejection and three invalid compiler fixtures pass. Five invalid
no-callable names independently return no native target; their compiler failures
are not counted as binding matches. Production and default tests remain JVM-free.

Final local checks pass: **173 Rust tests**, two intentionally ignored manual
measurements, one compile-fail doctest, formatting, strict Clippy/docs, examples,
real JAR/JMOD checks without Java on PATH, twelve offline fetch-security tests and
Cargo package verification. Eight new native regressions cover lexical/value
collisions, source/binary methods, refusal boundaries and prepared/workspace
visibility. Independent review checked nested call/index positions, all twenty
source queries and their 62 candidate records. No new dependencies were added.
This is navigation soundness work, not overload selection, method-reference
conformance, semantic diagnostics or completion/refactoring readiness.

## Contextual source/binary method identity follow-up

Baseline: `10a03e7aec9a72e8b4148aaa5fb1ab37810355f5`. Nine initial native regressions
failed before implementation, covering mixed signatures, source-import conflation,
return descriptors and unrelated inheritance branches. The mixed compiler suite
also failed before the fix: all nine valid consumer queries reported
SignatureNotCompared despite javac selecting their source declarations.

Supported parameter identities now distinguish primitive tags, array rank and
actual declared classes in the owner's compilation context. Full descriptor syntax
and return components are consumed before returns are excluded from Java signature
identity. Hiding follows declaring-type ancestry, not traversal distance. Uncertain
generic/member-type/missing/ambiguous comparisons refuse equality. A unique valid
name still permits navigation without claiming invocation applicability. See
[model and limitations](METHOD_SIGNATURES.md).

Independent adversarial review found and fixed four further soundness holes:
primitive-looking binary class names losing their declared-type tag; synthetic
interface/Object edges falsely justifying overriding; inherited member types
falling through to unrelated package types; and generic zero-argument comparisons.
Both source and binary interface paths are covered. The class reader now exposes
`ClassSignature.is_interface`; external struct literals must supply that field.
Other resolve API/status/target fields are unchanged. No dependency was added.

Local final checks pass: **199 Rust tests**, two intentionally ignored manual
measurements, one compile-fail doctest, formatting, strict lint/docs, contributor
examples, no-Java-PATH real JAR/JMOD checks, twelve offline fetch-security tests
and Cargo package verification. Twenty-five new integration tests plus one
strict-descriptor unit test cover the foundation, including contextual binary
identity across workspace linking scopes. Twenty-six independent native probes
pass. Neither suite needs a JVM.

The original compiler suite still passes **59 source declaration matches and four
refusal boundaries**. The separate mixed suite compiles four handwritten dependency
classes into a temporary directory, registers no dependency sources, and checks
consumer sources against those bytes. It passes **five source-over-binary override
matches and four refusals**. javac verifies @Override; exact native candidate
provenance and omitted-binary/wrong-target controls prevent a source-only accidental
pass. No fixture class is executed, generated bytes are not committed, and no
compiler-selected binary-target/descriptor-equivalence proof is claimed.

The unchanged Commons Lang corpus retains **249 lossless source files, 85 parser
diagnostics in 24 files, 3,030 audited direct calls with zero wrong-namespace
targets, and 41 exact queries**. `signature-comparison.tsv` records all ten changed
status/count rows: seven StrBuilder.length sites now select the exact source
override; ObjectUtils.clone retains two provably different-arity candidates;
generic Objects.requireNonNull and Streams.stream become SignatureNotCompared
with unchanged candidates. Every unchanged candidate multiset was verified;
changed sets only remove the justified binary CharSequence.length ancestor.
Candidate ordering now follows deterministic declaration traversal rather than
spelling sort. These are navigation results, not argument-based overload selection.

Normalization and ancestor checks currently execute during lookup. One local
release run measured prepared-query p50/p95 **8.68/21.03 ms** versus **3.02/13.60 ms**
in the prior invocation-slice run on the same 41-query workload. The new full
acceptance run took 72.9 seconds, including 3,030 audit sites and 205 samples per
query mode. This is observed additional work, not a controlled benchmark or SLA;
no speedup is claimed. Repeated measurements, lookup reuse/caching, cancellation
and project-scale memory/latency gates remain needed before an interactive release.

## Experimental manifest/CLI vertical entry

Baseline: `68a293294135ec6ab5fbe79a0556c47e5daf2d10`. The optional `cli` feature
adds a native executable and strict versioned JSON adapter without changing core
analysis APIs or semantics. The first test build failed because the requested
executable entry point was absent; this was missing-interface compile evidence,
not a behavioral assertion against the old engine. Implementation then added
configuration, prepared declaration batches and parser reports. Manifest/request
paths, compilation ownership and explicit source/binary result boundaries are
part of the public experimental contract, not a promise of full go-to-definition.

Final local checks pass: 199 default Rust tests, 216 with the CLI feature (the
same core tests plus 17 subprocess tests), one compile-fail doctest, formatting,
strict all-feature Clippy/rustdoc, contributor examples, native source/class/JMOD
runtime checks, 12 fetch-security tests and feature-enabled Cargo package
verification. The optional comparator remains at 59 source matches/four refusals
and five mixed source/binary matches/four refusals. Core APIs and analysis source
are unchanged. Six new locked CLI-only dependencies have eleven unchanged
upstream license files; the default feature set does not enable serialization.

The shipped CLI is checked against the unchanged fixed Commons Lang inputs:
249 source files, 3,676,819 bytes and 7,615 platform classes. All 41 fixed query
statuses and 86 ordered candidates match, including 76 source and 10 binary
candidate records. Binary results have no fabricated source range. Parser reports
retain the 85 known gaps in 24 files, with exact UTF-8/UTF-16 ranges and an explicit
non-semantic classification. Without platform data, a source target remains
unchanged while Character becomes Unresolved with no candidates. Wrong/omitted
output controls verify the comparator does not accept incomplete results.

The CLI corpus harness uses a separate working directory, manifest-relative
inputs, and only PATH=/nonexistent in the child environment. It downloads nothing
and invokes no Java. Its subprocess timeout/output limits are harness safeguards,
not production parser CPU/stack containment. A one-run debug validation observed
approximately 3.2 seconds for inspection, 3.5 seconds for parser reporting and
4.5 seconds for the 41-query declaration process, each including a fresh load.
These observations are neither interactive latency budgets nor release benchmarks.

An independent review passed 88 probes covering malformed configuration, numeric/document/query
limits, visibility and declaration ownership, package-only targets, UTF-8/UTF-16
boundaries, symlink query aliases, non-UTF-8 OS arguments and broken output pipes.
Dynamic linkage showed libc/libgcc/loader and no JVM dependency. Syscall tracing
was not permitted in this environment, so no syscall-level isolation is claimed.
A capped nested-block probe completed but does not establish parser depth safety.
Two real platform inputs across main/test contexts report 15,230 total classes
in default inspection. Detailed inventory requires explicit `--details` and
returns an atomic limit error above the output budget rather than truncating.

This is a usable experimental command surface for trusted local inputs, not the
completed preview gate. Process supervision/cancellation, automatic reload,
unsaved overlays, LSP positions, full semantic diagnostics, persistent IDs,
overload selection and cross-platform binary distribution remain unfinished.

## Native CLI worker execution boundary

Baseline: `f3baf21f9005f64910bb90f2f4797d8263530ae6`. The public executable now
supervises a worker instance of the same native executable. Default worker
execution/capture deadline is 30 seconds, configurable from 1 to 300,000 ms.
Optional stdin-EOF cancellation and a worker lease handle cancellation and
supervisor disappearance. Core analysis APIs/semantics, the lockfile and all
dependency versions are unchanged. There is no Java or arbitrary command path.

Final checks pass: 199 default Rust tests, 229 with the CLI feature (199 core,
21 CLI integration and nine supervision tests), one doctest, strict formatting,
Clippy/rustdoc, examples, no-Java-environment source/class/JMOD checks and
feature-enabled package verification. The real CLI workload still matches all
249 source files, 7,615 platform classes, 41 queries/86 candidates and 85 parser
reports. Twelve fetch-security tests and both optional compiler suites retain
their previous passing results. Linux crash tests run with core dumps disabled.

Real-binary probes contain a 100,000-level parser stack SIGABRT and report the
worker reaped, turn short deadlines into `execution_timeout`, and turn opt-in
stdin EOF into `execution_cancelled`. Test-only fixtures cover abnormal exits,
concurrent bounded stream draining, inherited pipes, malformed/contradictory
protocol responses and bounded cleanup. Fixtures are not shipped as production
backdoors. Independent review compared eight old response payloads (excluding
declared capability/help additions), sent SIGTERM/SIGABRT to actual workers,
and killed the supervisor with SIGKILL while observing worker termination and
performing scratch-process reaping. Opaque IDs/paths equal to control-looking
spellings are preserved after fixing an initially overbroad reserved-name check.

The independent review also verified an important exclusion: a caller that does
not read final stdout can block delivery after worker execution has finished.
Closing that pipe reports `output_io`, but an OS-written prefix cannot be
retracted. The deadline does not cover final caller writes, protocol validation,
all thread/setup work or hard OS scheduling limits. It is not a memory/CPU quota,
security sandbox or arbitrary descendant-tree supervisor. Linux evidence is not
cross-platform certification. See [the exact contract](CLI_EXECUTION.md); editor
callers still need asynchronous stream handling and their own outer watchdog.

## Bounded primitive source calls

Baseline: `1dfa0576ac4c17e7b31bcf450da0ea7deed42eba`. The new call proof covers
explicit primitive locals/parameters and array elements in a closed same-owner
static source context. It uses strict identity/widening and a unique most-specific
formal vector. A separate negative guard refuses known unique-source arity and
primitive contradictions without requiring platform data. `CallNotResolved`
retains candidates rather than reporting Java errors. See
[eligibility, API compatibility and exclusions](PRIMITIVE_CALLS.md).

Before implementation, the native regression set recorded six failures and three
passes against the unchanged baseline. Independently authored native/compiler
comparisons also failed on the old executable, including four real source calls
and a unique source `m(1)` incorrectly selecting `m()`. These RED observations
were preserved before corrections; expected selections were not inferred from
new analyzer output.

Final checks pass: 216 default Rust tests, 247 all-feature tests (216 core, 22 CLI
integration and nine supervision), one doctest, strict fmt/Clippy/rustdoc,
examples, native/CLI/JMOD no-Java-environment checks, feature-enabled package
verification and twelve fetch-security tests. Seventeen new core tests cover the
bounded proof and refusal cases. A CLI integration test checks three singleton
contradictions without platform data, retaining exact candidates with status
CallNotResolved and a successful query exit code.

Independent review checked 74 exact query results, including prepared/rebuilt
parity and complete refusal candidate multisets. Five discovered proof leaks were
reproduced and corrected: outer receiver arguments overwriting inner arguments,
resource declarations typed as ordinary locals, constructors treated as methods,
annotation expressions borrowing body parameters, and enhanced-for variables
visible too early in iterable expressions. Valid receiver/enhanced-for cases were
also independently compiler-checked. This is not a full Java scope/control-flow
or conformance proof.

The optional compiler gate preserves the original 59 matches/four refusals and
mixed source/binary five matches/four refusals. The new suite has twelve source
matches, twenty conservative refusals and sixteen compiler-invalid exact-refusal
cases. Mutation, omitted-output and missing-Object controls pass. Optional javac
analysis of all 249 unchanged pinned sources confirms the four IEEE754rUtils
source method selections. No upstream application classes are generated or run.
Only this optional development gate executes Java; native execution still uses
PATH=/nonexistent with no inherited variables. This host has a JDK for the gate,
so these checks do not claim a physically JVM-free host or syscall isolation.

The complete real-source harness and shipped release CLI both match all 41
queries: 31 Resolved, seven OverloadNotResolved, two SignatureNotCompared and one
ReceiverNotBound. The four changed queries reduce 86 candidates to 66 (56 source,
ten binary); every other query and ordered candidate set is unchanged. The full
3,030-call negative namespace audit passes. Parser inventory stays at 85 errors
in 24 files; all 249 source files retain their original bytes.

One Linux cloud release run observed load/parse 466.93 ms, index preparation
61.40 ms, rebuild-per-query p50/p95 68.20/77.84 ms and prepared-query p50/p95
8.15/18.50 ms over 205 samples per mode. A separate shipped CLI run observed
581 ms inspection, 591 ms parser reporting and 896 ms for the 41-query batch,
each including fresh load. These noisy fixed-workload measurements are not an
improvement claim, IDE responsiveness budget or general project benchmark.
CLI SHA-256: `c62a978e43b23816efe3a69006893944f335771604a1b1e16784abf70ef0ff5d`.

No dependencies or production Java execution were added. General expression
and overload semantics, incremental reload, editor integration, semantic
diagnostics, references and refactoring remain on the project roadmap.

## Proven primitive call-result follow-up

Baseline: `35936339d4449ee2ef105e895c5343539538f28c`. A shared internal proof now
returns the selected source member before its explicit primitive scalar result
can type an outer invocation. It never borrows a return type from legacy
name-only Resolved. Return types remain outside overload identity/specificity.
Scope and work limits are documented in [CALL_RETURNS.md](CALL_RETURNS.md).

Pre-implementation RED was recorded independently at three levels: two native
exact-target regressions, twelve new handwritten compiler-selected targets, and
four real outer IEEE754rUtils calls. The preserved published release CLI also
failed the expanded real-project gate at q42 with OverloadNotResolved instead of
the compiler-proved selection. Its existing 41 query checks had passed first.

Final contributor checks pass: 233 default Rust tests, 265 all-feature tests
(233 core, 23 CLI integration and nine supervision), one doctest, strict fmt,
Clippy and rustdoc, examples, native source/JAR/JMOD and CLI runtime checks.
Two preexisting optional Cargo tests remain ignored; separate real-artifact
acceptance is run explicitly. Seventeen new native tests cover result selection,
refusals, source edits, old/new snapshots, owning compilation data and bounded
proof work. The actual CLI test changes an inner declaration's return type,
reloads from disk, checks the new exact outer target, and refuses without Object.
This is fresh one-shot loading, not a new persistent server or overlay API.

Independent review passed 59 exact scratch queries (fourteen compiler-matched
targets and forty-five full-identity refusals), plus two no-Object budget
regressions and independent snapshot/source-set checks. It found and reproduced
two issues before approval: malformed throws headers could provide result proof,
and exhausted negative guards could fall through to legacy success. Both now
conservatively refuse. Throws syntax checks do not prove Throwable inheritance,
checked exceptions or method-body correctness.

The final optional compiler gate passes the original 59 matches/four refusals,
mixed five/four, primitive fifteen/seventeen and return fourteen/fifteen valid-
source outcomes. The return suite includes twelve newly supported selections and
two existing controls. Sixteen original primitive-invalid and twelve new return-
invalid fixtures retain exact refusal candidates, with independent malformed-
throws RED preserved. Original primitive fixture bytes/goldens stay unchanged;
a reviewed overlay changes exactly three outcomes. Mutation/completeness and
missing-Object controls pass. Eight real source selections are checked against
javac analysis of all 249 unchanged sources; no upstream application is run.

The release corpus and shipped CLI preserve all original 41 queries and their
66 candidate records. Four additional outer calls resolve, giving 45 queries,
35 Resolved, seven OverloadNotResolved, two SignatureNotCompared and one
ReceiverNotBound, with seventy records (sixty source, ten binary). Thus none of
the original ten refusal gaps is claimed solved. All 249 files remain lossless,
the parser inventory remains 85 errors in 24 files, and the full 3,030-call
negative namespace audit passes.

Production and default native tests remain JVM-free; only the explicitly
optional compiler gate executes Java. No dependency version changes, external
project source copies or JDK binaries are included. Editor integration, reload/
overlays, distribution and broader semantic services remain separate gates.

One final Linux cloud release run recorded load/parse 493.92 ms, preparation
62.17 ms, rebuild p50/p95 69.58/81.21 ms and prepared p50/p95
11.12/18.31 ms over 225 samples per mode. The actual CLI recorded
551 ms inspection, 578 ms parser reporting and 976 ms for the 45-query batch,
including fresh loads. This expanded workload is not directly comparable to the
previous 41-query percentiles. No speedup, memory bound or preview latency budget
is claimed. CLI SHA-256: `784cd39cbf3629a7a462ad0dd505f7ff2f9535c70510ba9042bf69630ca19dcf`.

## Two-project delivery evaluation tooling

Baseline: `992b6edb3ad84463bbfbb9bb3351b3c2d2e3f987`. This slice changes preview
setup, acceptance, packaging and measurement tooling; it does not broaden the
Rust analyzer's Java semantics. The [evaluation protocol](CLI_PREVIEW.md) fixes
samples, correctness rules and resource-measurement semantics before final runs.

The second sample uses Gson 2.10.1 at a fixed upstream commit, with all 211
selected original Java files across core/extras main/test contexts. Six exact
source/dependency/platform artifact hashes and twenty retained notice files were
checked independently. Setup uses pinned HTTPS/cache inputs and copies supplied
JMOD data; no upstream script, build, test or Java application is executed.
The source graph is manually configured, not Maven/Gradle or JPMS import.
Generated GsonBuildConfig is not invented. Excluded roots and module-info
limitations remain disclosed.

All sixteen predeclared query sites are retained: eight source/local/field/method
targets and three binary types resolve; five outcomes remain unsupported or
unresolved. Exact candidate ranges and declarations were inspected in original
source bytes. Fourteen visibility, missing-edge/dependency, load-error, source
edit/rename/recovery and deliberate wrong-output controls pass. Parser reporting
retains 1,513 nodes in 58 files: core/main 161 in 24 files, core/test 1,348 in 30,
extras/main four in four and extras/test zero. These can cascade and are not
1,513 independent Java defects. No full-project compiler equivalence is claimed.

The documented source install, installed-binary smoke/uninstall, dirty developer
archive build and detached archive checker were exercised. Packaging builds this
repository with pinned Rust and a locked dependency graph; no project input code
is executed. It includes an indexed allowlist of docs/sample/license files,
source/build hashes, native ELF linkage and checksums. Dynamic Linux x86_64 GNU
compatibility is explicit, with an observed glibc symbol requirement through
2.39. Deterministic packaging does not certify bit-for-bit native rebuilds.
A final clean-commit archive and repeated resource results are separate delivery
evidence, not inferred from these developer packages or a green CI status.

Independent review identified and verified fixes for process-group cleanup after
reaping, partial output publication, unsafe report writes, private/untracked
package leftovers, changing-input provenance, redirect scope and archive/path
handling. The final offline suite has ninety Python tests: twenty-five package,
twenty-seven second-input setup, eight second-project validator, eighteen
measurement/cleanup and twelve existing input-security tests. The unchanged core
also passes 233 default Rust tests, 265 all-feature tests, one doctest, formatting,
Clippy, rustdoc, examples and source/JAR/JMOD/CLI no-Java-environment checks. The
existing forty-five Commons queries and all candidate records remain unchanged.

The measurement harness validates each result, includes the native worker in
resource observation, records unavailability/races, holds the leader identity
until group cleanup and writes output atomically without replacing existing
files. It runs no builds/downloads, and serial measurements must follow setup and
review work. Its wait4 high-water and sampled summed-RSS meanings remain distinct.
Source-edit timing is a fresh full reload, not an incremental editor session.
No final performance result, OS memory quota or cross-platform readiness is
claimed merely because this tooling exists. Process tracing was not permitted;
source inspection, native linkage and empty-environment runs are separate evidence.

## Unsaved-buffer snapshot and editor-coordinate slice

Baseline: `88ac50319df78915ae022ba3e8b4cd0daaa6b2c3`. This adds a bounded
whole-buffer update path, not new Java semantics or a persistent language server.
New native CLI tests first failed against the baseline's unknown `--overlays`
option. The fixed Gson editor gate also failed with that exact baseline binary
before the implementation and passed with the new release executable.

Thirteen core tests cover exact Unicode/newline positions, source ownership,
independent original compilation/workspace limits, order-independent batch byte
accounting, immutable old prepared views and binary provenance. They include
removing all staged source/JAR/JMOD inputs after loading and still deriving and
querying replacements without filesystem I/O. Fourteen CLI editor tests cover
full-text buffers, cross-source navigation/isolation, exact count/document/label
limits, malformed/unknown/duplicate fields, original input budgets, error envelopes,
timeout/EOF cancellation and old absolute-offset behavior. Two additional
supervisor tests cover option-value forwarding and overlay error validation.

The aggregate local checks pass 246 default Rust tests, 294 all-feature Rust tests
and two doctests, with the two pre-existing optional corpus tests still ignored.
The offline Python suite passes 103 tests: the previous ninety plus ten macOS
checker regressions and three editor-checker controls. Formatting, strict Clippy
and rustdoc also pass. These counts are Linux local evidence, not a claim that
macOS has run or that unsupported language constructs are correct.

Independent review additionally checked 55,987 mixed Unicode/newline strings
against a separate position model, 2,500 chained overlay batches against separate
byte-budget arithmetic, and five complete legacy CLI responses against the
baseline binary (excluding invocation identity). This is targeted adversarial
model evidence, not a formal proof or a compiler semantic oracle.

The fixed Gson editor gate overlays its predeclared extras-to-core interface
query and declaration files with an astral-character/CRLF comment prefix. It
independently shifts the original reviewed target's byte and absolute-UTF-16
ranges, checks the owner and caller labels, retains all 16 baseline disk queries,
and verifies every source path/hash and pinned artifact hash is unchanged.
Wrong-target mutation, wrong source owner and omitted-overlay disk restoration are
explicit controls. The comment-only replacements retain the counts of 1,513 parser reports
across 58 files; this does not reduce the known parser gaps. The analysis performs
no Java execution, fetch or upstream build. It runs in the optional extended CI
beside, not instead of, the existing independent development-only javac oracle.

The separate `macos-native` workflow defines two native hosted build/runtime
gates and first checks runner architecture and Rust target, then actual Mach-O
architecture/linkage and release CLI behavior. Offline checker tests reject wrong
architectures, non-executable/fat/ELF files, unknown/JVM/dyld/rpath dependencies and
invalid host/toolchain facts before executing a candidate. A configured job or
these mocked negative cases do not prove macOS compatibility: consult successful
jobs for the exact source revision. No user Mac was accessed, no Mac package was
uploaded or signed, and Linux-specific packaging/resource measurement remains
separate. See [the explicit platform boundary](MACOS_NATIVE.md).
