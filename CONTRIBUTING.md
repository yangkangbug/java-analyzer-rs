# Contributing

Use Rust 1.96.0 and the checked-in lockfile. Reproduce behavior with a failing
regression test before changing the implementation. Preserve unresolved or
ambiguous results when the engine cannot justify a unique target.

Run from the project root:

```sh
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
cargo run --locked --example resolve
cargo run --locked --example project
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
