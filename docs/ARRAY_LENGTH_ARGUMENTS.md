# Explicit array length in invocation arguments

The existing primitive and reference invocation proofs share a bounded, complete
`array.length` shape check and array-rank rule. A proved length is scalar `int`;
it participates in the existing strict and fixed-arity loose selection phases.
There is no synthetic source target for `length`, no public API or version change,
and no array type inference or new receiver resolver.

## Supported evidence

The receiver is one explicit local variable or parameter name, optionally wrapped
in transparent parentheses. Its declaration must already provide array rank
1..255. PrimitiveProof uses only its existing primitive-component facts.
ReferenceCallProof additionally supports plain reference components, resolved in
the declaration's file and scope with existing identity, access and complete-graph
checks. Declaration postfix dimensions are respected.

Existing declaration-modifier annotations can preserve these facts. A varargs
parameter is an array inside its method body; proving its length does not enable
varargs invocation expansion. Unsupported type-use trees remain refused.

Fields, captures, catch/resource/guard roots, arbitrary new/call/cast/indexed
receivers, inferred variables, unknown components, type parameters, local types
and applied generic reference components are not newly admitted. This does not
prove non-nullness or runtime safety.

## Shared proof and failure behavior

Both domains use the same direct FieldExpr shape/rank rules, not flattened name
chains. Parentheses, direct children and local lookup are charged to the existing
proof budget. Reference components still require their own existing proof.
No cache, budget increase or additional type inventory is introduced.

Failure never borrows a same-named field. The reference argument path retains
only its pre-existing exact `this.field` fallback, and budget exhaustion stops
before that fallback. A real `this.length` long field remains long. Strict
primitive widening still takes precedence over boxing.

## Evidence

The unchanged 460-file source corpus's fixed 167-query survey has 38 newly
resolved Commons calls and no changes to its 16 Gson queries. Independent javac
checks certify all 38 full targets: 37 binary owner/descriptors and one source
method's exact name range. This includes nine original ArrayUtils
`Math.min(array.length, endIndexExclusive)` calls selecting `Math.min(II)I`.
The earlier literal-zero overlays were design controls, not feature acceptance.

The prior 1,297 complete query records remain unchanged. Independent checks also
preserve all 3,393 type-parameter and 94 local-type records, boxing/wire controls,
and exercise the new source target through disk and unsaved Unicode LSP inputs.
Native tests cover both domains, malformed shapes, unknown competitors, ordinary
fields, declaration dimensions, unsupported roots, budget boundaries, and
edit/fresh/retained-snapshot equivalence.

Evidence is in `evidence/shared-array-length-20261118` and
`evidence/array-length-independent-20261118`. This is a bounded expression-evidence
increment, not a claim of complete Java array support.
