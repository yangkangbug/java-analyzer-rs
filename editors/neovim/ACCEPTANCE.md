# Frozen Neovim definition preview acceptance

This plan precedes the adapter implementation. The editor is official Neovim
0.11.5, running headless with no user configuration or plugins. It exercises real
built-in LSP synchronization, real buffers, and actual navigation. It does not
establish GUI acceptance, full Java support, a signed distribution, performance
budgets, or compatibility with the user's computer. No user computer is used.

## Fixed inputs and isolation

- Download only the three official platform archives recorded with exact sizes
  and SHA-256 digests in `scripts/neovim-inputs.json`. Verify before extraction.
- Keep upstream runtime documentation and licensing. Extract locally into a new
  dev-only directory, never into system or user configuration paths. No install
  scripts, Java, external project builds, or plugin manager are run.
- Fail closed on corrupt cache, unsafe archive paths/links, output replacement,
  unsupported platforms, excessive expansion, and missing executable/license.
- Use a fresh fixture and isolated HOME/XDG paths. Hash all source and binary
  fixture inputs before/after. Record exact analyzer/client executable hashes,
  client version, source Git revision/tree/dirty state, and script identities.

## Usable client surface

Provide `setup`, `start`, `stop`, `restart`, and `definition` in a small Lua
module. The caller explicitly supplies absolute analyzer and manifest paths.
There is no build import, parser-report mapping, automatic project reload, or
automatic download. Configuration/source-inventory changes require restart.
Restart is also recommended after external disk/dependency changes because the
client generation does not observe them. The worker rereads frozen paths per
query; it does not cache their bytes until restart or guarantee consistency with
unobserved external edits.

Neovim itself owns document open/change/close. Advertised server capabilities
must be full-text synchronization and definition. The safe definition mapping is
the supplied function, not a claim about Neovim's unwrapped default definition.

## Positive editor checks

1. Open an application and dependency buffer; resolve to the exact dependency
   buffer and UTF-16 target. Verify actual current buffer and cursor after jump.
2. Edit both buffers without saving. Include CJK, a supplementary character,
   CRLF, and URI characters space, `#`, and `%`. Verify exact target, unchanged
   source inputs, and a same-position equal-length symbol replacement.
3. With ordinary 150 ms change debounce, edit the dependency and immediately
   query the unchanged application. The query must see the new dependency.
   Use a supported public-client synchronization route for every attached buffer;
   sleeps, private changetracking calls, or generation guards alone do not count.
4. Close the dependency buffer; the edited reference must use the server's disk
   base rather than the closed overlay. Reopen and query the new buffer lifetime.
5. Unresolved and binary-only definitions return no fabricated source navigation.
6. Explicit cancellation settles and does not navigate. Shutdown/restart cleanly
   replace sessions and do not leak analyzer processes.

## Deterministic stale-response checks

The provided adapter must advance a whole-workspace generation synchronously on
local `on_lines` mutation, before delayed text synchronization, for every tracked
buffer. Capture this generation, session identity, request identity, origin
buffer lifetime and cursor context in the callback. Invalidate on open/close,
detach, edits in another buffer, stop and restart. Cancel obsolete requests and
discard obsolete callbacks before applying any navigation.

A transparent test proxy may hold an already computed response in transit.
Release it after (a) target-only edit with unchanged origin, (b) target close and
reopen, (c) origin close/reopen, or (d) server restart. None may jump. A subsequent
current request must succeed. The proxy tests client safety only; it is not proof
that server work was interrupted. Native worker cancellation is a separate gate.

## Separate native protocol coverage

Native server tests own malformed framing, request IDs, signed/nonconsecutive
versions and invalid lifecycle notifications. Keep these distinct from real
Neovim buffer events. Preserve the existing CLI and strict core coordinates.
Do not misuse `ContentModified` or `ServerCancelled` for ordinary queued edits.
