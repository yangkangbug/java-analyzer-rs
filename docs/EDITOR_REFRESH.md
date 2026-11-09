# Explicit Neovim refresh contract

This contract is written before the client lifecycle implementation. It extends
[the definition-only LSP contract](LSP_PREVIEW.md), not the Rust protocol or
resolver. The native broker freezes configuration and source ownership at
initialize. Explicit restart is the recovery boundary after source additions,
deletions, renames, manifest/root/dependency-edge changes and external changes.
No watcher, hot reload, build import, automatic retry, JVM or Java tool is added.
Source/binary provenance, ambiguous/unsupported refusals and native worker limits
remain unchanged. Filesystem/configuration loading is not an atomic transaction.

## Public lifecycle and resource bound

Only Neovim **0.11.5**, source commit
`9fa7c0a04638ee3d5fefbdcec347b1c07a5ce9ce`, is supported. At most one module-owned
running or retiring client and one coalesced latest pending start are permitted.
The sequence is retirement, observed safe process exit, fresh initialize, complete
loaded-buffer attach/replay, flush, then ready. Other independent LSP clients are
outside this ownership bound. This bounds owned sessions and queued restarts,
not all callback or host memory in a never-retired session: pinned Neovim can
suppress RequestCancelled callbacks after ordinary edit/newest-request
cancellation, leaving adapter callbacks pending until retirement. Stop/restart
and explicit cancellation guarantee settlement; changing ordinary edit
cancellation settlement is a separate, regression-backed slice.

- `setup(opts)` validates explicit absolute binary/manifest paths without starting
  software. It requires no owned client or queued start.
- `start([opts])` schedules a start. With no options an already active session is
  unchanged; explicit options replace the desired configuration and retire it.
- `restart([opts])` always retires the current session and replaces the single
  queued start. Omitted options reuse the latest requested configuration; the
  manifest is reread when the queued start runs, after safe retirement.
- `stop()` cancels the queued start and invalidates navigation immediately. It
  begins retirement, never abandons an owned process. A later explicit start or
  restart may queue a successor while safe retirement is pending.
- Calls may return the active client ID, or **nil** while work is queued. Callers
  must wait for `status().ready` and then read `status().client_id`; a restart
  cannot promise a new synchronous client ID.
- Latest reentrant start/restart/stop intent wins, including calls from result,
  LspAttach and on_exit callbacks. There is no automatic retry after failure.

`status()` retains active, ready, client_id, session, generation and error. It
adds `phase` (idle, initializing, replaying, ready, retiring, failed or blocked),
`pending_start`, `restart_blocked`, `cleanup` (pending, safe or uncertain), and
`observer_count`. Active means a desired initializing/replaying/ready session;
retirement is not active. Ready is false until every currently supported loaded
buffer has attached and the all-buffer flush has been sent. A client ID while
retiring identifies the old process, not a usable successor. Errors remain visible
until the next explicit successful start attempt; no usable old snapshot is
claimed. The optional on_exit callback observes each actual process exit.

Timeouts are independent of Neovim's exit_timeout (which applies to VimLeavePre,
not ordinary stop). Optional `lifecycle_timeouts` contains integer milliseconds:
`initialize_ms=35000`, `shutdown_ms=5000`, `force_ms=2000`. Each explicit value must
be 50..120000. Small values permit deterministic acceptance tests but may terminate
valid work and enter the blocked state. The default initialization deadline allows
the native 30-second execution budget plus cleanup. There is one live lifecycle
timer, replaced/cancelled as phases change, not one timer per restart request.

## Safe retirement and fail-closed behavior

`Client:is_stopped()` is not process-exit evidence: the pinned implementation sets
its stopping flag before shutdown completes. Ordinary retirement calls stop(false)
exactly once after initialization settles, and awaits on_exit code 0, signal 0.
A stop during initialize invalidates navigation immediately but does not call
stop(false) until initialization settles: the pinned client would otherwise
terminate the process without graceful native job cleanup. It never attaches or
replays a session already retiring.

The documented function-valued `cmd` hook uses public `vim.lsp.rpc.start` and wraps
only the public transport's initialize callback. This is necessary because pinned
rpc.lua schedules result callbacks: the initialize-error assertion in client.lua
runs outside its RPC try_call and is not reliably reported through on_error.
A native initialize RequestFailed (-32803) reply is observed before that assertion.
The native inspection job has completed and the broker is back in Phase::New;
the client sends exit and accepts its specified code 1, signal 0 as safe retirement.
It displays the refusal and permits an explicit corrected retry after safe exit.
Other initialization failures are not silently treated as this known refusal.

Timeout, forced stop, abnormal exit or uncertain transport/cleanup enters sticky
`restart_blocked=true`, drops the queued start, settles callbacks and never launches
an automatic successor. A timeout makes at most one forced stop(true) attempt,
then observes exit for force_ms; lack of observed exit remains uncertain. Even a
subsequent direct-process exit does not clear this block. There is deliberately
no reset/retry bypass in this module. Manual recovery requires checking and ending
any remaining analyzer processes using the operating system, then opening a fresh
Neovim session and explicitly starting again. Preserve dirty buffers before closing
Neovim. The client never claims OS process-tree containment or proof of descendant
cleanup from a direct process-exit callback. Normal safe exits rely on the unchanged
native broker's bounded graceful cleanup contract.

## Buffer preservation and result settlement

No buffer is saved, reloaded, deleted, recreated or rewritten. Buffer IDs, text,
modified state, fileformat and endofline options remain owned by the editor/user.
One stable nvim_buf_attach observer exists per observed loaded buffer lifetime;
it dispatches edits to the current session rather than retaining old-session
callbacks until the next edit. Unload/detach ends that observer lifetime.

After initialization, public vim.lsp.buf_attach_client sends full didOpen text for
all currently supported loaded existing Java sources, including dirty dependencies.
A public client:notify('$/setTrace', {value='off'}) flushes pending changes across
all attached buffers in the pinned client. This orders outbound messages, not a
server acknowledgement or an LSP barrier. Ready stays false through initial
LspAttach callbacks and the entire replay. Removed/outside-root buffers stay
loaded and dirty but are not attached or accepted as navigation origins.
Native ownership validation remains authoritative; source-root membership alone
cannot establish ownership or successful Java resolution.

Stop/restart advance the local session generation and settle every pending
on_result **exactly once**, without waiting for an old response: stale by default,
cancelled if explicitly cancelled. Late real responses cannot navigate or invoke
the callback again. Explicit cancellation also settles immediately. Ordinary
edit invalidation retains the existing response-time stale filtering. No obsolete
navigation is replayed automatically. Whole-workspace generation, buffer ticks,
open epoch, cursor/window, newest-request and exact-session guards all remain.
Old detach/exits/observers cannot erase replacement-session state.

## Required evidence

Keep failing frozen evidence before implementation. Real pinned headless Neovim
must exercise unchanged dirty app/dependency text across add/delete/rename,
configuration/ownership/edge refresh, repeated and reentrant calls, callback
settlement while real computed responses are held, first-LspAttach not-ready,
initialization refusal and corrected retry, stop/restart during initialization,
shutdown/initialization deadlines and forced/abnormal failure, and stable observer
counts. Release held real responses and prove they do not navigate or double-settle;
never replace these gates with mocked responses. Check exact target ranges, refusal
outcomes, process/client counts, fixture hashes, no-JVM environment and cleanup.
Linux headless evidence does not establish hosted macOS, GUI, other editor versions,
latency budgets or the user's computer. Full existing CONTRIBUTING checks remain
separate required gates, including the existing editor acceptance unchanged in
semantic strength.
