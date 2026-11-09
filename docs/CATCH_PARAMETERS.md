# Catch parameters and recovery

The catch-specific parser accepts annotations and `final`, one declared name,
qualified class types, and `|`-separated alternatives. Type-use annotations,
generic reference-type arguments (including primitive arrays), and annotated
postfix dimensions are represented without changing ordinary formal-parameter
parsing. The existing direct catch name/type tree layout is preserved for binding.

This follows the syntax distinction in
[JLS 21 §14.20](https://docs.oracle.com/javase/specs/jls/se21/html/jls-14.html#jls-14.20):
the catch declaration uses `VariableDeclaratorId`, so postfix dimensions belong
to its grammar. Thus parsing `E e[]` without a syntax error does **not** say that
an array is a legal exception type. Likewise, accepting `E<T>` as type syntax
does not establish that it denotes a legal catch type.

Primitive catch types, varargs, several parameters, missing names, invalid
modifiers, and malformed type/annotation tokens produce local diagnostics.
Repeated `final` is also diagnosed as a local declaration constraint. No full
exception type checking is implemented: Throwable ancestry, type-variable
restrictions, annotation applicability/values, alternative subtype conflicts,
exception reachability, and definite assignment are not checked here.

## Recovery boundary

Catch-only annotation handling first checks delimiters, then parses a bounded
token slice. Nested generic annotations follow this same path. Unexpected header
tokens remain in error nodes, preserving every UTF-8 source byte and the normal
UTF-16 conversion contract. A catch body is parsed only at an actual opening brace;
a missing brace does not invoke a block parser on the following statement.

Synchronization stops at the first brace, closing parenthesis, semicolon,
`catch`/`finally`, end of input, or an applicable declaration boundary. This
preserves catch bodies, later statements, and sibling methods in the regression
cases. It cannot infer whether an unmatched annotation-array brace was intended
as data or as the body, and it does not repair arbitrary brace imbalance. The
tests establish specific recovery behavior, not perfect recovery for every
malformed editor buffer.

## Binding and verification

The parameter retains its declaration range and is visible in its own catch
body, including nested blocks. It does not leak into the try body, sibling
catches, finally, or later statements. Prepared and rebuild queries agree.
Multi-catch member typing still uses the preexisting limited declared-type model;
union least-upper-bound inference and sound union member selection are not added.

Eight focused tests cover accepted and rejected forms, exact declaration/error
ranges, lossless text, UTF-16 edits, ordinary-parameter compatibility, and ordinary
incremental edits compared with a full parse. Five initial tests failed before
the implementation. Independent review additionally exercised 33 header forms
and 165 incremental replacements, and found a nested-annotation recovery escape
that was corrected before publication.

The original Commons Lang source archive is never rewritten. Its full 249-file
inventory moved from 63 files with 8,187 parser error nodes to 28 files with 149.
All 44 changed file counts decreased; 205 were unchanged. This is a recovery
improvement, not 8,038 distinct Java features or defects fixed. The exact per-file
comparison and separately pinned query targets live in
[`examples/real_source`](../examples/real_source/README.md). Zero parser errors
does not imply full Java correctness or complete analysis capability.
