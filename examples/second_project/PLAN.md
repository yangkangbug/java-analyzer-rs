# Second real-project CLI acceptance plan

This plan was frozen before observing the first CLI analysis results for this
project. The exact 16 sites and source-grounded goals are in `query-plan.json`.
All chosen sites remain in the corpus, including failed and unsupported cases.
There is no successful-resolution percentage target.

## Input and graph boundary

Gson 2.10.1, official upstream commit
`2ce6a6106c22bf0cce2e39f68b0da2f117d6e66b`, release tag
`gson-parent-2.10.1`. The official commit ZIP SHA-256 is
`3678276a90e914bfabeb069c2c7ff99460f5de951baf65e764a2c9731946da13`.
This is a manually supplied classpath/source-set model, not Maven/Gradle import,
JPMS analysis, an upstream build, a test-suite run or Java conformance evidence.
No upstream script, build system, test, application or JVM is executed.

The roots retain all 211 original Java files and 1,484,012 bytes in:

- `gson/main`: `gson/src/main/java/com`, 79 files, 593,352 bytes.
- `gson/test`: `gson/src/test/java`, 119 files, 821,635 bytes.
- `extras/main`: `extras/src/main/java`, 8 files, 41,289 bytes.
- `extras/test`: `extras/src/test/java`, 5 files, 27,736 bytes.

Edges: `gson/test -> gson/main`, `extras/main -> gson/main`, and
`extras/test -> extras/main`. The last sees `gson/main` transitively, not its
119 test files. All core test sources, including Java 17 files, remain selected.

The main package subtree excludes only `gson/src/main/java/module-info.java`
from its parent Java root. Upstream's ordinary compiler configuration also
excludes this file and separately processes it with ModiTect. A whole-root
negative control must reject it explicitly; this is not JPMS support.
`gson/src/main/java-templates/com/google/gson/internal/GsonBuildConfig.java` is not generated, copied into
the model or replaced. Two pinned uses must expose that missing generated input.
Metrics (6 Java files), proto (1 main and 3 test), the Android example (3), and
the raw template (1) are outside the chosen two-module model. These exclusions
were chosen from project structure before measuring analyzer success.

Every context receives explicit `java.base.jmod` and `java.sql.jmod` from pinned
Temurin 21.0.13+9. This covers selected source imports, not all platform module
readability or the transitive public signatures of java.sql. JUnit 4.13.2 and
Hamcrest core 1.3 are binary data only in test contexts. JSR250 API 1.0 is data
only in extras main/test. Binary contexts do not transitively export through
source edges. No compiled Gson JAR obscures source targets.

## Required checks, fixed before observation

- Summary inspection asserts exact owned/visible inventories and explicit binary
  counts/context. Parser reporting covers every selected owned source once,
  labels parser gaps rather than semantic Java errors, and checks source bounds
  and byte/UTF-16 conversion. Record every affected file; do not select roots or
  reject the whole corpus solely to achieve a low parser-gap count.
- The 16 queries cover two cross-module types, three test/main/helper types,
  a local and a field, one unique-name source method, three binary types, two
  overload refusals, one call-chain limitation and two missing-generated names.
  Source results must identify the independently inspected exact declaration
  name, kind, owner, compilation, path and byte/UTF-16 span. Binary results must
  retain the fixed archive origin and provide no fabricated source range.
  Complete refusal candidates are retained, without a guessed first winner.
- Main-to-test, core-to-extras, and extras-test-to-core-test file queries must be
  rejected as invisible, not resolved through a globally merged index.
- Removing the extras-to-core edge must remove the cross-module type target.
  Removing the test JUnit binary or extras JSR250 binary must remove its target.
- An invalid source root, missing binary path and inclusion of the upstream
  module descriptor must fail explicitly with bounded, nonpartial output.
- On disposable source copies, rename a selected field and its use, then load
  again and check the changed spelling/range. Restore original bytes and verify
  the original result. Malformed copied source must yield labeled parser gaps.
  A failed manifest load followed by a valid load must recover. These are fresh
  one-shot reloads, not a persistent/transactional reload or unsaved overlay.
- Corrupt an expected target and assert that the checker itself rejects it.
- Run the built product with an empty Java environment and PATH=/nonexistent,
  from a different working directory, with bounded wall time and output.

## Measurements and evidence limits

Measure the actual release executable after functional validation: load/preparation
where instrumented, invocation and batch timings, first/repeated query costs,
source-edit reload and peak RSS with exact revision/machine/artifact checksums.
State what includes supervisor/process/output overhead and what is not observable.
Do not confuse a new-process warm-filesystem run with a persistent warm index.
Initial budgets must follow the measured workload, not inherited microbenchmarks.
No speed comparison, full argument applicability, upstream build/test success or
Java language conformance claim follows from this fixture.
