# Optional reference-call compiler oracle

This handwritten Apache-2.0 fixture is development evidence for a narrow native
reference-instance-call proof. It is not a production Java dependency, a build
importer, general Java conformance, or an OpenIDEA/JDT LS replacement claim.

Run from the repository root after building its own native endpoint:

```sh
cargo build --locked --release --example oracle_bindings
python3 scripts/check-semantic-oracle.py \
  --jdk /path/to/checksum-verified-temurin-21.0.13+9 \
  --native target/release/examples/oracle_bindings \
  --sources-jar /path/to/pinned/commons-lang3-3.17.0-sources.jar
python3 -m unittest discover -s scripts -p 'test_reference_oracle.py' -v
```

The command runs all older suites too. The optional Commons input still checks
eight unchanged real-source primitive/return calls across all 249 source files;
those are not claimed as new reference-call sites. Fixed-artifact setup remains
separate from this offline check. No external project is built, no upstream
classes are generated, and no fixture program is executed.

## Inputs and independent evidence

`source/` has thirteen explicit files, twenty-two declaration markers, and
twenty-four query markers. `files.tsv`, the fixed checker inventory, and the
handwritten `expected.tsv` must agree. Arbitrary extra or missing Java files,
query markers, declaration markers, or result records fail the check.

The unchanged `BindingOracle.java` uses javac Trees to find the selected source
method. The comparator checks its exact path, owner, kind and name, and requires
its declaration-tree byte range to contain exactly the expected name marker,
not another same-named overload. The native target must match that marker's
exact UTF-8 start/end. An astral character before the queries checks the UTF-16
to UTF-8 conversion boundary. These goldens are not generated from native output.

Only a handwritten dependency, `dependency/dep/BinaryParent.java`, is compiled
to exercise a binary same-name competitor. The checker inventories its sole
class output. `java.lang.Object` and `java.lang.Iterable` are extracted as data
from the checksum-pinned JMOD. The combined directory must contain exactly those
three classes. Binary refusal evidence checks its path, owner, method kind,
name and explicit zero range. It does not claim to distinguish same-owner,
same-name binary overloads, which lack public source ranges.

Javac uses release 21, UTF-8, `-proc:none`, `-implicit:none`, an explicit
classpath and empty sourcepath. The native endpoint runs with PATH set to
`/nonexistent`. Only this optional comparator starts the selected Java tools.
Mandatory Python checker tests use in-memory evidence and local fixtures only;
mandatory Rust tests and the production analyzer remain JVM-free.

## Selected and refused cases

Fifteen exact source selections cover explicit reference parameters/locals,
local and parameter receivers, identity, multi-hop source widening, an
implemented source interface, an unrelated overload, unique more-specific
parameters, two-argument specificity, fixed arity, and a binary Object formal
endpoint. Thirteen were newly selectable over the preserved baseline; two
single-candidate calls were already navigable and are positive controls.

One receiver implements the real generic binary `Iterable<Leaf>` interface.
Its complete member inventory has no queried `pick` method, so its generic
ancestor does not itself invent a competing method. This does not instantiate
Iterable, infer type arguments, or permit generic competing signatures.

Nine compiler-valid refusals retain their exact native candidates, including
same-name source inheritance, a binary inherited competitor, a generic method,
`new`/null/field/call-result arguments, and constructed/call-result receivers.
Null is legal Java here but unsupported by the bounded native proof. A single
candidate on unsupported input must not silently become applicability proof.
No generic return chain, arbitrary expression typer, boxing or varargs proof is
claimed.

Eleven separate invalid cases must each produce javac compiler errors and no
successful compiler binding output. They cover:

- `c.only(1)` against `public void only()`, and `C.only()` against that instance method;
- a wrong reference argument and ambiguous unrelated interface overloads;
- a cross-package protected member through an invalid qualifying type;
- an inaccessible receiver class and inaccessible argument class;
- missing, cyclic and duplicate receiver ancestors, plus a missing argument type.

Their `invalid.tsv` records require exact native refusal evidence. Eight were
false singleton `Resolved` results before this slice. Cycle and duplicate-parent
cases retain their pre-existing `Cycle` and class-identity `Ambiguous` outcomes;
they are not counted as repaired selections or compiler-success comparisons.

## Negative controls and historical correction

The comparator rejects wrong same-named overloads; wrong compiler/native path,
kind, owner, name or range; an overlapping compiler range; missing/extra results;
duplicate/incomplete candidates; a missing binary competitor; and false success
on every invalid fixture. Removing Object must block every supported reference
selection. Removing Iterable must block the generic-ancestor selection. Unknown
or incomplete hierarchy inventory is not evidence that no competitor exists.

The original source fixture and `dev-tools/oracle/expected.tsv` remain unchanged.
`navigation-updates.tsv` documents exactly one reviewed change under its original
no-platform inputs: `receiver_read_call` (`Value m; m.read()`) retains the exact
public `lib.Value.read` candidate but becomes `CallNotResolved`, since Object's
member inventory is missing. The checker first verifies the original
`Resolved/read_method` row before applying the specialized exact check. The
other 62 queries still use the unchanged older comparator. This no-platform
batch therefore has 58 supported bindings and five refusals instead of 59/four.
A second full batch supplies explicit pinned Object data and must satisfy all
63 original goldens unchanged: 59 supported bindings and four refusals. No
shared comparator or original expectation is relaxed to hide the correction.

## Preserved RED evidence

Before core changes, a freshly built release endpoint at
`e0a47eb85ad11d6aa274654b263703fbdd5786c7` passed the entire older optional suite,
including the eight Commons calls. The endpoint and its SHA-256 were preserved
separately before implementation began. All twenty-four new valid queries were
independently compiler-checked against these handwritten markers. Thirteen
planned selections were `OverloadNotResolved`; the null singleton was
`Resolved` rather than the new conservative refusal. All eleven invalid cases
were individually rejected by javac; eight nevertheless falsely resolved in
the baseline native endpoint. Full compiler/native outputs and fixture hashes
are retained in the local validation artifact, not just a passing test count.
