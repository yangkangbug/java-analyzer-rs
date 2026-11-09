# Additive local-type containment probes

The original 16 queries and expected outcomes remain unchanged. This independent
five-query gate uses the complete existing 211-file/four-compilation model and
retains all 226 extracted Java paths without modifying source bytes.

```
python3 scripts/check-second-project.py /verified/second-project-inputs \
  --native /path/to/java-analyzer --local-types --output /new/report.json
```

The unmodified GsonTest.java SHA-256 is
`24eab771c94196f4a0ffe95e625b4bb41ee5701a5b84e987529ce1a38bb929df`.
The handwritten `local-type-probes.json` freezes these exact UTF-8 byte positions:

- threadAdapter at 5477 and 6138 must select Local 4740..4753, empty owner.
- adapterInstancesCreated at 5834 and 6042 must select Local 4650..4673, empty owner.
- DummyAdapter at 5900 remains Unresolved with no targets.

The original parser misclassified declarations after the local class as fields,
returned ambiguous fake fields for one capture, and lost the later local use.
`--capture-baseline --local-types` explicitly retains failing evidence; its
`validation: baseline_failure` is not a passing acceptance result. Normal runs
reject the mismatch. Reports include exact query records, source hashes for all
226 paths, and a parser-report count for every owned file. Compare inventories
before/after rather than rewriting the historic parser baseline. Error counts
are parser observations, not Java validity diagnoses.

This real-source check does not execute Java, upstream build scripts or tests.
It does not claim real Gson javac equivalence: the manual model lacks generated
GsonBuildConfig. The separate handwritten `dev-tools/oracle/local-type-v1` suite
provides independent javac-selected declaration evidence without claiming local
class binding support. Native lossless parser and malformed-source containment
regressions remain separate from this valid-source comparator.

The default checker command still runs only the original 16-query gate.
`--local-types` runs only the five additive probes. Trusted-main optional CI
invokes both as separate steps; passing one is not evidence for the other.

## Verified before/after result

The preserved pre-fix binary produced Field owners at uses 5477 and 6042,
two ambiguous fake fields at 5834, and Unresolved at 6138. The candidate passes
all four exact Local targets and preserves the unsupported local type at 5900.
The default 16-query gate separately passes unchanged: 11 Resolved and the five
existing SignatureNotCompared, OverloadNotResolved, two Unresolved, and
ReceiverNotBound outcomes. All 226 extracted Java source hashes match before and
after; the owned model remains 211 files and 1,484,012 bytes.

Parser observations changed from 1,513 reports to 331, with GsonTest.java changing
from 915 to four. This is not a semantic success count: the local declarations
remain explicitly unsupported and unrelated parser gaps remain. The separate
handwritten javac gate passes 11 surrounding declarations plus four unsupported
boundaries; it does not compile Gson or claim full-project equivalence.
