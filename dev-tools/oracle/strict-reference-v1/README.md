# Optional strict reference invocation phase oracle

These handwritten Apache-2.0 fixtures test a narrow JLS 5.3 / 15.12.2.2 correction:
proved reference actuals do not unbox during strict invocation. A unique strict
identity/reference-widening winner takes precedence over loose invocation.
This is not an unboxing implementation or general Java overload conformance.

The development-only command requires an already built native endpoint and the
existing checksum-pinned Temurin 21.0.13+9 archive and extracted JDK:

```sh
cargo build --locked --release --example oracle_bindings
python3 scripts/check-strict-reference-oracle.py \
  --jdk /path/to/pinned-acceptance/jdk \
  --jdk-archive /path/to/OpenJDK21U-jdk_x64_linux_hotspot_21.0.13_9.tar.gz \
  --native target/release/examples/oracle_bindings
python3 -m unittest discover -s scripts -p 'test_strict_reference_oracle.py' -v
```

No downloads, upstream builds, or fixture program execution occur. Before starting
Java, the comparator verifies the archive checksum in `acceptance-inputs.json`
and every installed regular archive member against that archive, including Java
executables and libraries. It separately verifies the platform JMOD checksum and
extracts exactly nine class files as native input data. Only the existing
`BindingOracle.java` helper is compiled; fixture source is parsed and attributed
with javac Trees, not generated or executed. UTF-8, release 21, `-proc:none`,
`-implicit:none`, explicit empty classpath/sourcepath and a sanitized environment
are retained. Native analysis runs with `PATH=/nonexistent`.

## Independent identities and fixed inventory

Six explicit source files contain 30 declaration markers and 15 query markers.
The fixed inventory, markers, and handwritten expectations must all agree.
An astral/non-ASCII prefix exercises UTF-8 positions. Each compiler result must
match the independently expected path, kind, owner, name, and complete exact
one-line declaration byte range. Native targets must match exact name ranges
and complete candidate sets. A successful compiler invocation alone never passes.

Six planned native selections cover Child-to-Parent widening against int,
Parent identity against int, Object/Parent/int specificity in two declaration
orders, a two-slot reference vector competing with primitive slots in either
position, and an ordinary String literal against String/int overloads.

Nine compiler-valid cases explicitly allowlist bounded native limitations:

- Integer against Object/int selects Object in javac's strict phase. Native must
  still refuse because real Integer's generic binary graph lacks its required
  conversion evidence. This semantic expectation does not relax metadata guards.
- Integer against int alone is valid through later unboxing; native must refuse.
- An int[] competitor is a reference array, not a scalar primitive to skip.
- A null actual is legal but outside this native actual-type proof.
- Static, private, generic, varargs, and inherited same-name competitors each
  preserve their complete refusal inventory, even when javac selects a reference.

Four separate compiler-invalid directories must fail with specified diagnostic
codes and no successful binding records: duplicate declarations, an unknown
reference formal in a mixed primitive/reference vector, an unknown actual, and
Child passed to int with no strict candidate. They are not compiler-resolved
samples. Their native statuses and exact candidate inventories are checked
separately. Removing Object independently requires all 15 valid cases to retain
exact refusal inventories. No legacy fixtures or shared comparator are relaxed.

## Baseline and mutation controls

`--baseline` checks all exact old native refusal inventories, while retaining
identical javac-selected identities, and additionally requires that the new
selection expectations reject the baseline. The baseline endpoint was built
from an isolated `git archive 360d987` snapshot, not the changing worktree.
Local evidence is retained in the separate validation directory, including
revision/tree IDs, native binary digest, full before/after output and original
pre-observation goldens.

Draft refusal *status labels* were corrected after baseline observation:
varargs with int... is OverloadNotResolved, not SignatureNotCompared; duplicate
methods are Ambiguous, not OverloadNotResolved. The Object-omission control also separately records SignatureNotCompared for
specific/reverse/wrapper, rather than assuming their full-platform refusal
status survives removal of a named formal type. Their refusal requirement,
compiler expectation, source bytes and exact candidate inventories did not
change. These are disclosed harness-label corrections, not production fixes or
changes to the six independent selected-declaration goldens.

Offline Python tests never invoke Java or native binaries. They reject missing,
extra and duplicate records/fixtures, incorrect compiler identity/ranges,
wrong same-name native overloads, false successes on allowlisted limitations,
and incomplete/duplicate candidate inventories. Checksum failure controls fail
before archive use. Mandatory Cargo tests and the production runtime remain
JVM-free. This optional suite does not compile Gson or claim whole-project Java
equivalence; real-source acceptance is a separate gate.
