# Explicit project, platform, and archive loading

These early M1 slices load one flat compilation scope. A build-tool adapter can
emit this input model without coupling the analysis kernel to Maven, Gradle,
OpenIDEA, or a JVM. The library never discovers, executes, or downloads Java tools.

```rust
use java_engine::{ClasspathEntry, PlatformEntry, ProjectConfig, ProjectSnapshot};
use std::path::PathBuf;

let config = ProjectConfig {
    source_roots: vec![PathBuf::from("src/main/java")],
    platform: vec![PlatformEntry::Jmod(PathBuf::from("platform/java.base.jmod"))],
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
- Platform entries are processed in caller order before application classpath
  entries (also in caller order). For the same internal binary name, the first
  binary wins, with selected and ignored origins in `shadowed_classes()`.
  Platform inputs may be `PlatformEntry::Jmod` or `PlatformEntry::Jar` (e.g. rt.jar).
  An empty platform list loads no implicit classes and performs no discovery.
- Duplicate source types and source/binary collisions remain ambiguous in the
  existing binder. This conservative behavior is not a javac source-preference rule.
- A directory class's path relative to its root must match its internal name.
  JAR origins use `archive!/actual-entry`; JMOD origins retain
  `archive!/classes/actual-entry`. Conflicting distinct classes that would share
  this textual identity fail explicitly, even if an earlier binary shadows one.
- Traversal is deterministic. Overlapping source roots, symlink roots/entries,
  non-UTF-8 paths/source, missing inputs, and invalid classes/JARs/JMODs fail the load.
  Incomplete Java syntax is retained and available as parse errors for editor use.
- Loading is not an atomic filesystem transaction. Avoid concurrent file changes
  during a load. The implementation does not harden against hostile filesystem
  races or offer a parser CPU/stack budget.

The optional platform example takes only an explicit local JMOD path:

```sh
cargo run --locked --example platform -- /path/to/java.base.jmod
# After building, the same executable runs without Java or environment discovery:
env -i PATH=/nonexistent target/debug/examples/platform /path/to/java.base.jmod
```

It loads a checked-in source fixture and resolves implicit `java.lang.String` and
inherited `Object.toString()`, printing the class count and actual archive origins.
The JMOD is caller-provided data, not a runtime installation requirement.

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

JMOD 1.0 inputs have a `JM 01 00` header followed by ZIP data. Only `.class`
entries beneath `classes/` are selected; `classes/module-info.class` and
`classes/META-INF/` resources are ignored. All sections still undergo metadata
validation and count toward limits, including native libraries, commands, and
legal resources. Nothing is extracted, loaded as executable code, or run.
Selected class names must match their paths below `classes/`. JMODs do not use
JAR manifests or multi-release selection. Descriptors, JPMS exports, readability,
module dependencies, and platform release identity are not interpreted.

`ProjectLimits` defaults: 100,000 visited directory/archive entries; 128 binary
platform-plus-application entries; 8 MiB per source/directory class file;
256 MiB total raw input;
512 MiB aggregate declared archive expansion; 64 directory levels. `JarOptions`
defaults: release17; 256 MiB compressed archive; 100,000 entries; 16 MiB per entry;
512 MiB declared total expansion. `JmodOptions` has the same archive/count/total
bounds but a 64 MiB per-entry ceiling (to allow larger ignored native sections)
and no target release. Project loading clamps archive byte/count/expansion limits
to the remaining project budget across both platform and application archives,
including ignored resources, versions, and nonclass JMOD sections. The four-byte
JMOD header counts as input bytes. Archive limit/read errors retain the JAR/JMOD
error kind and archive origin. All inputs must fit the configured project bounds.
These are configurable safety ceilings, not measured latency or memory budgets.

## Still unsupported

- JDK discovery, jimage/ct.sym inputs, JPMS graphs and visibility, automatic
  module semantics, nested application JARs, dependency downloads, Maven/Gradle import.
  `module-info.java` fails explicitly. Binary module descriptors are ignored on
  this flat classpath, as opposed to being interpreted as a module path.
- `target_release` selects archive variants only. It does not enforce Java syntax
  level, class-file compatibility with that release, or `--release` platform APIs.
- Full generics, overload applicability, static/instance distinction, references,
  completion, semantic diagnostics, and safe rename remain roadmap work.
- Incremental project reloads, cached indexes, cancellation, and real-project
  performance guarantees are not implemented.

JAR/JMOD/class bytes are ordinary input data. Native analysis requires neither a Java
executable, a JVM library, JNI, JDT LS, nor a Java sidecar. Java compilation,
application execution, and debugging are separate responsibilities.

After `cargo build --locked --examples`, run
`sh scripts/check-platform-runtime.sh /path/to/java.base.jmod` for the optional
platform acceptance check with an empty environment and no executable search path.
