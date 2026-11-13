# Traditional switch scope oracle

Developer-only compiler evidence. The native runtime has no Java dependency.
Predictions were handwritten before any compiler/native observation, then verified
by the pinned javac declaration-tree oracle. Neither fixture programs nor external
project build scripts are executed.

The three source files cover one shared switch-body scope across colon labels,
fallthrough, nested ordinary blocks, nested switches, UTF-8 offsets, an independent
local after the switch, enum selector typing, and local-class containment.
Nineteen complete query rows predict sixteen ordinary declaration bindings and
three deliberate refusals: constant/enum case labels and a local-class body.
Case labels are not claimed to have ordinary expression-name semantics.

Three separate invalid source files are rejected by javac: a switch local leaking
out of its body, duplicate local declarations across labels, and duplicate case
values. These are compiler controls, not native semantic-diagnostic claims.

The baseline executable is the exact pre-change oracle_bindings from local
dd69d97 / remote 436277a8. Full baseline and candidate rows are separate. A candidate
row is derived only from the handwritten target marker and explicit predicted
status/kind/owner, never from candidate output. All source bytes, predictions,
compiler records and frozen baseline are pinned by hashes. Comparator mutation
controls reject omissions, extra rows, status/target changes and unsupported
queries made falsely successful.

Run:
    python3 scripts/check-traditional-switch-oracle.py --jdk /pinned/jdk \
      --jdk-archive /pinned/archive --native target/release/examples/oracle_bindings

Use --baseline only with the recorded pre-change executable. Capture is a
one-time freeze operation that refuses to overwrite the contract. Parsing and
incremental recovery cases are covered separately by native Rust tests.
This fixture is not a claim of complete switch semantics or Java conformance.
