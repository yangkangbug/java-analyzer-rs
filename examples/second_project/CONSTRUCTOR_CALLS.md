# Additive construction-argument acceptance on unchanged Gson

This gate corrects exactly one historical query: original `overload_add`, byte
1237 in `gson/src/test/java/com/google/gson/JsonArrayTest.java`, whose expression
is `a.add(new JsonObject())`. The frozen baseline `6ae7947` returns
`OverloadNotResolved` with five candidates. The required result is `Resolved`
with only `com.google.gson.JsonArray.add(JsonElement)`, declaration-name bytes
`3703..3706` in `gson/src/main/java/com/google/gson/JsonArray.java`.
The declaration spelling and constructor argument expression are checked against
unchanged upstream bytes, independently of the native output.

The original `query-plan.json` and `expected.json` remain byte-for-byte historical
records. `constructor-call-probe.json` contains the explicit additive correction.
Before applying it, the checker validates the exact old query and complete old
five-candidate row, the historical artifact hashes and every extracted source.
It never replaces an arbitrary refusal, catches a failure as acceptance, or
regenerates expected targets from successful analyzer results. The other original
15 queries retain their exact expectations. This correction now also applies to
the default acceptance and reference-call preservation checks, so those gates
reject a native binary that still returns the old constructor-argument refusal.
Historical reference-call baseline capture retains its historical original rows.

The dedicated `--constructor-calls` gate checks the complete 16 original rows,
the three previous reference-call probes, five local-type probes, full parser
records and every extracted Java file before and after analysis. The old local
and reference probe plans and reports remain unchanged. These checks do not
widen generic `fromJson`, chained calls, `remove(1)`, or missing generated inputs.
The old reference-v1 handwritten new-argument refusal uses an implicit default
constructor and remains outside the supported slice.

## Input and inventory provenance

Use the existing checksum-pinned `scripts/second-project-inputs.json` setup.
There are **226 extracted upstream Java paths**, of which **211 are selected**
by the four-compilation source model. The remaining 15 are intentionally excluded
by that original model, not removed in response to this change. Source hashes
cover all 226, while the parser inventory and selected-source hashes cover 211.
The complete parser result retains 331 reports in 58 affected files, with canonical
SHA-256 `751c68b759848ec1d2fef620a323d54a67771817f0e727ef3f630f8da660e7df`.
These are distinct inventories; neither count supersedes the other.

No generated `GsonBuildConfig` source is fabricated. Actual whole-Gson compilation
remains blocked by that missing generated input; this gate makes **no real-Gson
javac equivalence claim**. The independent handwritten constructor-v1 compiler
oracle is separate evidence for the narrow language rule, not a successful
whole-project compile. No upstream build script, Java application or Gson test is
executed by this native acceptance gate. Native analysis receives only
`PATH=/nonexistent`; verified JMODs are read as data.

## Reproduce

Build the native executable separately with the locked repository toolchain, and
prepare the existing pinned inputs. No new input fetch is needed if already cached.

```sh
python3 -m unittest discover -s scripts -p 'test_constructor_call_acceptance.py' -v
python3 scripts/check-second-project.py /verified/second-project \
  --native target/debug/java-analyzer --constructor-calls --output constructor.json
```

The three additive modes are mutually exclusive. A preserved `6ae7947` native
binary must fail the command above. Adding `--capture-baseline` instead requires
the exact historical original rows, still requires the already-supported prior
reference/local probes and unchanged parser/source records, and labels the result
`baseline_verified_not_acceptance`. It never swallows mismatched results.
Outputs use exclusive creation and do not overwrite earlier evidence.

The supported construction argument is only plain `new Type()` for an accessible,
nongeneric, top-level concrete ordinary source class with exactly one explicit
public nongeneric fixed zero-argument constructor, a real block with no parser
error nodes and no `throws` clause. This does not prove constructor-body semantics
or implicit-super validity. Constructor annotations are allowed. Implicit defaults, additional
constructors, anonymous classes, arrays, diamond, inference, new-expression
receivers and chains remain unsupported. Existing method competitors, hierarchy,
access checks and proof budgets still apply. See
[the semantic boundary](../../docs/REFERENCE_CALLS.md).
