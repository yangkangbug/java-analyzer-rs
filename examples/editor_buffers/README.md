# Unsaved source buffers, one invocation

Build the native CLI with `cargo build --locked --features cli --bin java-analyzer`.
From the source repository root, run:

```sh
target/debug/java-analyzer declaration \
  --manifest examples/cli_project/project.json \
  --requests examples/editor_buffers/requests.json \
  --overlays examples/editor_buffers/overlays.json

target/debug/java-analyzer parser-report \
  --manifest examples/cli_project/project.json \
  --overlays examples/editor_buffers/overlays.json
```

The JSON files replace two existing buffers only. The app refers to `editedValue`
in the unsaved library buffer; the original disk files still contain `value`.
The initial comment includes a supplementary character and CRLF. The query is
zero-based line 5, UTF-16 character 8. It should resolve to `lib/main`'s field in
that buffer, not a fabricated range in the unchanged disk file.

All document data paths are relative to the **manifest directory**, not this
example directory. Source result ranges remain exact UTF-8 bytes and absolute
UTF-16 units. Use the same buffer text and the library's strict `LineIndex` if an
editor needs line/character target ranges. There is no silent CRLF/surrogate clamp.

`result.overlay_state` echoes revision 41 and each file's caller-assigned version.
These labels do not establish cross-process monotonicity or reject stale results.
An editor must compare its current whole workspace/buffer generation before
accepting a result. Changing another buffer, closing/reopening a document or
changing configuration/dependencies can invalidate a target too.

Run the existing `examples/cli_project/requests.json` without `--overlays` to read
the original disk state again. No Java source is written, and there is no
persistent open/change/close server. Every invocation still loads disk and binary
inputs; this example is not an incremental-latency demonstration.

See the [protocol and limits](../../docs/CLI_OVERLAYS.md),
[predeclared acceptance](PLAN.md) and [core snapshot contract](../../docs/SOURCE_OVERLAYS.md).
