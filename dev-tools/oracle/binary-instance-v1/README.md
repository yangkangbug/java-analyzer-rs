# Binary instance strict invocation oracle

Integrated into the semantic runner with an explicit native CLI argument. All
Java tools are from the archive-and-installed-bytes verified developer JDK.
Handwritten dependencies and probes are compiled, never executed. Official
javap independently reports each selected owner/name/JVM descriptor; these
expectations are hand-authored, not inferred from native results.

Two distinct compilation contexts load the same class artifacts. Default
response schema stays v1 without new fields; opt-in details give Method
descriptor and the actual linking context. Class and field precision must not
change. Full candidate multiplicity is retained, never deduplicated by a lossy
v1 projection. Standalone context behavior is covered in Rust, not claimed
from the manifest-only CLI.

Public native/abstract/final/synchronized instance methods are legal declaration
selections. Static, varargs, generic, inaccessible, array-formal, ancestor/interface
same-name and malformed flag declarations stay conservative. Array-return
methods may be selected, but binary results do not propagate as receivers.

## Pre-candidate scope correction

The first handwritten forecast expected Outer.Inner to resolve, but the frozen
old CLI returned ReceiverNotBound and the implementer confirmed nested-type
lookup is unchanged. `history/` preserves the exact original forecast, flags,
probe source and complete baseline records; `scope-revision.json` pins them.
Outer.Inner remains an exact negative. A legal top-level Dollar$Owner now tests
raw-dollar binary identity without expanding nested lookup. This correction
preceded all candidate execution.

The next frozen baseline also refused top-level Dollar$Owner: existing
classfile dotted-name/nesting handling treats every dollar as nesting. Revision
3 pins the prior revision and records this second pre-candidate correction.
Both queries stay exact ReceiverNotBound controls. This CLI suite does not
claim raw-dollar Method identity selection coverage; that representation needs
independent Rust target serialization tests. The 11 ordinary direct-binary
positive probes are unchanged.

## Strict fallback topology revision

`history/pre-candidate-topology-v1.json` retains the singleton refusal forecast.
`topology-mechanism.json` preserves the independently compiled Face/FaceOwner
metadata and source mechanism: legacy ancestor hiding versus strict fallback
with empty ancestor sets. `topology-revision.json` pins an explicit Ambiguous
two-candidate contract for both contexts. It does not attribute the change to
identity: the two declaring owners already differed. No new candidate was run
to produce this revision. All eleven direct positive predictions are unchanged.
