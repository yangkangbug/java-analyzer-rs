# Traditional switch statement structure

Historical stage: the original opaque expression/arrow boundary below is extended
by [non-pattern switch expressions](SWITCH_EXPRESSIONS.md) in 0.8.0. Its original
validation records remain historical evidence, not the current capability list.

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
their normal incremental path. Blocks nested inside a selector or label fall
back to the enclosing supported block too: selector errors, label errors and
unsupported label tokens can change the whole switch's commit/recovery decision.
This also covers anonymous-class bodies nested inside label expressions; syntax
acceptance does not prove that such an expression is a legal Java constant label. Incremental results must match a fresh full
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


## Caller-local inventory eligibility

The pre-existing switch-sensitive invocation gates now distinguish declaration
inventory from the current callable's unsupported control-flow context. A legal
traditional switch in another method no longer disables an otherwise supported
primitive, implicit-instance, static-qualified or explicitly static-imported call,
or the existing binary foreign-package candidate-exclusion certificate.

The index retains switch start positions in source order. A shared bounded
lower-bound check determines whether the verified direct caller contains a switch.
Only existing whole-file-gate consumers use this result. Their current
caller still refuses when it contains a switch anywhere, including a nested body.
Primitive recovery is limited to previously unresolved overloads. Singleton
primitive positive and negative checks retain their original whole-file entry
qualification, including the original exhaustion behavior when enabled. Existing
single-name navigation is not reinterpreted as strict applicability verification.
Proof objects without a caller retain the old whole-file gate. The additional
search cost is charged to the existing proof budget only on this recovery path;
the old globally valid path has zero additional cost.

This is **not** a universal ban on calls in switches. Existing qualified instance
paths which did not use the old gate remain unchanged, including successful calls
inside case bodies. Legacy rootless name-navigation results also remain unchanged
and do not constitute strict applicability proof.

True parser errors, LocalType diagnostics, malformed method headers, damaged
fields, anonymous-body containment, incomplete ancestors and complete competing
declarations retain their previous boundaries. In particular this does not
certify arbitrary damaged-body recovery or remove the source TypeStatic target
file's syntax check. Switch positions and eligibility are snapshot-local index
facts; no ancestor result cache or new public API is introduced.

Regressions compare unrelated versus current-caller switches, existing instance
successes, binary package-private competitors, explicit None fallback, nested
switch ordering, shared-budget exhaustion, real syntax errors, incremental edits
and old prepared snapshots. These checks are separate from switch semantics and
do not claim that arbitrary switch bodies are fully analyzed.
