# Binary payload reuse within one preparation

## Fixed scope and acceptance contract

This optimization removes repeated conversion of the **same immutable binary
signature allocation** into binder records during one workspace preparation.
It does not change Java resolution, public targets, archive loading, source
ownership, compilation visibility, CLI supervision, or the one-shot LSP lifecycle.
There is no persistent cache, incremental analysis, stable symbol ID, result
cache, new applicability rule, or broader OpenIDEA/GUI release claim.

The motivating release measurement at `da509b660510` used the pinned Gson
core/extras main/test workload: 211 sources in four compilation scopes, with
explicit platform inputs. A native public-API phase probe observed median load
509.65 ms, overlay 8.87 ms, preparation 259.42 ms and warm query 1.23 ms;
source-only preparation was 25.77 ms. These are single-environment observations,
not performance budgets. The difference includes non-shareable work and is not
a predicted speedup. End-to-end LSP costs still include loading, processes and
protocol handling. Existing [archive reuse](ARCHIVE_REUSE.md) addresses a separate
stage before binder preparation.

Acceptance requires all of the following, fixed before candidate timing:

- Direct construction and ownership evidence, not merely equal query answers.
- Unchanged exact results and refusal boundaries, including caller and declaration
  contexts, signature type identity, superclass versions, provenance and order.
- The full native contributor checks and independent production-diff review.
- Serial paired baseline/candidate measurements on identical pinned inputs and
  query targets, with no simultaneous build or benchmark. Measure complete LSP
  requests as well as preparation phases and resource use. Retain failures and
  report sample counts, methodology and observed ranges. No new CI timing cutoff
  or claim of an editor-wide latency budget follows from these measurements.

## Representation and lifetime

Each `ClassRec` remains a separate contextual instance with its own compilation
scope. Source payloads remain uniquely owned. Binary payloads contain only the
existing pure `class_from_signature` conversion: names, origin, hierarchy
spellings, member descriptors, raw signature ownership and syntax-level flags.
Their data is immutable and shared by `Arc` within an index.

`Index::scoped` creates a local map keyed by `Arc::as_ptr` for the input
`ClassSignature`. Every map value retains that exact signature through a strong
`Arc`, so its key cannot be recycled during the map's lifetime. Equal origin,
name, bytes or public targets never authorize a cache hit. There is no structural
hash, disk lookup or cache shared with another preparation. The map is dropped
when construction returns; its size is bounded by the distinct binary signature
allocations in those inputs. Derived payloads live only as long as the index's
contextual records. Preparing again constructs new payloads, including after a
source-only overlay that retains the same input signatures.

Resolved ancestry, parameter type identities, `ClassIdentity`, access decisions,
method applicability and query work budgets are **not** in the payload. They are
computed with the original scope and caller. The original `binary_signature`
Arc is retained for bounded literal-identity checks. Descriptor validation still
covers all signature methods, including constructors; navigation still excludes
constructors and non-method entries with the original order/filtering.

The flat-project path has no duplicate signature allocation to reuse. It builds
one binary payload per existing input and otherwise retains its previous behavior.
Source records acquire a private ownership indirection, but they are not deduplicated
or shared between independently indexed scopes or snapshots.

## Invariant tests

`src/bind/binary_payload_tests.rs` covers:

- Four contextual records produce one payload and two member records instead of
  four payloads and eight members. The fail-first test failed on the original
  construction count (`4 != 1`) before the implementation changed.
- Payload/member pointer equality proves actual sharing; contextual identities
  remain different. Strong/weak counts show the construction map has gone away,
  a second preparation allocates a different payload, and dropping each index
  releases its payload independently.
- Distinct signature allocations remain distinct even with identical bytes,
  names and origins. Different origins preserve ambiguity and target order.
- Shared signatures retain per-scope parents and reference parameter identities,
  absent parameter types, overload refusals, package/protected/private access and
  dependent-source declaration context.
- Malformed descriptors, generic refusals, constructor inventory validation,
  member filtering and original signature identity remain unchanged.
- Changed/replaced/removed binary inputs and source edits leave old prepared
  results intact; simultaneous old/new prepared queries remain independent.
- Sources remain independently owned, including when source records are cloned.

A test-only uncached builder reconstructs every binary occurrence using the old
construction strategy. Tests compare every byte offset, missing paths and extreme
positions against this builder, prepared queries and the rebuilding entry point.
This is an optimization differential, not an independent Java semantics oracle.

`tests/workspace.rs` also uses native, hand-assembled class/JAR/JMOD fixtures. One
shared JAR links against different platform versions and preserves a dependency
source's original owner. Replacing that JAR at the same path does not affect an
old prepared snapshot. Another shared JAR resolves one descriptor type in scopes
with separate supplied versions and refuses comparison in the scope missing it.
These fixtures assert shared input signature pointers rather than inferring reuse
from matching path strings. Existing workspace precedence, budgets, overlays,
prepared Send/Sync and real-source refusal gates remain applicable.

Focused reproduction, without a JVM or downloads:

```sh
cargo test --offline --locked --lib binary_payload_tests
cargo test --offline --locked --test workspace shared_jar
cargo test --offline --locked --test prepared
```

Run the complete checks in [CONTRIBUTING.md](../CONTRIBUTING.md) before acceptance.

## Initial candidate native verification, 2026-11-10

Local Linux checks passed 324 default Rust tests, 430 all-feature Rust tests,
two doctests and 333 offline Python tests. Two existing manual measurement tests
remain ignored in each Rust suite. Formatting, strict Clippy/rustdoc, examples,
native CLI runtime checks and supplied-platform/Commons binary checks passed.

The unchanged Commons source gate retained all 249 sources, 85 parser reports,
the 3,030-call namespace audit and 45 fixed query expectations. The Gson
constructor, string and strict-reference gates retained all 211 selected sources,
226 source hashes, 331 parser reports in 58 files, exact targets and refusal
controls. The Gson wire-level LSP lifecycle also passed.

The separately checksum-verified Neovim 0.11.5 headless client passed all 13
definition checks and 28 explicit-refresh scenarios against the release
candidate. These are native Linux/headless checks, not Mac or OpenIDEA GUI
acceptance. No JVM or upstream project code was executed, and no download or system
installation was performed. The development-only compiler oracle is separate and was not rerun for
this nonsemantic optimization.

## Single-compilation bypass addendum

This adjustment is fixed before measuring the second candidate. The first
candidate's paired full-Gson LSP medians improved from 1043.80 to 835.13 ms for
unchanged navigation and from 1063.60 to 820.88 ms for target edits. However,
the Commons single-compilation control regressed from 678.95 to 702.40 ms and
from 694.80 to 729.64 ms respectively (about 3.5% and 5%). Both were slower in
six of seven pairs. These adverse results
are retained, not dismissed or excluded from acceptance. The full-Gson phase
probe's preparation median improved from 267.79 to 103.34 ms, while its
source-only preparation median increased from 23.99 to 26.18 ms. Source-only
end-to-end Gson navigation was roughly unchanged. An empty binary cache cannot
explain that source-only preparation change.

There is a structural reason to remove map work for exactly one compilation:
`Project.binaries` is keyed by each immutable signature's own origin, so the same
signature allocation cannot occur twice within one project. Such a preparation
cannot get a reuse hit. The second candidate constructs each binary payload
directly in this case, avoiding map lookups, temporary retention and extra Arc
bookkeeping. It retains the existing scoped source construction, visibility,
record order and index-build accounting; it does not delegate to `Index::build`.
Empty and multiple-compilation behavior stay unchanged.

Test-only counters establish that two single-scope signatures perform zero reuse
lookups and retain zero map entries. That test failed with two lookups before the
bypass. The existing four-scope test still requires four lookups, one retained
entry, one payload and four contextual records. Empty-index and every-offset
uncached differential checks remain required.

The adjustment deliberately did not redesign source ownership or binary
allocation. The complete native checks and fixed performance/control/diversity
matrix were repeated, retaining the baseline and both candidate artifacts. The
completed second-candidate results below still show single-scope costs. The two
candidate windows are separate comparisons against a freshly remeasured baseline,
not a controlled direct estimate of how much latency the bypass recovered.

## Second candidate native verification, 2026-11-10

After the single-compilation bypass, all nine focused payload tests passed,
followed by 326 default Rust tests, 432 all-feature Rust tests, two doctests and
333 offline Python tests. The two existing manual measurements remain ignored
in each Rust suite. Strict Clippy/rustdoc, formatting, examples and runtime checks
passed again. The unchanged Commons, Gson constructor/string/strict-reference,
Gson LSP and real Neovim suites were repeated against the revised code: all
preservation checks, 13 definition checks and 28 refresh scenarios passed.
The freshly built release Commons example also retained the full 3,030-call
audit and 45 query expectations. These correctness runs are not the paired
second-candidate performance measurement.

## Final second-candidate measurements, 2026-11-10

The [public numeric appendix](measurements/binary-payload-reuse-2026-11-10.json)
contains all 192 ordered LSP observations, including excluded warmups, every
paired delta, min/median/max, resource coverage and phase samples. It contains
measurement data and portable workload identities, not machine-local paths.

Both binaries were Rust 1.96.0 Linux x86_64 release builds, measured serially on
Debian 13 with nine visible CPUs (AMD EPYC 9V74). The baseline was the preserved
source-equivalent `da509b660510` artifact; the final candidate was measured before
commit. Binary SHA-256 values are:

- Baseline: `7d1fc3d7bf0aef3189d3e15b5488a07f2330dc925236249063467af82f3e813a`.
- Final candidate: `344b109e57c2675e604c7171131c70bce6168c1c7f07e2a98ea27c7f9740f236`.
- Final candidate compact Rust/build digest: `dd76699e8ef536d6b977a6612656b33cb2d6e26a1a172e1d4cfd94a8fad97fe8`.

Receipts and before/after checks identify the compared bytes; they are not
independent historical build attestations. No build or other benchmark ran
concurrently. The complete fixed matrix ran once, with no selected extra runs.
Each workload/variant has one excluded warmup and seven measured observations.
Baseline/candidate order reverses each round. Initialization was recorded
separately, once per session, and is not a startup benchmark.

### Workload and timing boundary

The full models use the pinned Commons Lang 3.17.0 published sources (249 files,
one scope) and Gson 2.10.1 core/extras main/test selection (211 selected files,
226 source hashes, four scopes). Target release is 21. Both use supplied Temurin
21.0.13+9 `java.base.jmod`; Gson also uses `java.sql.jmod` and its configured
JUnit/Hamcrest/JSR250 inputs. Pins and provenance are in
[`scripts/acceptance-inputs.json`](../scripts/acceptance-inputs.json) and
[`scripts/second-project-inputs.json`](../scripts/second-project-inputs.json).
These are explicit partial project models, not build imports or JPMS closure.

The main Gson query is `cross_interface`, at byte 1082 in
`PostConstructAdapterFactory.java`, selecting `TypeAdapterFactory` at bytes
6402..6420. Commons uses `q01`, `StringUtils.java` byte 309172, selecting
`ArrayUtils` at bytes 2159..2169. Complete relative source paths and expected
positions are in the appendix. Source inventories, queries and buffer sets are
identical across compared variants.

Unchanged definitions and full-target-buffer edits are separate workloads.
The edit adds `// LSP reload measurement {iteration} 😀\r\n` before the original
target text; exact UTF-16 response ranges must reflect that prefix. Timing starts
at local request/notification preparation and ends at the complete correlated
LSP response. It includes serialization, IO and concurrent sampler overhead;
exact-result assertions and cleanup are outside successful latency. No GUI,
Host admission, caret movement or rendering is timed.

### Full-context latency and remaining cost

Milliseconds, minimum / median / maximum. The paired delta is candidate minus
baseline for corresponding rounds: positive means slower. A ratio of separately
computed medians can disagree with the median of paired differences.

| Workload | Baseline | Final candidate | Paired median delta | Slower pairs |
| --- | ---: | ---: | ---: | ---: |
| Gson unchanged | 995.06 / 1132.02 / 1312.87 | 780.51 / 881.49 / 961.09 | -238.90 | 0/7 |
| Gson target edit | 1003.90 / 1124.96 / 1248.30 | 779.28 / 872.42 / 981.99 | -246.06 | 0/7 |
| Commons unchanged | 670.65 / 737.15 / 768.66 | 708.41 / 727.72 / 811.07 | +9.87 | 5/7 |
| Commons target edit | 729.90 / 749.87 / 811.56 | 740.09 / 756.01 / 806.49 | +14.11 | 5/7 |

Gson's two median latencies decreased 22.1% and 22.4%, with all seven pairs faster
for each operation. **The single-compilation penalty was not eliminated.**
Commons unchanged has a lower marginal median (737.15 to 727.72 ms), but five of
seven pairs were slower and the paired median cost was +9.87 ms. Commons target
edits rose from 749.87 to 756.01 ms, also slower in five pairs, with paired median
+14.11 ms. Do not describe the former marginal median as a demonstrated Commons
speedup. A remaining single-scope cost is observed, but these runs do not isolate
its cause.

The result supports the narrow shared-binary multi-scope optimization, not a
universal latency improvement. Full-context edit medians remain about 872 ms for
Gson and 756 ms for Commons, so this does not complete low-latency editing. The
first candidate's window and regressions above remain separate; these runs do not
prove that the bypass caused a specific candidate-to-candidate latency recovery.

### Source-only and navigation-diversity controls

Source-only retains identical sources, type queries, overlays and edits but
removes every platform/classpath entry. It has different semantics and is not a
replacement for the full-context measurement. Results are mixed, not evidence of
a general source-only speedup.

Diversity uses full context and unchanged buffers. Gson's `unique_method_size`
is established unique-name source navigation, not general overload selection.
Its `overload_fromjson` control requires `SignatureNotCompared`, all 11 fixed
CLI candidates, and a successful null LSP result. Commons `q28` checks supported
primitive overload selection, and `q07` requires `OverloadNotResolved`, all nine
fixed candidates and successful null. An error response never counts as refusal.
The extra opened buffers are identical between variants (five for Gson, three
for Commons).

Milliseconds, medians; all ranges and samples are in the appendix.

| Mode / workload | Baseline | Final candidate | Paired median delta | Slower pairs |
| --- | ---: | ---: | ---: | ---: |
| sources / Commons unchanged | 187.24 | 188.17 | -0.36 | 3/7 |
| sources / Commons target edit | 193.93 | 195.56 | +1.13 | 4/7 |
| sources / Gson unchanged | 138.45 | 138.72 | -5.11 | 3/7 |
| sources / Gson target edit | 138.35 | 135.39 | +0.22 | 4/7 |
| diversity / Commons method navigation | 689.77 | 699.08 | -4.85 | 3/7 |
| diversity / Commons overload refusal | 678.98 | 700.12 | +9.55 | 5/7 |
| diversity / Gson method navigation | 1012.87 | 781.35 | -228.20 | 0/7 |
| diversity / Gson overload refusal | 1039.30 | 802.03 | -235.03 | 0/7 |

Gson's method and refusal controls were faster in all seven pairs. Commons
refusal had a +3.1% marginal median and was slower in five pairs. All 192 LSP
responses passed exact checks (168 measured plus 24 warmups); eight fixed CLI
semantic checks and 3,200 phase target assertions also passed. These selected
queries do not establish complete Java semantics.

An initial first-candidate diversity preflight failed before any timed sample:
the historical `overload_add` refusal had already been superseded by the committed
constructor expectation for `add(new JsonObject())`. Only that unmeasured refusal
was replaced with the independently reviewed, existing `overload_fromjson`
expectation. The correction was fixed before the second candidate's matrix; no
expected answer was learned from it. The failure and first candidate's adverse
controls remain part of the history, rather than being presented as first-attempt
successes. The second candidate had no failed measurement or preflight.

### Resources and phase diagnosis

Median sampled native process-group RSS, KiB:

| Workload | Baseline | Final candidate |
| --- | ---: | ---: |
| Gson unchanged | 256568 | 166476 |
| Gson target edit | 256548 | 166520 |
| Commons unchanged | 170944 | 171060 |
| Commons target edit | 171076 | 170996 |

Gson's observed group RSS was about 35% lower; Commons was essentially unchanged.
The group covers the active LSP broker, native supervisor and analysis worker,
excluding the Python client and the other idle session.
The requested sampling interval was 5 ms. All 192 observations saw three native
processes simultaneously at least once and all sampling threads stopped. Maximum
scan gaps were 20.986 ms (full), 11.889 ms (sources) and 9.597 ms (diversity), with
4, 0 and 7 process-read failures/races respectively. The scans are non-atomic,
shared pages are counted repeatedly, and short processes/peaks can be missed.
This is not unique physical memory, a quota, a leak test or complete peak coverage.

A separate unchanged public-API probe used one warmup plus seven observations
per block, each checking 100 prepared queries. Blocks ran baseline full, final
candidate full, final candidate source-only, baseline source-only. They were not
interleaved samples; phase medians must not be added to predict LSP wall time.

Gson full-context phase milliseconds, minimum / median / maximum:

| Phase | Baseline | Final candidate |
| --- | ---: | ---: |
| load_ms | 497.858 / 518.543 / 548.849 | 515.802 / 534.705 / 610.655 |
| overlay_ms | 7.965 / 8.795 / 11.452 | 8.197 / 8.659 / 10.016 |
| prepare_ms | 255.838 / 267.123 / 275.803 | 104.487 / 106.065 / 127.177 |
| warm_query_mean_ms | 1.061 / 1.133 / 1.170 | 1.139 / 1.242 / 1.316 |

Preparation fell about 60.3%, from 267.123 to 106.065 ms. Loading and already
prepared queries did **not** improve: medians rose from 518.543 to 534.705 ms and
from 1.133 to 1.242 ms respectively. The latter is each observation's mean of
100 queries. Source-only preparation was 27.739 versus 24.822 ms in its separate
blocks, while source-only LSP controls above remained mixed. No broad source-only
speedup follows. Phase self-memory snapshots also cannot be substituted for the
native process-group RSS observations.

These are single-environment observations with seven samples, not a p95,
statistical-significance claim, cold-cache test, throughput result, GUI/Mac
acceptance or comparison with IntelliJ/JDT LS.

## Reproduce the primary gates

Use already provisioned, checksum-verified inputs from the existing
[acceptance setup](ACCEPTANCE_PLAN.md), [Gson model](../examples/second_project/README.md)
and [Neovim input manifest](../scripts/neovim-inputs.json). In the example below,
`inputs/gson` contains the six pinned Gson input files including both JMODs;
`inputs/commons` contains the pinned published source JAR, and
`tools/neovim/bin/nvim` is the separately verified Neovim 0.11.5 binary with its
`INPUT.json`/license/runtime tree. Choose a new output directory. Input setup is
separate; these checks do not download, run a JVM or execute upstream project code.

```sh
cargo build --offline --locked --release --features cli \
  --bin java-analyzer --example real_source
mkdir -p payload-results
python3 scripts/check-cli-acceptance.py \
  inputs/commons/commons-lang3-3.17.0-sources.jar inputs/gson/java.base.jmod \
  --native target/release/java-analyzer
python3 scripts/check-real-source.py \
  inputs/commons/commons-lang3-3.17.0-sources.jar inputs/gson/java.base.jmod \
  --profile release --rounds 1
python3 scripts/check-second-project.py inputs/gson --constructor-calls \
  --native target/release/java-analyzer --output payload-results/gson.json
python3 scripts/check-string-call-acceptance.py inputs/gson \
  --native target/release/java-analyzer --output payload-results/strings.json
python3 scripts/check-strict-reference-acceptance.py inputs/gson \
  --native target/release/java-analyzer --output payload-results/strict.json
python3 scripts/check-lsp-project.py inputs/gson --native target/release/java-analyzer
python3 scripts/check-neovim-editor.py --nvim tools/neovim/bin/nvim \
  --native target/release/java-analyzer --output payload-results/neovim-definition
python3 scripts/check-neovim-refresh.py --nvim tools/neovim/bin/nvim \
  --native target/release/java-analyzer --output payload-results/neovim-refresh
```

Keep two independently preserved, already-built release binaries and their
matching receipts. Follow the [receipt format and source-digest procedure](ARCHIVE_REUSE.md#reproduce-the-supplemental-measurement).
The existing entry point below reproduces the primary full-context unchanged
and target-edit workloads, without building during timing:

```sh
python3 scripts/measure-lsp-reload.py \
  --baseline preserved/baseline/java-analyzer --baseline-receipt baseline.json \
  --candidate preserved/candidate/java-analyzer --candidate-receipt candidate.json \
  --sources-jar inputs/commons/commons-lang3-3.17.0-sources.jar \
  --java-base-jmod inputs/gson/java.base.jmod --second-inputs-dir inputs/gson \
  --repetitions 7 --output payload-results/primary-comparison.json
```

This entry point does not run the additional source-only, diversity or phase
passes. Their exact workload definitions, sample ordering and complete numeric
evidence are recorded above and in the appendix; do not label the primary command
as reproduction of the entire three-pass matrix. The normal contributor checks
remain required independently of all measurement commands.
