# Explicit native configuration bootstrap

`java-analyzer configure` turns caller-supplied project inputs into the existing
[schema-v1 manifest](CLI.md). It validates the graph and loads every configured
input with the same bounded native loader used by `inspect` before emitting a
manifest. It does not discover a build model, dependencies or a JDK, run a build,
generate Java sources, download files, or improve semantic coverage.

## Start with explicit source roots

Run from the repository root after building the optional CLI:

```sh
target/release/java-analyzer configure \
  --compilation lib main --source-root examples/cli_project/lib/main --no-platform \
  --compilation app main --source-root examples/cli_project/app/main --no-platform \
    --depends-on lib main \
  --compilation app test --source-root examples/cli_project/app/test --no-platform \
    --depends-on app main
```

Success writes only the manifest JSON and a newline to stdout. Save that output
to a new file to use with `inspect`, `declaration`, `parser-report`, or the LSP
adapter's manifest initialization option. The command itself never creates or
replaces a file. Shell redirection happens before validation and can truncate an
existing file, so do not redirect over an existing configuration. For example,
POSIX shells can use `(set -C; java-analyzer configure ... > new-project.json)` to
refuse an existing destination. A failed command can still leave an empty file
created by the shell; check its exit status before using it.

The output contains absolute paths derived from the invocation directory. Saving
it elsewhere does not reinterpret its inputs. It is machine-local configuration,
not a portable checkout manifest. Handwritten manifests may still use the existing
manifest-relative path rules. Declaration request documents and `--file` still
resolve their source paths relative to the manifest directory; use absolute source
paths when reusing queries beside a relocated manifest. Configured symlink leaves and encountered symlink
entries remain rejected; this command does not canonicalize them away first.
Parent-directory aliases retain the loader's existing behavior.

## Compilation groups

Every option after `--compilation PROJECT SOURCE_SET` belongs to that group until
the next `--compilation`. Both ID components are separate, opaque, nonempty,
whitespace-free arguments; punctuation is not a hierarchy separator.

- `--source-root PATH`: repeat for explicit recursive source directories, or use
  `--binary-only` to explicitly choose no source roots. These choices conflict.
- `--exclude-source PATH`: repeat literal file/directory exclusions for the current
  compilation. Each rule is relative to every source root, regardless of option
  order. Nonempty exclusions require roots and cannot be used with binary-only
  groups. No globbing or negation is supported. See [source selection](SOURCE_SELECTION.md).
- `--depends-on PROJECT SOURCE_SET`: repeat for directed **source** visibility.
  This does not export another compilation's binary or platform dependencies.
- `--classpath-jar PATH` and `--classpath-dir PATH`: repeat in precedence order.
  The first binary definition wins under the existing loader rules.
- `--platform-jmod PATH` and `--platform-jar PATH`: explicitly supply platform
  class data in precedence order, or use `--no-platform`. Each group must make
  this choice; platform inputs are not inherited from another group.
- `--target-release N`: positive `u16` selection for multi-release JAR entries,
  default 17. This is not a Java source level, a compiler `--release` model, or
  proof that the supplied platform matches that release.

Relative input paths resolve against the current working directory. Paths and ID
components that look like execution flags are still consumed as option values.
Arguments are not shell expressions and no wildcard or environment expansion is
performed by the executable. Normal shell quoting and expansion happen outside
it. Missing options, duplicate singleton choices and contradictory choices fail.

For a real platform, replace `--no-platform` with an explicit input such as
`--platform-jmod /path/to/jdk/jmods/java.base.jmod`. JMOD/JAR files are read as data;
the native command never executes their contents or a Java tool. Additional
platform modules and external dependencies must also be supplied explicitly.

## Validation and boundaries

The command uses the existing default per-input and workspace budgets. It checks
graph identities/edges, source ownership, source and archive inputs, and path
types through the normal manifest adapter and native loader. It cannot certify
that a caller chose the complete or correct build configuration. Successful load
is not proof that Java source compiles, that parser gaps are absent, or that every
declaration query can resolve. Included `module-info.java` remains unsupported:
use an explicit `--exclude-source module-info.java` only when deliberately choosing
a flat source subset. This does not add JPMS or generated-source production.

Output is deterministic for unchanged arguments and inputs, with no snapshot ID
or timestamp. Its complete encoded document, including the newline, must fit the
existing 1 MiB manifest-document limit. The supervised worker's existing timeout,
cancellation, bounded capture and error contract also apply. Validation, loading
or supervision failure emits no manifest bytes to stdout and returns the existing
JSON error on stderr with exit status 2. Final output delivery can fail after a
prefix has been written, as described in the [execution contract](CLI_EXECUTION.md).
Always check the exit status. Limits are not weakened to make a project load.

Arguments have a separate 1 MiB bound, counting each UTF-8 argument plus one byte
for its separator, including the command and execution flags at the public entry
point. Exceeding it returns `document_limit`. The operating system may impose a
smaller argument-list or per-argument limit before the executable can start.

Loading proves the inputs were valid during this invocation, not an atomic
filesystem transaction or a future guarantee. Other commands load them again.
Changing a saved manifest or adding/removing sources still requires restarting
the [LSP session](LSP_PREVIEW.md); bootstrap is not a reload mechanism.

The new entry point removes manual JSON construction for explicit inputs. General
build import, generated inputs, project discovery and semantic completeness
remain separate work. Empty exclusions are omitted from generated manifests;
nonempty values are preserved literally, including values resembling execution
flags such as `--timeout-ms` or `--cancel-on-stdin-close`.

## Acceptance

`tests/cli_configure.rs` exercises the shipped binary with an empty Java
environment, including multi-compilation visibility, exact source navigation,
typed archive provenance, manifest relocation, invalid inputs, output/input
bounds, cancellation, and consuming the generated manifest through LSP initialize.
Supervisor unit tests reject malformed manifest success responses without
reloading any inputs outside the worker deadline.

With the existing checksum-pinned acceptance inputs already available locally:

```sh
python3 scripts/check-configure-acceptance.py \
  /path/to/acceptance/sources.jar /path/to/acceptance/java.base.jmod \
  /path/to/second-project --native target/debug/java-analyzer
```

This separate offline gate generates Commons Lang and all four Gson compilation
configurations through the native command. Independent, pre-existing manual
models supply the expected roots, graph, ordered binaries, platform data and
budgets. Complete owned/visible source inventories, historical declaration
expectations, parser-gap records, and source hashes must remain unchanged. Missing
generated `GsonBuildConfig` retains `Unresolved` for its type and `ReceiverNotBound`
for its `VERSION` field access, with no invented targets or manufactured source.
The upstream Java template remains outside the selected source roots and its
original bytes are preserved.

Commons fits full binary-origin inspection. Gson's complete four-scope detailed
inventory exceeds the existing 10000-item response limit. Both manual and
generated configurations must preserve that refusal and agree on the full-graph
summary. Separate, explicitly labeled binary-only projections compare each
scope's binary-origin sequence without claiming a detailed whole-graph response.
Relocated manifests are inspected directly. Identical generated bytes are also
saved beside the historical request documents for their unchanged relative-path
queries. Offline mutation tests check that the comparator catches changed scope,
ordering, inventories, budgets, parser gaps, malformed output and file writes.

The `extended-acceptance` workflow runs this native gate separately from its
optional compiler oracles. Passing it is configuration-preservation evidence,
not a new Java-conformance or interactive-performance claim.
