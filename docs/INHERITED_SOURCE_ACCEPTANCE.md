# Shared inherited-source acceptance matrix

This slice reuses the current strict Gson gate, the Commons CLI gate and the
existing semantic oracle suite. It does not introduce another standalone checker
or migrate old feature pipelines.

## Frozen real-source observations

`inherited-source-probes.json` pins predecessor `39fa8de8`, its native executable,
source archive, source bytes and historical contracts. The 35 records retain all
29 cast-null records and append six complete records:

- JsonArrayTest byte 5305: unique inherited `getAsJsonObject()`, formerly
  `CallNotResolved`, must select JsonElement 3012..3027.
- functional/InheritanceTest byte 2883: JsonObject's own single-String-argument
  overload competes with JsonElement's no-argument declaration. Formerly
  `OverloadNotResolved`; must select the same parent declaration 3012..3027.
- JsonArrayTest 3001, JsonObjectTest 6712 and JsonPrimitiveTest 10817: child
  `deepCopy` implementations plus the abstract parent declaration remain exact
  two-candidate `Ambiguous` refusals. Same-signature override is not this slice.
- JsonArrayTest 6612: source JsonElement.toString plus binary Object.toString
  remains an exact two-candidate `Ambiguous` refusal.

The frozen inherited-source stage checks 67 rows in exact groups (16, 3, 5, 8, 35), including
all candidate identities, multiplicities, source ownership and UTF-8/UTF-16
ranges. The independent parser stage remains unchanged at 166 current records.
Earlier feature captures retain explicit historical proof/parser stages.
The new capture uses the existing strict entry point:

```
python3 scripts/check-strict-reference-acceptance.py INPUTS --native FROZEN_BINARY --capture-proof-stage inherited_source
```

This mode verifies the exact frozen binary and predecessor records; it is not
current acceptance. Current mode never accepts either old or new interchangeably.
Baseline records and two actual RED observations are retained under
`evidence/inherited-source-20261112/`.

## Cross-project preservation, not invented gains

`examples/real_source/inherited-source-preservation.json` freezes four existing
Commons ReflectionToStringBuilder `this` getter calls (24240, 26630, 27668, 27712).
They already selected ToStringBuilder.getObject/getStyle/getStringBuffer before
this implementation. The existing Commons gate now checks their complete records
and source hashes. They are regression controls, not four new successes.
`super` receiver binding, generic inheritance and binary/interface methods are
not implicitly enabled by these source-only observations.

## Shared independent semantic matrix

The inherited-source-v1 handwritten multi-file suite is called by the existing
semantic oracle runner. It distinguishes unique/two-level inherited methods,
parent/child different-signature overloads and parent-declaration import context
from child name shadowing. Overrides, unsupported interface/generic paths and
incomplete/unknown competing signatures retain exact refusals. Compiler target
identity and native complete candidate records are independently checked.
The fixtures are compiled for developer comparison, never executed as programs;
Java is not a default-test or runtime dependency. Fixture bytes and any explicit
updates to older oracle expectations are hash-pinned, with old goldens retained.

Snapshot/edit refresh checks live in Rust alongside the implementation. They do
not add a new editor adapter or a second acceptance framework. In particular,
return interpretation and access checks must keep the selected declaring owner,
not substitute the subclass receiver's file/import scope.

## Subsequent source-override stage

The same checker and semantic suite now host `source_override`; no additional
runner stack is introduced. The original `source-override-probes.json` forecast
pins predecessor `dcb31d23` and remains unchanged, including its four-positive
prediction. Its 36 records retain all 35 previous positions and add JsonNullTest
position 1261. All four deepCopy sites were genuinely `Ambiguous` with two exact
source candidates on the frozen predecessor.

The first candidate matched the first 35 records but failed JsonNull 1261.
Independent diagnosis proved that **any local-variable annotation** prevents
`reference_declaration` from establishing the receiver's reference type before
override selection. The real local has `@SuppressWarnings("deprecation")`.
Removing local annotations in a separate handwritten diagnostic resolves the
same structural override; `@A`, `@A()` and the argument annotation all refuse.
This diagnostic is not a modified-Gson success claim. Method-header annotation
support does not fix local-variable typing.

The explicit `source-override-probes-v2.json` revision therefore defers only
JsonNull 1261 to its complete original two-candidate refusal. The original plan,
first-failure SHA, exact diagnostic source/log bytes, real source hashes, original
prediction and revised full record are pinned in the version chain. Current
validation requires V2 and rejects the original fourth-positive result; there
is no old-or-new union. The 36-row count is unchanged. **Three real Gson calls**
are improved (3001, 6712, 10817); JsonNull is not reported as solved. All other
prior results, including binary Object.toString at 6612, remain identical.

Current source-selection is exactly 68 rows in groups (16,3,5,8,36). The earlier
35-query capture still uses `--capture-proof-stage inherited_source`; the new
36-query capture uses `--capture-proof-stage source_override`. Each checks its
own binary SHA and complete predecessor expectations. Their prior proof stages
are respectively cast_null and inherited_source; both use the independent
current 166-record parser contract. Neither mode accepts old-or-new results.

The independent handwritten oracle has six new selected calls and seven total
selected calls, separate from the three real Gson improvements. It distinguishes static receiver declaration
selection from runtime dispatch. Plain source ancestor relationships, exact
formal signatures, final restrictions and source return compatibility require
positive evidence. Covariant returns, multi-level overrides and other overloads
are tested independently of false/absent @Override annotations. Unsupported
binary/interface, generic/array, incomplete signatures and invalid declarations
remain explicit refusal controls. Annotation acceptance for override metadata
is not permission to broaden old chained-return typing.

The four existing Commons getter controls remain preservation, not newly fixed
calls. Rust snapshot regressions own edit/rebuild checks. Original baseline
records and RED observations are retained under
`evidence/source-override-20261112/`; candidate acceptance requires the unchanged
expectations, historical captures, complete optional oracle and aggregate gates.
