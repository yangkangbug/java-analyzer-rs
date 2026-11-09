# Local-type containment oracle

This separate opt-in suite uses the existing BindingOracle and pinned development
JDK. The handwritten source is parsed and analyzed with `Trees.getElement`; no
fixture application is compiled to runnable output or executed. Production and
ordinary Cargo tests require no JVM.

`expected.tsv` is hand-authored. Eleven supported queries require exact native
path, kind, name, empty local/parameter owner or exact member owner, and UTF-8 name
token ranges independently associated with javac's selected declaration tree.
Four compiler-valid queries inside the local class or naming that class require
native `Unresolved` with zero targets. Those are documented unsupported cases,
not semantic-equivalence successes. The fixture includes an astral character,
local/field shadow collisions, declaration order, nested scope, enclosing locals
and parameters, and unaffected sibling methods. Local-class fields and parameters
must not substitute for enclosing declarations. Wrong shadow targets, false owners,
wrong ranges, missing output and fabricated unsupported targets must fail.

Run through `scripts/check-semantic-oracle.py --jdk /verified/jdk`; every previous
oracle suite remains independently required. The helper is unchanged.

Before the core change, the preserved endpoint failed `after_type` with
`Unresolved` instead of the enclosing local. It also returned a fake outer field
for the later local, leaked an enclosing parameter into a local-class body, and
lost sibling method bindings. The compiler accepted all 15 marked queries.

The independent real Gson probes live in
`examples/second_project/local-type-probes.json`. They are separate evidence:
manual reviewed exact positions over checksum-pinned unchanged upstream bytes,
not a claim that the real Gson project compiled or matched javac. The manual
source model lacks generated GsonBuildConfig. No upstream build or test scripts
are run, and no generated substitute source is supplied.

After containment, all 11 surrounding bindings and four zero-target unsupported
boundaries pass, along with all existing oracle suites and the mutation controls.
