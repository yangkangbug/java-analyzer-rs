# java-analyzer-rs

An independent Rust Java analysis project, extracted from OpenIDEA's
`crates/java-engine`. The Cargo package remains `java-engine` so existing
`java_engine` imports continue to work. This is an early analysis kernel,
not a complete Java language server or an IntelliJ-equivalent engine.

It parses a Java syntax subset, reparses balanced blocks after edits, reads
class-file signatures, and resolves some names across registered source and
class files. The analysis library and its tests do not start a JVM.

## Build and try it

Install Rust 1.96.0 with rustfmt and Clippy. The pinned `rust-toolchain.toml`
selects it. Cargo downloads the locked registry dependencies on the
first build; no OpenIDEA checkout, Node, Tauri, JDK, or JDT LS is required.

```sh
cargo test --locked
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo run --locked --example resolve
```

Default direct dependencies are `cafebabe` 0.9.0 for class files and exact-pinned
`zip` 9.5.0 with pure-Rust DEFLATE for archives. The optional `cli` feature adds
JSON serialization at the executable boundary. See [third-party notices](THIRD_PARTY_LICENSES.md).
`publish = false` is intentional; this package has not been published to crates.io.
The independent source repository is
[yangkangbug/java-analyzer-rs](https://github.com/yangkangbug/java-analyzer-rs).

The [unsaved-buffer example](examples/editor_buffers/README.md) adds caller-tagged
whole-text replacements and UTF-16 line/character queries to the one-shot CLI.
A separate [definition-only LSP preview](docs/LSP_PREVIEW.md) connects those
snapshots to an editor. The [Neovim companion](editors/neovim/README.md) guards
navigation against edits in any attached buffer, including responses already in
transit. It requires explicit project configuration and does not add completion,
references, rename, hover or semantic diagnostics. [Native macOS CI](docs/MACOS_NATIVE.md) is a separate
source-build/runtime gate, not a signed Mac distribution or user-device validation.
The [archive-reuse acceptance plan](docs/ARCHIVE_REUSE.md) addresses repeated
dependency decoding within a project load without adding a cross-request cache
or changing the requirement for explicit project configuration.
The [explicit editor refresh contract](docs/EDITOR_REFRESH.md) serializes session
replacement after source/configuration changes and replays unsaved buffers before
navigation becomes ready. It does not add automatic reload or source generation.
The [local-type containment boundary](docs/LOCAL_TYPES.md) repairs surrounding
variable navigation after unsupported method-local type declarations without
publishing those types as ordinary class members.
The [reference instance-call proof](docs/REFERENCE_CALLS.md) handles a bounded
set of explicit local/parameter, proved zero-argument construction, and ordinary
string-literal arguments with complete inheritance and access evidence. The
[string-call gate](examples/second_project/STRING_CALLS.md) checks exact real-source
navigation while retaining unsupported overloads. Missing evidence stays refused, even for a unique candidate.
The [strict reference-phase gate](examples/second_project/STRICT_REFERENCE_PHASE.md)
additionally proves Gson `remove(a)` despite its scalar `int` competitor, without
adding unboxing, integer-argument typing or chained-call inference.
A later [single-primitive-argument instance-call proof](docs/QUALIFIED_PRIMITIVE_CALLS.md)
adds explicit scalar locals/parameters and bounded decimal `int` literals, without
boxing, mixed-argument or call-result receiver inference.
The subsequent [mixed strict-invocation proof](docs/MIXED_STRICT_CALLS.md) combines
those existing argument types across multiple positions without boxing, generic
inference, conversion scoring or call-result receiver propagation.

The later [one-layer call-result receiver proof](docs/CALL_RESULT_RECEIVERS.md)
composes those strict calls through an explicit nongeneric reference return,
with declaration-context lookup and one shared budget. Call-result arguments,
general expression inference and deeper chains remain unsupported.
The subsequent [plain-null strict-argument proof](docs/NULL_STRICT_CALLS.md)
adds the null invocation conversion to validated reference formals only. Formal
specificity and complete competitor checks remain unchanged; unknown arguments,
arrays, generics, boxing and additional expression inference are not enabled.
That phase's sole new real-Gson success was `set(0, null)`; its predicted
`add(propertyName, null)` target was explicitly deferred rather than counted as
success. The later [exact binary argument compatibility proof](docs/BINARY_ARGUMENT_COMPATIBILITY.md)
addresses the ordinary binary-root cross-scope gap using selected-origin and
complete modeled inheritance-closure evidence, without global type unification
or cross-scope subtype widening. Independent signature comparison is budgeted;
small complete closures support both shared and independent parses, while shared
signatures can allow proofs that oversized independent copies must refuse.

### Experimental project CLI

Configure explicit compilations, source roots, local dependencies and platform
data without writing Rust. The checked-in sample includes a library and an
application's separate main/test roots:

```sh
cargo build --locked --release --features cli --bin java-analyzer
target/release/java-analyzer inspect --manifest examples/cli_project/project.json
target/release/java-analyzer declaration --manifest examples/cli_project/project.json --requests examples/cli_project/requests.json
target/release/java-analyzer parser-report --manifest examples/cli_project/project.json
```

The CLI emits versioned JSON. `inspect` summarizes binary counts; add `--details`
for the complete origin inventory under explicit output limits. A declaration
batch shares one prepared snapshot.
The native [configuration bootstrap](docs/CLI_CONFIGURE.md) emits a validated
manifest from explicit compilation groups, roots and ordered dependency inputs,
without hand-writing JSON. Optional [literal source exclusions](docs/SOURCE_SELECTION.md)
omit explicit files or prune directories relative to each source root. They do not
infer build configuration, generate Java, or enable JPMS.
Source targets retain exact byte ranges; binary targets explicitly have no source
range. Refused or ambiguous results remain candidates, not a guessed first match.
Parser reports may indicate unsupported grammar rather than invalid Java.
This is limited declaration navigation with narrow checked primitive-call and
reference-instance-call subsets, not general applicability, semantic diagnostics,
a build importer or a complete editor language server. Inputs are local and
explicit; no Java executable is called or JDK discovered.

For an explicitly supported installation/build path and Linux archive boundary,
see [CLI installation](docs/CLI_INSTALL.md). The [preview evaluation protocol](docs/CLI_PREVIEW.md)
separates reproducible checks, observed resource use and remaining usability gaps.

See the [command/configuration contract](docs/CLI.md) and
[remaining preview gates](docs/PREVIEW_PLAN.md). The public CLI supervises a worker
running the same native executable, with a default 30-second execution deadline,
structured crash/output errors and optional stdin-EOF cancellation. See
[execution guarantees and limits](docs/CLI_EXECUTION.md). This is not a security
sandbox, CPU quota or memory limit; use trusted local projects. Direct library
callers do not receive this process boundary automatically. Automatic project
reload, cross-platform release certification and interactive performance budgets
remain unfinished.

## Project boundaries

- [Roadmap and acceptance gates](ROADMAP.md)
- [Minimal runnable preview plan](docs/PREVIEW_PLAN.md)
- [Extraction provenance](PROVENANCE.md)
- [Migration verification](docs/MIGRATION.md)
- [Contributor checks](CONTRIBUTING.md)

OpenIDEA's original copy remains in place and still uses JDT LS. An analysis
engine that runs without a JVM does not remove Java's JVM requirement for
building, running, or debugging user applications. Build-system import tools
may also need a JVM; they belong outside the native analysis core.

## Goal and what this crate actually does

The goal is a native Java analysis engine: syntax, binding, class path,
generics and overload resolution, then reference search. Syntax, class-file
signatures, and name binding across source files and those class files are
implemented. Explicit source roots and an ordered directory/JAR classpath can now
be loaded as an immutable snapshot, with explicit platform JMOD or legacy JAR inputs.
A JDK is not included or discovered. Narrow same-class static primitive-call and
qualified reference-instance-call proofs can select source overloads. General
overload selection and generic instantiation are not implemented; see the
[primitive](docs/PRIMITIVE_CALLS.md) and [reference](docs/REFERENCE_CALLS.md) boundaries.

### Syntax

- Lexer for whitespace, comments, identifiers (Unicode letters, `$`, `_`, ASCII digits),
  number forms other than hex floats, strings, text blocks, and operators.
- Parser for package and import declarations, classes, interfaces, enums,
  records, fields, methods, constructors, and the statements and expressions
  listed in `src/lib.rs`, including lambdas, method references, and
  try-with-resources (`Resource` nodes).
- Names, literals, `this`, and `super` used as expressions are nodes. Tokens stay leaves.
- [Primitive/void class literals](docs/CLASS_LITERALS.md), including primitive arrays,
  have explicit syntax nodes; reflection types and class-literal overload proofs
  are not inferred.
- Comma-separated field/local declarations preserve each name and postfix array
  suffix. See [declaration binding boundaries](docs/DECLARATORS.md).
- Basic-for initialization/update support statement-expression lists without a
  general comma operator. See [for-loop boundaries](docs/FOR_LOOPS.md).
- Catch parameters support annotations and `final`, with bounded malformed-header
  recovery. See [catch parsing](docs/CATCH_PARAMETERS.md) for syntax versus type
  legality, union-typing limits, and recovery boundaries.
- Error nodes. A broken statement does not have to discard the rest of the block.
  `switch` is still an error node that skips one brace group. Patterns, modules,
  and explicit type arguments on a call (`Collections.<String>emptyList()`) are
  outside this subset.
- After an edit, a `{ ... }` block or type body is parsed again when the edit
  is strictly inside its braces and the node's new text still has balanced
  outer braces. That check reimplements IntelliJ's `checkBraceBalance`
  (see `NOTICE`). Whether `(...)` before `->` is a lambda follows
  `parseLambdaAfterParenth` from the same commit. Any other edit parses the
  whole file again. `UpdateMode::Full` means a full parse.
- `apply_edit` does not parse the file twice. `apply_edit_verified` does, and
  uses the full parse when the fragment tree differs. Tests call both and also
  compare the default path with a parse of the edited text. Those tests are
  the cases in `tests/syntax.rs`. They are not a proof that every Java program
  round-trips.
- Deleting a quote is a lexical-state change. If the new string swallows the
  closing brace, the brace check fails and the edit is a full parse. If the
  string ends at the newline and the braces still balance, the enclosing block
  is parsed again and the sibling method node is kept.

### Names, class files, and supertypes

`Project` holds parsed files and class files by caller-chosen path.
`resolve_at` rebuilds the index from those inputs on every call. Nothing is
cached between those legacy queries, so `edit`, `add_class_bytes`, `add_class_directory`,
and `remove_class` are visible to the next query. This rebuild is not a
performance claim. For repeated queries on unchanged inputs, `Project::prepare()`
builds a reusable borrowed index. `WorkspaceSnapshot::prepare()` shares one index
across prepared compilation views. See [prepared analysis](docs/PREPARED_ANALYSIS.md)
for lifetime, memory, and identity boundaries. These handles do not perform
incremental invalidation or add persistent symbol IDs.

`add_class_bytes` parses one class file. `add_class_directory` reads `*.class`
files under a directory and does not follow symlinks. A truncated file, a bad
magic, a bad constant pool, or a major version outside 45..=71 (or a minor
other than 0 or 65535 when the major is at least 56) is an error and is not
inserted. The lower-level `Project` API still accepts manually registered inputs.
`ProjectSnapshot::load` loads explicit source roots, class directories, JARs, and platform JMODs
without invoking Java. Maven and Gradle are not read. See
[project loading](docs/PROJECT_LOADING.md) for configuration and bounds.

Class files are parsed by `cafebabe` 0.9.0 (see `NOTICE`). The reader keeps
the binary name, the superclass, interfaces, field and method descriptors, and
the raw `Signature` attribute. `$` in a binary name is treated as javac's
nesting separator. Constructors (`<init>`) are parsed and are not selected.
`<clinit>` is dropped. Generic signatures are stored and not instantiated; they
keep unsupported comparisons explicit. `ClassSignature.is_interface` retains
ACC_INTERFACE so its synthetic Object entry is not mistaken for an override edge.
Code constructing this public struct literally must now supply that field.
See [method signature identity](docs/METHOD_SIGNATURES.md).

An ordinary value/name reference is searched in this order, which is the JLS
6.5 order restricted to what this index stores:

1. Local, parameter, or resource in the innermost recorded scope. A resource's
   scope runs from the `try` through the try block, and not into `catch` or
   `finally`. A local in a nested block hides an outer local of the same name.
2. Field of a lexically enclosing type, then the same name on `extends` and
   `implements`. A field on the nearer type hides one farther away. `this.name`
   uses that search. `super` is `ReceiverNotBound`.
3. Member type declared in a lexically enclosing type. Inherited member types
   are not searched. A nested type of a sibling class is not a candidate.
4. Single-type import. If that type is not in the project, the result is
   `Unresolved`. The search does not continue.
5. Top-level type in the same package, which includes the current compilation
   unit and any class file with that package and simple name. Two definitions
   of that name are `Ambiguous`.
6. On-demand imports, plus `java.lang` only when that type was actually added
   as source or as a class file. The JDK is not assumed to exist.

An unqualified invocation such as `m()` searches methods instead of the value
namespace. A local, parameter or field called `m` does not become its target.
Arguments and receiver prefixes still use value lookup (`m(m)` and `m.read()`).
A call does not fall through to a class of the same name. See
[invocation namespace boundaries](docs/CALL_NAMESPACES.md). `extends` and
`implements` are resolved in the compilation unit that wrote them, so `Lib` in
package `lib` finds `lib.Lib`. A class or interface with no written superclass
records `java.lang.Object` as that name; `enum` records `java.lang.Enum` and
`record` records `java.lang.Record`. Members of those types appear only when
the type itself was added.

Access of a field or method:

- `public` is visible.
- `private` is visible only inside the declaring class. It is not inherited.
- no modifier is visible only in the same package.
- `protected` is visible in the same package, inside the receiver type, or
  from a use site whose enclosing type has the declaring type as a supertype.
  JLS 6.6.2's extra rule about the qualifying type is not implemented.
- more than one of `public`, `protected`, and `private` is `NotDetermined`.

`static` and instance are not distinguished.

A qualified name (`b.Foo`, `box.value`) walks the longest indexed class prefix
and then its members. A source field used as a receiver (`box.result.value`)
keeps its declaration's file, imports, package, and enclosing type, including
when the field is inherited. Access to the next member is still checked at
the caller. Source array receivers are deliberately not treated as scalar element objects.
General array receiver/length/clone binding is not implemented. Primitive array
elements can be typed only inside the supported call-argument proof. Object-valued class-file
field receivers are not bound yet.
A package prefix of an indexed class is `Package`, not a type.
`Type::new` is `ConstructorRefNotBound`.

Statuses that refuse to guess:

| Status | When |
| --- | --- |
| `Resolved` | Exactly one candidate |
| `Unresolved` | No candidate, including a missing import target |
| `Ambiguous` | Conflicting types, fields or same-signature methods, including unresolved bridge duplicates. The first one is not chosen |
| `OverloadNotResolved` | No supported unique overload selection. The narrow primitive proof did not establish a winner; original candidates remain hints, not proven applicable methods |
| `CallNotResolved` | A single source invocation was not proved applicable, including known contradictions and unsupported argument/conversion cases. The retained target is a candidate, not a selection or a Java error diagnostic |
| `SignatureNotCompared` | A malformed signature or unsupported/unresolved type comparison prevents distinguishing inherited equal signatures from overloads. Mixed source/binary representation alone no longer causes refusal |
| `Cycle` | A supertype path visits a type that is already on that path |
| `NotDetermined` | Every candidate has contradictory access flags |
| `ReceiverNotBound` | The qualifier is a value whose declared type is not an indexed class, or it is `super` |
| `StaticImportNotApplied` | The simple name is a static import. Those imports are recorded and not applied |
| `Package` | The name is only a package prefix |
| `ConstructorRefNotBound` | The selector is `new` after `::` |

The original one-candidate name-navigation rule follows `ResolveClassUtil.resolveClass`
at the commit in `NOTICE`. Source invocation guards now reject supported argument
contradictions, while a closed same-class primitive subset can justify overload
selection. Outside those boundaries, a unique name is still not a general
applicability proof. The scope processor is not a translation of `ClassResolverProcessor`.

The added public `ResolveStatus::CallNotResolved` variant requires clients with
exhaustive matches to add a case. Public target fields are unchanged. See
[primitive calls and compatibility](docs/PRIMITIVE_CALLS.md) and
[proven call-result propagation](docs/CALL_RETURNS.md).

Not implemented, and not claimed:

- A JDK or any classpath the caller did not add. `java.lang.String` stays
  unresolved until that source or class file is inserted.
- General overload selection and arbitrary expression typing. The supported
  same-class static subset uses explicit primitive locals/parameters, array
  elements and scalar results of already-proved direct source calls; literal arguments, boxing, reference conversions and varargs
  expansion remain outside it.
- Complete generic, bridge and class/interface/default-method override semantics.
  Supported mixed source/binary signatures can now match; uncertain cases refuse.
- Instantiating a `Signature` attribute or a source type argument.
- Static imports, modules, and local classes (a class inside a method is not a
  statement in this parser).
- The full protected-access rule, and any distinction between static and instance.
- Multi-catch beyond the first type before `|`.
- JDK discovery, jimage/ct.sym, JPMS module graphs, Maven, Gradle, reference search,
  rename, and refactoring. JAR loading is a flat-classpath subset, described below.
- Reuse inside the one reparsed brace node. That node is parsed again from its
  text. Green nodes outside it keep their allocations.

## IntelliJ map

Pinned tree: `840d3be11dfdeada533d05052008b48816b6cc4e`. This is not a PSI port
and not an IntelliJ-equivalent resolver.

| Piece | Status |
| --- | --- |
| Brace-balance reparse check | Reimplemented from `SyntaxBuilderUtil.checkBraceBalance` |
| Lambda lookahead | Reimplemented from `parseLambdaAfterParenth` |
| Statement recovery | Reimplemented from `StatementParser.parseStatements`: one bad token, then continue |
| One class candidate | Same decision as `ResolveClassUtil.resolveClass`: one result, otherwise none |
| Node ranges | Leading trivia stays on the parent. A literal node's slice is the literal |
| Method reference `::` | Independent parser |
| `PrattExpressionParser` as a whole | Not translated |
| `ClassResolverProcessor` and the scope walk | Not translated. Search order above is the restricted JLS order |
| Compiled class reading | Not ported. `StubBuildingVisitor` extends ASM `ClassVisitor` and builds stubs; `ClsClassImpl` then implements `PsiClass`. This crate parses with `cafebabe` and puts the signature in the same index as a source type |
| Supertype member search | Independent walk of the indexed `extends` / `implements` links. Not `PsiClass.getSuperTypes` |
| PSI, stubs, stub indexes, find-usages | Not started |
| Compiler, inference, persistent JAR indexes | Not started; native JAR loading is implemented |

## Layout

`ParsedFile::parse` builds an owned green tree (`Arc` nodes). `apply_edit`
takes UTF-8 byte offsets. `apply_utf16_edit` converts UTF-16 offsets first;
a supplementary character is two UTF-16 units, and an offset inside one is
rejected. `Project::insert`, `Project::edit`, `Project::add_class_bytes`,
`Project::add_class_directory`, `Project::remove_class`, and
`Project::resolve_at` are the low-level binding API. `ProjectConfig` and
`ProjectSnapshot` provide explicit source-root and classpath loading.
`WorkspaceSnapshot` adds compilation-scoped dependency views; `read_jar`
is also available separately. Class decoding uses `cafebabe`; archive decoding
uses `zip` with pure-Rust DEFLATE. See [dependency licenses](THIRD_PARTY_LICENSES.md)
for the exact locked graph. No analysis dependency invokes or embeds a JVM.

## Tests

`tests/fixtures/` was written for this crate. It is not IntelliJ `testData`.
`tests/syntax.rs` checks valid and broken files, try-with-resources, qualified
names, generics (`List<List<String>>`), sibling reuse, a cross-method edit,
UTF-16 emoji offsets, and quote deletion. Each edit is compared with
`apply_edit_verified` and with a full parse. `tests/bind.rs` checks same-named
classes, explicit and star imports, qualified names, nested members, local
shadowing, a `for` variable's scope, resources, overload refusal, a source
method hiding a superclass method with the same parameter list, a call that
does not bind to a class, a missing import, static imports, contradictory
access, and an edit that changes the next query.
`tests/signatures.rs` checks scalar/array distinction, dimensions before and after
parameter names, varargs equivalence, inheritance, malformed parameters, and edits.
`tests/method_identity.rs` additionally checks source/binary parameter identity,
imports and compilation contexts, descriptors, hierarchy branches and conservative
unsupported cases. Signature comparison alone does not select overloads or substitute generics.
`tests/primitive_calls.rs` checks the separate narrow call proof and conservative
single-candidate refusal. `tests/reference_calls.rs` checks explicit reference
conversion, access and complete hierarchy evidence, compilation identities,
candidate preservation and bounded refusal. The
[real-Gson construction gate](examples/second_project/CONSTRUCTOR_CALLS.md) checks
one explicit historical query correction while preserving the other 15 queries,
previous probes and all source/parser records. It does not claim whole-Gson javac
equivalence. Type-use annotations on array dimensions
and typed-lambda postfix dimensions are outside this parser subset.
`tests/receivers.rs` checks cross-file field chains, conflicting caller imports,
same-name/offset collisions, inherited and nested field types, method calls,
caller access checks, missing and ambiguous receiver types, and declaration
edits. `tests/classfile.rs` checks
`tests/fixtures/classes/Lib.class` (JVMS §4, major 61, minor 0, assembled
rather than compiled by javac; `assemble_lib` must reproduce those bytes),
truncated input, a bad magic, a bad constant-pool tag, an unsupported version,
public versus private and package access, add/remove/replace of a class file,
a directory of `.class` files, a source type and a class file with the same
name, two interface fields, a supertype cycle, and `java.lang.Object` only
after that class file is added. `tests/baseline.rs` times one fixed sample
in-process. That number is not a comparison with IDEA, and the test count is
not a coverage proof.

From this repository root, with Rust 1.96:

```sh
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

These commands use this project's own `Cargo.toml` and `Cargo.lock`.
The root workflow runs formatting, lint, tests, docs, and examples. It also runs
the project example with an empty environment and no executable search path.

## Project loading and JVM-free analysis

See [project loading](docs/PROJECT_LOADING.md) and the
[validation record](docs/VALIDATION.md). These are early M1 slices, not a complete
build model or JDK import. Run the checked-in multi-file/JAR fixture:

```sh
cargo run --locked --example project
cargo build --locked --examples
sh scripts/check-native-runtime.sh
```

The example resolves `app.Use -> app.Bridge -> lib.Lib.run()` from a local JAR.
No Java installation, Java executable, JDT LS, JNI bridge, or Java sidecar is
required by analysis. JARs, JMODs, and class files are input data, decoded in Rust.
`inspect_project` also accepts a source root, source-relative file, UTF-8 byte
offset, and local JAR paths. Java application compilation/run/debug remains a
separate concern and is not part of this library.

To supply platform classes explicitly (optional, no Java executable is used):

```sh
cargo run --locked --example platform -- /path/to/java.base.jmod
```

The example resolves implicit `java.lang.String` and inherited `Object.toString()`.
`ProjectConfig.platform` accepts ordered JMOD or legacy platform JAR paths and
loads them before the application classpath. No JPMS exports/readability,
JDK discovery, or `--release` compatibility is implied. The mandatory suite uses
hand-assembled class signatures and requires no JDK artifact.

## Compilation visibility

`WorkspaceConfig` and `WorkspaceSnapshot` add explicit `(project, source_set)`
contexts for main/test and multi-project analysis. For example,
`app/test -> app/main -> lib/main` makes library production sources visible to
application tests without exposing `lib/test` or unrelated projects.

- Source dependency edges are directed and transitive. No relationship is inferred
  from names such as `main` or `test`.
- Each compilation supplies its own platform and binary classpath. Source edges
  do not export JARs. Declarations and binary hierarchies keep their own compilation
  context, so consumer/test types cannot silently fix dependency declarations.
- Graph errors, shared source ownership, and aggregate input/expansion budgets
  fail explicitly. Reloading changed edges creates a new snapshot; older snapshots
  retain their original results.

```sh
cargo run --locked --example scopes
cargo build --locked --examples
sh scripts/check-native-runtime.sh
```

The checked-in fixture demonstrates an inherited field resolving while library
test helpers and unrelated types remain unresolved. See
[the explicit graph contract](docs/PROJECT_LOADING.md#explicit-compilation-graphs)
for exact rules and limitations. This is not a Maven/Gradle importer, JPMS access
model, or incremental query cache.

## Real-source acceptance

An optional [offline acceptance harness](examples/real_source/README.md) loads all
249 published Commons Lang 3.17.0 Java sources plus explicitly supplied platform
data. It checks 15 fixed query positions, reports parser gaps, and measures
preparation separately from repeated queries. It currently exposes substantial
parser/semantic limitations; it is not a claim of complete project support.
No upstream source archive or JDK data is bundled, and no Java process runs.
The default native CI stays separate from the opt-in, trusted-main
[extended acceptance workflow](.github/workflows/extended-acceptance.yml). That
workflow verifies fixed external inputs and runs an independent development-only
javac comparator. See [validation boundaries](docs/ACCEPTANCE_PLAN.md); production
analysis and default native tests do not acquire a Java dependency.

## Later stages

Follow [ROADMAP.md](ROADMAP.md) for the project-level milestones. The semantic
work below follows explicit classpath and JDK/model foundations.

1. Broader argument applicability and overload selection, beyond the supported
   source primitive subset. Comparing declaration signatures alone does not
   choose a call target.
2. Generic substitution and inference for source and stored `Signature` attributes;
   unsupported generic comparisons currently remain explicit.
3. Reference search, then one refactoring at a time.

Passing the tests in this crate does not mean those stages are done.
