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
selects it. Cargo downloads the three locked registry dependencies on the
first build; no OpenIDEA checkout, Node, Tauri, JDK, or JDT LS is required.

```sh
cargo test --locked
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo run --locked --example resolve
```

The direct dependency is `cafebabe` 0.9.0. Its transitive dependencies are
`bitflags` 1.3.2 and `cesu8` 1.1.0. See [third-party notices](THIRD_PARTY_LICENSES.md).
`publish = false` is intentional; this package has not been published to crates.io.
The independent source repository is
[yangkangbug/java-analyzer-rs](https://github.com/yangkangbug/java-analyzer-rs).

## Project boundaries

- [Roadmap and acceptance gates](ROADMAP.md)
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
be loaded as an immutable snapshot. A JDK is not included. Overload selection and generic instantiation
are not.

### Syntax

- Lexer for whitespace, comments, identifiers (Unicode letters, `$`, `_`, ASCII digits),
  number forms other than hex floats, strings, text blocks, and operators.
- Parser for package and import declarations, classes, interfaces, enums,
  records, fields, methods, constructors, and the statements and expressions
  listed in `src/lib.rs`, including lambdas, method references, and
  try-with-resources (`Resource` nodes).
- Names, literals, `this`, and `super` used as expressions are nodes. Tokens stay leaves.
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
cached between queries, so `edit`, `add_class_bytes`, `add_class_directory`,
and `remove_class` are visible to the next query. This rebuild is not a
performance claim.

`add_class_bytes` parses one class file. `add_class_directory` reads `*.class`
files under a directory and does not follow symlinks. A truncated file, a bad
magic, a bad constant pool, or a major version outside 45..=71 (or a minor
other than 0 or 65535 when the major is at least 56) is an error and is not
inserted. The lower-level `Project` API still accepts manually registered inputs.
`ProjectSnapshot::load` loads explicit source roots, class directories, and JARs
without invoking Java. Maven and Gradle are not read. See
[project loading](docs/PROJECT_LOADING.md) for configuration and bounds.

Class files are parsed by `cafebabe` 0.9.0 (see `NOTICE`). The reader keeps
the binary name, the superclass, interfaces, field and method descriptors, and
the raw `Signature` attribute. `$` in a binary name is treated as javac's
nesting separator. Constructors (`<init>`) are parsed and are not selected.
`<clinit>` is dropped. Generic signatures are stored and not instantiated.

A simple name is searched in this order, which is the JLS 6.5 order restricted
to what this index stores:

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

A call does not fall through to a class of the same name. `extends` and
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
the caller. Object-valued class-file field receivers are not bound yet.
A package prefix of an indexed class is `Package`, not a type.
`Type::new` is `ConstructorRefNotBound`.

Statuses that refuse to guess:

| Status | When |
| --- | --- |
| `Resolved` | Exactly one candidate |
| `Unresolved` | No candidate, including a missing import target |
| `Ambiguous` | Two or more types or fields. The first one is not chosen |
| `OverloadNotResolved` | Two or more methods remain after a same-descriptor method nearer in the hierarchy hides a farther one. Descriptors are the JVM descriptor for a class file, or the source parameter base names plus array dimensions (varargs add one dimension). The call's argument types are not compared, and the first method is not chosen |
| `SignatureNotCompared` | The name exists as both a source declaration and a class-file member, or a parameter list could not be read. Override and overload are both possible |
| `Cycle` | A supertype path visits a type that is already on that path |
| `NotDetermined` | Every candidate has contradictory access flags |
| `ReceiverNotBound` | The qualifier is a value whose declared type is not an indexed class, or it is `super` |
| `StaticImportNotApplied` | The simple name is a static import. Those imports are recorded and not applied |
| `Package` | The name is only a package prefix |
| `ConstructorRefNotBound` | The selector is `new` after `::` |

The one-candidate rule is the same decision as `ResolveClassUtil.resolveClass`
at the commit in `NOTICE`. The scope processor is not a translation of
`ClassResolverProcessor`.

Not implemented, and not claimed:

- A JDK or any classpath the caller did not add. `java.lang.String` stays
  unresolved until that source or class file is inserted.
- Choosing one overload from several. A call `m(1)` with both `m()` and `m(int)`
  indexed is `OverloadNotResolved`.
- Treating a source method and a class-file method of the same name as an
  override. That is `SignatureNotCompared`.
- Instantiating a `Signature` attribute or a source type argument.
- Static imports, modules, and local classes (a class inside a method is not a
  statement in this parser).
- The full protected-access rule, and any distinction between static and instance.
- Multi-catch beyond the first type before `|`.
- JDK discovery/JMOD, JPMS module graphs, Maven, Gradle, reference search,
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
`ProjectSnapshot` provide explicit source-root and classpath loading; `read_jar`
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
The shape comparison still does not perform overload selection, generic substitution,
or source-to-binary signature comparison. Type-use annotations on array dimensions
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
[validation record](docs/VALIDATION.md). This is the first M1 slice, not a complete
build model or JDK import. Run the checked-in multi-file/JAR fixture:

```sh
cargo run --locked --example project
cargo build --locked --examples
sh scripts/check-native-runtime.sh
```

The example resolves `app.Use -> app.Bridge -> lib.Lib.run()` from a local JAR.
No Java installation, Java executable, JDT LS, JNI bridge, or Java sidecar is
required by analysis. JARs and class files are input data, decoded in Rust.
`inspect_project` also accepts a source root, source-relative file, UTF-8 byte
offset, and local JAR paths. Java application compilation/run/debug remains a
separate concern and is not part of this library.

## Later stages

Follow [ROADMAP.md](ROADMAP.md) for the project-level milestones. The semantic
work below follows explicit classpath and JDK/model foundations.

1. Overload selection, first by arity and then by types, including a comparison
   of a source parameter list with a class-file descriptor once erasure is
   known. Until then a mixed pair stays `SignatureNotCompared`.
2. Instantiating `Signature` attributes. They are stored and unused.
3. Reference search, then one refactoring at a time.

Passing the tests in this crate does not mean those stages are done.
