# Shared own-static invocation selection

Bare calls inside a supported static source method now reuse the existing
complete invocation selector and exact enclosing-owner evidence. Navigation
enters this strict path only where the previous result was an unresolved
overload family. Existing singleton declaration navigation remains name-level:
it is not a proof of applicability or a source of return-type evidence.

Once admitted, an unknown competitor, ambiguity, exhausted budget, or invalid
instance winner ends this proof. It cannot fall back to the primitive selector.
Static and instance declarations remain competitors; access and static
eligibility are checked on the selected winner. External type-qualified calls
retain their existing public-access restriction. Internal invocation-result
composition uses the same strict owner mode, not singleton navigation hints.

## Shared fixed-arity adapters

Source ellipsis parameters and binary varargs descriptors already carry an array
rank. A shared validity check admits those declarations in strict phase 1 and
loose phase 2 with that final array parameter, without adding another dimension.
Malformed tails and incomplete families remain unknown. There is no spread
(variable-arity phase 3) invocation support. A call needing phase 3 is not proved
invalid Java merely because this engine cannot select it.

Explicit primitive array indexing can supply an argument under the existing
ArrayValue proof: prove base and index once, accept scalar byte/short/char/int
indices, and decrement rank. No boxed-index unboxing is added. The existing
rank-one reference-array argument path is preserved; multidimensional reference
projection, generic arrays, new array-return inference, and broader receiver
lookup are not added.

These mechanisms share existing conversions, strict-before-loose ordering,
specificity, complete-family and erased-signature checks. They do not change
budgets, add caches, alter the project model, or introduce ancestor member-type
resolution. Public API and version 0.7 remain unchanged.

## Evidence and compatibility

The unchanged original Gson/Commons 167-query survey gains 18 exact source
targets: nine ArrayUtils reverse calls and nine shift calls. All 18 are checked
against independent javac declaration-name ranges. The old 1,297 complete query
objects, 3,393 type-parameter queries and 94 local-type queries remain unchanged.
Independent disk and unsaved-Unicode LSP checks cover the 18 new sites.

The first isolated implementation genuinely regressed primitive index arguments
and fixed-arity calls involving varargs declarations. The two shared adapters
restore these cases without fallback, larger budgets or weakened negative
controls. Historical failures are retained. Twenty-three old refusal assertions
are migrated individually using 17 exact source compiler fixtures, two exact
private source controls, three paired binary compiler fixtures, and one artificial
String metadata inventory control. That last control is not a claim about real
JDK String overloads. Unknown, generic, malformed, access and platform controls
remain refusal tests.

This is bounded invocation analysis, not complete Java typing or overload
resolution. Optional javac validation is test evidence, never a runtime dependency.

## Current acceptance projections

The first formal gate retained 46 passing and six failing entries. The six
failures represent four independently confirmed selection upgrades, not six
product defects: Commons q26 repeat(String,int) (shared by source/CLI/configure),
primitive null_refusal selecting nullable(B), strict_varargs selecting its
non-varargs reference overload, and effective-binary query 18 selecting
dep.Base.pick(I)I rather than the int[] varargs competitor. Current entry points
hard-check the former complete records before projecting these precise targets.
Historical TSV/JSON goldens remain unchanged; wrong overload, owner, kind, range,
origin and context mutations remain rejected. The original 45-query Commons
batch has only q26 changed; six following semantic suites (120 queries) have only
null_refusal changed.
