# Minimal usable preview gate

This is a proposed delivery gate, not a completion date or a claim that every
item below is delivered. An [experimental CLI](CLI.md) now exercises the
configuration/declaration/report path. A definition-only editor slice now adds
full-text synchronization; the broader hardening and usability gates remain open.
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
and the remaining end-to-end gates below are still open. Proven primitive
call results now compose through nested calls; this improves definition evidence
without closing any delivery gate by itself.
The separate [reference instance-call proof](REFERENCE_CALLS.md) adds two exact
Gson overload targets with complete hierarchy/access evidence and explicit
refusal boundaries. This improves the semantic path without certifying general
project support or OpenIDEA client integration.

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
   development-only. Native hosted Linux and Mac checks are not cross-platform
   distribution certification; consult exact successful runs and platform limits.

## Additional gate for an editor-connected preview

The first [small LSP adapter](LSP_PREVIEW.md) supports initialize, open/change/close,
definition, cancellation and shutdown. Parser reports remain in the CLI, not
editor diagnostics. Unsaved overlays, snapshot revisions, stale-result
rejection, off-UI execution and cancellation/restart are required. Whole-buffer
UTF-16 helpers are not an LSP line/column implementation. Validate rapid edits,
CRLF/astral characters, dependency changes, cancellation and reopen behavior.

Persistent symbol IDs are not a prerequisite for a one-shot CLI, but results that
outlive a request must belong to a known revision/context. Reference search, rename,
completion, semantic diagnostics, generic inference and full overload selection
remain later capabilities, each with its own correctness gate.

## Delivery-evaluation slice

[Linux installation and local archive verification](CLI_INSTALL.md) and the
[fixed two-project/resource protocol](CLI_PREVIEW.md) provide concrete ways to
exercise this CLI. Gson adds a real core/extras main/test shape, with all selected
files and predeclared refusal cases retained. Its substantial parser gaps remain
a readiness limit, not a successful full-project compatibility claim.

The one-shot commands are not persistent editor sessions; `lsp` is a separate
entry point with fresh supervised analysis per query. Distribution checks cover
Linux x86_64 GNU only. Native hosted Mac source builds are tested separately;
Mac/Windows distribution and a broader glibc matrix remain untested.
Measured resource observations do not by themselves establish latency/memory
budgets or certify arbitrary source inputs.

## Unsaved-buffer foundation

[Whole-text overlays](CLI_OVERLAYS.md) and strict UTF-16 line/character queries
provide the first unsaved declaration/parser-report path. The supplied overlay
set is complete for one invocation; omitting a closed document on the next
invocation reads disk again. It is not a persistent open/change/close protocol.
Caller versions label results but do not implement stale-result rejection; an
adapter must compare its current whole workspace generation before accepting them.

The immutable library update preserves old snapshots/prepared views and avoids
binary I/O, but clones inputs and rebuilds prepared indexes. The one-shot CLI
still reloads disk/binaries on every invocation. Native macOS architecture gates
are defined separately; only exact successful CI runs prove their tested cases.
These foundation tests alone imply no signed distribution, supported OS matrix or
real editor application. The [Neovim acceptance plan](../editors/neovim/ACCEPTANCE.md)
defines the separate real-client navigation and late-response checks. Existing
CLI resource measurements do not establish this new adapter's latency or memory.
