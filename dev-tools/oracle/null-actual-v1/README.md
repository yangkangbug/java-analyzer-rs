# Plain null actual oracle

Handwritten Apache-2.0 fixtures; development-only compiler comparison, not a Java
runtime dependency. The comparator verifies the pinned official JDK archive and
installed tool bytes, then runs only the trusted analysis helper. Fixture programs
are never executed. Native analysis uses class files as data with PATH=/nonexistent.

Null is independent evidence, not an unknown expression, Object, or a widening
reference conversion. JLS 5.3 separately permits conversion of the null type to a
reference type in strict invocation. Supported formals still require the existing
complete nongeneric, nonarray declaration and hierarchy proof. Applicability may
admit null for reference formals, but the winner is determined from formal-to-formal
specificity, not the argument's imagined runtime type or declaration order.

Seven independent compiler selections cover related formals, reversed order,
multiple-null vectors, reference versus primitive competition, a sole reference
formal, bounded parentheses and the existing one-level inner(null).next() receiver
composition. Four compiler-valid native refusals retain unknown call-result actuals,
generic competitors, array formals and field expressions. Three compiler-invalid
fixtures cover null-to-primitive, unrelated reference ambiguity and crossed vector
specificity. Exact candidate records and Object-omission refusals are checked.

```sh
python3 scripts/check-null-actual-oracle.py \
  --jdk /path/to/verified/jdk --jdk-archive /path/to/pinned/archive \
  --native target/release/examples/oracle_bindings
python3 -m unittest discover -s scripts -p 'test_null*.py'
```

`--baseline` checks the frozen pre-null binary and proves it cannot pass current
selected expectations. Historical real-source capture stages remain separate:
a935, 6208877, 4eecefd0 and 66486e21 refer to their own fixed overlays. The new
`null_actual` proof stage does not replace earlier feature-specific capture rules;
parser stages remain independent. Current source-exclusion preservation has exactly
40 queries in groups 16/3/5/8/8, not an old-or-new count range.

The pinned prior-null-updates.json adds exactly five current oracle selections in
reference, string, strict-reference, qualified-primitive and mixed-strict suites.
It verifies all prior source bytes, full marker metadata, earlier guard updates and
complete expected tuples. Original TSVs remain unchanged. Their historical baseline
and missing-platform expectations are retained. Unqualified primitive nullable(null)
and compiler-invalid ambiguous(null) do not become successes.

The real-source null plan is independently frozen; its expectations are not generated
from candidate output. Passing this small compiler fixture does not imply that every
real-source target passes or that platform reference typing is generally supported.

## Explicit real-source scope revision

The original null-actual-probes.json remains byte-for-byte unchanged. It forecast
two real Gson successes; the first candidate disproved the second forecast at
JsonObjectTest byte 2139. The failing receipt hash and full diagnostic records are
preserved in null-actual-deferred-diagnosis.json. The same pre-existing String-local
cross-compilation identity limitation also rejects byte 1653 without a null actual.
Replacing only bytes 2143..2155 with an equal-width String literal or null changes
2139 to the exact candidate, leaving the independent 1653 control unchanged. Input
mutation hashes and all result metadata are recorded; these diagnostic source
mutations are never used as the actual acceptance corpus.

The explicit null-actual-probes-v2.json supersedes the original pinned plan. Only
2139 is revised to the complete CallNotResolved result and marked deferred_refusal.
2527 remains the required new exact success; all other seven records retain their
original values. The validator verifies both version hashes and this exact revision,
and rejects the former two-positive result rather than accepting old-or-new. This
slice does not solve ordinary String-local binary identity across compilations.
