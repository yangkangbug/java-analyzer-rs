# Return declaration inventory oracle

This is a bounded return-header inventory slice, not a generic type solver or a
complete parameter/ancestor grammar. The runner reuses the existing verified-JDK
checker, BindingOracle, fixture marker reader, native oracle endpoint, and pinned
Object/platform framing. Java analyzes handwritten fixture sources only; fixture
classes are never generated or executed by BindingOracle.

## Freeze and scope

All fixtures and positive forecasts were authored before any candidate execution
by this oracle worker. The exact published 0697877b native oracle is pinned as
`e03ce5307cce36444382b1be62eb0c23546b8d83ad10035cb642aa96f511ded6`.
`contract.json` preserves all complete baseline query records, independent javac
source targets/ranges or full deterministic compiler diagnostics, and separately
predicted current records. Its one-time capture refuses an existing contract or
a different baseline executable. The manifest pins every fixture, prediction,
and contract. README alone is explanatory/unpinned. Raw preparation logs are
preserved separately in local recovery evidence, not used as contract inputs.

27 cases comprise 11 predicted new selections, one already-supported control,
four preserved refusals, nine compiler-invalid return heads, and two compiler-valid
resource-bound refusals. All candidate rows are complete exact comparisons, not
status-only or target-name-only checks. A selected source target includes path,
kind, owner, name, and exact byte range. Compiler records include full declaration
range; malformed diagnostics use the fixed compiler's code and Locale.ROOT message,
with environment-specific source/work directory strings explicitly forbidden.

The positive shapes cover nested generic types, unbounded and upper/lower
wildcards, reference/primitive array arguments, generic return arrays, qualified
parameterized segments, reference-array wildcard bounds, and a complex-return
caller body. Same-name generic-parameter and generic-method competitors remain
present and refused. A complex source invocation return cannot supply a receiver.
`? extends int[]` is compiler-valid but remains a parser-boundary deferral in this
slice. Direct superclass grammar is deliberately not expanded.

The depth case has 40 nested generic layers, above the existing depth limit 32.
The work case uses a correctly declared 4096-parameter generic class with 4096
actual arguments, not an invalid-arity shortcut. Independent javac accepts both.

## Preserved preparation findings

`predictions-first.json` remains intact, and all failed capture logs are preserved
in local recovery evidence. The old binary
unexpectedly resolved a query surrounded by an illegal top-level wildcard return
header. The completed contract records that actual old success and predicts a
full two-candidate refusal, rather than pretending the old version refused it.
Likewise, the wide but valid header previously resolved; the explicit work-bound
contract requires refusal. The second preparation failure was the oracle's
line-range assumption for an inline method, corrected to the hand-authored method's
`public` start and closing brace without changing fixture/native predictions.
No candidate output was used to rewrite a forecast.

## Run

From the repository root, with the existing verified local developer inputs:

```sh
python3 scripts/check-header-inventory-oracle.py \
  --jdk ../inputs/commons/jdk \
  --jdk-archive ../inputs-cache/d7f5d5cf865a2902b70ff549b97586a7d3f234e016f041ab447ea740c7d66d1c \
  --native evidence/header-inventory-20261112/baseline-oracle --baseline

# Candidate: use the same interface, without --baseline.
python3 scripts/check-header-inventory-oracle.py \
  --jdk ../inputs/commons/jdk \
  --jdk-archive ../inputs-cache/d7f5d5cf865a2902b70ff549b97586a7d3f234e016f041ab447ea740c7d66d1c \
  --native target/release/examples/oracle_bindings

python3 -m unittest discover -s scripts -p 'test_header_inventory_oracle.py' -v
```
