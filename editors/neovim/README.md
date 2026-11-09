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
```

`examples/cli_project/project.json` is a small manual-manifest starting point.
The module reads the manifest's source-root paths to attach existing loaded Java
buffers, including dependency roots outside the manifest directory. The native
server validates actual ownership and supported inventory. Opening another
existing `.java` buffer under those roots attaches it automatically.

- `setup(opts)` validates paths without starting software. Stop before changing
  setup. Paths are passed as arguments, not shell commands.
- `start([opts])` starts one session and returns its client ID. Initialization is
  asynchronous. `status().ready` becomes true after the required capabilities.
- `definition([opts])` uses the current window/cursor and returns a request ID
  and cancel function. No result means no navigation. The optional
  `on_result` callback receives `{status, result, error}`; status is `jumped`,
  `empty`, `stale`, `cancelled`, or `error`.
- `stop()` immediately invalidates pending navigation and requests LSP shutdown.
- `restart([opts])` creates a new independent client session. The optional setup
  `on_exit(code, signal)` callback observes each session's actual process exit.

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
```

The checker creates isolated HOME/XDG/log directories, fixture files and a local
transparent test proxy. It runs actual Neovim buffer APIs and LSP, checks real
buffer/cursor navigation, and starts the native broker with an empty environment
except `PATH=/nonexistent`. It uses the invoking Python's absolute executable.
Fixture sources, a handwritten class-file fixture and executable hashes must stay
unchanged. Stdout/stderr capture and time are bounded; cleanup checks direct
process exits, native sessions, drainers and the owned process group.

`result.json` records individual checks, process outcome, exact client/native
hashes, script hashes, input hashes and Git revision/tree/dirty state.
`protocol.jsonl` records only the generated public fixture session, not user data.
The CI build step, not package `--version` alone, establishes which source built
the tested native binary. Keep reports tied to that build.

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
