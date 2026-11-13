# Traditional switch statement structure

This slice parses traditional colon-label switch statements as lossless syntax
instead of treating their complete bodies as opaque errors. It does not implement
switch expressions, arrow rules, pattern matching, label constant evaluation,
exhaustiveness, control-flow analysis or definite assignment.

A SwitchStmt contains a selector and a SwitchBody. The whole body owns one lexical
scope, shared across consecutive labels and fallthrough groups. A SwitchLabel
does not create a scope; explicit nested blocks do. Locals do not leak after the
switch, and opaque local type declarations keep their existing containment.
Lexical visibility is not a guarantee that Java permits reading an uninitialized
local on a particular control-flow path.

Traditional switches also remain outside the strict unqualified-call proof
inventory, preserving that pre-switch semantic boundary even though parsing now
succeeds. Unique-name navigation and the existing arity guard remain separate
from a proof of applicability. Removing a parser limitation must not silently
activate a different invocation-semantics stage elsewhere in the file.

Case-label binding is deliberately unavailable. An enum label or constant name
must not be reinterpreted as an unrelated ordinary local or field. Selector and
body expressions continue through the existing binder where supported.

The SwitchBody is not an independently reparseable ordinary Block. Edits fall
back to the enclosing supported block, while explicitly nested blocks retain
their normal incremental path. Blocks nested inside a selector fall back to the
enclosing supported block too: selector errors can change the whole switch's
commit/recovery decision. Incremental results must match a fresh full
parse, including malformed and unsupported constructs.

Modern/unsupported label forms remain errors rather than partially becoming
ordinary declarations. Expression-position switches stay opaque; they must not
be recovered as standalone statement switches and accidentally expose locals.

## Design reference

IntelliJ's
[StatementParser.kt](https://github.com/JetBrains/intellij-community/blob/840d3be11dfdeada533d05052008b48816b6cc4e/java/java-syntax/src/com/intellij/java/syntax/parser/StatementParser.kt)
separates switch statement and switch-label parsing. This engine borrows the
structural separation only, not its modern pattern/rule support or PSI machinery.
No IntelliJ code is copied.

[JLS 14.11](https://docs.oracle.com/javase/specs/jls/se17/html/jls-14.html#jls-14.11)
and [JLS 6.3](https://docs.oracle.com/javase/specs/jls/se17/html/jls-6.html#jls-6.3)
describe switch blocks and local-variable scope. Parsing those structures does
not establish selector compatibility or case-value legality.

## Evidence policy

Native tests require lossless token coverage, scope and label refusal boundaries,
malformed recovery, and incremental/full equivalence. An optional independently
checked compiler fixture compares declaration identities and separately records
invalid Java controls without executing fixture programs. Java remains absent
from production and mandatory native tests.

Real Commons/Gson input hashes and original parser artifacts remain frozen.
Current-stage parser changes must be explicit and preserve every unrelated
complete report; newly exposed unsupported syntax is reported, not suppressed.
Existing navigation contracts are verified separately from parser improvements.

## Pinned real-source evidence

The exact full-report comparator retains the pre-change outputs, source hashes
and original predictions for 249 Commons and 211 Gson files. Gson reports fall
from 166 to 138 exactly as predicted. Commons reports fall from 85 to 60 rather
than the predicted 58: two already-unsupported trailing-dot numeric literals
inside JavaVersion's formerly opaque switch are now visible. The failed
prediction is preserved, with only those two exact additional records accepted.

The complete Commons unqualified-call inventory grows from 3030 to 3077.
All 3030 historical calls remain outside the newly structured switches; each
of the 47 additional calls is pinned to its exact source spelling and byte
offset within an old opaque switch range. Existing declaration targets and
refusals remain separate checks, not inferred from the inventory count.

See [the frozen parser stage](../examples/traditional_switch/README.md) and
[the independent scope oracle](../dev-tools/oracle/traditional-switch-v1/README.md).
These are bounded parser, lexical-scope and preservation results, not general
switch type checking or Java language conformance.
