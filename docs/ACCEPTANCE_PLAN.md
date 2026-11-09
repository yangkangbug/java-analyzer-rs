# Independent acceptance: current gates and remaining work

## Separate execution boundaries

The default `ci` workflow runs repository tests, compile-fail documentation,
formatting, lint, docs, examples, and the checked-in native-runtime smoke script.
Those tests require neither a JDK nor external Java artifacts. After the pinned
Rust toolchain and locked Cargo dependencies are available, they can run offline.
Initial CI toolchain/dependency installation still uses the network.

The separate `extended-acceptance` workflow performs explicit network setup,
real-source analysis, and an optional development-only compiler comparison. It
runs manually on main or on trusted main pushes changing its workflow, input
manifest, validation scripts, fixtures, or native acceptance examples. It does
not run for pull requests or arbitrary branches, and core-only edits do not
currently trigger this extended workflow automatically. A default CI success
must not be presented as success of the external corpus/compiler checks.

This workflow uses read-only repository permissions, disables persisted checkout
credentials, pins official actions to commit SHAs, selects Rust 1.96.0, limits
runtime/concurrency, and uploads no upstream archive/JDK data. It does not use
`pull_request_target` or repository secrets. Archive and toolchain pins do not
make the entire hosted runner/dependency graph hermetic or byte-reproducible.
Network/setup failure fails explicitly; skipped or absent extended runs are not
acceptance evidence.

## Fixed inputs and safe preparation

`scripts/acceptance-inputs.json` records official URLs, SHA-256 hashes, compressed
sizes, extraction bounds, and license/provenance for Commons Lang 3.17.0 sources
and Temurin 21.0.13+9 Linux x64. The fetcher rehashes digest-addressed cache entries,
requires HTTPS, bounds downloads/decompression/file counts, rejects unsafe or
duplicate paths and special files, and rejects symlink ancestors of local paths.
Internal archive links are validated and materialized as regular file copies.
Fresh staged output is published only on successful validation.

Default preparation extracts only `java.base.jmod` and legal notices. Compiler
executables require `--include-oracle-jdk`; preparation itself never runs Java.
Both source and JMOD hashes are checked again by the real-source wrapper. The
analyzer runs with only `PATH=/nonexistent` in its environment. The host may still
have Java installed; this is not a claim of a physically JVM-free machine.

Commons Lang is Apache-2.0. Temurin/OpenJDK is GPL-2.0 with Classpath and assembly
exceptions plus component-specific notices. Complete provided legal files are
retained with local inputs. Archives, upstream sources, and the JDK are not
committed, bundled in the source ZIP, or uploaded as CI artifacts. They remain
external validation inputs, not Cargo or production runtime dependencies.

## Reproduce

Build the Rust acceptance endpoints first. Python 3.11+ is needed only for optional
validation/setup scripts. Linux x64 is the pinned compiler distribution's target.
Output directories must not already exist; cache entries are reverified.

```sh
cargo build --locked --example real_source --example oracle_bindings
python3 -m unittest discover -s scripts -p 'test_fetch_acceptance_inputs.py' -v
python3 scripts/fetch-acceptance-inputs.py --cache /tmp/java-acceptance-cache --output /tmp/java-acceptance --include-oracle-jdk
python3 scripts/check-real-source.py /tmp/java-acceptance/sources.jar /tmp/java-acceptance/java.base.jmod --profile debug --rounds 1
python3 scripts/check-semantic-oracle.py --jdk /tmp/java-acceptance/jdk --native target/debug/examples/oracle_bindings
```

For no-network setup after filling the cache, pass `--offline` and a fresh output
directory. Omit `--include-oracle-jdk` when only corpus analysis is needed. The
comparator downloads nothing and executes only the supplied compiler/runtime.
Its version check is not a cryptographic verifier: use the verified extraction
flow rather than an arbitrary ambient JDK.

## Independent compiler evidence

The three-file handwritten fixture and expectations are versioned under
`dev-tools/oracle`. javac `Trees.getElement` selects each referenced declaration,
then `Trees.getPath` supplies its actual declaration tree and range. Compiler
UTF-16 positions are converted to UTF-8 bytes, including an astral-character
case. The wrapper associates the actual compiler tree with a unique declaration
marker and checks the native target's path, kind, name, owner convention and exact
name-token range. Expectations are not generated from native output.

Eleven supported bindings are checked against this independent evidence. Two
separate cases verify explicit refusal boundaries: javac selects an overload and
resolves `super.inherited`, while the native engine respectively reports
`OverloadNotResolved` and `ReceiverNotBound`. These are not semantic-equivalence
passes. Missing-name/incomplete compiler failures, native missing-name behavior,
wrong-golden and corrupted-compiler evidence, and optimized-Python rejection are
negative controls. Both compiler stages disable annotation processing and
implicit compilation, use UTF-8 and release 21, and use empty controlled
classpath/sourcepath. Fixture programs are analyzed, never generated or executed.
Only the development comparator step executes Java. The Rust endpoint does not.

The oracle exposed a real defect: a shadowed `super` field incorrectly resolved
to the subclass field. A conservative syntax-receiver guard now returns no target
instead of silently dropping `super`. Full super binding is still unimplemented.
`this`, ordinary inheritance, locals/parameters, and qualified type-prefix lookup
have separate regression coverage across legacy/prepared/workspace APIs.

## Remaining coverage limits

The full real-source workload still has known parser/semantic gaps. Its default
fifteen fixed queries and error-node inventory are not whole-project semantic
coverage. Prepared/rebuild agreement proves consistency, not Java correctness.
The small independent oracle does not cover generic inference, overload selection,
platform bindings, JPMS, complete access control, malformed-buffer equivalence,
or general Java conformance. A compiler oracle cannot replace editor recovery,
cancellation or incremental invalidation tests.

Next steps are broader independently reviewed fixtures, complete diagnostic
comparison, explicit unsupported-result accounting, additional versioned real
projects, and an intentional decision about making extended gates required or
triggering them on every core change. Compiler/Java requirements must stay outside
the production/native analysis core and default native test path.
