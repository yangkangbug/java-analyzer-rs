# Query-local binary conversion compatibility across compilation scopes

## Bounded ancestor conversion

The strict invocation proof now has one conversion entry for applicability,
class-formal bounds and specificity. Its original strict conversion remains a
separate nonrecursive proof. A complete actual graph containing the exact formal
identity uses that proof alone. Otherwise, a charged identity scan avoids a
necessarily unsuccessful first strict search, and the complete graph may identify
a unique corresponding declaration in another compilation. Every potential raw JVM-name match and cross-scope dotted
match is collected before eligibility is checked: source replacements, generic
declarations, dollar-name projections and competing correspondences cannot be
filtered away to manufacture a unique binary candidate.

For a new ancestor bridge, both corresponding endpoints must be nongeneric,
top-level binary declarations with the exact same raw binary name. The actual
must independently convert to that ancestor using only the original strict
proof, not another bridge or an unknown parameterized edge. The ancestor and
formal then undergo the existing complete binary closure comparison described
below. An exact-root bridge retains its previous nongeneric binary qualification;
this does not newly enable nested-ancestor conversion.

Any candidate that cannot be certified stays unknown, rather than becoming an
inapplicable competitor. Multiple correspondences, missing/ambiguous/cyclic
graphs, unequal selected origins or modeled metadata, and exhausted proof work
all refuse. When exact identity and every possible correspondence are absent
from the complete graph, that completed nominal negative certificate rules out
the conversion without repeating the strict traversal. This is sufficient
negative evidence, not a claim of identical budget outcomes to the old traversal.
Scanning, name work and closure comparison consume the existing shared budget, without a larger
limit or cross-query compatibility cache.

This is a conversion certificate, not declaration equality. ClassIdentity,
same_class, erased duplicate checks and override identity are unchanged. Generic
actual slots retain their original caller/declaration identities; nominal method
returns still use their declaring context. The certificate is not an Object
exception, a shared platform domain, arbitrary library equivalence, JPMS, runtime
classloader equivalence or generic inheritance support.

## Measured boundary

An optional independent compiler comparison checks seven exact source targets
across explicit library/application compilations. The original Gson
`TypeAdapterTest.toJson(null)` query now selects `TypeAdapter.toJson` at bytes
8696..8702, matching javac. The neighboring `fromJson` query and two calls needing
boxing remain refused. Four same-compilation probes and a broader fixed
393-query inventory retain their complete previous results. These checks are
bounded evidence, not whole-project coverage or IntelliJ parity.

The first schedule repeated negative proofs and regressed an existing chained
`fromJson(...).length()` target and `JsonArray.add((Number)null)`. Reusing the
already-complete negative certificate removed that redundant work; the exact old
successes were restored without raising the work limit. Queries still refuse when
the shared proof budget is exhausted; a nearby edit can change that outcome.

## Historical exact-root stage

The following describes the original narrower stage and its retained evidence;
the bounded ancestor conversion above supersedes only its explicit prohibition
on cross-scope widening and its argument-only integration.

This slice adds a bounded flat-classpath invocation compatibility proof, not a
new global Java type identity. It applies only to a strict reference argument
and its formal when both resolved roots are nongeneric binaries with the same
binary name in different compilation scopes. No String-specific shortcut is
used. It reuses the former literal-only complete binary evidence comparison.

Both roots must already have valid declaration-context lookup, access and full
inheritance graph evidence. Each scope independently selects a unique root and
all superclass/interface ancestors. The resulting complete closures must match
node by node in selected origin and every modeled `ClassSignature` field.
That includes class names, version/access/final/interface flags, generic signature
text, superclass/interface lists and modeled field/method signatures.

A shared signature Arc may avoid re-comparing that node's immutable metadata,
but cannot skip either scope's graph proof or any ancestor. Separately parsed
signature equality remains supported and consumes the same proof's work budget;
oversized unshared evidence can conservatively exhaust it. No cache hit is an
independent reason to accept an argument. Shared and independently parsed small
closures can establish the same target when the shared proof budget suffices.
This is not a promise of cache-independent coverage: sharing can leave enough
budget for a proof that independently parsed copies must conservatively refuse.

## Deliberate limits

- `ClassIdentity`, ordinary `converts`, and formal-to-formal specificity are
  unchanged. The bridge is only an exact-root actual/formal applicability branch.
- No cross-scope traversal from binary Child to Parent is enabled, even if a
  later ancestor has a matching name or origin. Source identity is not relaxed.
  A matching name at a differently scoped ancestor is instead an unknown
  conversion, including source replacements. It refuses the entire selection;
  it cannot be treated as an inapplicable competitor to manufacture a winner.
- Same names or even equal bytes at different origins do not establish this
  proof. MR-JAR base/version entries have distinct actual-entry origins and
  remain distinct even when their modeled signatures match.
- Target-release option equality is not claimed. If different selection options
  select the same actual entries and complete modeled closures, this evidence
  can agree; different selected entries do not agree.
- This is modeled-signature evidence, not a raw class/JAR byte hash, runtime
  classloader identity, JPMS/module equivalence or whole-classpath equivalence.
- Types mentioned only by field/method descriptors or generic-signature text are
  compared as metadata strings, not recursively resolved as another dependency
  closure. The proof does not claim those member types have matching identities
  in the two scopes. Subsequent member/return reasoning must keep its declaration
  context and its own proof; no global equivalence is cached or substituted.
- Generic roots, arrays, ambiguity, inaccessible/incomplete inventories,
  duplicate names and exhausted budgets remain refusals. All same-name method
  competitors still undergo the existing complete validation before selection.

## Evidence and historical stages

`src/bind/binary_actual_tests.rs` starts with a genuine failing non-String Token
argument across scopes. Handwritten binary bytes cover shared and independent
parses, different root/MR-entry origins, equal root with different ancestor
origin/signature, changed root metadata, source shadows, missing/duplicate/cyclic
closure, generic/array and unknown-competitor refusal, work exhaustion and
unchanged cross-scope widening refusal. A separate boundary test demonstrates
that non-inheritance member dependencies are not asserted equal by this proof.
Existing workspace tests explicitly advance ordinary String/Object same-evidence
arguments while retaining different-origin and ancestor-selection refusals.

The frozen predecessor is `0bdf6e0f`. Real-source targets are Gson
`JsonObjectTest` bytes 1653 (`add(propertyName, value)`) and 2139
(`add(propertyName, null)`), both to `JsonObject.add` at 2345..2348. Byte 2139 was
explicitly deferred by the [plain-null V2 contract](NULL_STRICT_CALLS.md), not
silently counted as a prior success. That original V1 failed prediction, V2
scope correction and diagnostic record remain unchanged; this is a new additive
capability with its own frozen baseline. The field-argument refusal remains.

The first true two-compilation oracle predicted that a separately parsed JDK
String directory would also resolve. That candidate failed, and its first-failure
log is retained. The fixed JDK String signature has 11 fields and 167 retained
methods (excluding `<clinit>`). Member names, descriptors and generic-signature
text alone cost 5,794 units per copy, or 11,588 for both, before class metadata,
origins, ancestors and graph work. This necessarily exceeds the existing 4,096
proof budget. The diagnostic is retained as
`evidence/binary-actual-20261112/directory-string-budget-diagnosis.json`.
The explicit revised oracle keeps that directory String case as a budget-refusal
control, adds a shared-JMOD String positive, and retains the small independently
parsed non-String Token positive and all previous negative cases. No product
budget, signature comparison or graph requirement is relaxed. Successful proofs
keep the same declared target; cache sharing can affect whether a proof fits.

Native analysis and required tests remain JVM-free. Optional independent compiler
comparisons are separate evidence, not a runtime dependency or a claim of full
Java semantic conformance.
