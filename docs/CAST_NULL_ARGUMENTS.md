# Explicit plain-reference cast-null arguments

Strict source-instance invocation can classify `(Type) null` as the explicit
reference type, not as the untyped null actual. This preserves overload choice:
`(Parent)null` does not choose a Child overload merely because plain null would.

The complete CastExpr must contain `(`, one plain reference Type, `)`, and one
operand. The operand must be exactly null with only bounded parentheses/trivia.
The type is resolved at the cast position in the caller compilation and lexical
context, with access, shadowing, uniqueness and complete hierarchy checks. Outer
and operand parentheses share depth and the existing invocation work budget.

This is a strict-argument-only fallback, not general expression or receiver
inference. It does not support primitive, generic, array, annotated, intersection
or nested casts, ordinary reference operands, unknown expressions, call results,
boxing or new conversions. All competing declarations and formal types still
require complete proof; unknown conversion is not false applicability. Existing
cross-scope binary evidence and its independent-signature budget limitations
remain unchanged.

## Evidence scope

The frozen predecessor is `a8429ed3`. Of 20 real Gson cast-null probes, only
JsonArrayTest byte 9164 `(Number)null` and byte 9573 `(JsonElement)null` are new
positive targets: JsonArray.add at 3192..3195 and 3703..3706 respectively. The
other 18 remain refusals. In particular String, Boolean and Character encounter
unknown generic Comparable-edge competitor proofs; other numeric casts need
unsupported cross-scope widening. They are not counted as recovered targets.
The new frozen plan retains nine existing queries alongside these twenty.

Rust RED tests separately establish Token, source-defined String and Parent
selection plus bounded parentheses. These synthetic positives do not claim the
real Gson String overload set succeeds. Negative tests retain malformed syntax
with platform evidence, unsupported operands/types, type-variable shadowing,
unknown competitors and combined depth limits. Earlier single cast-null refusal
assertions become explicit positive checks plus nested-cast refusal checks;
historical real-source acceptance artifacts are not rewritten.

No new persistent metadata or global type identity is introduced. Native analysis
remains JVM-free; optional compiler evidence is separate from runtime behavior.
