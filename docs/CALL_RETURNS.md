# Proven primitive call results

The analyzer can now compose the existing strict source-call proof: an inner
call that has a proved selected declaration can supply its explicitly declared
primitive scalar result to an outer call. For example, `max(max(a, b), c)` can
select both source methods from explicitly typed float parameters. No method
body, Java program, compiler or JVM is executed to infer the result.

## Evidence carried through the expression

An internal proof returns the actual selected source member. It is separate from
the public navigation result: legacy `Resolved` can still mean a unique name
without argument applicability, and is never enough to type an inner result.

Each inner call independently checks its own method name against the complete
same-owner static/nongeneric candidate set and supplied binary Object context.
The caller must still satisfy every [primitive-call restriction](PRIMITIVE_CALLS.md).
Parameter applicability and specificity select the inner declaration first.
Its return type cannot create overloads, remove competitors, or steer selection.
Only then does its explicit scalar result participate in the outer proof.

Supported result types are boolean, byte, short, char, int, long, float and double.
They are declaration types, not values: a body returning an invalid expression
is not diagnosed here. The result header is validated independently of parser
error counts. A throws-clause Type must not be mistaken for the result Type.
Throws headers accept only plain qualified reference syntax; Throwable inheritance
and checked-exception analysis are not implemented. Only a structurally complete
direct unqualified CallExpr can supply this evidence.

## Bounds and refusals

The shared proof has a recursion depth limit of 32 and a work budget of 4096
charged proof units. These are conservative analysis limits, not Java language
limits, exact CPU-operation quotas or a bound on all parsing/indexing work.
Exhaustion supplies no type and cannot turn into a guessed winner. The native
CLI's separate execution supervision still applies.

Void, reference, generic, annotated or array results remain unknown for this
slice. Prefix array returns and postfix result dimensions are not propagated;
the latter currently also encounter a parser limitation. Literal, field, cast,
conditional and general call-result typing remain unsupported. Qualified,
inherited, binary and constructor calls do not acquire new proof support.
Existing primitive-array arguments and fixed-array varargs treatment remain,
but returning an array does not become a new typed expression.

A nested call may have a valid source-navigation target while its result type is
unsupported. That does not justify selecting the outer invocation. Existing
neutral refusal statuses and candidate identity are preserved. Outside the
proof's guarded context, legacy name-navigation caveats still apply; this is
not a universal claim that all invalid or unsupported calls refuse.

## Real-project and preview boundary

Four additional queries in the unchanged Commons Lang sources check the outer
calls in double/float max/min three-parameter implementations. Their inner calls
already resolved at the previous revision. The four expected outer declarations
are independently compiler-selected, not taken from analyzer output.

The original 41 query records are retained unchanged. The expanded set of 45
has 35 resolved positions and the same ten old refused positions. Adding these
four queries is not a claim that any of the ten old gaps was solved. The fixed
parser inventory, source bytes and 3,030-call namespace audit remain separate
acceptance checks. See [source provenance and comparisons](../examples/real_source/README.md).

This improves a composable expression-typing path through the actual CLI, not
preview delivery by itself. A second real project shape, reproducible distribution,
measured memory/reload behavior and editor buffer/position integration remain
[explicit preview gates](PREVIEW_PLAN.md). General semantic diagnostics,
completion, references and refactoring are not delivered by this slice.
