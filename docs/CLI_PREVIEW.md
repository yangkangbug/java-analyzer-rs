# Experimental CLI preview evaluation

This protocol fixes the samples and evaluation rules before final resource
measurements. It evaluates the existing one-shot native CLI, not a language server
or a completed replacement for JDT LS. Runtime commands do not execute Java,
Maven, Gradle, downloaded build scripts or project application/test code.

## Samples fixed in advance

1. Apache Commons Lang 3.17.0: all 249 published main-source files, checksum-pinned
   java.base data and the existing 45 exact navigation queries. Keep all ten
   refused positions and all parser reports. The source archive is not a complete
   repository/build-model sample. See [its provenance and limits](../examples/real_source/README.md).
2. Gson 2.10.1, immutable upstream commit 2ce6a6106c22bf0cce2e39f68b0da2f117d6e66b:
   211 unchanged Java files in core/extras main/test, four explicit compilation
   contexts, fixed dependency data and sixteen predeclared query categories.
   Keep cross-module/source-set cases, binary origins, ambiguous/unsupported calls
   and missing generated-source outcomes. See the frozen
   [sample and control plan](../examples/second_project/PLAN.md).

Gson configuration is a manually inspected classpath manifest. It is not automatic
Maven import, complete dependency closure or JPMS implementation. Generated
GsonBuildConfig is not manufactured. Module-info is outside the selected package
root and is separately tested as unsupported. Excluded metric/proto/Android
source roots are disclosed, not silently counted as analyzed.

These projects were selected for different shapes, not to maximize resolution
percentages. Initial Gson analysis reports 1,513 parser gaps in 58 selected files.
Those reports do not prove invalid Java; they expose substantial unsupported
parser coverage. A handful of successful navigation samples cannot erase that
readiness limit. No full upstream build or test conformance is claimed.

## Correctness and delivery gates

- Verify every external artifact hash, complete selected source inventory and
  preserved original bytes. Bound archives and paths. Setup is a separate explicit
  network action; analysis, package smoke and measurement do not fetch inputs.
- Compare all predeclared results with exact source ranges or explicit binary
  provenance. Keep refused candidates. Mutated/missing expected output must fail
  the checker. Prepared/rebuilt agreement alone is not a semantic oracle.
- Test source/test and module visibility, missing edges/dependencies, missing
  generated input, invalid roots, source edit/rename/restore and parser labels.
  Reload means a new CLI invocation loading disk, not persistent transactional
  reload, unsaved overlays or stale editor-result handling.
- Build/install only the analyzer's reviewed source using the pinned locked Rust
  toolchain. Check the detached archive and smoke project with no Java environment.
  Preserve licenses, checksums, source/build provenance and the observed libc
  compatibility floor. See [the installation contract](CLI_INSTALL.md).
- Limit binary support to the tested Linux x86_64 GNU environment. Do not advertise
  static linkage, older-glibc compatibility, macOS or Windows certification.
  Deterministic archive metadata is separate from reproducible binary compilation.

A correctness failure, crash, timeout or unexpected output fails that workload.
Missing memory observations must be explicit and cannot become a memory pass.
Parser reports and intentionally unsupported query outcomes are retained evidence,
not rewritten to make these checks pass. Passing the protocol is bounded sample
acceptance, not a declaration that arbitrary projects are safe or supported.

## Resource measurement protocol

Use the actual fresh package-build binary and its checksum/build receipt. Stage
and hash inputs before timing. For each fixed workload, do one excluded warmup,
then seven measured repetitions in serial round-robin order. Do not run builds,
compiler comparisons or other benchmark workers concurrently.

The seven workloads are four Commons Lang commands (detailed inspection,
parser reporting, the 45-query batch and its single-file edit/full reload) and
three Gson commands (summary inspection, parser reporting and the 16-query batch).
The measured edit belongs only to Commons Lang; Gson edit/rename behavior is
covered separately by correctness controls. For the edit, shift
only affected expected byte/UTF-16 ranges and restore the staged source after
each edited invocation, including failure paths. Preserve the original archive.

Every sample is a fresh CLI process and full snapshot load with a likely warm OS
filesystem cache. Total wall time includes supervision, load/parse/preparation,
query work and serialization/capture. It is not a true cold-cache benchmark,
per-query latency, an incremental update, or an IDE response-time promise.
Report raw samples, minimum, median and maximum; seven samples do not justify a
stable tail-latency percentile. Performance is initially observational, without
an invented speed or memory threshold.

Use separate wall-time/output safety limits for the measured command and harness.
On Linux, report wait4 resource accounting and sampled process-group RSS with
precise semantics: maximum individual-process high-water RSS is not summed peak
memory; sampled simultaneous supervisor/worker RSS double-counts shared pages
and can miss short-lived peaks. Record sampling interval, observed processes and
read failures. Do not relabel parent-only RSS as analyzer memory. No denied tracing,
cache flushing, system security change or memory quota is required.

After separately preparing the pinned data and building/checking the local
package, run from the matching reviewed source checkout:

```sh
python3 scripts/measure-cli-preview.py /path/to/sources.jar /path/to/java.base.jmod \
  --native /path/to/extracted/bin/java-analyzer \
  --build-provenance /path/to/extracted/BUILD.json \
  --second-inputs-dir /path/to/gson-inputs \
  --repetitions 7 --output /path/to/new-results.json
```

Use new output paths. The sibling protocol JSON is written before warmups and
samples; failure evidence is retained rather than relabeled as a successful run.
Development bundles are explicitly dirty evidence and are not final delivery.

Record source revision/tree or clearly marked dirty state, source-input digest,
Rust/build target, binary checksum, artifact checksums and safe host characteristics.
Omit private paths, usernames, hostname and full environment. Publish the exact
protocol before results; retain failures and limitations alongside measurements.

## Still outside an editor-ready preview

Automatic project import, broad Java expression/generic/overload correctness,
semantic diagnostics, completion, references and refactoring remain incomplete.
Editor integration additionally needs unsaved buffers, line/column positions,
revision-safe results, asynchronous delivery and cancellation/restart. The
[current preview gate list](PREVIEW_PLAN.md) remains authoritative; this delivery
slice does not silently mark every gate complete.
