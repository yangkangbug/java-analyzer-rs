# Contributing

Use Rust 1.96.0 and the checked-in lockfile. Reproduce behavior with a failing
regression test before changing the implementation. Preserve unresolved or
ambiguous results when the engine cannot justify a unique target.

Run from the project root:

```sh
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
cargo test --doc --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
cargo run --locked --example resolve
cargo run --locked --example project
cargo run --locked --example scopes
cargo build --locked --examples
sh scripts/check-native-runtime.sh
```

Document what is unsupported. Passing this test suite is not evidence of full
Java conformance or a performance comparison with IntelliJ or JDT LS. Keep
license notices and source provenance when copying or adapting code.

Do not add OpenIDEA/Tauri dependencies to the analysis core. Put editor and
build-tool integrations behind explicit adapter boundaries. See ROADMAP.md.

The runtime script is a Linux/POSIX check. Its optional local Commons Lang JAR
argument runs the pinned real-artifact check described in docs/VALIDATION.md.
Do not add Java tools to the mandatory test or analysis runtime requirements.

Platform regressions in `tests/platform.rs` and archive regressions in
`tests/jmod.rs` construct class/archive bytes in Rust; no javac or installed JDK
is required. The platform example is built in CI, but running it with real JDK
data is optional: `cargo run --locked --example platform -- /path/to/java.base.jmod`.
Treat the supplied JMOD strictly as data. Never execute Java/JMOD tools or load
native library sections to perform analysis. Record actual-artifact checks
separately from mandatory tests and do not commit a JDK binary.

After `cargo build --locked --examples`, run
`sh scripts/check-platform-runtime.sh /path/to/java.base.jmod` for the optional
platform acceptance check with an empty environment and no executable search path.

Prepared-handle compile-fail documentation is part of `cargo test --doc`. The
optional [real-source acceptance harness](examples/real_source/README.md) uses
checksum-pinned local artifacts and records parser gaps and timings without
performance thresholds. It must not fetch inputs or invoke Java.

The full external real-source harness is not currently part of default CI. Keep
its results separate from CI status. See [acceptance boundaries and next work](docs/ACCEPTANCE_PLAN.md).
