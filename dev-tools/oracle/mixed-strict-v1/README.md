# Mixed strict-invocation oracle (optional developer check)

Handwritten Apache-2.0 fixtures, not upstream applications. The comparator verifies
both the pinned official JDK archive and installed tool bytes, then compiles only
the trusted BindingOracle helper. It analyzes fixture declarations but never runs
fixture programs. Native analysis still runs with PATH=/nonexistent and treats
platform class files as data. This is not a mandatory Cargo/runtime dependency.

Seven expected native selections cover primitive/reference argument order, multiple
primitive actuals, per-formal specificity, reversed declaration order, three actuals,
primitive widening and the one-candidate set(int,Ref) case. Four compiler-valid
cases must remain native refusals: an unknown call-result argument, null, boxing,
and varargs. The two separate compiler-invalid fixtures cover invocation constant
narrowing and crossed formal specificity (neither candidate dominates every formal).
All native refusals assert complete candidate multisets, not just failure status.

The frozen previous binary is checked with --baseline: it must satisfy the exact
old refusal records and must fail the seven new selected expectations. Candidate
mode is strict and never accepts either old or new results. Removing Object.class
from this check's own temporary platform must recover exact conservative refusals.
The independent real-Gson checker also preserves outer getAsString refusal; choosing
set(0,b) does not establish a return type for further receiver chains.

Run from the repository root, after building the native example separately:

```sh
python3 scripts/check-mixed-strict-oracle.py \
  --jdk /path/to/verified/jdk \
  --jdk-archive /path/to/pinned/jdk-archive \
  --native target/release/examples/oracle_bindings
python3 -m unittest discover -s scripts -p 'test_mixed_strict*.py'
```

This finite evidence is not a claim of full Java conformance, general overload
resolution, inherited/binary selection, generic inference, or performance.
