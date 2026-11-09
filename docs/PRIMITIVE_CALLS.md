# Bounded primitive source-call selection

This slice improves source navigation, not general Java overload resolution or
semantic diagnostics. The production analyzer reads source and class data in Rust.
It never invokes Java. The optional compiler oracle is development-only.

## Positive proof

A direct unqualified call can select a source method when all of these hold:

- The call is in the body of a direct static, nongeneric method in the same
  top-level nongeneric ordinary source class. No explicit superclass or interfaces
  are written. The owning compilation supplies one unambiguous binary
  `java.lang.Object`, with no parent/interfaces or competing same-name method.
- The source file has no parser errors and its declaration/scope inventory passes
  additional structural checks. Zero parser errors alone is not sufficient.
- Every same-name source declaration is a supported static nongeneric method with
  fully known primitive scalar or primitive-array parameter types. Unsupported
  competitors are not discarded to manufacture a winner.
- Arguments are explicitly typed method parameters or ordinary locals,
  parentheses around supported expressions, or primitive-array elements indexed
  by supported byte/short/char/int expressions. Array rank is preserved.
- Strict identity or primitive widening makes an applicable fixed-arity candidate,
  and exactly one formal vector is most specific. Selection is not based on a
  numeric conversion score, declaration order, or argument count alone.

Primitive arrays are invariant. A varargs declaration participates only with its
fixed array parameter; expanded varargs is not implemented. An inner direct call
can be selected even when its result is the receiver of a later unsupported call.
The later call's arguments must never be borrowed by the inner invocation.

## Refusals and compatibility

`CallNotResolved` retains a single source candidate as a hint, without selecting
it. It is not a Java error diagnosis. Within the closed positive-proof context,
a unique but unsupported call also refuses, even when javac accepts it through
boxing, references or expanded varargs. Unselected overloads retain
`OverloadNotResolved` and their original identities; existing `Ambiguous` and
`SignatureNotCompared` outcomes are also preserved.

A separate negative guard rejects demonstrable fixed-arity and known primitive
contradictions for a uniquely collected source method, including `m(1)` against
`m()`. This guard does not require Object data and does not infer the literal's
type to count arguments. It does not cover qualified/binary calls or a singleton
produced by hiding multiple inherited declarations. Outside these explicit
checks, existing unique-name navigation is preserved; `Resolved` is not a
universal applicability guarantee.

The public `ResolveStatus` enum gains `CallNotResolved`. Rust clients with
exhaustive matches must add a branch. Target fields, binary descriptors and query
signatures are unchanged. CLI schema version remains 1 with an additive status
and `limited_call_selection` capability; `method_argument_applicability` remains
false to avoid claiming general support. A valid declaration query still exits 0
when selection is refused. Candidates must not be opened as selected definitions.

## Deliberately unsupported

No literal typing, field-value typing, nested call-result typing, lambda inference,
reference conversion, boxing/unboxing, generic inference, expanded varargs,
qualified/inherited/binary overload choice, constructor choice or full access/
control-flow analysis is added. Resource bindings remain unknown rather than
falling through to outer names. Annotation calls and enhanced-for iterable headers
cannot borrow method/loop bindings outside their scope. Enhanced-for body locals
are supported when otherwise eligible. Malformed syntax and ambiguous inventories cannot supply the new positive proof.
Legacy singleton navigation may still resolve outside the guarded contexts;
parser recovery is not proof of valid Java.

## Evidence

Seventeen native regression tests cover selection, ambiguity, unsupported cases,
known singleton contradictions, source-set visibility, snapshot isolation and
review-discovered scope/receiver failures. Independent review checked 74 exact
query results, including complete refusal candidate multisets and rebuilt/prepared
parity. This is a bounded sample, not a language conformance claim.

The separate compiler gate adds 12 exact source matches and 20 explicit refusals,
plus 16 compiler-invalid cases and mutation/completeness/Object-omission controls.
It also independently analyzes all 249 unchanged pinned Commons Lang sources to
check four IEEE754rUtils calls: double/float `max(array[j], max)` and
`min(array[j], min)` now select their exact two-parameter declarations. The other
37 fixed navigation results remain unchanged. The parser inventory remains 85
error nodes in 24 files; this work does not reduce parser gaps.

See [real-source provenance and query records](../examples/real_source/README.md),
[optional oracle boundaries](ACCEPTANCE_PLAN.md), [signature identities](METHOD_SIGNATURES.md)
and [remaining preview gates](PREVIEW_PLAN.md).
