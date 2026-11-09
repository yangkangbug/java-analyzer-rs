# Narrow LSP navigation acceptance

The source baseline is fef343e9ff0a9b562079a0565b838c8f66dd4084. This plan is
written before implementation and real-client acceptance. Preserve the old CLI
schema, strict positions and all existing source/project/semantic gates.

## Surface and deliberate limits

Provide a standard stdio LSP initialization/full-text document lifecycle and source
definition path, plus request cancellation and clean shutdown. Use one explicitly
configured manifest. Configuration changes require a client/server restart in
this slice. Do not advertise completion, references, rename, hover, incremental
sync, semantic diagnostics, automatic project import or persistent semantic caches.
The CLI remains the explicit parser-report entry; no error squiggles are implied.

The broker must not load or parse Java on its scheduling thread. Loading and
analysis run in supervised native child processes. Frozen configuration preserves
its original path base. Definition line ranges come from the same analyzed source
snapshot, never from a later disk reread. Fresh jobs still read sources/binaries;
this does not retain a usable semantic snapshot after failed disk loads.

## Native protocol and execution gates

- Correct Content-Length byte framing with fragmented/coalesced frames, strict
  finite headers/body sizes, JSON-RPC integer/string IDs and lifecycle ordering.
- Full-text synchronization, negative/nonconsecutive signed document versions,
  duplicate/nonincreasing versions, close/reopen and aliases. Malformed known
  document updates must not silently leave stale text usable. Recovery is explicit.
- One whole-workspace generation, immutable per-job full buffer capture, source
  ownership, generation check before output and at most one terminal response.
- Proper LSP character-past-line-end normalization at the editor boundary, without
  weakening strict CLI positions. Invalid lines/surrogate interiors remain
  explicit; EOF does not invent a definition.
- Exact source-only navigation. Binary/ambiguous/unsupported targets return no
  source location. A newer ordinary document update may obsolete a result, but
  must not misuse ContentModified or ServerCancelled. Real client cancellation
  and shutdown must terminate/reap work within the declared reliability bounds.
- Saturated queues, cancellation storms, worker crash/timeout, malformed child
  output, EOF, blocked client stdout and uncertain cleanup cannot create unlimited
  children/threads or silently drop a still-accepted request's response.

These are reliability limits, not CPU/RSS quotas or an OS security sandbox.

## Genuine editor gate

Use official checksum-pinned Neovim headlessly, with its real built-in LSP client
and repository-owned Lua adapter. Disable user config/modelines/plugins and use
private test directories. No user computer is accessed and no project scripts or
Java programs are executed. An additional handwritten wire harness does not count
as editor interoperability or a VS Code GUI test.

- Open application and dependency files as actual editor buffers. Change both
  without saving. Request a definition and verify the navigation callback's actual
  target buffer/cursor, exact URI/range, Unicode, CRLF and encoded filename cases.
- **Under ordinary debounce, edit the dependency and immediately query from the
  unchanged application.** Flush/synchronize every relevant buffer before sending
  the request. Flushing only the query buffer or waiting a fixed sleep is not proof.
- A generation filter must guard the entire editor workspace and server session,
  not just the query buffer's changedtick. Delay a response after it has left the
  server, then edit only its target buffer: the callback must not move the cursor.
- Repeat the delayed-response control for close/reopen and server restart. A
  transparent test proxy proves client filtering, not server work cancellation.
- Exercise real cancellation, unresolved/binary results, close-to-disk behavior,
  shutdown and child cleanup. All fixture source and pinned input bytes stay intact.

Linux and each hosted Mac architecture need their own actual run. Passing a
workflow definition, source build or mocked transport does not establish those
runs. This stage does not install on the user's Mac or create a signed Mac release.

## Real-project bridge

Retain the fixed Commons Lang and Gson acceptance. Add a controlled Gson
extras-to-core source definition through the editor-facing worker/protocol using
its original reviewed target, with unsaved comment prefixes only and independently
computed UTF-16 target coordinates. Keep all parser/refusal baselines and compare
complete source path/hash inventories. This demonstrates the buffer/position path,
not a new claim of full Java semantics or a performance improvement.

No stale-result guarantee covers unobserved external disk changes or a result
already applied by a client outside the supplied guard. No latency budget is
inferred from prior one-shot measurements.
