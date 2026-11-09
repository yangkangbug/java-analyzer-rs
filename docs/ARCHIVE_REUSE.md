# Scoped archive reuse: contract and measurements

This plan is fixed before implementing the optimization. The baseline is
`a0e1a1aee960d6a3e2e6f25a7fd0f126c9d4224b`. The objective is less repeated
work during real editor navigation, without changing supported Java semantics,
project visibility, disk reload behavior between requests, or process isolation.

## Choice of slice

The current Gson model has four explicit compilation scopes sharing platform
archives and some dependencies. Each LSP definition reloads the whole workspace,
applies its full-text overlays and prepares an index. Repeated archive decoding
within that single load is a candidate for reuse; it is not evidence that all
query latency comes from archive reading.

An automatic Maven/Gradle importer is deferred. The pinned Gson build uses parent
dependency management, profiles, generated sources and separate module-descriptor
handling that the current manifest cannot fully express. Its four-scope acceptance
model is manually selected, not imported. The Commons source archive is not a
complete build checkout. Neither sample establishes a supported build-import
subset. No project script, build tool, JVM or dependency resolver is executed.

## Intended contract

- Reuse only successful JAR/JMOD signature reads during one workspace load.
  A new load starts empty. There is no process-global, cross-request or on-disk
  cache, no watched filesystem and no retained semantic query result.
- Identity includes the checked canonical archive path, archive format, exact
  JAR target release and freshly captured archive bytes. A JMOD is not a JAR,
  and equal binary names from different origins or release selections must not
  become one input. Neither path identity nor metadata alone proves a hit.
- Check paths and perform a bounded fresh byte read for each logical occurrence.
  Compare captured bytes exactly before reusing parsed signatures; a miss parses
  those same captured bytes rather than rereading the path. Changed, missing or
  corrupt inputs cannot silently reuse an earlier occurrence. This is still not
  an atomic filesystem snapshot; callers must not mutate files during a read.
- Reuse only if the recorded compressed bytes, entry count, largest declared
  entry and total declared expansion satisfy every current effective archive
  limit. All entries count, including ignored resources and versions. Otherwise
  use the ordinary parser on the already captured bytes, preserving its
  rejection/error precedence without a second filesystem read.
- Charge each logical occurrence to its compilation and workspace budgets even
  on a reuse hit. Sharing does not enlarge the accepted input or archive budgets.
- Share immutable parsed class signatures only. Compilation-local classpath
  precedence, shadowed origins, source collisions and independently constructed
  scoped binder records must remain unchanged. Never share a resolved hierarchy
  or a scope's selected members with another scope.
- Private reference-counted signature storage may make snapshot and overlay
  cloning cheaper. Public signature access stays borrowed and immutable. Cache
  keys, vectors and reference counts still consume memory. Retained raw archive
  bytes and entry count must have fixed private caps; exceeding a reuse budget
  evicts or skips retention, never rejects an otherwise valid input. This is not
  a process memory quota or a claim of zero overhead.
- LSP jobs still use their existing native supervisor/worker, cancellation and
  generation guards. Each request loads again, so source/JAR edits, corruption,
  removals and source-inventory changes cannot be hidden by a previous request.

## Falsifying tests and acceptance

Before implementation, add a failing test that proves duplicate archive decoding
or full signature cloning, using test-only observation rather than a production
counter. Correct result equality alone cannot prove that work was reused.

Native tests must cover:

1. Actual reuse for a shared JAR and JMOD, and fresh reading in a later load.
2. Release, format and origin separation; duplicate ordering and source/binary
   collision behavior; one shared class linked to different per-scope parents.
3. Stricter later limits for compressed bytes, all-entry count, ignored entry
   size and total expansion, plus compilation/workspace cumulative budgets.
   A cache hit must never bypass a failure the ordinary reader would report.
4. Signature clone/remove/replace isolation, old snapshot validity and Send+Sync.
5. Changed, missing and corrupt archives across loads and subsequent occurrences;
   path and symlink refusal; bounded retention eviction/miss behavior; failure
   leaves old snapshots usable and a subsequent valid load recovers.

Run the complete contributor checks and existing fixed Commons/Gson inventories,
query targets, refusal cases and parser reports. Do not alter their golden results
to accommodate an optimization. Rerun the real Gson LSP overlay lifecycle and
actual Neovim acceptance, including late responses, cancellation and restart.
Keep the independent development-only compiler oracle separate from no-JVM
production and offline tests. Hosted Mac checks remain distinct from Linux.

## Measurement protocol

Record exact source revision/tree, release binary checksum and pinned artifact
identities. Use serial observations on the same machine, with no concurrent builds
or benchmark workers. Keep initialization, unchanged sequential definition queries
and target-buffer-edit queries separate. Use an excluded warmup and seven measured
samples; report raw samples and minimum/median/maximum, not a tail percentile or
an IDE latency guarantee. Verify exact independently reviewed target locations for
every timed query and restore disposable edit inputs.

Measure load, overlay, preparation and query phases separately where a trusted
local probe can observe them. Phase probes are diagnostic evidence, not the
shipped editor runtime. Report relevant memory observations with their process
coverage and sampling limits; do not label one process's RSS as combined peak RSS.
Compare final release LSP with a retained baseline binary in alternating order.
No speedup is claimed until final same-workload measurements exist. A remaining
multi-second reload is still a usability limitation even if redundant decoding
is removed. This slice does not close incremental-analysis or project-import
milestones.

## Implemented verification

The private load-local FIFO holds at most 16 successful archive records and
64 MiB of boxed raw bytes. Every occurrence still reads the file. Unchanged
bytes reuse immutable signatures only after satisfying current effective limits;
each compilation still constructs its own binding records and pays its logical
input budgets. `ProjectSnapshot::load` remains an uncached comparison path.

Three regressions failed before implementation: repeated decode counts, separate
release-key reuse counts, and deep-cloned signature storage. All twelve new native
regressions now pass. Independent review also checked 2,100 cached/ordinary limit
combinations with exact error/result equality, same-size/same-mtime archive edits,
ignored-resource changes and empty/zero-cap behavior. These extra review probes
are separate from the checked-in default tests.

Local Linux validation passed 259 default and 344 all-feature Rust tests, two
doctests, strict Clippy/rustdoc, 169 offline Python tests, the full unchanged
Commons/Gson acceptance sets, the Gson LSP overlay lifecycle and all 13 real
Neovim checks. Two existing manual microbenchmarks remain ignored in each Rust
suite. The independent compiler oracle also passed; only that development check
uses Java. The analyzer, archive reader, editor acceptance and measurement run
without Java. Hosted Mac verification is a separate CI gate.

## Observed Linux results, 2026-11-09

The [full portable raw result](measurements/archive-reuse-2026-11-09.json) includes
the frozen protocol, all samples, checked target coordinates, input/build hashes,
source inventories, host facts and final identity checks. Its SHA-256 is
`771f88699f25638443b59a906665301a2388845abf7a7f3b9f9359f527afcb11`.
It contains 8 excluded warmups and 56 measured requests, seven per
project/workload/variant. All 64 exact target checks passed. No build or other
benchmark ran concurrently during this final comparison.

Milliseconds, minimum / median / maximum:

| Project and operation | Baseline | Candidate |
| --- | ---: | ---: |
| Gson unchanged definition | 2432.4 / 2467.3 / 2798.7 | 877.0 / 1003.6 / 1218.3 |
| Gson target edit + definition | 2343.3 / 2401.3 / 2706.8 | 912.4 / 977.2 / 1078.1 |
| Commons unchanged definition | 702.1 / 786.8 / 883.6 | 660.8 / 734.9 / 761.2 |
| Commons target edit + definition | 743.8 / 768.9 / 842.7 | 676.0 / 729.5 / 755.7 |

Gson's two median latencies were about 59.3% lower. Commons improved by 6.6%
and 5.1%, respectively. The single-scope workload has no repeated platform archive
within its load, so broad speedup claims would be misleading. These are two fixed
successful source definitions, not the full semantic acceptance set or an IDE
performance budget. Each request still reloads and prepares the project.

Median sampled simultaneous process-group RSS, KiB:

| Project and operation | Baseline | Candidate |
| --- | ---: | ---: |
| Gson unchanged definition | 413688 | 233708 |
| Gson target edit + definition | 413688 | 233688 |
| Commons unchanged definition | 188628 | 164364 |
| Commons target edit + definition | 188816 | 164316 |

Every measured sample observed all three native processes. RSS sums count shared
pages more than once; scans are non-atomic and can miss short peaks/processes.
They are neither unique physical memory nor a memory quota. The requested sample
interval was 5 ms; the largest observed gap was 19.97 ms. Ten process-read
failures/races occurred across nine sample rows and remain in the raw report.
These do not invalidate the exact navigation checks, but limit memory conclusions.

Both binaries are Linux x86_64 release builds, Rust 1.96.0. The retained baseline
binary SHA-256 is
`08e270a3f832a1cc865b7834b40ac40a778170695679742a657fcf0e9ad5deef`;
the candidate is
`dbb587ea45ce7d3473f3bf852c448159577df0d0c94030bb1678adf52e0f03b4`.
The baseline's Rust/build inputs match `a0e1a1a`; this is source equivalence, not
independent historical build attestation. The candidate was measured before
commit with `source_dirty=true` and exact Rust/build digest
`fbd58b71cfe27af101311c99b7b690d0149677ccfdaa5268f7fe342415e1bb6a`.
Documentation/tooling hashes are recorded separately. No statistical significance,
cold-cache performance, other-machine speedup or GUI/Mac latency is claimed.

## Reproduce the supplemental measurement

Prepare the existing checksum-pinned inputs separately. Keep two already-built
release binaries and their JSON receipts; the measurement does not build, install
or download software. Receipts must state `binary_sha256`, `git_revision`,
`rust_build_inputs_sha256`, `source_dirty`, `build_profile: "release"` and
`target: "x86_64-unknown-linux-gnu"`. Use `source_revision_kind: "source_equivalent"`
for a retained binary with matching sources rather than an attested build commit.

The tool's `rust_source_identity(revision=None)` helper computes the exact source
digest: compact, sorted JSON mapping every relative `src/**/*.rs` filename plus
Cargo.toml, Cargo.lock and rust-toolchain.toml to its file SHA-256, then SHA-256 of
that JSON. Passing a 40-hex revision reads that Git tree; omitting it reads current
Rust/build files, including new files. The candidate receipt must match current
source bytes and its actual Rust-source dirty status. Receipts are operator
declarations cross-checked against files, not trusted build attestations.

```sh
python3 scripts/measure-lsp-reload.py \
  --baseline /absolute/baseline/java-analyzer --baseline-receipt baseline.json \
  --candidate /absolute/candidate/java-analyzer --candidate-receipt candidate.json \
  --sources-jar /absolute/inputs/sources.jar \
  --java-base-jmod /absolute/inputs/java.base.jmod \
  --second-inputs-dir /absolute/inputs/gson \
  --repetitions 7 --output /absolute/new-results.json
```

Existing outputs are refused. The protocol is saved before sessions start, and
failed or interrupted runs retain incomplete evidence rather than a success.
The Linux-only tool has no timing threshold and is not run as a performance gate
in CI. Its offline accounting, input and cleanup tests join the normal Python
suite. The earlier exploratory and one-repetition plumbing runs were not used
for the table above.
