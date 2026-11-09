# Explicit source visibility fixture

This original fixture is loaded by `tests/workspace.rs` and `examples/scopes.rs`.
No Java compiler or JVM is required or invoked. The caller supplies these edges:

- `app/test` depends on `app/main`.
- `app/main` depends on `lib/main`.
- `lib/test` depends on `lib/main`.
- `unrelated/main` has no dependencies.

Source visibility is transitive only along the directed edges. `App` and
`AppTest` can resolve the inherited `lib.Lib.value` field. `AppTest` deliberately
contains unresolved references to `lib.LibTest` and `unrelated.Other`, which
exist on disk but are not visible from `app/test`. The fixture exercises editor
analysis of both valid and unresolved references; it is not claimed to compile
as a complete Java project. No platform or binary libraries are configured here.
