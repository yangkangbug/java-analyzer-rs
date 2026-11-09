# Fixed Gson LSP wire acceptance

`scripts/check-lsp-project.py` extends the previously reviewed Gson
`cross_interface` case through the native stdio LSP adapter. It is a handwritten
protocol harness, **not** an actual editor interoperability run. The real Neovim
client gate is separate, as are the native transport/cancellation reliability
tests.

## Fixed expectations

Use the same checksum-pinned offline inputs and manual four-compilation model as
[`second_project`](../second_project/README.md). The query remains the
`TypeAdapterFactory` reference at byte 1082 in extras/main's
`PostConstructAdapterFactory.java`. Its independently reviewed target remains
core/main's `TypeAdapterFactory.java`, bytes 6402..6420. Both the original plan
and exact target record are checked before contacting the server. Expected LSP
coordinates are computed from those pinned source bytes, never server output.

The nine definition stages retain the original source-target expectation:

1. Open the query buffer with its original disk bytes and require the exact disk
   target. This preview requires an open synchronized query buffer; this is not
   a zero-overlay definition request. Close it before the unsaved-buffer phase.
2. Two unsaved source buffers with distinct CRLF/astral comment prefixes and
   signed negative open versions, -11 and -23.
3. Full replacements with larger, nonconsecutive negative versions, -7 and -3.
4. Only the target buffer changes, version 100; the unchanged query buffer must
   resolve to the new target position.
5. A duplicate target version 100 desynchronizes the document. Navigation must
   return null or an explicit desynchronized-buffer error (-32602), never a
   stale target or an unrelated error. The fixed plan requires refusal but does
   not prescribe null versus this error. The actual response is recorded.
6. A valid full replacement, version 104, recovers the document.
7. Closing the target buffer restores the target's disk range while the query
   buffer remains unsaved.
8. Close both buffers and verify that a request from the closed query buffer is
   explicitly refused with -32602 and an open-document requirement.
9. Reopen the query with its original disk bytes and a lower version in the new
   open incarnation. Require exactly the original disk target, then close again.

The harness verifies standard initialization/full synchronization capabilities
and shutdown/exit. Before/after checks cover the complete 226-file upstream Java
path/hash inventory and all six pinned input hashes unchanged. Only 211 of the
226 Java files are in the manual source-set model; excluded files are still
included in the integrity check.

## Run

Build only this repository's trusted native executable, then supply an already
verified input directory. This script never downloads inputs or runs upstream
build scripts, tests, Java programs, or JVM tools.

```sh
cargo build --locked --release --features cli --bin java-analyzer
python3 scripts/check-lsp-project.py /path/to/second-project/verified \
  --native target/release/java-analyzer
python3 -m unittest discover -s scripts -p 'test_lsp_project.py' -v
```

The subprocess receives an empty executable search path and no inherited Java
environment. A distinct working directory tests original manifest-relative path
handling. POSIX nonblocking pipes bound both reads and writes, with a 60-second
per-operation and 240-second session deadline, bounded headers, bodies, aggregate
output and notification counts. Shutdown has a separate 10-second bound.
`waitid(WNOWAIT)` reserves the native process-group identity until harness cleanup
and reaping. This harness containment is not evidence that the server itself
reaps every descendant; dedicated production reliability tests own that claim.

Offline unit tests use literal UTF-16 coordinates and injected wrong target/range
responses. A checker cannot pass by generating its oracle from its own engine's
output. The process tests execute only repository-owned Python snippets.

## Evidence boundaries

The reviewed 16-query semantic/refusal oracle remains in
`scripts/check-second-project.py`. This checker reuses it for two distinct
one-shot CLI declaration batches before/after the LSP session, each with no
overlays or overlay metadata, and requires their exact query records to agree.
Those CLI results are reported separately from the nine LSP requests. The LSP
baseline/recovery instead use an open query buffer containing original disk
bytes, which is still an overlay.

The 1,513 parser reports across 58 files remain the responsibility of the existing
full second-project acceptance. They are not rerun or inferred here. Existing
one-shot unsaved-buffer evidence remains in `scripts/check-editor-project.py`.

Passing this fixed bridge does not establish complete Java semantics, a persistent
incremental semantic index, latency improvements, VS Code interoperability,
hosted macOS execution, or the stronger client-side stale-navigation guarantee.
It does not execute Gson or build it.

The initial harness attempted a request from a closed query buffer. The first
new-server run correctly refused that invalid lifecycle. The harness was then
corrected to open the original query bytes for baseline/recovery and to test the
closed-query refusal explicitly. The earlier failure and pre-run freeze records
are retained separately; no Java target or range expectation was relaxed.

## Local observation

On 2026-11-09, the revised harness passed on Linux with the locally built native
binary SHA-256
`08e270a3f832a1cc865b7834b40ac40a778170695679742a657fcf0e9ad5deef`.
All nine LSP stages and both separate 16-query disk batches passed. The complete
226-file Java inventory and all six pinned inputs were unchanged. The original
target's zero-based UTF-16 range was line 163, characters 17..35; unsaved target
prefixes moved it to lines 165, 167, 169 and 171, then closing the target restored
line 163. Desynchronized and closed-query stages returned the explicit -32602
reasons described above. This is a local native-wire observation, not a hosted
macOS, real-editor, release-distribution or timing result.
