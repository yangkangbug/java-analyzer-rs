# Contributing

Use Rust 1.96.0 and the checked-in lockfile. Reproduce behavior with a failing
regression test before changing the implementation. Preserve unresolved or
ambiguous results when the engine cannot justify a unique target.

Run from the project root:

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --locked
# POSIX: crash-containment tests intentionally abort a child; disable core dumps.
ulimit -c 0
cargo test --all-targets --all-features --locked
cargo test --doc --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --locked
cargo run --locked --example resolve
cargo run --locked --example project
cargo run --locked --example scopes
cargo build --locked --examples
sh scripts/check-native-runtime.sh
cargo build --locked --features cli --bin java-analyzer
sh scripts/check-cli-runtime.sh
```

Document what is unsupported. Passing this test suite is not evidence of full
Java conformance or a performance comparison with IntelliJ or JDT LS. Keep
license notices and source provenance when copying or adapting code.

Do not add OpenIDEA/Tauri dependencies to the analysis core. Put editor and
build-tool integrations behind explicit adapter boundaries. See ROADMAP.md.

The runtime script is a Linux/POSIX check. Its optional local Commons Lang JAR
argument runs the pinned real-artifact check described in docs/VALIDATION.md.
Do not add Java tools to the mandatory test or analysis runtime requirements.

The optional `cli` feature adds only the executable's JSON adapter dependencies.
Test both the default library build and the feature-enabled executable. The CLI
subprocess tests and runtime script do not invoke Java; parser reports are not
semantic diagnostics. See [CLI contract](docs/CLI.md) for the experimental boundary.
Execution supervision tests include deterministic worker failure/timeout fixtures
and real native-binary checks. Test-only arbitrary child commands must remain
under `cfg(test)`; the production supervisor runs only its own native executable.
Keep wall-clock/capture tests distinct from OS sandbox, memory or CPU quota claims.

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

The external corpus and javac comparator run in the separate optional
`extended-acceptance` workflow, not default native CI. Keep these statuses distinct.
See [execution boundaries and reproduction](docs/ACCEPTANCE_PLAN.md). Fetch-security
unit tests are offline Python tests; only explicit input setup downloads archives.
Only the optional comparator runs Java. It may compile explicit handwritten
dependency fixtures into temporary class files, but must never execute fixture
programs or make those files a requirement of native Cargo tests.
The strict-reference comparator separately verifies its pinned JDK archive and
installed tool bytes before optional compiler execution. Its exact compiler
targets, native selections and intentional native refusals remain distinct.
The real-Gson strict-reference gate uses only the native executable and pinned
source/platform data; it does not compile or execute Gson.

Preview tooling has offline standard-library tests: `python3 -m unittest discover
-s scripts -p "test_*.py" -v`. Packaging builds only this repository with the pinned
locked Rust toolchain, then tests its trusted local artifact. Do not execute
external project build scripts or applications during input setup or runtime
acceptance. Keep fixed network input setup separate from offline analysis and
measurements; record unsupported outcomes, not a success-only sample. See
[preview evaluation](docs/CLI_PREVIEW.md) and [installation](docs/CLI_INSTALL.md).

The separate [native macOS gate](docs/MACOS_NATIVE.md) runs on standard hosted
Apple Silicon and Intel machines. It tests native source builds and CLI behavior,
not Mac packaging or distribution. Its offline checker regressions are included
in the Python discovery command above. Linux-only packaging, resource measurements
and input-publication tests remain in Linux CI; do not silently relax their
platform or no-replacement guarantees to make the Mac job green.

The [definition-only editor adapter](docs/LSP_PREVIEW.md) has native broker and
private-job tests in the feature-enabled Cargo suite. Keep its framing, queue,
child cleanup and cross-buffer invalidation tests separate from semantic proofs.
Real Neovim acceptance uses the checksum-pinned optional `editor-acceptance`
workflow, not a network dependency of native Cargo tests. Its isolated headless
client runs without user configuration or plugins; it must never use a user's
computer. Input setup, offline analysis, real client navigation and optional javac
comparison remain separately identified gates. A successful handwritten protocol
test is not evidence that a real editor navigates correctly.

The explicit refresh checker uses the same pinned client and bounded test runner.
It verifies session retirement and full unsaved-buffer replay separately from
native resolver semantics. Keep initialization/cleanup failures, reentrant client
events and process-overlap controls in the real editor suite, and keep adversarial
report mutations in offline Python discovery. See [the refresh contract](docs/EDITOR_REFRESH.md).

The supplemental Linux [LSP reload measurement](docs/ARCHIVE_REUSE.md) compares
two already-built release binaries using pinned local inputs. Its offline tests
are included in Python discovery. Run actual timings serially without builds or
other benchmarks, retain failed evidence and exact target checks, and never turn
these observations into an invented CI performance threshold.
