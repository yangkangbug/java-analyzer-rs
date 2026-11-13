# Extraction validation

Validated on 2026-11-09 using Rust 1.96.0 on Linux. This is a source extraction,
not a claim that the engine has replaced JDT LS or implements all of Java.

## Preservation and equivalence

- The original OpenIDEA checkout, branch, and uncommitted changes were retained.
- `src/`, `tests/`, manifest, lockfile, license, toolchain, and format configuration
  were copied without changes. All 22 entries in `extraction-sha256.json` were
  checked against both trees. Documentation, CI, attribution, and the example
  were added or adapted only in the independent project.
- `patches/openidea-java-engine-fixes.patch` captures both pre-extraction fixes
  against OpenIDEA PR #1 HEAD `74abe11aec055635c8fec1c60e8861892b7f90f9`.
  It was checked in reverse against the modified tree and applied forward to
  the original files; resulting bytes matched every changed file.
- The independent Cargo metadata has one package, no workspace/path dependencies,
  and publication disabled. Its locked dependency graph is `cafebabe 0.9.0`,
  `bitflags 1.3.2`, and `cesu8 1.1.0`.

## Tests and checks actually run

- Original PR baseline before fixes: 47 tests passed.
- Receiver regression suite: 8 failed / 2 passed before its fix; all 10 passed
  afterward. It includes a demonstrated wrong-target binding, not only missing
  results.
- Array/varargs signature suite: 9 failed / 2 passed before its fix; all 11 passed
  afterward. It covers ranks, equivalent array/varargs forms, postfix parameter
  dimensions, generic-argument isolation, malformed parameters, and edits.
- Modified OpenIDEA crate: all 68 tests passed with `cargo test -p java-engine
  --locked --offline`; scoped Clippy with `-D warnings`, repository formatting,
  and whitespace checks passed.
- Independent project: `cargo test --all-targets --locked --offline` passed all
  68 tests. The example target also compiled.
- `cargo fmt --all --check` passed.
- `cargo clippy --all-targets --locked --offline -- -D warnings` passed.
- `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked --offline` passed.
- `cargo run --locked --offline --example resolve` produced
  `demo.Value::count in demo/Value.java` and passed its target assertions.
- `cargo package --allow-dirty --locked --offline` built and verified the local
  package. The license, notices, lockfile, binary test fixture, regression tests,
  and example were present in the package file list. Nothing was published.

The first source-tree test downloaded the three locked Cargo dependencies.
Subsequent checks used the cache offline. No JDK or JDT LS was installed or
started for these checks. A separate code review checked both fixes and the
extraction's identities, notices, and dependency boundary.

## Not verified or not implemented

- Whole OpenIDEA workspace build, desktop UI, or production integration.
- Windows/macOS execution or real-project performance.
- Full overload selection, generic inference, JAR/JDK project loading,
  completion, references, rename, or complete Java conformance.
- At extraction, the included GitHub Actions workflow had only been checked
  locally. Remote CI must be checked separately for the exact pushed commit;
  local validation is not a substitute for a successful remote run.

The original one-sample timing test is not a benchmark against IntelliJ or JDT LS.
See ROADMAP.md for project-level milestones and acceptance gates.


## Binary method target details (early API change)

`Target` adds `binary: Option<BinaryTargetIdentity>`; downstream struct literals
must initialize it (`None` for source/non-method targets). `MemberSignature`
adds `raw_access_flags: u16`, preserving original method/field bits. This is a
Rust source compatibility change in the unpublished early crate; the crate
version has not been silently treated as a stability guarantee. Derived target
equality now distinguishes binary method descriptors and declaration contexts.
Do not populate a workspace context by guessing from an origin shared by several
compilations. Default CLI v1 target serialization remains its old projection;
`declaration --target-details` opts into versioned detail. See
[BINARY_METHOD_CALLS.md](BINARY_METHOD_CALLS.md) for scope and conservative
contractions and absent source locations. The later [binary-result proof](BINARY_CALL_RETURNS.md)
adds bounded result composition without changing target serialization.

## Raw binary ancestor identity

`ClassSignature` now retains `binary_super_class: Option<String>` and
`binary_interfaces: Vec<String>` in original class-file order. These are JVM
internal names, preserving `/` and `$`; existing `super_class` and `interfaces`
remain dotted projections for compatibility with readers. Code constructing a
`ClassSignature` literal must supply both new fields consistently with the dotted
fields. Prefer `parse_class_bytes` for actual class-file inputs.

Strict reference proofs require the selected ancestor's original binary name to
match the recorded edge. Source replacements use their package and nesting to
construct the corresponding JVM name. Missing or ambiguous raw identities refuse
the proof rather than substituting a same-dotted-name declaration. Exact Object
root evidence requires `java/lang/Object`, not just its dotted spelling. This
corrects previously unsupported success for a missing `Outer$Inner` ancestor
replaced by the unrelated package-level `Outer/Inner`. It does not add nested-type
selection, inherited binary calls, generic substitution or a persistent identity.
