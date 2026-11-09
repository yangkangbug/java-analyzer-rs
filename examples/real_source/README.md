# Optional real-source acceptance inputs

This is a fixed, limited name-binding workload over **all 249 published Java
source files** of Apache Commons Lang 3.17.0 (3,676,819 source bytes). It is not
the earlier one-file synthetic consumer of the Commons Lang binary JAR. No
upstream project build, test suite, Maven, Gradle, Java process, or compiler runs.
The published sources exclude upstream test/build inputs, so this is not a
complete repository/build-model acceptance test.

## Reproduce

Obtain these exact public artifacts separately; the harness never downloads.
Do not supply the Commons Lang binary JAR: that would mix duplicate source and
binary declarations and could obscure source-binding gaps.

- Sources: <https://repo.maven.apache.org/maven2/org/apache/commons/commons-lang3/3.17.0/commons-lang3-3.17.0-sources.jar>
- Sources SHA-256: `5fdcac21ad329766054a95367d7583dfcdca737d221d5e01a5f2a198c04c6b18`
- Platform data: Temurin 21.0.13+9 Linux x64 `java.base.jmod`, as documented in
  [the platform validation record](../../docs/VALIDATION.md#native-platform-input-follow-up).
- JMOD SHA-256: `fad51abeafe6a1ecb0c2696a88684dc417cc534b74b5d865042df7381d9d0e91`

From the repository root, using the pinned Rust toolchain and Python 3.11+:

```sh
cargo build --locked --release --example real_source
python3 scripts/check-real-source.py /path/to/commons-lang3-3.17.0-sources.jar /path/to/java.base.jmod --rounds 5
```

The Python standard-library wrapper verifies both hashes, extracts only Java
source data into a temporary directory, then launches the already-built Rust
example with only `PATH=/nonexistent` in its environment. Extraction does not run
upstream code. Use `--profile debug` after building the debug example if desired.
Direct execution of the Rust example does not verify artifact hashes; use the
wrapper for acceptance. Files are not retained after the wrapper exits.

## Provenance and license

Apache Commons Lang is Apache-2.0. The source archive contains
`META-INF/LICENSE.txt` and `META-INF/NOTICE.txt`; the latter says:

> Apache Commons Lang
> Copyright 2001-2024 The Apache Software Foundation
> This product includes software developed at The Apache Software Foundation
> (https://www.apache.org/).

No upstream source/archive/JDK data is redistributed here. The TSV files contain
query identifiers, byte positions, observed names/targets, and parser error
counts, rather than copied implementation source. Normal artifact use remains
subject to the upstream licenses. The JMOD's separate provenance and licensing
are covered by its official Temurin distribution; it remains an external input.

## Fixed query corpus and evidence boundary

`queries.tsv` pins 15 real source positions, the queried spelling, status, and
target count. `targets.tsv` records each ordered candidate separately, including
kind, owner, name, source range and source path or JMOD suffix.
The example verifies every target field and checks exact equality between
prepared and rebuild-per-query results on every timed query, outside the timer.
There are eleven resolved queries, three explicit overload refusals, and one
unbound receiver:

- Same-package source class/field, source superclass, local parameter and field.
- Explicit `Objects` import, implicit `java.lang` classes, binary `String.length`.
- `Character.isWhitespace` and `Objects.requireNonNull` remain overload refusals.
- `ArrayUtils.isEmpty` now finds all nine source overload declarations after catch
  parsing no longer loses that part of the file. It remains `OverloadNotResolved`:
  argument-based overload selection is not implemented. The earlier baseline
  reported `Unresolved`. Each candidate range was checked against its unmodified
  declaration, rather than selecting one candidate to make the query pass.
- `ImmutablePair`'s `Map.Entry` receiver remains `ReceiverNotBound`.
- Two CharSequenceUtils loop-variable uses (`j--` and the body argument) now
  resolve to the exact `j` declaration at bytes 1234..1235. These expectations
  were checked independently against the original source and lexical scope,
  not generated from prepared/rebuild agreement.

The current baseline inventory is 249 losslessly represented files, 225 without
reported parser errors and 24 with a total of 85 error nodes. Zero errors is not proof
of Java conformance. Error nodes can cascade and are not independent defects.
`parser-errors.tsv` records this baseline; changes are reported, **not rejected**
by a fixed count gate. The example prints all affected file names, counts, and
the first error's source line/byte position. Query status/target baselines are
asserted and should be reviewed explicitly when semantics improve.

A narrow diagnostic experiment on in-memory copies removed only `final` after
`catch (`. It reduced ArrayUtils's 3,041 errors to 54 (one occurrence), ClassUtils's
444 to zero (four occurrences), and ObjectUtils's 429 to zero (one occurrence).
This identified the catch-parameter parsing/recovery fix now implemented; it did
not represent thousands of unrelated defects. The full unmodified corpus improved
from 8,187 to 149 error nodes, and 63 to 28 affected files. All 44 changed file
counts decreased; no file count increased. `catch-parser-comparison.tsv` retains
the exact before/after inventory for every source file. Other remaining errors at that stage
included unsupported `switch` and multiple initializer/update expressions in a
`for` header. The acceptance run always uses the **unmodified** archive bytes.
See [catch parsing boundaries](../../docs/CATCH_PARAMETERS.md).

The following for-header slice removes all 70 diagnostics from four files with
multiple update expressions, while the unrelated contextual-`var` recovery group
increases by six diagnostics. The total is now 85, not 79. The complete per-file
comparison and all source-grounded diagnostic groups are retained in
`for-parser-comparison.tsv` and `for-diagnostic-causes.tsv`. See
[for grammar and recovery](../../docs/FOR_LOOPS.md).

## Measurements

Load/parse/platform decoding and one index preparation are separately timed.
The 15-query corpus is first checked, then repeated five times by default (75
samples per mode). Prepared/rebuild order alternates between rounds. Latencies
include result construction but exclude equality checks and output. p50/p95 use
nearest rank over individual query samples, with total/min/max also reported.
The corpus is heterogeneous; these aggregates are only this fixed workload.

Before the catch-parser follow-up, one Linux cloud release run on 2026-11-09 recorded load/parse 494.47 ms, index
preparation 51.69 ms, rebuild-per-query p50/p95 59.27/72.77 ms and prepared warm
query p50/p95 7.60/16.86 ms. These noisy wall-clock observations are not a speed
promise, comparison against an IDE, stable budget, or general project benchmark.
The harness reports current measurements; it has no timing assertions. No peak
memory, cold filesystem-cache, edit-to-result, cancellation, incremental
invalidation, or medium/large project measurements have been made here.

## CI and independent correctness evidence

This external corpus harness remains separate from default native CI. It is now
also run by the opt-in trusted-main `extended-acceptance` workflow after fixed-
hash input setup. That workflow separately runs a small javac declaration oracle;
its Java execution is development-only and never part of the native analyzer.
Prepared/rebuild equality alone is not a Java semantic oracle. See the exact
[execution and evidence boundaries](../../docs/ACCEPTANCE_PLAN.md).
