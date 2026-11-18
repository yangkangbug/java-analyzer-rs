# Fixed-arity loose invocation

The existing bounded method selector now shares two invocation phases. Complete
receiver, argument, candidate-family, override and formal evidence is prepared
once. Strict applicability is evaluated first; only a **proved-empty** strict
applicable set starts loose applicability. Unknown evidence or exhausted work
never starts a fallback. A nonempty strict set is final for phase selection,
even when most-specific, access or static/instance checks subsequently fail.

Loose applicability supports scalar primitive boxing followed by reference
widening, and exact wrapper unboxing followed by primitive widening. Most-specific
selection still compares the original formals using the strict relation.
It does not box or unbox formal types to break ambiguities.

The eight language-defined wrapper names locate unique configured binary
declarations. Boxing uses the caller's linking scope. Unboxing selects in the
actual declaration's scope and requires exact declaration identity. Raw names
alone do not establish compatibility across scopes. Wrappers must be public,
final, nongeneric, non-interface top-level binaries with complete ancestry.
These checks do not authenticate a JDK vendor or trust arbitrary archive contents.
Reference widening retains the existing bounded compatibility evidence.

There is no primitive-widen-then-box conversion, invocation constant narrowing,
arbitrary Number/Object unboxing, variable-arity phase, new generic inference,
array conversion or new receiver typing. Null literals cannot be unboxed.
A statically typed wrapper can be applicable even if its runtime value would be
null; analysis does not execute code or prove runtime safety. Existing singleton
name-level navigation is not promoted to a full Java applicability guarantee.

## Evidence and limits

The original Gson TypeAdapterTest calls toJson(1) and toJsonTree(1) now reach
the exact method declaration name ranges selected independently by javac.
Thirteen original JsonObject.addProperty controls, including a String input,
remain refused because their earlier strict proof exceeds the existing budget.
No budget was increased and no proof cache was added.

A separately identified historical same-main diagnostic probe also improves;
it is not another original project call. The remaining complete-record corpus
checks (including 393 broad calls, 848 receiver queries and anonymous declaration
and capture controls) retain their previous results. Native tests cover phase
priority, mixed ambiguity, unknown competitors, all eight pairs, missing/duplicate
or source-impostor wrappers, linking scope, exact work boundaries and edit/fresh/
retained-snapshot equivalence. No public API or protocol shape changes; version
0.7 remains unchanged.

The optional pinned compiler oracles retain their original expected.tsv files.
Their existing current projections advance only strict_loose (unboxing),
mixed_boxed and primitive_boxed (boxing), asserting the previous complete tuple and checking the
same independently selected method identity. State/kind/owner/range mutations
are rejected. Historical baseline and omitted-platform refusal contracts remain.
