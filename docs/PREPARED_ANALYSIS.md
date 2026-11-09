# Prepared analysis and identity boundaries

`Project::prepare()` and `WorkspaceSnapshot::prepare()` explicitly build a binder
index once. The returned handles borrow their immutable inputs and own the derived
index. A prepared workspace's compilation views borrow that same prepared handle;
they do not copy or rebuild its index. Call `resolve_at` on the prepared project or
on `prepared_workspace.scope(&id)` for repeated queries.

```rust
use java_engine::{Project, ResolveStatus};

let mut project = Project::new();
let text = "class Sample { int value; int get() { return value; } }";
project.insert("Sample.java", text);
let prepared = project.prepare();
let result = prepared.resolve_at("Sample.java", text.rfind("value").unwrap() as u32);
assert_eq!(result.status, ResolveStatus::Resolved);
```

For a loaded flat snapshot, use `snapshot.project().prepare()`. For a compilation
graph, use `workspace.prepare()` rather than preparing one compilation's local
inputs: the latter would intentionally omit dependency sources.

## Lifetime and correctness

- Preparing performs no filesystem reads, archive decoding, downloads, or Java
  execution. Inputs have already been parsed/loaded.
- Rust's shared borrow prevents mutating a `Project` while a prepared handle will
  still be used. For edits, end that borrow, change the project, then prepare again.
  A fresh filesystem snapshot similarly needs its own preparation.
- Independently loaded old/new snapshots can coexist with their own prepared
  handles. An old handle does not discover new files, changed edges, or new JARs.
- Missing and invisible source paths remain unresolved. Source declarations and
  binary hierarchies retain their original compilation context. Preparation does
  not flatten scope visibility or change ambiguous/unsupported results.
- Legacy `resolve_at` APIs still rebuild from current inputs on every call. They
  remain available for one-off queries and differential validation.

This retains index records and cloned names/member metadata for the handle's
lifetime. It trades repeated construction for retained memory. It is not a result
cache, incremental invalidation engine, content-addressed store, or cancellation
mechanism. Input loading still repeats physical archives in different contexts.
Lookup still scans collections and walks syntax/hierarchies; retaining the index
does not make every query constant-time.

## What is and is not an identity

`CompilationId` is a caller-assigned graph identity. Source paths and binary
`archive!/entry` paths are provenance. A `Target` contains that provenance, a name,
an owner spelling, and a range. None is a persistent symbol ID.

An edit can move or replace a declaration at the same path/range. A class file can
replace a previous version at the same origin. Even within one workspace, the
same binary file can link against different dependency contexts. Consequently,
equal public `Target` values do **not** prove equal semantic identities across
snapshots or linking contexts. Keep the analysis snapshot/context with results;
do not use target equality as the basis of reference search or safe rename.

Future identity work needs separate input/revision and declaration identities,
context-qualified binary instances, and explicit edit/rename/delete matching
rules. Offset hashes or sequential numbers alone would hide these requirements.
This slice deliberately adds no such IDs until those contracts and their stale-
result tests are designed. Prepared handles establish a bounded, immutable query
lifetime that this later work can build on.

## Measurement

Correctness tests compare fresh and prepared results and count index construction
directly. They impose no timing threshold. Optional versioned real-source checks
record loading, preparation, and repeated-query costs separately, with build mode,
query corpus, and parser gaps. Single-machine measurements are observations, not
an editor latency guarantee or a comparison with IntelliJ/JDT LS.
