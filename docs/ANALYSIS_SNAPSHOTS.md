# Caller-owned prepared analysis versions

`AnalysisSnapshot` owns both an immutable `WorkspaceSnapshot` and its prepared
declaration index. A clone retains this same version behind an Arc. The owning
input variable may go out of scope; no self-referential object, unsafe code or
second resolver is needed. The existing borrowed `prepare()` API remains valid.

This gives an embedding application an explicit lifecycle:

1. `AnalysisSnapshot::load(&config)` loads and prepares a new root.
   `AnalysisSnapshot::new(workspace)` instead consumes already-loaded inputs.
2. `scope(&id).resolve_at(path, byte)` reuses the exact retained index.
3. `with_source_overlays(&batch)` validates the entire replacement transaction
   against the old version's ownership and all byte budgets.
4. A real change creates a new immutable input/index pair, rebuilding the whole
   index exactly once. No old handle changes, including handles used concurrently.
5. An empty or byte-identical batch shares the original state only after full
   validation. No-op updates avoid indexing, not all allocation or validation.
   An empty batch does not revert earlier replacements.
6. A fresh `load` establishes disk source inventory, binary dependencies and graph
   changes. It never reuses another load's prepared index. Failed loads/updates do
   not invalidate previously returned handles.

Run the complete small example without Java:

```sh
cargo run --locked --example analysis_session
```

The example loads a two-compilation fixture, navigates to its dependency, derives
a target-buffer version, verifies old/new exact target ranges, drops an owner and
reloads the unchanged disk inputs. It does not write project files.

## Correctness boundary

The owned and borrowed handles use the same `OwnedScopedIndex` implementation.
Queries obtain syntax from that index's exact workspace version. Source ownership,
transitive visibility, binary linking contexts and refusal behavior are unchanged.
A scope's `inputs()` exposes only that compilation's own immutable inputs; its
legacy flat resolver is not a substitute for the scope's graph-aware resolver.

Native regressions cover build counts, clone/drop/concurrent reads, dependency
edits, full-rebuild equivalence, no-op validation, duplicate/unknown ownership and
file/workspace budget failures. Two distinct versions of a same-named binary
remain isolated across scopes and retain exact target provenance after overlays.

## Not yet a persistent editor worker

This API does not change the CLI or LSP job lifecycle. Current definition jobs
still load and prepare a complete workspace per request. This is not a claim of
faster shipped LSP navigation. Borrowed prepared handles already reuse their
index for repeated queries; the new API removes ownership/lifetime friction for
long-lived callers and explicitly preserves versions across updates.

There is no fine-grained dependency invalidation, query-result cache, filesystem
watcher, stable symbol ID, automatic graph reload, memory quota or cooperative
cancellation. Retaining many versions retains their inputs and indexes; callers
must release old handles they no longer need. Arc sharing itself is not a bounded
cache or a maximum-memory guarantee.

A persistent worker remains a separate integration milestone. It must preserve
generation guards and transaction publication while adding explicit cancellation,
timeouts, crash containment, memory policy, refresh and replay validation. Merely
placing this handle in a server is not evidence that those contracts are met.

## Comparison protocol

For a repeated-navigation embedding, compare cold load/preparation, repeated
queries, identical-buffer batches and real source updates separately. Verify exact
target records for every measured request and old/new/full-rebuild equivalence.
Use fixed source/dependency identities, serial runs, an excluded warmup and raw
samples. Compare against both retained borrowed handles and rebuild-per-version
workflows where applicable; savings over rebuilding are not a faster binder.

A local fixed Commons/Gson differential probe additionally compares 577 previously
recorded query locations on base, derived and full-rebuilt versions, including
refusals and complete target records. The sample is a biased regression set, not
a Java semantic coverage percentage. Local timings remain observations, not an
editor latency guarantee or CI threshold.
