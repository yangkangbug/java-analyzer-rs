# Native configuration acceptance

This optional offline gate exercises the shipped `configure` command on the
same pinned Commons Lang and four-compilation Gson inputs as the existing
acceptance suites. It requires an already-built CLI and Python 3.11+ on POSIX
with `waitid(WNOWAIT)`. It never downloads inputs, runs upstream build scripts,
executes Java, or manufactures missing Java sources.

```sh
cargo build --locked --release --features cli --bin java-analyzer
python3 scripts/check-configure-acceptance.py \
  /verified/commons/sources.jar /verified/commons/java.base.jmod \
  /verified/gson \
  --native target/release/java-analyzer \
  --output /new/configure-report.json
python3 -m unittest discover -s scripts -p 'test_configure_acceptance.py' -v
```

Input setup is separate: use `scripts/fetch-acceptance-inputs.py` and
`scripts/fetch-second-project-inputs.py` as documented in the existing corpus
plans. The Gson directory must contain all six checksum-pinned inputs. Report
creation is exclusive: the checker refuses to replace an existing report.

## Independent controls

The manual manifests come from the previously reviewed corpus models. Configure
arguments are spelled out separately, not translated from emitted JSON. Checks
require strict schema-v1 JSON, a newline, absolute paths, and explicit unchanged
public resource defaults. Only path spelling and omitted manual defaults are
normalized. Compilation order, opaque two-part identifiers, source root order,
dependency order, binary order and duplicate multiplicity are never discarded.

Every native process receives only `PATH=/nonexistent`, with no inherited Java
variables. Inputs are cwd-relative, and the emitted manifest is inspected from
a different directory after relocation. Identical emitted bytes are then saved
beside the manual control for historical manifest-relative query documents.
That second copy changes no query file, offset, expectation, source or manifest
byte. Fixture trees are checked for writes around successful and failed
configure invocations.

- Commons Lang: all 249 Java files and 3,676,819 bytes; the original 45 queries
  and 70 targets; unchanged parser inventory of 85 reports in 24 files. Full
  source and binary inspection plus every parser/declaration result must match
  the manual control.
- Gson: all four original source scopes and edges, 211 selected Java files and
  1,484,012 bytes, plus hashes for all 226 extracted Java files including
  exclusions. The unchanged string gate runs all eight string probes, original
  16 queries including the constructor correction, three reference probes,
  five local-type probes and the frozen complete parser-record digest (331
  reports in 58 files). Manual and generated results must match exactly.
- Missing `com.google.gson.internal.GsonBuildConfig` is not repaired. Its type
  query remains `Unresolved`; its `VERSION` field query remains `ReceiverNotBound`.
  Neither may have invented targets. The original raw Java template remains
  byte-for-byte preserved among the excluded sources and must not become owned
  or loaded source.

Four-scope Gson `inspect --details` exceeds the existing 10,000-result-item cap.
The gate requires the same `output_limit` refusal for both manifests and compares
the complete four-scope **summary**, including owned/visible source inventories,
edges, ordered configuration, class counts and shadowing. It does not raise the
cap or claim whole-graph detailed inspection succeeded. Exact binary-origin
sequences are additionally compared in four explicitly labelled binary-only
projections. Each removes source roots and dependency edges while retaining
that scope's original binary inputs, order and target release. These projections
are not evidence of source visibility; the full-graph summary and unchanged
query suite provide that evidence.

Eight negative native controls cover missing source/platform acknowledgments,
conflicting empty-input flags, missing source paths, corrupt binary archives,
unknown dependencies and duplicate compilations. Every one must return exit 2,
strict newline-terminated stderr JSON, no stdout and no product file writes.
Offline checker tests reject plausible wrong scopes, reordered or duplicated
inputs/inventories, altered budgets, malformed JSON, missing newlines, fabricated
generated-source resolutions, unexpected writes and overwritten reports.

This is bounded fixture evidence. It is not automatic Maven/Gradle import,
complete JDK/JPMS modeling, Java conformance, a semantic diagnostics suite,
upstream test execution, or a performance comparison. `target_release` controls
multi-release JAR selection only.
