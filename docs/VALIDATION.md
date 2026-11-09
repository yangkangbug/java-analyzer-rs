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
