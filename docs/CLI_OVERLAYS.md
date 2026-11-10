# One-shot unsaved source buffers

This optional schema-1 adapter adds full-text replacements for **existing loaded
source files**, including dependency sources. It is not an editor server, LSP,
incremental edit protocol, atomic disk reload, or automatic stale-result filter.
The executable still loads the configured disk sources and binary inputs on every
invocation, then derives one immutable workspace with the supplied replacements.
It never writes source files. The core overlay operation does no filesystem or
archive I/O and preserves the original snapshot and prepared handles.
The existing compound `persistent_server_reload_overlays:false` capability remains
unchanged: it describes the absent persistent facility, not this one-shot input.

## Input

All three commands accept `--overlays FILE`. This JSON document is read relative
to the invoking working directory, just like `--manifest` and `--requests`. Source
paths **inside** it are relative to the canonical manifest directory, not the
location of the overlays document. Absolute source paths are allowed.

```json
{
  "schema_version": 1,
  "revision": 7,
  "files": [
    {
      "compilation": { "project": "app", "source_set": "main" },
      "file": "src/A.java",
      "version": 11,
      "text": "class A { int value; void run() { value++; } }"
    }
  ]
}
```

All fields are required. Unknown and duplicate fields are rejected at every
object level. `revision` and each `version` are unsigned JSON integers in
`0..9007199254740991`, the exact integer range supported by common editor JSON
implementations. Fractions, strings, null, signs on negative values and overflow
are not accepted. The labels need not increase between invocations: this CLI
cannot compare state across processes.

`files` contains zero to 1024 entries. An empty array is a valid labeled disk
snapshot. The entire overlays JSON document, including text, escaping and
whitespace, is limited to **1 MiB**, the same fixed cap as manifests and requests.
The JSON document itself must be a regular file, not a symlink. Source path aliases
(including symlinks) are canonicalized and accepted only for an already loaded
source with the exact stated owner. Two aliases for the same source are rejected.
A consumer compilation cannot claim ownership of a visible dependency source.
Empty/NUL paths, unknown owners, unloaded files and additions are rejected. A
missing disk source cannot be created or rescued with an overlay.

Every overlay is checked before constructing the replacement snapshot. All query
paths, visibility and positions are checked against that replacement before
preparing the index or returning any query. Failure returns no partial results.
Full-text replacements retain the original per-file, per-compilation and workspace
input-byte budgets; smaller replacement text can reclaim input-byte budget. This
is not a limit on total allocated memory. The existing deadline, capture, response
byte and result-item limits still apply. Each echoed overlay file consumes one
result item. There is no network, Java, compiler, Maven/Gradle or build-script step.

## UTF-16 line positions

A single declaration query can use `--line N --character N` instead of `--byte N`
or `--utf16 N`. Both flags are required together and cannot be mixed with offset
flags. A batch query has this new tagged position alternative:

```json
{
  "compilation": { "project": "app", "source_set": "main" },
  "file": "src/A.java",
  "position": { "encoding": "utf16_line", "line": 0, "character": 35 }
}
```

Line and character are zero-based `u32` integers. The CLI flags accept ASCII decimal
digits only, without signs, fractions or whitespace. Character counts UTF-16 code units
within a line, excluding its line terminator. LF, CRLF and bare CR delimit lines.
CRLF is one line break, not two. A supplementary Unicode character uses two UTF-16
units. A position inside its surrogate pair, beyond a line's content, outside the
line inventory or at declaration-query EOF is rejected without clamping. Empty
and trailing lines exist, but the final EOF position cannot be queried. The core
line index permits exact EOF conversion for range endpoints; the declaration
adapter deliberately rejects EOF queries.

`utf8` and `utf16` positions retain their **absolute offset** meaning and exact
output shapes. They still count CR and LF separately. Existing valid offset
positions are not rejected merely because a line/character conversion is not
available, such as an offset between CR and LF.

Source declaration and parser-report ranges deliberately remain the existing
four-field byte/absolute-UTF-16 objects, even in overlay or line-query mode. The
new mode does not add line-range fields. An editor converts those bytes using the
same source buffer associated with the response's revision. The native
`LineIndex::byte_to_position` API provides strict conversion; an inexact boundary
such as a CRLF interior has no line position and must not be silently clamped.
Binary results still have no source range. Parser reports still mean parser
limitation or invalid syntax, never Java semantic diagnostics.

## Result correlation and stale responses

Only commands supplied with `--overlays` add `result.overlay_state`:

```json
{
  "revision": 7,
  "files": [
    {
      "compilation": { "project": "app", "source_set": "main" },
      "file": "/canonical/project/src/A.java",
      "version": 11
    }
  ]
}
```

Files preserve input order, with canonical absolute paths and owner IDs. Text is
not echoed. This state labels the **whole result**, including declarations in a
different source buffer. Non-overlaid files use the disk content loaded for that
invocation. Neither these labels nor the invocation-local snapshot ID is a content
hash, atomic filesystem revision, persistent identity or stale-result guarantee.

The editor must allocate a whole-workspace/editor revision for the entire request,
including the document set, buffer versions, dependency edits and configuration
it considers current. Before using any result, compare the response revision and
relevant document versions with that current state; discard mismatches. Comparing
only the requesting file's version misses edits to a declaration in another
buffer. A matching caller label does not detect unobserved disk changes, and the
CLI does not enforce label monotonicity or automatically reject an old response.

Omitting a previously supplied overlay means the next independent invocation uses
that file's disk text. Omitting `--overlays` entirely preserves all old result and
range objects and emits no `overlay_state`. A line query does not itself create an
overlay state. There is no persistent open/change/close session.

## Failures

Existing errors retain their codes. The supervisor also recognizes these codes:

- `invalid_overlay`: label bounds, duplicate canonical source, owner mismatch or
  unloaded source. Shape/type/duplicate-field errors remain `invalid_json`.
- `overlay_limit`: more than 1024 overlays or an original source/workspace input
  budget exceeded by replacements. A JSON document over 1 MiB is `document_limit`.

Unknown compilation owners are `unknown_compilation`; missing paths are `io`;
invalid path strings are `invalid_path`. Invalid line coordinates are
`invalid_position` with a query index. No new error object fields are introduced.
EOF cancellation and timeout retain the [execution contract](CLI_EXECUTION.md).

Native tests in `tests/cli_overlays.rs` run the real supervised executable with an
empty environment. They cover simultaneous source/dependency edits, strict
positions, parser gaps, source ownership, aliases, labels, limits, all-or-nothing
batches and unchanged disk bytes. See the frozen
[editor-buffer acceptance plan](../examples/editor_buffers/PLAN.md) for the
separate real-project check and remaining platform gates.


## Explicit source selection

[Source exclusions](SOURCE_SELECTION.md) are applied when the disk snapshot is
loaded. An excluded file is not an owned source; an overlay or didOpen cannot add
it back. Frozen LSP manifests and source inventories do not change when the
configuration file changes. Restart the session after changing exclusions.
Unknown-document didChange still desynchronizes the session and blocks navigation;
it must not silently accept dirty text from outside the selected inventory.
