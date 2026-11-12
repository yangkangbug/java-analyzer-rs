# Explicit reference cast-null acceptance

The additive `cast-null-probes.json` freezes predecessor `a8429ed3`, its native
binary SHA-256, the pinned Gson archive/source bytes and all earlier contracts.
Its 29 full query records comprise the previous nine binary-argument records,
unchanged, and all 20 `JsonArrayTest` cast-null `add` sites. Only byte 9164
(`(Number) null`, target `JsonArray.add` 3192..3195) and byte 9573
(`(JsonElement) null`, target 3703..3706) are new positive expectations.
All other 18 retain their exact five-candidate `OverloadNotResolved` records.

Before formal freezing, a temporary handwritten five-type parameter probe in
Gson's actual test compilation called the real five-overload `JsonArray.add`.
Number and JsonElement selected; String, Boolean and Character did not. Their
generic Comparable ancestry leaves other candidate conversions unknown. Exact
binary applicability for the matching candidate does not prove the others
inapplicable. Thus an initial exploratory estimate of 13 improvements was never
frozen as a promise or silently revised after implementation. The preflight is
not a real-project success and is retained in
`evidence/cast-null-20261112/typed-parameter-preflight.json` with source text,
hash, complete query results and unchanged original-source hashes. Seven further
casts involving Integer/Double/Float/BigInteger still require unsupported
cross-compilation widening to Number. This slice does not change `converts`.

## Exact stages and regressions

`proof_stage=cast_null` is the latest additive overlay. `binary_argument` stays
the frozen previous proof stage, and every earlier capture retains its explicit
stage. These stages do not replace older feature-specific baseline semantics.
Parser stage remains independently `current` (166 records) or historical
`baseline` (331); propagation tests exercise real parser checks using synthetic
records and do not claim unavailable historical executables were tested.

The source-selection gate requires exactly 61 navigation/refusal rows grouped
as 16 original, 3 reference, 5 local, 8 string, and 29 current proof rows. Missing,
duplicated, reordered group identities or equal-total substitutions fail.
Every query's positions, compilation/provenance, complete candidate multiset,
UTF-8/UTF-16 declaration ranges and all source/parser digests remain checked.
The original plans and goldens are not overwritten.

`check-cast-null-acceptance.py --capture-baseline` requires the exact frozen
native binary and all 29 predecessor results. Current mode requires only the
new contract, never either old or new. Real baseline queries and the two RED
observations are retained under `evidence/cast-null-20261112/`.

## Independent compiler evidence

The optional handwritten `cast-null-v1` oracle verifies the fixed official JDK
archive and installed bytes. It compiles fixtures but never executes them, and
is not part of the Rust runtime or default JVM-free tests. Typed Parent null
must not select Child; plain null's existing Child selection stays unchanged.
Array, generic, unknown, primitive, nested-cast, call-operand, excessive-depth
and generic-ancestor boundaries remain separate refusal controls. Valid compiler
selections and invalid compiler inputs are distinguished from conservative
native refusals. The fixture manifest and exact target tuples are pinned.

The current-stage implementation is only accepted after the native checker,
optional oracle, prior historical captures and aggregate project gates pass;
freezing or synthetic test success alone is not candidate acceptance.
