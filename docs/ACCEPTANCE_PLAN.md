# Acceptance boundaries and next validation work

## What runs today

The default CI runs repository unit/integration tests, compile-fail documentation,
formatting, lint, docs, examples, and the checked-in native-runtime smoke script.
Its fixtures require no Java/JVM executable. The full checksum-pinned Commons Lang
source/JMOD harness is **optional and currently run locally**, not part of default
CI. A green CI badge does not mean that external corpus ran remotely.

Prepared-versus-rebuild equality verifies consistency of two query paths that
share resolver logic. It is not an independent Java semantic oracle. Fixed
source-target assertions and the small hand-authored lexical-scope oracle add
independently reviewed expectations, but cover only selected facts. Error-node
counts and lossless text coverage likewise do not establish language conformance.

## Next isolated validation slice (not implemented here)

1. Add a separate opt-in real-corpus CI job. Fetch the exact published sources
   archive and official platform distribution with pinned URLs/SHA-256 values;
   verify before extraction and extract only source text and JMOD data. Cache by
   digest, keep artifacts temporary, and run the compiled native analyzer without
   Java in its environment. Do not silently substitute a preinstalled JDK or an
   unpinned upstream branch. Clearly distinguish skipped, download-failed, and
   analyzed results. Licensing/provenance must remain explicit.
2. Version a small independently reviewed Java acceptance project and expected
   declaration bindings/diagnostics for the supported subset. Cover positive,
   negative, shadowing, overload-refusal and incomplete-input cases. Review
   expectations separately from implementation and preserve every changed
   expectation with its reason rather than updating it automatically to match.
3. Evaluate an optional development-only compiler oracle using a pinned JDK's
   compiler/tree APIs to report declaration bindings or diagnostics on complete
   valid fixtures. Keep it in a separate job/tool boundary with an explicit Java
   requirement. It must never become an analysis-library, production-runtime, or
   mandatory native-test dependency. A compiler oracle also needs exact language
   level/classpath and cannot stand in for malformed editor-buffer recovery tests.
4. Compare the native output against that independent evidence and report
   unsupported results separately. Do not claim correctness simply because two
   implementations agree, because compilation succeeds, or because one fixed
   source library has no parser diagnostics.

This plan intentionally does not change CI/network/runtime requirements during
the for-header fix. The next implementation should verify costs, artifact access,
license handling and precise oracle scope before making it a regular gate.
