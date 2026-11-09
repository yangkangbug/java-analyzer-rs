# Second real project: Gson core/extras

This optional acceptance workload runs the actual native CLI on a four-compilation
manual source-set model from [Gson 2.10.1](https://github.com/google/gson/tree/2ce6a6106c22bf0cce2e39f68b0da2f117d6e66b).
It retains **all 211 selected original Java files (1,484,012 bytes)**, both main
and test roots, a real cross-module source dependency, and test-only binary data.
It neither builds Gson nor executes its tests. All archives and Java sources are
input data; Java, Maven, Gradle and upstream scripts are never started.

The [frozen plan](PLAN.md) and `query-plan.json` preceded observation of CLI
analysis results. The corpus has not been narrowed in response to parser failures
or unsuccessful navigation. In particular, all Java 17 test sources remain in it.
This is a distinct project shape, not a claim of automatic Maven import, Java
conformance, JPMS support or upstream build/test success.

## Reproduce

Build the repository's native release executable separately with the pinned Rust
toolchain and lockfile. Python 3.11+ is required only by setup/acceptance tooling.

```sh
cargo build --locked --release --features cli --bin java-analyzer
python3 -m unittest discover -s scripts -p 'test_second_project.py' -v
python3 scripts/fetch-second-project-inputs.py --cache /tmp/gson-input-cache \
  --platform-dir /path/to/verified/jmods --output /tmp/gson-inputs
python3 scripts/check-second-project.py /tmp/gson-inputs \
  --native target/release/java-analyzer --output /tmp/gson-acceptance.json
```

The setup helper downloads only four pinned archives and requires already supplied
`java.base.jmod` and `java.sql.jmod`. It verifies both against the Temurin pins;
no JDK discovery or downloading occurs in this helper. Use `--offline` after
populating its cache. Output directories and the optional result file must be new.
The checker rechecks every checksum, validates the source archive before writing
sources, uses private disposable files, and runs from a different working directory
with only `PATH=/nonexistent` in the product environment. Each invocation has a
40-second outer harness deadline and bounded captured output, in addition to the
product's native execution supervisor. This is not an OS security sandbox.

## Fixed provenance and dependency scope

The sole input pin manifest is
[`scripts/second-project-inputs.json`](../../scripts/second-project-inputs.json).
The source is the official immutable commit archive:

- Commit: `2ce6a6106c22bf0cce2e39f68b0da2f117d6e66b`.
- Release tag: `gson-parent-2.10.1`, verified through upstream's annotated tag.
- URL: <https://codeload.github.com/google/gson/zip/2ce6a6106c22bf0cce2e39f68b0da2f117d6e66b>.
- Size: 628,229 bytes.
- SHA-256: `3678276a90e914bfabeb069c2c7ff99460f5de951baf65e764a2c9731946da13`.
- License: Apache-2.0, retained upstream `LICENSE` and source headers.

All files in this immutable ZIP were compared byte-for-byte with the corresponding
release-tag ZIP during selection. The annotated tag is unsigned; pinning content
is reproducibility evidence, not a cryptographic upstream signature claim.

Binary inputs are JUnit 4.13.2 (EPL-1.0), Hamcrest core 1.3 (BSD-3-Clause), and
JSR250 API 1.0 (CDDL-1.0). The latter's JAR has no license file; the canonical pin
records its publisher POM license evidence. JUnit/Hamcrest appear only in test
contexts; JSR250 appears only in extras main/test. The platform data comes from
the already pinned Temurin 21.0.13+9 Linux x64 distribution and retains its separate
GPL-2.0 with Classpath/assembly exceptions and component notices.
No Gson binary JAR duplicates the source declarations. No upstream archive,
source tree or JDK is redistributed in this repository or release package.

Manual edges are `gson/test -> gson/main`, `extras/main -> gson/main`, and
`extras/test -> extras/main`. The last reaches core main transitively, not core
tests. Owned/visible source counts are respectively 119/198, 8/87 and 5/92;
core main owns and sees 79. Each compilation separately receives base/sql data,
because source dependencies do not export binary contexts. Summary inspection
reports class counts of 7,693 for core main, 8,088 for core test, 7,704 for extras
main and 8,099 for extras test. Full detailed inspection would exceed the CLI's
aggregate result-item cap; this harness deliberately requests documented summary
inspection and independently verifies binary provenance on selected queries.

Only the core package subtree is a main root. Upstream's ordinary compiler step
also excludes `module-info.java`; the descriptor is handled by a separate plugin.
The checker separately includes the full root and verifies explicit JPMS refusal.
The template `gson/src/main/java-templates/com/google/gson/internal/GsonBuildConfig.java`
is intentionally not rendered or added as a source root. Its absence remains
visible in two fixed query outcomes. Metrics, proto and the Android example are
out of scope. The explicit base/sql subset covers source imports in the chosen
roots, not every transitive platform API signature or JPMS module-readability
closure. See PLAN.md for every excluded root and source count.

## Exact navigation and negative controls

The initial baseline at analyzer `992b6edb3ad84463bbfbb9bb3351b3c2d2e3f987` is:

- 11 `Resolved` sites: eight exact source/local/field/method declarations and
  three binary classes with explicit archive provenance and no source range.
- `Gson.fromJson`: `SignatureNotCompared`, retaining all 11 source candidates.
- `JsonArray.add`: `OverloadNotResolved`, retaining all five source candidates.
- Chained `array.remove(1).getAsString()`: `Unresolved`.
- Missing generated `GsonBuildConfig`: `Unresolved`; its `VERSION` receiver:
  `ReceiverNotBound`.

`expected.json` lists the complete 27 ordered target/candidate records. All 24
source declaration records were checked against the unmodified source declaration
name and enclosing declaration, including the complete 11-method `fromJson` and
five-method `add` sets. Expected ranges are not inferred solely from agreement
between two analyzer modes. This source inspection is not a compiler oracle.
The source `size()` result is unique-name navigation, not proof of general method
argument applicability. Refusal candidates are never treated as selected targets.

Every query checks requested/owning compilation, canonical source path, exact
byte position and status. Source targets additionally check kind, owner, name,
compilation and UTF-8/absolute-UTF-16 name ranges. Binary targets check exact archive
entry, loaded contexts, unavailable-source marker and absence of fabricated ranges.

Fourteen controls verify three visibility rejections; removal of a source edge,
JUnit, JSR250 and SQL origins; missing root and binary paths; JPMS refusal; a
field-spelling edit followed by a fresh load; malformed source reporting; restored
source after failed loads; and rejection of an intentionally corrupted target.
The edit is a disposable text change, not a tested Java refactoring. Reload means
new one-shot invocation and fresh preparation, not a persistent server, atomic
snapshot rollback, unsaved overlay or incremental invalidation.

## Parser gaps and readiness limits

All selected files are reported once in their owning context. At the initial
baseline, **58 files contain 1,513 parser-report nodes**:

- Core main: 24 files, 161 nodes.
- Core test: 30 files, 1,348 nodes.
- Extras main: four files, four nodes.
- Extras test: no reported nodes in its five files.

`parser-errors.tsv` retains the entire affected-file inventory. The wrapper checks
every report range, byte/UTF-16 conversion and explicit parser-limitation label.
Changes are reported, not rejected by a fixed low-error-count threshold. Zero
reported nodes is not proof of Java correctness, and cascaded nodes are not
independent defects. Illustrative first-gap contexts are explicit generic method
arguments in GsonTest (915 nodes), local record declarations in Java17RecordTest
(231), primitive class literals in Primitives (54), and switch in UtcDateTypeAdapter
(one). These substantial gaps prevent describing the workload as fully supported.
No type, overload or unresolved-name semantic diagnostics are claimed.

## Performance-tool helper contract

The repository-authored module exposes:

- `prepare_fixture(inputs_dir, workdir) -> Fixture`: checks six pinned local
  inputs; writes into a new or empty private work directory; provides absolute
  `manifest` and `requests` paths, source bytes and exact expectations.
- `check_response(command, parsed_response, fixture)`: validates actual product
  `inspect` (summary), `declaration` and `parser-report` responses. It raises on
  any contract mismatch. Run it outside the measurement interval.
- `fixture.metadata`: portable fixed project revision, input hashes/sizes,
  source/query counts and scope limitations, without host or temporary paths.

The caller owns temporary-directory lifetime. Preparation does not fetch, build,
run project code or execute external plugins. Full acceptance additionally runs
mutation/isolation controls; those are not part of benchmark timing. No throughput,
latency or memory budget is inferred from this functional baseline.

## Additive constructor-argument correction

The historical 16-query files above remain unchanged. The current gate applies
one explicitly validated correction for `a.add(new JsonObject())`; the other
15 outcomes, earlier local/reference probes and parser/source records remain fixed.
See [construction-argument acceptance](CONSTRUCTOR_CALLS.md) for the exact old/new
rows, inventory provenance, baseline RED and candidate checks, and limitations.
