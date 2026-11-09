# Experimental native project CLI

`java-analyzer` is an opt-in, one-shot project adapter around the native Rust
library. It loads a manually configured compilation graph, inspects its inputs,
performs limited declaration navigation, and reports parser gaps. It does not
start Java, JNI, JDT LS, Maven, Gradle, or a network client. Explicit JAR/JMOD inputs
are data, not executable tools. No JDK or dependency is discovered implicitly.

This implements part of the [preview plan](PREVIEW_PLAN.md), **not all preview
gates**. It has no persistent server, reload transaction, unsaved overlay, LSP,
distribution certification, or measured project-scale performance budget. It now
supervises a separate native worker with a default 30-second wall-clock timeout,
optional stdin-EOF cancellation and machine-readable crash/output failures. See
[execution bounds and remaining limits](CLI_EXECUTION.md). Use trusted local
inputs: this is not a security sandbox, CPU quota or whole-process memory limit.

## Build and run the checked-in project

Use the repository's pinned Rust toolchain and lockfile. From the repository root:

```sh
cargo build --locked --release --features cli --bin java-analyzer
target/release/java-analyzer inspect --manifest examples/cli_project/project.json
target/release/java-analyzer declaration --manifest examples/cli_project/project.json \
  --requests examples/cli_project/requests.json
target/release/java-analyzer parser-report --manifest examples/cli_project/project.json
```

The three batch queries return `Resolved`, `Resolved`, `OverloadNotResolved`.
Both resolved queries navigate to `value` in `lib/main/lib/Library.java`, bytes
`52..57`. The refusal keeps both `choose` candidates without selecting an overload.
The graph is `app/test -> app/main -> lib/main`. Main does not see test sources.
No platform data is necessary for these examples.

A single query into the same sample:

```sh
target/release/java-analyzer declaration --manifest examples/cli_project/project.json \
  --project app --source-set main --file app/main/app/App.java --byte 85
```

Or install from the checked-out source with
`cargo install --path . --locked --features cli --bin java-analyzer`.
This is a source build route, not a signed/prebuilt binary distribution.
The package is not published on crates.io. `cargo build` without `--features cli`
continues to build the library without the CLI serialization dependencies.

`--help` and `--version` each return JSON containing version, command usage and
capability boundaries. No text-output mode is provided in schema version 1. Every
command accepts `--timeout-ms N` (ASCII decimal digits, 1..300000; default 30000)
and optional `--cancel-on-stdin-close`. Duplicates and invalid values fail. No
unlimited timeout is available. Read both output streams concurrently; final
output delivery is outside the worker deadline. See [execution contract](CLI_EXECUTION.md).

## Commands and path rules

- `inspect --manifest FILE [--details]`: loaded inputs, visibility, shadowed binaries
  and effective configuration, sorted by compilation ID. By default, binary inputs
  are summarized by `class_count`; `--details` requests the full `class_origins`
  inventory. This flag is accepted only by `inspect`.
- `declaration --manifest FILE --project ID --source-set ID --file PATH --byte N`:
  one query. Substitute `--utf16 N` for `--byte N`; exactly one is required.
- `declaration --manifest FILE --requests FILE`: ordered batch, validated before
  any result is returned, with one workspace load and one prepared index.
- `parser-report --manifest FILE [--project ID --source-set ID]`: parser inventory
  of **owned** sources only, not a repeated inventory of dependency sources.

`--manifest` and `--requests` locate JSON documents relative to the invoking
working directory, or accept absolute paths. **Every data path inside either
document, and the single-query `--file`, is relative to the canonical manifest's
directory**, not the requests file's directory or the current working directory.
Explicit absolute data paths are accepted. There is no upward manifest search or
default source root. For running from an arbitrary directory, pass absolute paths
to both JSON documents. Empty paths and NUL in paths are rejected.

Document files and configured input leaves must be regular files/directories,
not symlinks. The core rejects symlink entries encountered under source/class
roots. Parent-directory symlinks are canonicalized, not a filesystem sandbox.
A **query-only alias**, including a symlink or `./`/`../`, is accepted if its
canonical path is already a loaded source visible in the requested compilation.
It cannot add a new source. Output source paths are canonical absolute UTF-8 paths.

## Version 1 project manifest

The root object and every nested object reject unknown and duplicate fields.
Comments, trailing commas, invalid UTF-8, noninteger limits and out-of-range
integers are errors. IDs contain separate nonempty whitespace-free `project`
and `source_set` strings. Their punctuation is opaque, not split on `/` or `:`.

```json
{
  "schema_version": 1,
  "compilations": [
    {
      "id": { "project": "app", "source_set": "main" },
      "source_roots": ["src/main/java"],
      "dependencies": [{ "project": "library", "source_set": "main" }],
      "classpath": [
        { "kind": "directory", "path": "build/dependency-classes" },
        { "kind": "jar", "path": "deps/dependency.jar" }
      ],
      "platform": [{ "kind": "jmod", "path": "jdk/jmods/java.base.jmod" }],
      "target_release": 17,
      "limits": { "max_file_bytes": 8388608 },
      "jar_limits": { "max_entries": 100000 },
      "jmod_limits": { "max_entry_bytes": 67108864 }
    },
    {
      "id": { "project": "library", "source_set": "main" },
      "source_roots": ["library/src/main/java"]
    }
  ],
  "limits": { "max_compilations": 128 }
}
```

This illustrative manifest requires those actual paths to exist. Use the
checked-in sample above for an immediately runnable configuration.

Required: `schema_version`, nonempty `compilations`, each compilation's `id` and
`source_roots`. An explicitly empty `source_roots` array is allowed for a binary-only
compilation. Optional `dependencies`, `classpath` and `platform` default to empty
arrays; `target_release` defaults to 17. Platform entries accept `jmod` or `jar`;
classpath entries accept `directory` or `jar`. Platform entries precede application
binaries, in configured order. First binary definition wins; shadowed origins are
reported. Source collisions remain ambiguous.

Dependencies export reachable **source** declarations transitively. Each
compilation owns its binary/platform context; dependencies do not export binaries.
Duplicate IDs/edges, unknown dependencies, cycles and canonical source ownership
by multiple compilations fail the load. Source `module-info.java` fails explicitly:
this is a project/source-set graph, not JPMS. For loading semantics see
[project inputs](PROJECT_LOADING.md) and [prepared analysis](PREPARED_ANALYSIS.md).

`target_release` must be positive and selects multi-release JAR entries only.
It is **not** a parser language level or a `javac --release` API model. Platform
JMODs are flat class-library data; module readability/exports are not enforced.

### Limit fields and defaults

Every listed limit object is optional; omitted fields retain their own defaults,
including partial `jmod_limits` overrides retaining the 64 MiB entry limit.
Unlisted fields fail. Values are unsigned integers.

| Object | Field | Default |
| --- | --- | ---: |
| root `limits` | `max_compilations` | 128 |
| root `limits` | `max_dependency_edges` | 1024 |
| root `limits` | `max_entries` | 200000 |
| root `limits` | `max_input_bytes` | 536870912 |
| root `limits` | `max_archive_expanded_bytes` | 1073741824 |
| compilation `limits` | `max_entries` | 100000 |
| compilation `limits` | `max_classpath_entries` | 128 |
| compilation `limits` | `max_file_bytes` | 8388608 |
| compilation `limits` | `max_input_bytes` | 268435456 |
| compilation `limits` | `max_archive_expanded_bytes` | 536870912 |
| compilation `limits` | `max_directory_depth` | 64 |
| `jar_limits`, `jmod_limits` | `max_archive_bytes` | 268435456 |
| `jar_limits`, `jmod_limits` | `max_entries` | 100000 |
| `jar_limits` | `max_entry_bytes` | 16777216 |
| `jmod_limits` | `max_entry_bytes` | 67108864 |
| `jar_limits`, `jmod_limits` | `max_total_uncompressed_bytes` | 536870912 |

`max_dependency_edges`, `max_classpath_entries` and `max_directory_depth` may be
zero to forbid edges/binary entries/subdirectories. Other budgets must be positive.
CLI graph ceilings are 128 compilations and 1024 dependency edges: callers can
lower but not raise these two limits. `max_file_bytes` cannot exceed `u32::MAX`.
Other configurable budgets may be raised, potentially causing substantial CPU
and memory use; do not treat them as hostile-input process containment.

Fixed adapter limits: each manifest/request document is at most **1 MiB**, a batch
contains **1..1024 queries**, and a response permits at most **10000 result items**
and **16 MiB serialized UTF-8 JSON including its final newline**. Result items count
compilation records, owned/visible source-path references, requested binary origins,
shadowed records, query records, candidates and parser gaps as applicable.
Counts are aggregate, not per query/file. Exceeding a cap fails the command without
partial stdout. There is no silent truncation. Narrow the manifest, parser filter
or batch if necessary. Default inspection reports exact binary counts without
binary-origin lists, so main/test scopes can each load the same large platform.
Detailed inspection may still exceed the cap, for example two copies of a
7615-class platform produce 15230 origin records. Such a request fails explicitly;
it does not silently truncate or switch to summary mode.

Input byte/archive expansion limits remain the core loader's limits. Parsed
trees, preparation, resolver candidate lists and parser error collection can
allocate before the output limit is checked. The CLI worker isolates ordinary
parser crashes from the supervisor and is subject to a wall-clock deadline. This
does not impose maximum total process memory or make filesystem loading atomic. Supervisor memory exhaustion or
external termination can still prevent a JSON error. A consumer must not run this
preview synchronously on an editor UI thread or expose it as a trusted service for
hostile inputs. [Execution details](CLI_EXECUTION.md) distinguish worker deadlines,
cleanup grace, parent-death handling, and final-output backpressure.

## Requests and position semantics

```json
{
  "schema_version": 1,
  "queries": [
    {
      "compilation": { "project": "app", "source_set": "main" },
      "file": "app/main/app/App.java",
      "position": { "encoding": "utf8", "offset": 85 }
    }
  ]
}
```

All fields are required; unknown/duplicate fields fail. `encoding` is exactly
`utf8` or `utf16`. Positions are zero-based **absolute** offsets in the entire
unchanged loaded file: bytes for UTF-8, code units for UTF-16. They are not LSP
line/character pairs. CR and LF each occupy one byte/code unit. An astral character
occupies four UTF-8 bytes and two UTF-16 code units. Interior UTF-8 byte positions,
interior surrogate pairs, EOF, negative offsets and offsets above `u32::MAX` fail.
Choose an offset on an identifier for declaration lookup. Whitespace or a valid
nonidentifier position may legitimately produce `Unresolved`.

Syntax/version/ID shape and batch-count checks occur before loading the manifest's
project inputs. Unknown configured compilation IDs also fail before loading.
All query paths, visibility and position bounds are then checked against the
loaded snapshot **before preparation**. An invalid query fails the whole batch
with `query_index`; no valid prefix is returned. A dependency source may be queried
from a consumer, but binding uses that source's owner context. Outside/invisible
files are `invisible_source` errors, not ordinary `Unresolved` results.

## Responses and identity

Successful commands emit one JSON object on stdout, ending with a newline:

```json
{
  "schema_version": 1,
  "command": "declaration",
  "snapshot": {
    "id": "invocation-local-token",
    "identity": "invocation_local",
    "manifest": "/absolute/project.json"
  },
  "capabilities": { "experimental": true },
  "result": {
    "index_builds": 1,
    "queries": []
  }
}
```

This is a shortened envelope example, not an actual zero-query response.
`capabilities` explicitly reports limited binding, absent general method applicability,
absent semantic diagnostics, parser classification, native runtime, missing
server/reload/overlay facilities, native worker execution bounds, and adapter
limits. Snapshot ID is an **invocation-local label** only, not a content hash, persistent symbol ID,
filesystem revision or cross-process equality test. Every batch query belongs to
that one loaded snapshot. A new invocation reloads disk inputs and prepares anew;
no index survives between processes.

Declaration query records contain `query_index`, requested `compilation`,
`source_compilation`, canonical `file`, original `position`, converted
`byte_offset`, `status` and ordered `targets`. Candidates are not deduplicated or
reordered by the adapter. Each target has `kind`, `name`, `owner` and `origin`:

- **Source:** `origin:"source"`, `source_available:true`, canonical `path`, owning
  `compilation`, and `range` with `start_byte`, `end_byte`, `start_utf16`, `end_utf16`.
  Ranges are half-open and cover the exact declaration identifier in that snapshot.
  The adapter verifies that the source slice equals `name` before reporting it.
- **Binary:** `origin:"binary"`, `source_available:false`, `path` preserving the
  class-directory or `archive!/entry` origin, `binary_name`, `loaded_contexts`,
  `linking_context:null`, and `identity_precision:"origin_kind_owner_name_only"`.
  **No `range` or synthetic `0..0` coordinates are emitted.** `loaded_contexts`
  names exactly where that origin was loaded, not the context selected during
  binding. The core target does not expose that context or a member descriptor.
  Same-looking candidates can be distinct; do not invent exact binary symbol
  identity or use this response to choose an overload. No source attachment or
  decompilation is implemented.
- **Package:** `origin:"package"`, `source_available:false`, without `path` or
  `range`. A package prefix is not a source or binary declaration.

`inspect.result` contains `detail_mode` (`summary` by default, `full` with
`--details`), `workspace_limits` and sorted `compilations`, each with `id`,
`configuration` (effective defaults; configured path spellings remain
manifest-relative), canonical `source_paths`, `visible_source_paths`, `class_count`,
and `shadowed_classes` records containing `binary_name`, `selected_origin` and
`shadowed_origin`. Only `full` mode includes a `class_origins` array; summary mode
omits that field rather than returning a partial or empty inventory.

`parser-report.result` contains `classification:"parser_limit_or_invalid_syntax"`,
`java_semantic_diagnostics:false`, `scope:"owned_sources_only"`, `source_count`,
`files_with_reports`, `report_count`, and `files`. Every owned file, including
those with zero reports, has `path`, `compilation`, and `reports`. Each report has
the same `classification`, parser `message`, and a half-open byte/UTF-16 `range`.
Unsupported grammar and invalid syntax are not separated reliably by this parser.
A report does not prove the source is invalid Java; zero reports do not prove
semantic correctness. There are no type, unresolved-name or overload diagnostics.

### Binding statuses

Statuses retain the library's names. See [binding boundaries](../README.md) for
supported lookup/access/signature rules:

- `Resolved`: one limited navigation candidate. **Not proof of method argument
  applicability, accessibility under every Java rule, or successful compilation.**
  The [primitive source-call subset](PRIMITIVE_CALLS.md) has a stronger supported
  argument proof, but that guarantee does not extend to all Resolved results.
- `Unresolved`: no candidate within this configured subset, including missing
  implicit-platform types. Not a compiler diagnostic.
- `Ambiguous`: conflicting candidates; no first-candidate selection.
- `OverloadNotResolved`: no unique winner established by supported call selection;
  original candidates remain, including unsupported cases.
- `CallNotResolved`: a single source invocation was not proved applicable. The
  candidate may be known contradictory or merely unsupported, such as boxing or
  expanded varargs in the closed source-call subset. It is not a Java error
  diagnostic and must not be treated as successful navigation. A uniquely collected
  unqualified source `m()` queried by `m(1)` is conservatively refused; qualified,
  binary and inherited-hidden-singleton guards remain outside this change.
- `SignatureNotCompared`: unsupported/missing/ambiguous signature type identity.
- `ReceiverNotBound`, `StaticImportNotApplied`, `ConstructorRefNotBound`:
  explicitly unsupported receiver/static-import/constructor-reference binding.
- `Cycle`: encountered supertype cycle.
- `NotDetermined`: contradictory access flags prevent a justified result.
- `Package`: package prefix only.

`CallNotResolved` is an added public library enum variant; exhaustive client
matches must handle it. CLI schema version stays 1, with the additive status and
`capabilities.limited_call_selection` fields documented here, including
`primitive_source_call_returns` for [proved scalar result propagation](CALL_RETURNS.md). The existing
`method_argument_applicability:false` flag still means general applicability is
not provided. Check status before using a candidate as a navigation target.

### Exit codes and errors

Exit **0** means a command produced a valid result, including unresolved,
ambiguous/refused queries or parser reports. It never means Java compilation
succeeded. Errors exit **2**, write nothing to stdout before a completed response,
and emit one JSON object on stderr:

```json
{
  "schema_version": 1,
  "error": {
    "code": "invalid_position",
    "message": "position is outside source, at EOF, inside UTF-8 bytes or inside a UTF-16 surrogate pair",
    "path": "/absolute/A.java",
    "query_index": 0
  }
}
```

Stable codes: `usage`, `io`, `invalid_path`, `invalid_json`, `unsupported_version`,
`invalid_configuration`, `document_limit`, `request_limit`, `unknown_compilation`,
`load_failed`, `invisible_source`, `invalid_position`, `invalid_target`,
`output_limit`, `output_io`. Supervision adds `execution_timeout`,
`execution_cancelled`, `execution_io`, `worker_failed`, `worker_output`, and
`worker_output_limit`; see [execution errors](CLI_EXECUTION.md#machine-readable-failures).
`path` and zero-based `query_index` are included when
applicable; prose `message` is explanatory, not a machine-stable protocol.
`invalid_target` means the adapter could not verify the core target's provenance
or exact source range, so no declaration result was emitted. Loader details
retain the offending source/archive path where available. A broken output pipe
can leave an OS-written stdout prefix before `output_io`; transport errors are
not the all-or-nothing validation guarantee. Worker aborts/signals are mapped to
`worker_failed`. Termination or exhaustion of the supervisor itself remains outside this JSON error contract. Supervisor
errors include `worker_reaped` when a worker was started; `false` means cleanup
was not confirmed within its bounded grace.

## Validation

`tests/cli.rs` runs the actual executable with an empty environment and an
unrelated working directory. Coverage includes compilation visibility, prepared
batch reuse, source ownership/ranges, binary and package provenance, ambiguity,
UTF-8/UTF-16/CRLF/astral positions, strict JSON/configuration, missing dependencies,
invalid UTF-8, document/result caps, summary/detailed inspection across repeated
platform contexts, symlink aliases and the checked-in sample.
Run `cargo test --locked --features cli --bin java-analyzer --test cli` for the
CLI contract plus deterministic supervision tests, real timeout/cancellation,
recursive parser crash isolation, and Linux parent-death coverage.
The [fixed-source CLI acceptance](../examples/cli_acceptance/README.md) is a separate
optional real-project gate, not a replacement for native tests or full conformance.
