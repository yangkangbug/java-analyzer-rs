# Traditional-switch parser evidence

This additive stage retains complete normalized parser records and source hashes
for all 249 Commons and 211 Gson source files. Historical fixtures are unchanged.
The pre-execution prediction is retained: remove exactly the old opaque switch
reports, not arbitrary parser reports.

Gson matches that prediction exactly (166 to 138 reports). Commons does not:
85 to predicted 58, actual 60. JavaVersion.java reveals two pre-existing trailing-dot
numeric-literal limitations inside the former opaque switch (byte/UTF-16 offsets
5071 and 5075, expected name). The original failed prediction remains frozen;
the comparator adds only these two exact records. No unrelated report, range,
source, order or summary change is accepted.

The complete call inventory separately audits every unqualified invocation, with
historical calls outside SwitchStmt separated from newly visible switch calls.
Existing declaration-target fixtures are not rewritten.

Run scripts/check-traditional-switch-acceptance.py with the pinned Commons
source JAR, java.base.jmod, Gson inputs and native executable. --baseline selects
only the pre-switch full records; it is not an old-or-new acceptance alternative.
The parser current stage in older Gson gates remains the frozen class-literal
stage. Candidate gates explicitly propagate the new switch stage instead.

These checks execute native analysis only. They do not run upstream applications,
build scripts or Java; the independent compiler oracle remains optional.
