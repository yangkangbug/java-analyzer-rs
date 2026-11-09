# Minimal usable preview gate

This is a proposed delivery gate, not a completion date or a claim that every
item below is delivered. An [experimental CLI](CLI.md) now exercises the
configuration/declaration/report path; the hardening and editor gates remain open.
The first vertical path is: configure an ordinary project without writing Rust,
query a source position, receive an exact source declaration or explicit limit,
and inspect clearly labeled parser gaps. It does not replace all JDT LS features.

## Existing foundation

Explicit source roots, ordered binary dependencies, platform JMOD/JAR data and
compilation/source-set relationships already load into native immutable snapshots.
Prepared indexes, declaration provenance, UTF-8/absolute-UTF-16 helpers and parser
error ranges exist. The optional native CLI adds strict versioned project and
query documents with JSON results. The fixed real-source and optional compiler
gates provide bounded evidence, not a language-conformance suite. A first
[primitive source-call proof](PRIMITIVE_CALLS.md) now improves four real project
queries and refuses known unique-source contradictions. General call applicability
and the remaining end-to-end gates below are still open.

## Offline CLI preview: required acceptance

1. A versioned project manifest exposes compilation IDs, source roots, ordered
   dependencies, explicit platform data, target JAR release and resource limits.
   Paths are relative to the manifest; invalid inputs name the offending path.
   Manual configuration is explicit. No hidden Maven/Gradle/JVM import fallback.
2. A product executable provides project inspection, declaration lookup and parser
   reporting, with machine-readable results and documented exit/error statuses.
   Repeated queries reuse a prepared snapshot. The existing inspect_project
   example is only a starting point, not this delivered interface.
3. Source targets identify the exact declaration and snapshot/context. Refused or
   ambiguous results expose candidates without choosing the first. Unique-name
   navigation is not advertised as proven argument applicability. Binary targets
   retain origin/identity and say source is unavailable, rather than opening 0..0
   as a fabricated source range. Source attachment/decompilation can wait.
4. Parser limitations are separated from proven Java errors. There is no claim of
   type, unresolved-name or overload semantic diagnostics. Unsupported grammar
   must not be presented as evidence that a user's otherwise valid source is bad.
5. Reload observes changed sources, dependencies and graph edges. A failed load
   does not replace a usable snapshot. UTF-8 and UTF-16 position conversion has
   explicit bounds and supplementary-character/CRLF coverage.
6. Bad input cannot indefinitely freeze the host. Existing byte/archive limits
   are supplemented by bounded worker/process execution, timeout/cancel handling
   and recoverable failures. Recursive parser/tree work is not bounded merely by
   an input-byte limit. This does not require a full incremental query engine.
   The CLI now has [native worker supervision](CLI_EXECUTION.md), an execution
   deadline, bounded protocol capture and optional stdin-EOF cancellation. This
   isolates worker failures, not all host resource exhaustion: memory limits,
   blocked caller-output handling and core cooperative cancellation remain open.
7. Test the shipped interface on the fixed Commons Lang workload and one distinct
   project shape, including source/test isolation, missing dependency, source edit,
   malformed buffer and reload failure. Report supported and refused cases.
8. Measure the actual release executable: load/preparation, cold/warm query latency,
   peak RSS and reload latency, recording revision, machine and input checksums.
   Set initial budgets from those results; no inherited microbenchmark promise.
9. Deliver a documented binary/install route and checksum for a declared platform,
   with clean-environment smoke checks. Validate that it runs without Java/JAVA_HOME
   and has no JVM process or library dependency. Optional compiler tests remain
   development-only. Current CI coverage is Linux, not cross-platform certification.

## Additional gate for an editor-connected preview

Add a deliberately small protocol/adapter: initialize, open/change/close, definition
and parser-report delivery. Unsaved overlays, snapshot revisions, stale-result
rejection, off-UI execution and cancellation/restart are required. Whole-buffer
UTF-16 helpers are not an LSP line/column implementation. Validate rapid edits,
CRLF/astral characters, dependency changes, cancellation and reopen behavior.

Persistent symbol IDs are not a prerequisite for a one-shot CLI, but results that
outlive a request must belong to a known revision/context. Reference search, rename,
completion, semantic diagnostics, generic inference and full overload selection
remain later capabilities, each with its own correctness gate.
