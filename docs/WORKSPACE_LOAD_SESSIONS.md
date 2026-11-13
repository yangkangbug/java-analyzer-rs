# Caller-owned workspace load sessions

Create a caller-owned session with let mut session = WorkspaceLoadSession::default(),
then call session.load(&config) repeatedly. Each call performs a fresh workspace
load, with bounded reuse of successfully decoded archive signatures across calls.
The caller owns the session lifetime. No global cache, service or watcher exists.

## What is preserved

Every call still validates configuration/graph, walks current source and binary
directories, rejects unsafe/unsupported paths, captures current bytes and applies
all current budgets. Every archive occurrence is charged, even when repeated
across compilation scopes. Canonical path, JAR/JMOD format, exact target release
and exact captured bytes identify eligible reuse; metadata alone never does.

A hit must satisfy current tightened archive limits. A miss decodes the same
captured bytes, without a second disk read. Failed loads return no snapshot.
Successful entries encountered before a later failure may stay in the session,
but the next load still captures and validates each occurrence again.

Sources are parsed anew. Workspace contexts and prepared indexes are rebuilt;
there is no complete workspace/result cache. Classpath order, compilation identity
and source visibility retain their ordinary semantics.

## Ownership and bounds

The session retains at most 16 archive entries and 64 MiB of raw archive bytes
under the existing FIFO policy. Parsed signatures, active captures, caller-held
snapshots and indexes are outside that cap. It is not a process-memory quota.
Drop the session to release its ownership; independently retained snapshots stay
valid. Concurrent loads need separate sessions or caller synchronization.

The existing WorkspaceSnapshot::load keeps a fresh per-call archive cache.
AnalysisSnapshot::load and all CLI/editor/LSP defaults are unchanged.

## Disk and inventory semantics

Each call reads current inputs; this is not an atomic multi-file filesystem
snapshot and does not promise disk consistency at response-send time.
Source inventory is rediscovered. A caller that freezes inventory must compare
the returned inventory and refuse drift according to its existing policy.
The LSP continues its existing per-request load and inventory-rejection path;
this core API is not connected to it.

## Example and validation

Run cargo run --example load_session for an offline source-to-JAR example,
including fresh-load result equivalence and independence after session drop.

Tests cover fresh captures with decoding reuse, budget tightening, exact-byte
changes, failed-load recovery, source inventory drift, identity/order changes,
and a test-only capture-before-parse mutation proving no second disk read.
Existing archive release/format, unsafe-path and FIFO-limit tests still apply.

A local prototype using fixed Commons/Gson inputs observed full load-plus-index
medians of about 467/543 ms with fresh loads versus 151/213 ms with retained archive
decoding. These serial observations exclude LSP/process costs and are not a
general performance guarantee or memory bound.
