# Frozen strict-reference acceptance

Selected at repository 360d9878196825e5891280ae9c5b5c934e1875e3, before observing a candidate. Native source baseline: dcf446d. Frozen native SHA-256: f8c89bc9cd9318334c0f9d62c8a9343ef9a243ce1ce975c50730a8033c3255e6. Independent source inspection and the earlier Gson audit select these exact sites; expected answers are not regenerated from candidate output.

At JsonArrayTest selector byte 1843, `array.remove(a)` must select JsonArray.remove(JsonElement), name bytes 4945..4951 in gson/main, from gson/test. The explicit local is JsonPrimitive a and JsonPrimitive explicitly extends JsonElement. JLS 5.1.5, 5.3 and 15.12.2.2 permit widening reference in strict invocation and exclude unboxing in that phase. A strict winner stops later phases. The scalar int competitor at 5494..5500 is therefore inapplicable. Baseline is exactly OverloadNotResolved with both candidates, not an arbitrary error.

Preserve remove(1) at 1976 as OverloadNotResolved with both candidates; set(0,b) at 2412 as CallNotResolved with sole set name 4463..4466; and outer getAsString at 1986 as Unresolved with no candidates. Integer typing, mixed argument typing and call-result receivers are out of scope.

The JSON plan freezes source archive/source file hashes, exact query expressions, source and formal spellings, old and desired targets, and historical artifact hashes. The checker validates all ordered query metadata and candidate multisets including duplicates, compilation, provenance and UTF-8/UTF-16 ranges. Existing string, constructor, reference, local and original gates run unchanged in both modes, preserving all 211 parser-file records and 226 Java hashes. No historical expectation is regenerated.

Offline usage (already provisioned pinned inputs and already built native executable):

    python3 scripts/check-strict-reference-acceptance.py INPUTS --native NATIVE --output REPORT

`--capture-baseline` checks exact old outcomes but never claims candidate acceptance. A frozen old native must pass that mode and fail default mode. Analysis uses the existing bounded runner with PATH=/nonexistent, executes no Java or upstream code, fetches nothing, and uses no user computer. This is source/JLS evidence, not javac equivalence or full Java conformance.

## Observed result

The frozen native passed exact baseline capture and failed candidate acceptance
specifically at `remove(a)`. Debug and release candidates selected only the
independently specified `remove(JsonElement)` declaration. All three refusal
controls, historical query records, 226 source hashes and the complete 211-file
parser inventory remained unchanged (331 reports in 58 files).

An actual framed LSP comparison on the unchanged pinned project returned null
from the baseline and the exact source range from the debug candidate. The three
control queries stayed null. This is protocol evidence, not a real-editor Gson
scenario. The separate native LSP regression exercises unsaved target changes,
UTF-16 coordinates, refusal/repair and stale versions.

The [handwritten strict-phase comparator](../../dev-tools/oracle/strict-reference-v1/README.md)
executes pinned javac only for independent development validation. Its compiler
results and bounded native refusals are distinguished explicitly; it does not
compile Gson or add a JVM to the analysis engine or preview runtime.
