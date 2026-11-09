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
