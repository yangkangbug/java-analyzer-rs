# Additive reference-call acceptance on unchanged Gson

This gate measures two new declaration selections on the fixed Gson 2.10.1
source archive. It is not a new success ratio for the project, a compiler-parity
claim, or general instance-call support. The original 16 query plan and expected
records, the five local-type probes and their historical report remain unchanged.

## Frozen question

`reference-call-probes.json` was selected against clean engine revision
`e0a47eb85ad11d6aa274654b263703fbdd5786c7` before the implementation changed.
The release CLI and `oracle_bindings` binaries were built with the pinned locked
Rust toolchain and copied before any core edits. The plan has three queries in
`gson/src/test/java/com/google/gson/JsonArrayTest.java`:

| UTF-8 byte | Expression | Baseline | Required candidate result |
| --- | --- | --- | --- |
| 1814 | `array.add(a)` | `OverloadNotResolved`, five candidates | `Resolved`, `JsonArray.add(JsonElement)` only |
| 1901 | `array.add(a)` | `OverloadNotResolved`, five candidates | Same exact declaration |
| 1976 | `array.remove(1)` | `OverloadNotResolved`, two candidates | The same refusal and candidates |

The explicit declaration is `JsonPrimitive a = new JsonPrimitive("a")`.
The receiver has the explicit local type `JsonArray`. The upstream declaration
is still `JsonArray extends JsonElement implements Iterable<JsonElement>`;
no hierarchy edge is removed, rewritten or generated to make the proof work.
The proof must establish applicability and reject competing reference overloads,
not return the first source method named `add`.

The five baseline `add` declaration-name ranges are `2652..2655`, `2918..2921`,
`3192..3195`, `3454..3457`, and `3703..3706` in
`gson/src/main/java/com/google/gson/JsonArray.java`. Only `3703..3706` is the new
selected target. The `remove` candidate ranges stay `4945..4951` and `5494..5500`.
Every comparison also checks origin, compilation, kind, owner, name, UTF-8 and
UTF-16 ranges, requested file/position and query order. Refusal candidates are an
exact multiset: order is immaterial, but missing, extra and duplicate records fail.

## Preservation and source integrity

The new gate additionally rechecks all 16 original query/target records and all
five local-type probes. This preserves the original `new JsonObject()` overload
refusal at byte 1237, generic `fromJson`, chained calls and missing generated
inputs. It compares the full 211-file parser inventory to the existing local-type
report: 331 parser reports in 58 files. A canonical SHA-256 of the complete
parser result, captured from the preserved clean baseline after removing only
the temporary source-root prefix, also fixes every message and byte/UTF-16 range.
These are parser limitations, not Java
semantic diagnostics or independently supported language features.

The existing secure preparation validates the pinned archive and JAR/JMOD inputs
before extraction. All 226 upstream Java files, including 15 excluded from the
manual four-compilation model, are hashed before and after native analysis.
The new gate checks those hashes against the historical inventory. No generated
`GsonBuildConfig` is supplied, no source is edited, and no upstream build scripts,
Java programs or upstream tests execute. The existing default gate's disposable
source-mutation controls are separate and restore their fixture afterward.

The analysis child receives only `PATH=/nonexistent`. Verified `java.base` and
`java.sql` JMODs are data, not a Java runtime. The independent handwritten compiler
oracle is a separate optional development gate. Because the real Gson source
model still lacks generated `GsonBuildConfig`, this report does **not** claim
whole-project compilation or real-Gson javac equivalence.

## Reproduction

Prepare the pinned inputs with the existing documented input-setup flow, then:

```sh
cargo build --locked --release --features cli --bin java-analyzer
python3 -m unittest discover -s scripts -p 'test_reference_call_acceptance.py' -v
python3 scripts/check-second-project.py /verified/second-project \
  --native target/release/java-analyzer --reference-calls --output reference-calls.json
```

The optional extended-acceptance workflow keeps default, local-type and reference-
call checks as distinct steps. `--reference-calls` and `--local-types` are mutually
exclusive. For a separately preserved clean baseline binary:

```sh
python3 scripts/check-second-project.py /verified/second-project \
  --native /preserved/baseline/java-analyzer --reference-calls --capture-baseline
```

Baseline mode validates the exact frozen baseline, including all candidates; it
never catches arbitrary checker failures and calls them success. Its output is
labeled `baseline_verified_not_acceptance`. A baseline run without that option
must fail the candidate gate. Output files are created exclusively and never
silently replace prior evidence.

The offline negative tests exercise malformed/missing/extra query records,
wrong status and declaration identity, byte/UTF-16 ranges, candidate omissions and
duplicates, duplicate JSON keys, same-count parser-message/range drift and source-file mutation,
addition, removal or symlink substitution. Timing is not a pass criterion and no
project-scale latency or performance improvement is asserted.

The final frozen candidate passed this gate, the unchanged original and local-type
gates, and the separate 249-file Commons source/45-query CLI preservation gates.
See `reference-call-comparison.json` for the observed portable before/after
records, exact source inventories and single-run timing observations. Those timings
are not an isolated benchmark or a performance-improvement claim.
