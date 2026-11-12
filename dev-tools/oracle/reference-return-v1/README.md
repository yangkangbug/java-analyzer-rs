# One-level reference-return oracle

These independent handwritten Apache-2.0 fixtures are compiler inputs, never
applications to execute. The optional comparator verifies the pinned JDK archive
and installed tools, compiles only the trusted BindingOracle helper and obtains
static declaration identities from javac. The native analyzer still runs without
a Java search path. No new default Cargo or analysis runtime dependency is added.

Two selections use a direct and parenthesized one-level receiver. Factory imports
model.Product while the caller imports shadow.Product. Factory's body creates
model.Child, whose value method overrides Product.value. Both expected navigation
targets are nevertheless model.Product.value: the declared return type, not a
runtime guess, caller import, or override. Cross-compilation identity is tested in
native Rust fixtures; this compiler comparator's source root is one compilation
and does not claim that separate-scope coverage.

Five compiler-valid native refusals cover array/index receivers, a generic method
return, extra chain depth, an unsupported inner argument and a call result used as
an ordinary argument. A separate compiler-invalid fixture has an unknown return
type. Candidate outer receiver failures are terminal ReceiverNotBound with zero
targets. The unchanged ordinary argument case retains CallNotResolved and its
complete candidate. Three further semantically valid String `+` and `+=` receiver
cases (including call origins on either side of `+=`) are independently compiler-checked and must be refused by native analysis.
Their outer String method is binary, so this source-target-only helper checks
compile validity rather than pretending to provide a source declaration range.
Object omission tests exact refusal records for the seven main fixture queries.

```sh
python3 scripts/check-reference-return-oracle.py \
  --jdk /path/to/verified/jdk --jdk-archive /path/to/pinned/archive \
  --native target/release/examples/oracle_bindings
python3 -m unittest discover -s scripts -p 'test_reference_return*.py'
```

`--baseline` describes the exact frozen 4eecefd0 refusal records, and separately
proves they fail candidate expectations. Candidate mode never accepts old-or-new.
The real Gson contract fixes both outer calls at bytes 1986 and 2489 to
JsonElement.getAsString, bytes 6434..6445. JsonElement is an abstract class but this
method is directly declared and concrete; this is not abstract method inference.

## Preservation stages

`proof_stage` labels only additive overlays, not the historical semantics of a
feature's own `--capture-baseline` result:

- `pre_primitive`: original strict/reference state before primitive selection;
- `single_primitive`: remove(1) selected, mixed set still refused;
- `mixed`: set(0,b) selected, call-return receivers still refused;
- `reference_return`: latest explicit overlays, including both outer calls.

Parser stage is independent: the old strict/string capture stages retain 331
reports, while a935, 6208877 and 4eecefd0 captures retain 166 reports. Every available
one of those three binaries was executed; synthetic cross-layer parser tests do
not claim to execute unavailable earlier historical binaries. Original query
plans and expected files remain unchanged. The original 16-row inventory applies
one explicit correction at index 13 (byte 1986), retaining all other full rows.

## Earlier oracle refusals

The digest-pinned receiver-guard-updates.json preserves both older fixture trees
and golden TSVs. Only primitive-v1 receiver_spine_outer_refusal and reference-v1
reference_receiver_return_refusal change from Unresolved with no targets to
ReceiverNotBound with no targets. Their unqualified inner calls remain unsupported;
this is terminal-refusal hardening, not additional successful propagation. Current
suites validate all pinned sources, the complete query/declaration marker maps,
all earlier expected tuples and the prior primitive-return overlay before applying
these exact two changes. Historical goldens are not rewritten.
