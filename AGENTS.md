# java-analyzer-rs contributor guide

This is a standalone Rust library. Cargo package: java-engine. Rust: 1.96.0.
Read README.md, ROADMAP.md, NOTICE, and CONTRIBUTING.md before changing scope.

- Preserve UTF-8 source ranges and explicit UTF-16 conversion at API boundaries.
- Keep syntax recovery lossless; compare incremental edits with full parsing.
- Do not invent resolved results for unsupported or ambiguous Java semantics.
- Preserve source/class-file provenance and caller-context access checks.
- Add focused failing regressions, then run the checks in CONTRIBUTING.md.
- Analysis tests must not depend on JDT LS or a JVM. Optional compiler-oracle
  tests must be clearly separated and must not become runtime dependencies.
- No OpenIDEA, Tauri, or build-system implementation dependencies in the core.
- Preserve license notices, dependency versions, and extraction attribution.
- Do not treat microbenchmark timings as project-scale performance evidence.
