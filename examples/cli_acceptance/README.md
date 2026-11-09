# Optional shipped-CLI acceptance

This gate runs the actual `java-analyzer` product executable. It does not compile
an alternate library harness or invoke Java, Maven, Gradle, JDT LS, a network
fetcher, or upstream code. It uses the same external checksum-pinned Apache
Commons Lang 3.17.0 source JAR and Temurin 21.0.13+9 `java.base.jmod` as the
[real-source workload](../real_source/README.md).

## Reproduce

Use Rust 1.96.0, Python 3.11+, and a POSIX host. Obtain inputs separately using
the [documented fixed-input setup](../../docs/ACCEPTANCE_PLAN.md). From the
repository root:

```sh
cargo build --locked --features cli --bin java-analyzer
python3 scripts/check-cli-acceptance.py /path/to/sources.jar /path/to/java.base.jmod --native target/debug/java-analyzer
```

For the optimized product, build with `--release` and pass
`--native target/release/java-analyzer` (also the harness's default). `--native`
selects an already-built executable; the harness never builds it. It prints the
executable's SHA-256 and each subprocess's end-to-end elapsed time and output
size. These observations are neither an isolated query benchmark nor a
performance budget. They include loading, parsing, preparation where needed,
serialization and process startup.

Every invocation has a 60-second timeout and a 16-MiB combined stdout/stderr
capture limit by default. Override with `--timeout-seconds` (greater than zero,
at most 600) or `--max-output-bytes` (1 KiB through 64 MiB). A timeout or excess
output fails the gate and kills the subprocess group. These are **acceptance
harness controls**, not production parser CPU/stack/memory isolation, host
cancellation, or a guarantee that arbitrary user input is safe.

## Checks

- Reuse the input setup's path/type, duplicate, CRC, expanded-size and entry-count
  checks before extracting only Java source bytes into a fresh temporary directory.
  Source license and notice are checked and retained during the run. Both input
  hashes are verified. Extraction never uses `extractall` or creates archive links.
- Assert the complete inventory: 249 sources and 3,676,819 original source bytes,
  with 7,615 platform class origins and no shadowed classes. No Commons Lang binary
  JAR is loaded alongside the sources. The harness uses `inspect --details` to
  verify every origin as well as the summary class count; ordinary `inspect`
  omits the full origin list.
- Generate schema-version-1 project and request documents in different directories.
  Source and platform paths are manifest-relative, and requests use that same base.
  Run the CLI from a third, unrelated working directory, with no inherited
  environment variables and only `PATH=/nonexistent`. There is no `JAVA_HOME`.
- Compare all 41 existing [query goldens](../real_source/queries.tsv) and every one
  of their 86 ordered [target/candidate records](../real_source/targets.tsv).
  Source names/ranges are independently checked against the original archive
  bytes, including absolute UTF-16 endpoints. Query order, compilation context,
  invocation-local snapshot metadata and one prepared index for the batch are
  checked too.
- Require 76 source candidates to expose exact source origins and ranges. Require
  ten binary candidates to expose exact JMOD origins, binary names and loaded
  contexts, with `source_available: false` and no fabricated `0..0` source range.
  The CLI's unknown binary linking context and limited identity precision stay
  explicit. Loaded contexts are not a claim of exact linking provenance.
- Retain all refusals and candidates. The current corpus has 27 resolved queries,
  eleven overload refusals, two unsupported signature comparisons and one unbound
  receiver. Name resolution is not argument applicability or generic inference.
- Require parser reports for all 249 sources with the
  `parser_limit_or_invalid_syntax` label, valid byte/UTF-16 ranges, and internally
  consistent file/gap counts. They must explicitly say they are not Java semantic
  diagnostics. The current inventory is 24 affected files and 85 reports.
  Changes from [the parser baseline](../real_source/parser-errors.tsv) are printed,
  not rejected by a fixed error-count gate. Omitted files, wrong labels and
  inconsistent counts fail.
- Run seven comparator controls against altered response copies: remove a query,
  change a declaration range, drop an overload candidate, invent a successful
  resolution, remove a parser file, mislabel semantic diagnostics, and corrupt a
  report count. Each alteration must be rejected.
- Load a second manifest with the platform omitted. Its inspection must have no
  binary origins; the source `ArrayUtils` target must remain unchanged while the
  `Character` query becomes `Unresolved` with no candidates. An empty executable
  search path is separate from the presence of explicitly supplied platform data.

The source/test/dependent-project shape is covered separately by the
[CLI example](../cli_project/) and native CLI integration tests. This fixed
published-source workload does not run the upstream project build/tests and does
not repeat the real-source library harness's 3,030-call namespace audit. Neither
gate establishes full Java conformance, complete repository/build-model support,
an editor protocol, reload/overlay support, or a replacement for all JDT LS
features.

## Provenance and licenses

Checksums, exact URLs and licensing are recorded in
[`scripts/acceptance-inputs.json`](../../scripts/acceptance-inputs.json).
Commons Lang is Apache-2.0 and retains its archive `META-INF/LICENSE.txt` and
`META-INF/NOTICE.txt` during extraction. Temurin is GPL-2.0 with the Classpath
Exception and assembly exception, plus its distribution component notices;
input setup retains `jdk-legal/`. The JMOD is read as class-library data only.
No upstream sources, source archive, JDK binaries, or legal-notice copies are
committed or redistributed by this gate. Temporary extracted files and generated
JSON documents are removed after the run.
