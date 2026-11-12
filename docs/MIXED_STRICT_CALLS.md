# Mixed strict-invocation argument proof

This extends the [qualified scalar-primitive slice](QUALIFIED_PRIMITIVE_CALLS.md)
and [reference instance-call proof](REFERENCE_CALLS.md), not general Java overload
resolution. Each argument independently supplies one of the already-supported
reference or scalar-primitive types. Parentheses preserve that evidence. Decimal
int literal spelling and range checks are unchanged.

The source-instance receiver, caller, class hierarchy and every same-name
declaration retain the existing ownership/access/header/closure requirements.
All formal vectors are validated before arity or applicability filtering,
including inaccessible, unknown or unsupported competitors of a different arity.
Unknown actuals cannot be skipped because another slot is applicable.

Strict applicability is position-wise: reference identity/proved widening or
primitive identity/widening. There is no boxing, unboxing or invocation constant
narrowing. Every position of one applicable formal vector must convert to the
corresponding position of every other applicable vector for it to be the unique
most-specific winner. Crossing vectors stay ambiguous; argument order, declaration
order and summed conversion scores cannot manufacture a target.

All expression classification, declaration lookup, hierarchy closure, conversion
and pairwise comparison share the existing work/depth budget. A failed reference
proof does not reset that budget when trying the scalar-primitive path. The special
cross-compilation String-literal identity proof retains its complete binary-origin
requirements; ordinary same-named source or parameter types cannot borrow it.

## Boundary and evidence

No new expression typing is added: null, primitive arrays, arithmetic, unary
expressions, fields and general call-result receiver types remain unsupported by
this slice. No generic inference, expanded varargs, inherited/binary method
selection or reflection typing is introduced. Existing supported reference
constructions and strings remain governed by their earlier exact contracts.

The additive [frozen Gson plan](../examples/second_project/mixed-strict-probes.json)
selects `set(0,b)` at byte 2412 of JsonArrayTest.java and the declaration-name range
4463..4466 of JsonArray.set(int,JsonElement). It preserves both remove(a) and
remove(1) winners and the outer getAsString receiver refusal. Historical plans
are retained, not rewritten as though the new winner had always been supported.

The stages remain distinct. The qualified-primitive checker's historical capture
still verifies the a9353caf stage: remove(1) is unselected. The new mixed checker's
baseline capture verifies 6208877: remove(1) is selected but set(0,b) is not.
Normal current acceptance requires both additive corrections and rejects either
older stage. These stages all retain the separate exact 166-report/44-file parser
contract; the older 331-report parser baseline is not silently substituted.
Current strict-reference preservation applies the two frozen additive plans in
sequence, without editing their original targets or refusals.

The Rust regressions cover swapped argument positions, two primitive arguments,
crossing and dominating vectors, reversed declaration order, unknown arguments
and competitors, narrowing/boxing refusals, lexical scope, shared budgets,
cross-compilation String identity and unchanged outer-receiver refusal.
The engine remains JVM-free. The separately gated compiler comparator is a
development oracle, not an analysis runtime dependency.

## Design reference

This independently implemented Rust composition follows the strict applicability
and formal-to-formal specificity boundaries in JLS 15.12.2 and the official IDEA
Community mechanisms/license references recorded in
[the preceding slice](QUALIFIED_PRIMITIVE_CALLS.md#intellij-idea-behavior-reference-and-license).
It does not copy the PSI type system or use a conversion-cost heuristic.

The later [one-layer return-receiver slice](CALL_RESULT_RECEIVERS.md) explicitly
advances the old outer-call refusals. It does not change this phase's frozen
baseline or add call-result arguments to the mixed invocation proof.
