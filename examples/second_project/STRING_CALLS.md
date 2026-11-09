# Additive string-literal argument acceptance

This gate asks eight fixed questions of unchanged Gson 2.10.1 sources. Seven
calls are expected to gain exact declaration selections; one overloaded call
must retain its complete refusal. It measures a bounded expression-typing slice,
not a project resolution percentage, general overload support or whole-Gson
compiler equivalence.

## Frozen source questions

The baseline is engine revision `aa3f2a12c3a24094667b7c225668c1b17585b161`.
All offsets below are UTF-8 declaration-selector offsets in
`gson/src/test/java/com/google/gson/`, compilation `gson/test`.

| File and line | Expression | Byte | Candidate target |
| --- | --- | --- | --- |
| JsonObjectTest.java:162 | `jsonObj.get("")` | 4826 | JsonObject.get(String), 5914..5917 |
| JsonObjectTest.java:221 | `o.remove("Hello")` | 6228 | JsonObject.remove(String), 2758..2764 |
| JsonObjectTest.java:233 | `o.remove("Hello")` | 6465 | Same remove declaration |
| JsonObjectTest.java:246 | `original.get("key")` | 6798 | Same get declaration |
| JsonObjectTest.java:247 | `copy.get("key")` | 6860 | Same get declaration |
| functional/JsonTreeTest.java:75 | `jsonElement.has("stringValue")` | 2542 | JsonObject.has(String), 5601..5604 |
| JsonObjectTest.java:241 | `original.add("key", firstEntry)` | 6656 | JsonObject.add(String, JsonElement), 2345..2348 |
| JsonArrayTest.java:222 | `jsonArray.add("Hello")` | 6394 | Retain OverloadNotResolved and all five add candidates |

The seven selected targets are in
`gson/src/main/java/com/google/gson/JsonObject.java`, compilation `gson/main`.
The aa3 baseline returns `CallNotResolved` with the exact single target hint at
each of those seven sites. Baseline capture verifies those refusals explicitly;
it never treats arbitrary checker failure as a valid baseline. Running that
binary in candidate mode must fail.

The last call retains JsonArray.add candidate-name ranges 2652..2655,
2918..2921, 3192..3195, 3454..3457 and 3703..3706. Although Java itself selects
add(String), this bounded conversion proof still refuses when generic binary
ancestry prevents establishing a competing overload's inapplicability. Do not
turn this gate green by deleting competitors or widening the claimed scope.

The get probes target inner direct calls only: they do not certify their outer
getAsBoolean/getAsJsonArray/size chains. Explicit local declaration types supply
receiver evidence even when their initializers are unsupported. The mixed add
probe composes the new String identity with the existing explicit local
`JsonArray firstEntry` reference-widening proof.

Literal identity crosses compilation scopes only when both supplied binary
closures have matching origins and complete retained ancestry signatures. This
is needed because the caller is in `gson/test` while the formal declaration is
in `gson/main`. Missing or different binary evidence still refuses; ordinary
reference identity and conversion rules remain unchanged.

## Integrity and preservation

`string-call-probes.json` is pinned in its entirety by SHA-256 in the checker.
It pins the source archive, every queried/target source and eight historical
acceptance artifacts. Exact argument expressions at the fixed offsets and
independently inspected formal spellings are checked, not just method names.
Expected targets are specified independently of native baseline output.

The standalone checker reuses secure local `prepare_fixture` and the current
constructor acceptance gate. Both string baseline and candidate modes preserve
all 16 original queries with the existing constructor correction, all three
reference queries, all five local-type queries, all 226 upstream Java hashes,
and the full 211-file parser inventory (331 reports in 58 files). The canonical
parser record digest preserves messages and byte/UTF-16 ranges as well as counts.
No historical query or golden is rewritten. No source is generated, repaired,
removed or changed, including excluded sources and missing GsonBuildConfig.

Every new output row is checked exhaustively, including order, input position,
source compilation, status, exact target identity and byte/UTF-16 ranges.
Candidates are exact multisets: missing, extra and duplicate candidates fail;
only ordering within a candidate set is immaterial. Strict JSON parsing rejects
duplicate object keys. Output files are created exclusively without replacement.

Offline mutation tests exercise baseline/candidate separation, every row's
omission/duplication, wrong statuses, candidate loss/duplication, wrong target
identity and ranges, changed query metadata, edited pins/plan, altered source
bytes, changed expression spellings and wrong formal signatures.

## Reproduction

Use already verified local inputs from the existing input-setup flow. This gate
never downloads inputs, runs upstream scripts, compiles Gson, or starts Java.
The native analysis child has only `PATH=/nonexistent`; supplied JMODs are data.

```sh
python3 -m unittest discover -s scripts -p 'test_string_call_acceptance.py' -v
python3 scripts/check-string-call-acceptance.py /verified/second-project \
  --native /preserved/java-analyzer-aa3-baseline --capture-baseline \
  --output string-baseline.json
# This must fail, not count as acceptance:
python3 scripts/check-string-call-acceptance.py /verified/second-project \
  --native /preserved/java-analyzer-aa3-baseline
# Candidate, after a locked native CLI build:
python3 scripts/check-string-call-acceptance.py /verified/second-project \
  --native target/debug/java-analyzer --output string-candidate.json
```

The report embeds all preservation records, before/after source hashes, native
binary hash and frozen plan hash. The separate handwritten compiler oracle
provides independent development evidence; this real-source gate does not claim
whole-project javac parity. No timing or performance threshold is asserted.

## Observed result

The frozen aa3 baseline passed strict baseline capture and failed candidate
acceptance. The candidate after the literal-only cross-compilation identity
bridge selected all seven intended source declarations, including the mixed
String/reference call, and retained the complete five-candidate add refusal.
All 16 original, three reference and five local-type records passed preservation;
all 226 source hashes and the complete 211-file parser result remained unchanged
(331 reports, 58 affected files). Eight offline mutation-test groups passed.
The initial candidate failed all seven selections because binary identities were
compilation-local; that failed evidence was retained rather than changing the
plan, corpus or expected targets.
