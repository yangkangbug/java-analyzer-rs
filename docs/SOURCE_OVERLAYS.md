# Whole-buffer source overlays and editor positions

`WorkspaceSnapshot::with_source_overlays` derives an immutable native analysis
snapshot from already loaded inputs. `LineIndex` translates strict zero-based
UTF-16 line/character positions at an adapter boundary. These are foundations for
an editor adapter, not a persistent server, LSP implementation, transactional disk
reload, semantic diagnostics, or a complete Java language service.

## Inputs, ownership and atomic application

Each `SourceOverlay` contains the owning `CompilationId`, the exact canonical
source `path` returned by the loader, and complete replacement `text`. The API
only accepts sources already in that snapshot. It does not canonicalize aliases,
read paths, create files, infer source roots, generate sources, execute builds or
change dependencies/platform data. A compilation that can see a dependency's
source does not become that source's owner.

The entire batch is validated before cloning inputs or parsing replacements:

1. Every owner must be configured, every path must be a loaded source identity,
   and the owner must match. Duplicate paths are rejected even if the text agrees.
2. Every replacement must fit its original compilation's file byte limit and
   the engine's `u32` source-range representation. Counts use decoded UTF-8 bytes,
   not Unicode characters, UTF-16 units or the length of a JSON spelling.
3. Subtract **all** replaced source byte counts before adding **all** replacement
   byte counts. Check each final compilation input total and the final workspace
   total. A grow/shrink batch has the same acceptance in either order.
4. Clone the validated snapshot, fully parse each replacement, and update source
   usage accounting. Syntax errors and empty buffers are retained as source input;
   a parser report is not a proven Java semantic error.

Original compilation budgets are retained separately from the temporary
remaining-workspace allowance used during loading. An earlier compilation can
shrink while a later one grows, provided both original compilation limits and the
workspace limit hold. Unchanged binary/class/archive input bytes remain charged;
entry counts and archive-expanded byte accounting do not change.

Validation errors return `SourceOverlayError` with a typed `kind`, explanatory
`detail`, and an optional offending `path`. Aggregate budget errors need not name
a single file. Kinds are `UnknownCompilation`, `UnknownSource`, `OwnerMismatch`,
`DuplicateSource`, and `LimitExceeded`. No partial new snapshot is returned, and
the original snapshot is not modified.

## Snapshot and prepared-handle lifetime

The derived snapshot owns its inputs and retains the original graph, visibility,
binary provenance and linking contexts. Source declarations continue binding in
their owner's context even when queried from a consumer compilation.

A derived snapshot does **not** reread source files, directories, JARs or JMODs.
Loaded immutable signatures share reference-counted storage, so their original
files need not still exist to derive and query the snapshot. Cloning still copies
source strings, maps and other snapshot metadata; it is not wholly zero-copy
storage. Unchanged syntax trees also share reference-counted nodes. Replaced files
receive full parses rather than a series of incremental text edits. Signature
sharing does not share a compilation's resolved hierarchy or binding records.

Prepare the new snapshot independently. Its prepared workspace still rebuilds
all source and binary index records once; there is no cross-revision index reuse,
semantic invalidation cache, stable symbol identity or performance promise. Old
snapshots and their old prepared handles remain usable without observing the new
text. This follows the [prepared analysis lifetime](PREPARED_ANALYSIS.md).

Omitted sources retain the receiving snapshot's current text. Therefore an empty
batch is a clone, not a close/reset event. A caller managing open buffers should
retain its disk-loaded base and derive from that base with the complete current
overlay set. Omitting a closed buffer then exposes the base text. To observe a
saved file, new/deleted source, changed dependency or graph, explicitly load a new
base and only replace the old one after that load succeeds. Disk loading itself
is not an atomic filesystem snapshot.

## Revisions and cancellation belong to the caller

The core overlay type has no version or revision fields. CLI correlation labels,
when supplied, are caller-provided metadata, not engine-validated cross-process
revisions. Echoing a label does not reject a stale result.

An editor should capture a global state generation for each request, advance it
when any relevant buffer, configuration, dependency or open/close state changes,
and only publish results for the current generation. Checking only the queried
file's version is insufficient because a result can target another changed file.
A delayed response after close/reopen must not be applied to the reopened buffer.
Source ranges remain tied to the exact analyzed snapshot, not future file bytes.

Direct library calls have no process isolation, cooperative cancellation or total
memory budget. Parsing and index building must run off the UI thread; the
supervised CLI supplies its existing coarse native worker timeout/cancellation
boundary. It still loads disk once per invocation. No cross-invocation binary
cache or persistent open/change/close session is introduced. See
[CLI execution limits](CLI_EXECUTION.md).

## Strict UTF-16 line/character conversion

`LineIndex::new(&str)` borrows the exact source and records its line boundaries.
`position_to_byte(Utf16Position { line, character })` and
`byte_to_position(u32)` return `Option`, rejecting invalid positions rather than
clamping or snapping. Line and character are zero-based; character counts UTF-16
code units, not graphemes or displayed columns. Tabs count as one unit, combining
marks independently, and an astral character as two units.

LF, CRLF and lone CR terminate lines. The content-end position maps to the start
of its line ending; no line/character position represents the interior of CRLF.
UTF-8 character interiors, UTF-16 surrogate-pair interiors, missing lines and
columns beyond content end are rejected. An empty source contains line zero;
a final line ending creates a trailing empty line.

EOF is valid for range conversion. A declaration-query adapter separately rejects
EOF, preserving the existing query contract. The legacy absolute UTF-8/UTF-16
helpers are unchanged: CR and LF each count in their absolute offsets. Strict
line positions are a separate boundary contract, not an assertion that all LSP
position-normalization behavior is implemented. Byte ranges whose endpoint is
inside CRLF cannot be represented by these strict line positions; callers must
retain the byte range or report conversion failure, never silently shift it.

Building the line index scans the source once. Byte lookup locates a line and
counts UTF-16 units within it; position lookup scans that line. The index does not
incrementally update or cache per-character offsets.

## Native verification

`tests/positions.rs` covers CRLF/lone CR/LF, empty and trailing lines, Chinese and
astral text, combining marks, strict invalid boundaries, EOF and an independent
character-walk comparison over short text combinations.

`tests/overlays.rs` covers unsaved whole-buffer resolution and parser reports,
unchanged disk bytes, empty buffers/batches, identity/owner failures, complete
batch validation, directed source/test visibility, declaration-owner binary
contexts and old prepared handles. A test deletes every source, JAR and JMOD
input after the base load, then derives and queries the new snapshot while
checking binary origins and shadowed classes. Byte-limit tests cover exact/over
boundaries, binary accounting, chained replacements, grow/shrink order and the
original-versus-temporary loading-budget distinction. Fixtures are assembled in
Rust; no Java toolchain is executed or required.
