# Explicit source selection

Source exclusions select a caller-specified flat source subset. They do not
discover a Maven/Gradle model, generate sources, enable JPMS, or infer whether the
remaining sources form a correct build. Binary classpaths and platform inputs are
unchanged. No Java executable is required.

## Core API, manifests and configure

`ProjectConfig.source_exclusions: Vec<String>` defaults to empty. Public struct
literals that do not use `..Default::default()` must now provide the field.

Each schema-v1 compilation accepts an optional `source_exclusions` string array.
Omitted or empty arrays preserve previous selection behavior; serialization omits
empty arrays. Null, nonarrays, nonstring entries, duplicate fields and unknown
fields are rejected. There is no new manifest version or flat-classpath flag.

```json
{
  "schema_version": 1,
  "compilations": [{
    "id": { "project": "app", "source_set": "main" },
    "source_roots": ["src/main/java"],
    "source_exclusions": ["module-info.java", "generated-stubs"]
  }]
}
```

The same selection can be emitted by the validated native bootstrap:

```sh
java-analyzer configure --compilation app main \
  --source-root src/main/java --exclude-source module-info.java \
  --exclude-source generated-stubs --no-platform
```

Repeat `--exclude-source PATH` within the current compilation group. Its next
argument is consumed literally, even if it is `--timeout-ms` or
`--cancel-on-stdin-close`. Rules remain relative to source roots, while configured
source-root paths follow the existing CLI path rules. Shell quoting remains the
caller's responsibility. Source-root and exclusion option order does not matter.

## Exact literal contract

Each rule applies independently relative to **every** configured source root in
the same compilation. A matched regular file is omitted, regardless of extension.
A matched real directory is pruned together with all descendants. Matching is
component-sensitive and case-sensitive; `generated` does not match
`generated-more`, and `A.java` does not match `a.java` or `nested/A.java`.
There is no globbing, negation, path normalization or Unicode normalization.

Rules are nonempty slash-separated relative paths. Empty components, `.`, `..`,
leading/trailing/repeated slashes, backslashes, colons, `* ? [ ] { }`, and Unicode
Cc controls (C0 U+0000–001F, DEL U+007F, C1 U+0080–009F) are rejected.
Spaces, Chinese text, leading `!`, leading `--`, and other literal punctuation
are permitted. Composed and decomposed Unicode spellings remain distinct.

At most 1,024 rules are accepted, each at most 4,096 UTF-8 bytes. Identical rules
are rejected; case-distinct rules, missing matches and redundant ancestor and
descendant rules are allowed. Nonempty exclusions require at least one root.

`ProjectConfig::validate_source_exclusions` is a public no-I/O validator. It
does not check root existence or matching files. Project loading calls it before
input I/O, workspace loading validates **all** compilations before any compilation
I/O, and both manifest conversion and typed supervisor configure-success
validation use the same checks. Invalid syntax uses `InvalidPath`; count/byte
bounds use `LimitExceeded`. Workspace errors retain the underlying input error.

## Traversal, budgets and ownership

Encountered entries count against the existing entry budget before selection.
Every encountered symlink is rejected, even when its name matches an exclusion.
Matched entries must have UTF-8 paths and be regular files or real directories;
matched devices, sockets and other special types fail. Descendants of a pruned
directory are not visited, counted, typechecked or read. Excluded contents do not
consume source byte budgets, and pruning avoids descendant depth checks.
Configured roots must still exist and be real directories.

Selection never applies to binary directory walks, archives or platform inputs.
Their symlink checks, entry/depth limits and byte budgets are unchanged.

Roots remain independent. With roots `src` and `src/nested` and rule `nested`,
the first root prunes `src/nested`, but the second can include
`src/nested/B.java`. Reversing root order does not change this. Including the same
canonical file twice still fails, both within one compilation and across owners.
Different compilations may select disjoint subsets of the same root.

An included `module-info.java` still fails with `UnsupportedModule`. Explicitly
omitting that exact file or pruning its parent is a source-subset decision, not
JPMS support or proof of complete build configuration.

## Snapshots, overlays and editor restarts

Reloading with changed exclusions produces a fresh immutable snapshot. Old
snapshots and prepared views remain unchanged. Excluded files are absent from
ownership and cannot be restored by unsaved overlays.

The LSP adapter freezes its manifest and source inventory. Editing exclusions on
disk or sending didChangeConfiguration does not update a live session. Restart
after changing selection. An excluded didOpen does not enroll that source;
subsequent unknown-document didChange still desynchronizes the session and
blocks navigation. This protection is not weakened by exclusions.

## Evidence boundaries

`tests/source_exclusions.rs` covers syntax, byte/count boundaries, literal Unicode,
pruning, budgets, root order, symlinks, binary isolation, ownership and immutable
reloads. Metadata-only unit tests check special-file and non-UTF-8 matched-entry
rejection without creating devices or sockets. CLI tests cover configuration,
strict schema/protocol validation, overlays and frozen LSP sessions.

Real-project comparisons and real-editor acceptance are separate gates. A native
protocol test is not evidence of actual editor navigation. Neovim acceptance runs
only in its isolated CI workflow, never on a user's computer. No optional Java
compiler oracle is needed for this feature's native test path.
