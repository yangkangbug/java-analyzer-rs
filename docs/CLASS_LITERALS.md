# Primitive class-literal syntax

The native parser accepts `byte.class`, `short.class`, `int.class`, `long.class`,
`float.class`, `double.class`, `boolean.class`, `char.class`, their empty-array
forms such as `int[][].class`, and scalar `void.class`. Whitespace and comments
between these tokens are retained losslessly. Each has a `ClassLiteralExpr` node,
not an ordinary primitive value or a guessed field binding.

A complete primitive/empty-dimensions/`.class` lookahead is required before this
branch consumes input. `void[].class`, `int[1].class`, `int.classX` and unfinished
`int.` do not become class-literal nodes. They still have parser errors, using the
existing recovery path. This is syntax support, not full Java legality checking.

No `Class<T>` type, reflection member, boxing, generic substitution or overload
applicability is inferred. Class-literal arguments cannot supply the engine's
primitive-argument proof. Unknown class-literal receivers are explicitly refused
rather than resolving a method/field selector as an unrelated same-spelled local.
The guard follows the receiver/callee and returned-value branches, not ordinary
call arguments. Existing reference-name `.class` representation remains
unchanged; reference-array and type-namespace semantics are not implemented by
this slice. There is no JVM, JDT LS, IDE or build-system runtime dependency.

## Evidence

`tests/class_literals.rs` first failed on the pre-change parser. It covers all
primitive scalar/array forms, scalar void, comments and Unicode, casts, return,
lambda and conditional expression positions, malformed tails, lossless trees,
incremental/full-parse agreement, following local navigation, and prepared/legacy
binding agreement. A positive primitive-call control resolves an `int` argument;
class-literal arguments keep both candidates with `OverloadNotResolved`.

The unchanged [Gson workload](../examples/second_project/README.md) retained all
211 files and 1,484,012 original source bytes. Against the same locked release
build profile, parser error nodes fell from 331 to 166 in 19 files; no file's
count increased. `Primitives.java` fell from 54 to zero. All 16 fixed navigation
queries and 14 negative controls still passed. The full per-file comparison is
[class-literal-parser-comparison.tsv](../examples/second_project/class-literal-parser-comparison.tsv).
Historical parser baselines were not rewritten.

All 249 Commons Lang source files still report 85 parser errors in 24 files, and
all 45 existing exact navigation queries pass. The native harness uses the pinned
archives as data with `PATH=/nonexistent`; it does not compile or run either Java
project. No upstream source or JDK archive is added to this repository.

Error counts include recovery cascades. This reduction does not establish Java
conformance, semantic diagnostics, reflection navigation or full-project support.
See the respective workload READMEs for hashes, scope and reproduction commands.

## Additive acceptance contract

The older reference-call and source-selection gates originally required the
pre-class-literal parser output to remain byte-for-byte unchanged. Their first
run on the syntax improvement correctly failed that old expectation. The
[class-literal parser contract](../examples/second_project/class-literal-parser-contract.json)
now separately pins the current full parser-record digest and the complete
211-file comparison, while checking the unchanged historical artifact hashes.

Normal acceptance requires exactly 166 reports in 44 files, every per-file
count, and every diagnostic message and byte/UTF-16 range through the canonical
digest. Explicit historical parser checks still require 331 reports in 58 files
and the old digest. The checker never accepts either version opportunistically.
Reference/constructor baseline capture selects the historical stage explicitly;
source-selection historical/manual/generated *manifests* all use the current
parser contract. Navigation targets, source hashes and refusal controls stay
unchanged. Offline mutation tests cover stage swaps and corrupted records.
