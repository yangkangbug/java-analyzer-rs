# Neovim definition preview

This optional adapter uses **Neovim 0.11.5's real built-in LSP client** and the
native `java-analyzer lsp --manifest FILE` broker. No Java, JDT LS, plugin manager,
build import, Node, or Electron is involved. The analyzer remains a limited Java
analysis preview. Only definition navigation is exposed here, not completion,
semantic diagnostics, parser-report UI, rename, or automatic project reload.

## Use with an explicit project

Build the analyzer from this repository with the pinned Rust toolchain:

```sh
cargo build --release --locked --features cli --bin java-analyzer
```

Add the repository's `editors/neovim` directory to `runtimepath`. In your own
Neovim configuration, provide **absolute paths**:

```lua
vim.opt.runtimepath:prepend('/absolute/path/java-analyzer-rs/editors/neovim')
local java = require('java_analyzer')
java.setup({
  binary = '/absolute/path/java-analyzer-rs/target/release/java-analyzer',
  manifest = '/absolute/path/to/project.json',
})
java.start()
vim.keymap.set('n', '<leader>jd', java.definition, { desc = 'Safe Java definition' })
vim.keymap.set('n', '<leader>jr', java.restart, { desc = 'Restart Java analysis and replay buffers' })
```

`examples/cli_project/project.json` is a small manual-manifest starting point.
The module reads each compilation's source roots and optional
`source_exclusions` list to attach eligible existing loaded Java files, including
dependency roots outside the manifest directory. Opening another eligible
`.java` buffer attaches it automatically. Directories named `*.java` are not
source files. The native server alone validates actual ownership and inventory.

An exclusion is an exact relative file path or directory subtree, applied
independently to every root in its compilation. Matching is literal,
component-boundary and case-sensitive, without globbing, negation or Unicode
normalization. A path eligible through any configured root may attach: excluding
`nested` from a parent root does not veto a separately configured nested root.
If two compilations actually own the same included canonical source, native
initialization still refuses; this filter does not pick an owner.

Before starting software, the adapter rejects malformed manifest/list/entry
types, duplicate or empty rules, more than 1024 rules, rules exceeding 4096 UTF-8
bytes, absolute paths, dot/dotdot or empty components, backslashes, colons,
`* ? [ ] { }`, and Unicode Cc controls (C0, DEL, C1). Nonempty exclusions require
source roots. Ordinary Unicode, spaces, leading `!` and `--` are literal.
Excluded buffers remain loaded and dirty but are not attached, synchronized or
used as navigation origins. Edits to them cannot send unknown-document changes
that invalidate an otherwise valid native session.

Selection is frozen for each session. After changing exclusions, explicitly call
`restart()`: safe retirement detaches old buffers, then newly eligible buffers
replay their full unsaved text before ready. Newly excluded buffers keep their
text, modified state and buffer options. A failed native initialization likewise
preserves dirty buffers, and a corrected manifest can be explicitly restarted
after safe refusal cleanup.

- `setup(opts)` validates paths without starting software. It requires no owned
  client or queued start; `stop()` is asynchronous. Paths are arguments, not shell
  commands.
- `start([opts])` schedules a session. Without options an active session stays
  unchanged; explicit options request replacement. `restart([opts])` always
  requests replacement, coalescing repeated calls to the latest configuration.
  Both can return nil while retirement/start is queued. Wait for `status().ready`,
  then read `status().client_id`; restart does not return a new ID synchronously.
- `definition([opts])` uses the current window/cursor and returns a request ID
  and cancel function. No result means no navigation. The optional
  `on_result` callback receives `{status, result, error}`; status is `jumped`,
  `empty`, `stale`, `cancelled`, or `error`.
- `stop()` immediately invalidates and settles pending navigation, cancels a queued
  start, and retires the owned session. The optional setup `on_exit(code, signal)`
  callback observes each session's actual process exit.

Restart waits for safe exit of the old session, initializes a new one, attaches
all supported loaded source buffers with their current full text, and flushes
changes before advertising ready. Dirty buffers are not saved, reloaded or
recreated. This includes dependency buffers, not just the current file. The
client owns at most one running/retiring session and one queued latest start.
See [the explicit refresh contract](../../docs/EDITOR_REFRESH.md) for lifecycle
states, bounded timeout options, exact callback settlement and failure recovery.

`status().phase` and `status().error` distinguish initialization, replay, retirement
and failure. A known native initialization refusal permits a corrected explicit
retry after safe exit. Forced, abnormal or uncertain cleanup sets
`status().restart_blocked`; no replacement is launched. This sticky failure cannot
be bypassed through `start` or `restart`. Check and end remaining analyzer processes
using the OS, preserve dirty buffers, then open a fresh Neovim session. This is not
an OS process-tree containment guarantee.

Do not use `vim.lsp.buf.definition()` when you want this adapter's stale-result
protection. The safe mapping must call the supplied `definition` function.
Other Java LSP clients may be active independently; this function sends only to
the explicitly started preview session.

## What stale-result protection covers

Each attached buffer is observed with `nvim_buf_attach` / `on_lines`, so a local
edit advances the **whole workspace generation immediately**, before Neovim's
ordinary 150 ms synchronization debounce. The definition callback checks that
generation, all captured buffer ticks, origin-buffer open epoch, current
window/cursor, newest navigation request, and the exact client session. Closing,
reopening, editing a different dependency buffer, canceling, stopping, or
restarting invalidates old navigation and requests cancellation. A response that
already finished at the server is still discarded by the client when obsolete.

Before every definition, the adapter calls the public
`client:notify('$/setTrace', {value='off'})` API, then `client:request(...)`.
In **the pinned Neovim 0.11.5 implementation**, non-didChange `Client:notify`
flushes pending changes for all attached buffers. `Client:request` alone flushes
only the origin buffer. This is a tested client-version behavior, **not an LSP
barrier response** or a generic promise about other editors/versions. The broker
ignores the standard trace notification. No private changetracking API is called.

Source: [Client:notify and Client:request](https://github.com/neovim/neovim/blob/9fa7c0a04638ee3d5fefbdcec347b1c07a5ce9ce/runtime/lua/vim/lsp/client.lua)
and [flush implementation](https://github.com/neovim/neovim/blob/9fa7c0a04638ee3d5fefbdcec347b1c07a5ce9ce/runtime/lua/vim/lsp/_changetracking.lua).
The adapter rejects other Neovim versions until separately validated.

The project configuration and source inventory are fixed for a server session.
Restart for changed manifests, roots, source additions/removals or renames.
Also restart after external disk/dependency edits: the client generation does
not observe those changes. Native queries reread files/binaries at frozen paths;
their bytes are **not cached until restart**. There is no guarantee of consistency
with unobserved external changes, filesystem transactions, or arbitrary other
plugins modifying protocol state. No source is saved by this module.

## Optional pinned development input

This setup runs only when explicitly requested, outside mandatory native Cargo
tests. It downloads and verifies data without executing it or installing into
system/user configuration paths:

```sh
python3 scripts/fetch-neovim-input.py \
  --cache /absolute/local/cache \
  --output /absolute/local/neovim-0.11.5
```

The output directory must be new. On macOS, use canonical paths for `/tmp` or
`/var` aliases, for example paths returned by Python `Path(...).resolve()`.
`--offline` requires a valid cached archive. `--platform` can select an exact
input; it never changes machines or runs the downloaded architecture.

Fixed official URLs and archive SHA-256 hashes:

- [Linux x86_64](https://github.com/neovim/neovim/releases/download/v0.11.5/nvim-linux-x86_64.tar.gz)
  `b2f91117be5b5ea39edd7297156dc2a4a8df4add6c95a90809a8df19e7ab6f52`
- [macOS arm64](https://github.com/neovim/neovim/releases/download/v0.11.5/nvim-macos-arm64.tar.gz)
  `79143d3b408f7034f90b7cf59af2276de09ef8a4c2f1a28e4c99581b249d3107`
- [macOS x86_64](https://github.com/neovim/neovim/releases/download/v0.11.5/nvim-macos-x86_64.tar.gz)
  `6612760a7037ca2518e456908baf5e43101fa79819d18979fc4d4e8441d9dfa5`

Exact sizes/budgets/provenance are in
[`scripts/neovim-inputs.json`](../../scripts/neovim-inputs.json).
The signed release tag points to source commit
`9fa7c0a04638ee3d5fefbdcec347b1c07a5ce9ce`; asset hashes were checked against the
official release metadata. This fixed baseline is not a claim to be latest.
The setup keeps all shipped runtime documentation and adds the exact upstream
[`LICENSE.txt`](licenses/Neovim-0.11.5-LICENSE.txt), verified by its manifest hash.
Neovim is Apache-2.0 with Vim-licensed portions and separate dependency licenses.
It is a dev-only input, not included in analyzer packages or Git source artifacts.

## Headless acceptance

```sh
python3 -m unittest discover -s scripts -p 'test_neovim*.py' -v
python3 scripts/check-neovim-editor.py \
  --nvim /absolute/local/neovim-0.11.5/bin/nvim \
  --native /absolute/path/target/release/java-analyzer \
  --output /absolute/local/new-editor-results
python3 scripts/check-neovim-refresh.py \
  --nvim /absolute/local/neovim-0.11.5/bin/nvim \
  --native /absolute/path/target/release/java-analyzer \
  --output /absolute/local/new-refresh-results
```

The checker creates isolated HOME/XDG/log directories, fixture files and a local
transparent test proxy. It runs actual Neovim buffer APIs and LSP, checks real
buffer/cursor navigation, and starts the native broker with an empty environment
except `PATH=/nonexistent`. It uses the invoking Python's absolute executable.
Fixture sources, a handwritten class-file fixture and executable hashes must stay
unchanged. Stdout/stderr capture and time are bounded; cleanup checks direct
process exits, native sessions, drainers and the owned process group.

The development checker observes child exit without reaping the session leader,
then signals its owned process group **before** `wait()`. Linux uses
`waitid(WNOWAIT)`. Darwin Python builds without `os.waitid` use standard-library
[`kqueue` / `EVFILT_PROC` / `NOTE_EXIT`](https://docs.python.org/3.12/library/select.html#kqueue-objects).
Apple documents [`NOTE_EXIT` as process-exit notification](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/kqueue.2.html);
XNU's [`filt_procattach`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_event.c)
captures edge-triggered events after attachment. A fixed Python gate
(`sys.executable -I -S`) waits on a private pipe until registration succeeds,
then closes the pipe and execs the target with the same PID/PGID. Registration
failure or gate EOF never starts the target. No sleep substitutes for registration.
The checker requires default `SIGCHLD` handling. Ownership loss fails the run and
never signals a possibly reused numeric group; unfinished output drainers cannot
keep the checker alive. This is trusted test-process cleanup, not an OS sandbox.

Darwin can return `EPERM` for a group containing only zombies, because XNU's
[`killpg1`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_sig.c)
excludes zombies from its signal-target count. The checker accepts that outcome
only after confirming the leader's exit without reaping and proving that it is
the **sole group member**. It calls the OS's `/usr/lib/libproc.dylib`
[`proc_listpids`](https://github.com/apple-oss-distributions/xnu/blob/main/libsyscall/wrappers/libproc/libproc.h)
with `PROC_PGRP_ONLY` and a fixed two-C-int buffer. XNU
[`proc_info.c`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/proc_info.c)
enumerates live and zombie members under its process-list lock. Zero, extra or
wrong PIDs, a full/malformed result, and any error fail closed. This uses only
the declared scalar ABI, not guessed process-structure layouts. Apple labels
libproc interfaces private and subject to change; this is a Darwin-specific
development-checker dependency, not a portable standard-library guarantee.

Any other group-signal failure remains a failed check. The checker makes a
best-effort signal/reap of its still-owned direct child and records both group
and reap failures; it never signals a numeric PID/group after reaping. EOF alone
does not prove cleanup: a regression keeps a live descendant with closed stdio
and requires the group check to reject it. Cleanup outcomes are explicit in
`result.json`, including the narrowly verified Darwin zombie-only case.

Offline regressions exercise ordering, registration failure, pre-release death,
exact target status, descriptor closure, bounded output and timeout cleanup.
On Linux, simulated kqueue events test only the gate plumbing, not Darwin kernel
behavior. Actual macOS acceptance still requires the separate hosted CI runs.

`result.json` records individual checks, process outcome, exact client/native
hashes, script hashes, input hashes and Git revision/tree/dirty state.
`protocol.jsonl` records only the generated public fixture session, not user data.
The CI build step, not package `--version` alone, establishes which source built
the tested native binary. Keep reports tied to that build.

The refresh checker adds actual source/configuration changes, dirty dependency
replay, first-attachment readiness, initialization refusal and retry, repeated
and reentrant lifecycle calls, callback settlement and bounded failure paths.
Intentional fixture mutations are recorded and restored; product operations must
not save dirty buffers. Protocol and process evidence distinguish safe exit from
`is_stopped()`, and failed/forced scenarios must not start replacement sessions.
Offline mutation tests reject inconsistent outcomes rather than trusting an
editor's success flag alone. Every run uses a new output directory.

See the [frozen acceptance plan](ACCEPTANCE.md). Coverage includes simultaneous
unsaved app/dependency edits, same-position symbols, Unicode/CRLF/escaped URIs,
immediate target-only edits with normal debounce, twenty rapid changes, close/
reopen, unresolved/binary-only null, cancellation, and session restart. A proxy
holds **real already-computed responses** to test late-result filtering. That
does not prove native worker interruption; native protocol tests own that gate.

Passing means this pinned **headless editor integration** passed on the reported
host. It does not validate GUI behavior, other Neovim versions, signed Mac
distribution, full Java semantics, editor latency, or the user's Mac. Linux and
hosted macOS runs are separate evidence. No user computer is needed or accessed.

## Independent source-selection gate

The trusted-main `editor-acceptance` workflow retains the existing 13-definition
and 28-refresh gates and adds `check-neovim-source-exclusions.py`. Its four
independent scenarios cover excluded file/subtree buffers (including later opens
and edits), changed selection across explicit restarts, nested-root reinclusion
plus duplicate-ownership refusal/corrected restart, and 62 native/client manifest
validation cases. Exact native target ranges, per-session full-text replay,
nonattachment/nonsynchronization, dirty-buffer preservation, process ownership
and clean exit are checked independently of the editor's success flag.

A bounded CI-only negative control runs the same built native candidate with the
exact old adapter from public commit
`d30ff6ba4ff67dae4a9862c67507bda4c0bb40ee`. The setup fetches only that immutable
file into a new owned temporary adapter directory, requires 26,196 bytes and
SHA-256 `a8be5c2bd9f85c2428ccc65492bccc31592f2131125412892736639c48e78ab6`,
and permits no redirects, arbitrary revisions or checkout changes. The negative
gate requires the specific `EXCLUDED_BUFFER_ATTACHED` witness for `One.java`,
the exact excluded `didOpen` text, successful real native initialize, preserved
dirty buffers, unchanged inputs/executables, and safe native/proxy exit.
A generic nonzero exit, timeout, crash or unrelated assertion does not count.

Expected wire URIs use the pinned client's exact RFC3986 literal set and
lowercase percent escapes, preserving pathname letter case. The native broker
preserves its sole opened client URI for a source target. Exact URI equality
remains required for replay, navigation and the legacy witness. Bounded public
diagnostics expose only fixed event labels, booleans and counts before temporary
evidence cleanup; arbitrary paths, source text and error strings are never printed.

The candidate adapter must then pass every scenario against that same native
binary and pinned client. Only bounded summaries and public hashes are printed;
owned client, adapter and fixture outputs are removed on every workflow outcome.
Linux, hosted macOS ARM and hosted macOS Intel remain separate evidence. This
reconstruction was reviewed and tested with offline Python mutations before
publication; it was not executed in a real local Neovim and does not establish
chronological real-editor fail-first evidence. Actual CI results, rather than
historical or reconstructed reports, establish RED/GREEN for the tested revision.
No user computer is used for real-editor acceptance.
