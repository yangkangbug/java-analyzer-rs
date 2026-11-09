# Local-type containment, not local-type binding

The pinned Gson workload exposed a false-navigation bug: a method-local class
could terminate recovery of its surrounding method. Later local declarations and
even a variable use were then indexed as fields of the enclosing class.

This slice preserves the boundary of unsupported local type declarations and
restores ordinary surrounding statements. It does not implement local-class,
local-record, or local-enum semantics.

## Native boundary

At statement positions, a distinct opaque syntax node preserves the local type's
header and balanced body, including nested braces. Its original tokens and source
positions remain intact. The body is not independently reparseable: edits use a
normal enclosing block or a full parse, with the existing complete-consumption
and lossless-source checks.

The binder does not publish this type as an enclosing member or a global class.
It does not index its fields, methods, parameters, or locals. Queries within the
unsupported declaration return no target. A scoped type-name barrier prevents
later uses in the same lexical scope from borrowing an unrelated member,
imported, or package type with the same spelling. Ordinary value names remain in
their own namespace. Receiver types are interpreted at their declarations, so a
variable declared before the barrier does not silently acquire the later type.

This is conservative containment. It is not a model of local-type identity,
captures, enclosing instances, inheritance, access, constructors, record
components, or implicitly static local enum/record contexts. Malformed or
incomplete source remains parser recovery, not a Java validity judgment.

## Evidence and limits

The [recorded comparison](../examples/second_project/local-type-comparison.json)
uses baseline `5894b8f06eedd06fc585dbb154ccefa1cdeb7047` and identifies the
precommit candidate with per-source hashes, a source-manifest digest, and native
binary hashes. It retains exact input pins, all 226 source hashes, all 211
selected-file parser counts, original query statuses, and before/after targets.

The fixed Gson source archive and the original sixteen-query plan remain
unchanged. Additive queries check the exact source path, compilation, target kind,
owner, and name-token range for surrounding variables. The old false field
results are retained as regression evidence rather than counted as previously
successful navigation.

Four real uses in GsonTest now select the two genuine locals, instead of a fake
field, ambiguous fake fields, a variable use indexed as its own declaration, or
no target. The local `DummyAdapter` type query still returns no target. The
[five-probe gate](../examples/second_project/LOCAL_TYPE_CONTAINMENT.md) runs
separately from the unchanged original sixteen-query gate in extended CI.

The optional independent javac comparator uses handwritten fixtures and the
compiler-selected declaration identity. Supported surrounding bindings and
explicitly unsupported local-type queries are reported separately. It does not
compile or run Gson, fill in its missing generated sources, or turn native and
rebuilt agreement into independent semantic proof.

The handwritten suite checks eleven supported bindings and four unsupported
queries. Native regression tests also cover lexical type shadowing, value/type
namespace separation, pre-declaration receiver types, unrelated global-name
fallback, cross-file leakage, Unicode ranges, and malformed/incremental edits.
A real stdio LSP test checks unsaved replacements, exact UTF-16 locations,
unsupported body refusal, stale-version desynchronization and newer-version
recovery. Existing cancellation and real Neovim checks remain separate gates.

Opaque bodies can replace many cascading parser reports with one explicit
unsupported boundary. A lower report count therefore does not mean that the
contents became supported Java syntax or semantics. The full corpus inventory,
remaining failures, and original navigation refusals stay visible.

The selected corpus reports 1,513 before and 331 after, with eight changed files;
GsonTest changes from 915 to four. These are recovery observations. During review,
primitive `.class` recovery was accidentally treated as a local declaration.
That regression was fixed, not accepted as a new baseline: Primitives.java's
complete tree, 54 reports, and 456 word-start lookup results match the old engine.
An additional native check verified token coverage on all 211 selected files
(1,484,012 bytes) and all 226 upstream Java files (1,550,994 bytes). Six real
GsonTest edits and 3,234 focused adverse edits agreed with full parsing.

Those edit checks are not a general incremental-parser proof. An unrelated
malformed nested member-class header still has a pre-existing incremental/full
tree mismatch, independently reproduced on the baseline. Missing closing braces
in a local type can conservatively keep later text inside the opaque boundary;
the original bytes remain intact, but intended semantic ownership is unknown.

The production library, CLI, JAR/JMOD loading, and LSP analysis remain native
Rust and require no JVM. The comparator's pinned JDK is development-only. The
editor's buffer generations, stale-result checks, and cancellation protocol are
unchanged. This slice does not add general overload selection, generic
inference, chained-call receivers, completion, or semantic diagnostics.
