# Explicit project and JAR loading

This first M1 slice loads one flat compilation scope. A build-tool adapter can
emit this input model without coupling the analysis kernel to Maven, Gradle,
OpenIDEA, or a JVM. The library never discovers, executes, or downloads Java tools.

```rust
use java_engine::{ClasspathEntry, ProjectConfig, ProjectSnapshot};
use std::path::PathBuf;

let config = ProjectConfig {
    source_roots: vec![PathBuf::from("src/main/java")],
    classpath: vec![
        ClasspathEntry::Directory(PathBuf::from("dependency-classes")),
        ClasspathEntry::Jar(PathBuf::from("dependencies/library.jar")),
    ],
    ..Default::default()
};
let snapshot = ProjectSnapshot::load(&config)?;
// source_paths() returns canonical absolute UTF-8 identities. Query with one of
// these keys; project().file(key) gives its owned source buffer and parse errors.
// resolve_at(key, byte) uses UTF-8 byte offsets, not UTF-16 code units.
# Ok::<(), java_engine::ProjectLoadError>(())
```

The runnable fixture is `cargo run --example project`. For another local project:

```sh
cargo run --example inspect_project -- src/main/java app/Use.java 106 deps/library.jar
```

The example prints resolution status and target origins. A binary member target
has `[0..0]` source coordinates because class-file declarations have no Java text
range; this is not decompilation or source attachment support.

## Snapshot and precedence contract

- Source roots are explicit, including any test/generated roots. No JDK is assumed.
- Each load owns parsed inputs. Later edits/deletions cannot change an old snapshot.
  Load again and swap only after success. A failed load cannot partly mutate an
  earlier snapshot. No file watcher or automatic reload is included.
- Paths are canonical absolute UTF-8 paths. They survive a reload at the same
  location, not a rename. They are not stable symbol IDs.
- Binary classpath order is caller order. For the same internal binary name, the
  first entry wins, with selected and ignored origins in `shadowed_classes()`.
- Duplicate source types and source/binary collisions remain ambiguous in the
  existing binder. This conservative behavior is not a javac source-preference rule.
- A directory class's path relative to its root must match its internal name.
  JAR origins use `archive!/actual-entry`. Conflicting distinct classes that would
  share this textual identity fail explicitly rather than overwrite each other.
- Traversal is deterministic. Overlapping source roots, symlink roots/entries,
  non-UTF-8 paths/source, missing inputs, and invalid classes/JARs fail the load.
  Incomplete Java syntax is retained and available as parse errors for editor use.
- Loading is not an atomic filesystem transaction. Avoid concurrent file changes
  during a load. The implementation does not harden against hostile filesystem
  races or offer a parser CPU/stack budget.

## Archive subset and resource limits

ZIP decoding uses exact-pinned zip 9.5.0, with only its pure-Rust DEFLATE backend.
Entries may be stored or DEFLATE-compressed. There is no extraction. Duplicate
names, unsafe entry paths, unsupported compression, encryption, and symlink
entries fail. Headers are checked for consistency. Selected classes and the
manifest are decompressed with byte bounds and CRC checks; ignored resources are
not fully decompressed or CRC-verified. JAR signing is not verified.

A main-section `Multi-Release: true` selects the highest numeric version >=9 and
<= `JarOptions.target_release`; otherwise base entries are used. This also handles
version-only classes and folded manifest values. Manifest lookup is
case-insensitive, conflicting manifest variants are errors. Selected class paths
must match internal binary names. A nonempty manifest `Class-Path` is an explicit
unsupported error: dependencies must be supplied by the caller; no URLs are fetched.

`ProjectLimits` defaults: 100,000 visited directory/archive entries; 128 binary
classpath entries; 8 MiB per source/directory class file; 256 MiB total raw input;
512 MiB aggregate declared archive expansion; 64 directory levels. `JarOptions`
defaults: release17; 256 MiB compressed archive; 100,000 entries; 16 MiB per entry;
512 MiB declared total expansion. Project loading clamps archive entry/expansion
limits to the remaining project budget, including ignored resources and versions.
These are configurable safety ceilings, not measured latency or memory budgets.

## Still unsupported

- JDK discovery, JMOD/jimage/ct.sym inputs, JPMS graphs and visibility, automatic
  module semantics, nested application JARs, dependency downloads, Maven/Gradle import.
  `module-info.java` fails explicitly. Binary module descriptors are ignored on
  this flat classpath, as opposed to being interpreted as a module path.
- `target_release` selects archive variants only. It does not enforce Java syntax
  level, class-file compatibility with that release, or `--release` platform APIs.
- Full generics, overload applicability, static/instance distinction, references,
  completion, semantic diagnostics, and safe rename remain roadmap work.
- Incremental project reloads, cached indexes, cancellation, and real-project
  performance guarantees are not implemented.

JAR/class bytes are ordinary input data. Native analysis requires neither a Java
executable, a JVM library, JNI, JDT LS, nor a Java sidecar. Java compilation,
application execution, and debugging are separate responsibilities.
