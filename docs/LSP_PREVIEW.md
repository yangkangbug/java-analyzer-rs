# Bounded definition-only LSP preview

This acceptance contract is fixed before implementation. The intended interface is
`java-analyzer lsp --manifest FILE`, using standard LSP 3.17 stdio framing. It is an
experimental source-navigation adapter, not a replacement for JDT LS or a claim of
general Java correctness. The existing one-shot CLI remains compatible.

## Scope and process boundary

Support initialize/initialized/shutdown/exit, full-text didOpen/didChange/didClose/
didSave, definition and cancellation. Advertise UTF-16, Full synchronization and
definition only. Existing CLI parser-report remains available; this server does
not publish semantic diagnostics or add custom parser-report/reload methods.

The persistent broker owns protocol and buffer state only. Java parsing, loading,
preparation and resolution run through separate one-shot native supervisor/worker
processes. Only this executable can be launched; there is no arbitrary command,
shell, JVM, build tool, network client or environment-selected worker. The existing
one-shot supervisor is not reused as an in-process persistent-host API.

There is one active analysis job and at most eight queued requests. Frozen job
inputs cannot be overwritten while a child is using them. Cancellation closes the
supervisor's stdin lease. An uncertain process cleanup stops the session instead
of allowing replacement processes to accumulate. These are reliability measures,
not OS memory/CPU quotas, a security sandbox or hard-real-time guarantees.

## Source and configuration identities

Initialize uses a native inspection job to validate the explicit manifest and
return its normalized frozen configuration, original canonical manifest identity
and unique canonical source owners. Relative inputs retain their original base.
Definitions consume that frozen configuration rather than rereading a mutable
manifest. Changing configuration or the loaded source inventory requires a new
client/server session. There is no automatic import, watcher or transactional
reload, and no cached semantic snapshot survives a failed one-shot disk load.

The shipped Neovim client provides [serialized explicit restart](EDITOR_REFRESH.md):
retire the old session safely, initialize a replacement, then replay all supported
loaded buffers before enabling navigation. This does not change the server's
frozen-ownership contract or introduce a reload request. Forced or uncertain
retirement fails closed rather than accumulating replacement sessions.

Definition queries require an open, synchronized document: didOpen captures its
complete text, even when it equals disk. A closed owned query receives an explicit
error; reopen it to query again. Closed target declarations remain navigable, and
closing a target buffer exposes its disk text on the next job.

Only configured existing files and local file URIs are supported. URI conversion
must preserve percent-encoded characters and Unicode. A canonical source has one
owner, never the first visible compilation or an inferred folder-based context.
Aliases share identity; simultaneous opens of different URIs for the same source
desynchronize it. Closing a rejected alias cannot silently close the original URI.
Non-file schemes, remote authorities, query/fragment components and invalid paths
are unsupported. Initial execution evidence is Linux; hosted platform checks are
reported separately.

## Acknowledged document admission (experimental version 1)

Standard clients may continue using ordinary didOpen notifications. A client that
needs to admit only files selected by the initialized snapshot can instead use one
bounded request per candidate. Initialization advertises:

```json
{"experimental":{"javaAnalyzer":{"documentAdmission":{
  "version":1,"method":"javaAnalyzer/admitDocument"
}}}}
```

The request is `javaAnalyzer/admitDocument`, with these parameters:

```json
{"expectedCanonicalPath":"/project/src/A.java","textDocument":{
  "uri":"file:///project/src/A.java","languageId":"java","version":-1,
  "text":"class A {}"
}}
```

The result is an exact receipt, with no inventory export or inferred owner:

```json
{"status":"admitted","uri":"file:///project/src/A.java",
 "canonicalPath":"/project/src/A.java","version":-1}
```

`status` is either `admitted` or `not-selected`. An admitted receipt means the
shared open transition enrolled that exact URI and signed-i32 version. A valid
candidate absent from the frozen canonical path-to-owner inventory returns
`not-selected`, with the same receipt fields and no state mutation or enrollment.
Excluded, outside-root and new-since-initialization paths all use that same
negative outcome. Candidate full text has already been transmitted to the local
engine when it rejects selection; this is **not** a zero-transmission boundary.
Clients must not subsequently synchronize or query rejected candidates.

The request requires initialization and the initialized notification. URI and
expectedCanonicalPath are each bounded to 8,192 UTF-8 bytes; the expected path
must be absolute and contain no control characters. Java languageId, full text
and signed-i32 version are validated even for a negative outcome. The broker
resolves the URI once, verifies the exact expected canonical path before any
mutation, and passes that resolved identity to the shared open transition. Only
open resolves filesystem paths; later changes and closes retain exact registered
URI identity even if the path is deleted or a symlink retargeted. Canonicalization
performs filesystem I/O and provides no hard latency bound. This does not detect
same-path inode/content replacement or create an atomic disk snapshot.

Malformed parameters, unresolved/vanished paths, identity mismatches, duplicate
URI/alias conflicts and overlay/document limits return JSON-RPC error -32602,
never `not-selected`. Lifecycle errors retain -32002/-32600. Some open failures
change/desynchronize state; this is an acknowledged transition, **not** a promise
of rollback on all errors. Such mutations publish the new workspace generation
and obsolete queued/active definitions exactly like ordinary sync notifications.
No additional analysis, traversal, subprocess, ownership rule or reload is added
by the admission request. Definition workers still reload the frozen manifest and
reject a changed source inventory. Source/configuration changes require restart.

Admission is handled synchronously by the bounded broker, not by its definition
queue. Once dispatched it cannot be cancelled or rolled back by a later
$/cancelRequest. A cancellation received before dispatch does not reserve a future
request ID. Exactly one terminal response is sent unless the transport/session
fails; cancellation after the response adds no second response. A timeout, lost
response or uncertain connection does not establish whether enrollment occurred.
An admission-dependent client must fail closed/retire the session on uncertainty,
errors or invalid receipts, rather than assume rejection or blindly retry. It must
check this exact capability version and method and never silently fall back to
bulk didOpen enrollment. Correlation and readiness still require client-local
session/document-incarnation checks and exact URI/path/version receipt checks.

The method is request-only; its notification form is ignored and cannot enroll a
document. Sending standard didOpen as a request remains invalid and stops the
session. Existing body/header/overlay/output, queue and writer-stall bounds apply
unchanged. Extra standard extension fields remain allowed.

## Buffers and revisions

Every accepted state transition has a monotonic whole-workspace generation.
Open/change/close/save, synchronization failures and recoveries invalidate results
from older generations, including requests made in a different source file.
Document identity includes the session and open incarnation, not just its path or
version. LSP document versions are signed i32 and must increase within an open
incarnation; internal nonnegative overlay labels are separate.

Full synchronization accepts full replacements in order. Incremental ranges,
nonincreasing versions, conflicting opens and limit violations must not silently
retain queryable old text. A desynchronized owned source blocks analysis until a
valid unambiguous full replacement or a complete close/reopen repairs it. Closing
removes the overlay and the next job loads disk. Source files are never written.

Each definition captures the complete overlay set. Check its generation before
starting work, after the worker returns and before committing output. Obsolete
work yields null plus a bounded explanatory log, which is this preview's policy,
not a standardized stale-result signal or proof that no definition exists. Actual
client cancellation receives one RequestCancelled response when observed first.
Do not misuse ContentModified or ServerCancelled for ordinary definition edits.

The shipped client must additionally gate navigation by its local whole-workspace
generation/session: a server cannot retract a response already sent before it
learns about an edit. Before requesting a definition, synchronize every supported
dirty buffer, including dependency buffers. A default client smoke alone does not
prove this stronger stale-navigation guarantee. Unobserved external disk changes
remain outside the revision contract; disk loading is not atomic.

## Definitions and positions

Return one source Location only for Resolved with exactly one verified source
target. Return null with a reason for ambiguous/refused/binary/package outcomes;
never navigate to the first candidate or invent binary source coordinates.
Resolved retains the engine's limited binding guarantee, not general method
applicability, full accessibility or successful compilation.

Worker output computes UTF-16 line ranges from the exact analyzed ParsedFile.
The broker must not reread a closed target file to convert byte offsets. Preserve
the old CLI's byte/absolute-UTF-16 range schema unless the editor job is selected.
LSP overlong character positions clamp to the line end only at the editor
boundary; invalid lines/surrogate interiors are refused. A valid EOF query has no
definition. Strict core and existing CLI position semantics stay unchanged.

## Transport limits and failures

Initial limits: 8 KiB headers, 2 MiB incoming JSON bodies, 16 MiB outgoing bodies,
1 MiB serialized aggregate overlays, 1,024 open URIs, 8 KiB per document URI,
256 UTF-8 bytes per string request ID, one active job and eight queued jobs. The native worker retains its existing 16 MiB stdout/1 MiB stderr
capture and bounded execution/cleanup contract. Bounded transport queues and a
writer-stall watchdog must prevent a nonreading client from accumulating output
or preventing process cleanup. Saturation is a session failure, not permission to
drop terminal responses and continue. Framing loss is terminal. All stdout bytes
must belong to LSP frames; human logs go to stderr or bounded LSP log messages.

Accept standard extension fields rather than imposing the CLI's unknown-field
policy on normal LSP clients. Enforce JSON-RPC 2.0, signed integer/string IDs,
proper byte Content-Length, valid UTF-8 and typed known parameters. Handle EOF,
unexpected exit, cancellation races and graceful shutdown without indefinite
joins. Private request files use exclusive creation and are cleaned after known
completion/cancellation/shutdown. Abrupt process death may leave temporary files;
never log source text or claim secure erasure.

## Frozen acceptance matrix

1. Native protocol tests: initialize ordering, capabilities, shutdown/exit, unknown
   requests, string/integer IDs, full replacements, negative/nonconsecutive
   versions, stale versions, malformed notifications, exact URI close identity,
   aliases, desynchronization recovery and unchanged files.
2. Framing tests: fragmented/coalesced messages, Unicode byte lengths, optional
   Content-Type, duplicate/missing/oversized Content-Length, header/body bounds,
   malformed JSON, EOF mid-frame, saturated queues and blocked stdout.
3. Native analysis tests: cross-file unsaved definitions and exact target ranges,
   CRLF/lone CR/astral positions, end-of-line normalization, invalid positions,
   source/test ownership, binary/refused/ambiguous results and failed disk loads.
4. Deterministic test-only job barriers: dependency edits while a request runs,
   close/reopen, explicit cancellation, cancellation storms, queue saturation,
   timeout/worker failure and uncertain-cleanup stop. No production fault command
   or environment variable may select arbitrary workers.
5. Actual pinned Neovim headless: use real buffers and built-in LSP sync; test the
   shipped wrapper's all-buffer synchronization and local generation guard,
   including immediate dependency edit then query and delayed response delivery.
   A Microsoft protocol-library harness is optional supplemental evidence, not
   a VS Code extension-host or GUI test.
6. Run the full CONTRIBUTING checks and existing CLI compatibility tests. Record
   exact source/binary/client identity. Do not equate fresh per-request reload
   timing with incremental query latency or promise an IDE performance budget.

## Implemented verification boundaries

`tests/cli_lsp.rs` exercises the actual server binary with an empty environment,
including lifecycle, EOF, cancellation/ID reuse, invalid framing and a client that
never reads stdout. On Linux, an additional real-source workload observes seven
separate supervisor/worker pairs through procfs, cancels six jobs and closes stdin
with the seventh running. It checks termination of the observed process identities
and that broker capture-reader threads do not accumulate. An observed zombie is
terminated, not proof that an unrelated system reaper has collected it.

The broker's unit-test-only job barriers separately prove scheduler ordering,
whole-workspace dependency edit/close/reopen invalidation, queue saturation and
request-ID reuse while old cleanup is pending. Writer barriers check the final
queued-response generation guard and bounded backpressure. Those in-memory
fixtures do not prove OS descendant cleanup. There are no production fault hooks,
arbitrary worker paths or environment-selected test commands.


## Explicit source selection

[Source exclusions](SOURCE_SELECTION.md) are applied when the disk snapshot is
loaded. An excluded file is not an owned source; an overlay or didOpen cannot add
it back. Frozen LSP manifests and source inventories do not change when the
configuration file changes. Restart the session after changing exclusions.
Clients that need selection acknowledgments can use the versioned per-document
admission request above instead of sending candidate didOpen notifications.
Unknown-document didChange still desynchronizes the session and blocks navigation;
it must not silently accept dirty text from outside the selected inventory.
