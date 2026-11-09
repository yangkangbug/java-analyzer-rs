# Optional independent declaration-binding oracle

This development tool uses a **checksum-verified Temurin 21.0.13+9 JDK**, not
an ambient JDK. It is outside the Rust library, normal Cargo tests and production
runtime. Build the native endpoint, then run:

```
cargo build --locked --example oracle_bindings
```

```
python3 scripts/check-semantic-oracle.py --jdk /path/to/verified/jdk
```

The optional acceptance-artifact fetch script can supply the pinned JDK. The
version guard here is not a cryptographic integrity check: verification belongs
to that fetch/extraction boundary. No download occurs in this comparator.

All fixture and helper sources are handwritten for this Apache-2.0 project.
`expected.tsv` is hand-authored, never regenerated from native output. Its rows
state the queried use, expected native status, and intended source declaration.
`D` comments label declarations and `Q` comments label queried identifiers.

The initial seven-file project checks single imports, cross-file inheritance, parameters,
local uses, a block local shadowing a field, return to field lookup after the
block, source-field receiver provenance, and a uniquely named method. The
compiler independently resolves each query with `Trees.getElement`, then obtains
its actual declaration using `Trees.getPath` and source positions. It emits
source path, element kind, enclosing type, name and complete declaration-tree
range. UTF-16 compiler positions are converted to UTF-8 byte offsets, tested by
an astral character before queries. The comparator associates that actual tree
with exactly one matching named declaration marker inside its range. It compares
the native result's path/kind/name/owner and exact name-token byte range. It does
not confuse compiling successfully or prepared/rebuild agreement with binding
agreement. Owner comparison follows the native public target conventions:
class owners
are their own qualified type identity (from javac TypeElement), members use the
declaring type, and locals/parameters have an explicitly empty native owner.
All target fields are checked; declaration path and exact name range identify
locals even though their native owner field is empty.

Additional files check comma-separated fields and locals, calls and array
initializers containing nested commas, per-declarator postfix arrays, and a
scalar sibling after an array declarator. They check earlier-local visibility
inside a later initializer, field lookup before a shadowing local declaration,
and restoration of field lookup after the nested block. `lib.Holder` imports
`model.Payload`, while a conflicting `app.Payload` tests that source-field chains
retain the declaration's import context, including later declarators. Array
variable queries prove declaration identity; they do not independently prove
native array-type dimensions. Native regression tests cover that representation.

The original three explicitly unsupported cases have separate expectations: javac selects the
`pick(int)` overload while native reports `OverloadNotResolved`, and javac binds
`super.inherited` while native reports `ReceiverNotBound`; indexed-array
`vector[0].count` binds a source field in javac while native reports `Unresolved`
with no target. These are **not**
reported as successful semantic equivalence. Missing-name and incomplete-input
fixtures are separate and must cause compiler errors; compiler error runs never
produce successful comparisons. Deliberately wrong golden targets (an unrelated
declaration, a sibling in a comma declaration, and the same-named field in the
wrong package) and a corrupted compiler declaration range must fail. Optimized
Python (`-O` or `PYTHONOPTIMIZE`) is rejected explicitly and tested as a subprocess
negative control. The missing-name case separately
requires native `Unresolved` with no target.

The `super_receiver` case deliberately includes a subclass field with the same
name. During initial development it exposed a real native soundness defect:
javac bound `super.inherited` to `lib.Base.inherited`, while native incorrectly
returned `app.Use.inherited` as `Resolved`. The gate requires the documented
conservative `ReceiverNotBound`, not the incorrect target, and now passes after
the narrow syntax-receiver guard restores that boundary. Merely testing an unshadowed inherited field would
have accidentally concealed this defect. This case must not be removed or
reclassified as a successful match to accommodate native output.

The original `fixtures-v1` suite has a fixed nine-file inventory, validates
marker/golden/output completeness, and rejects compiler errors. All compiler
and runtime calls use explicitly supplied paths, argument arrays, and a minimal
environment. Compiler stages use `-proc:none`, `-implicit:none`, UTF-8, and
`--release 21`, with controlled classpaths and sourcepaths. The helper is compiled
separately. Original fixture sources and mixed-suite consumer sources are only
parsed/analyzed, never generated or executed. The mixed suite described below
additionally compiles dependency fixture sources into temporary class files as
test inputs; those classes are never executed. The original suite uses an empty
classpath and sourcepath; mixed consumers use only the generated dependency
classpath and an empty sourcepath. The JDK platform API is available to javac,
but every queried declaration is in the fixture sources; no platform data is
implicitly loaded into the native analyzer.

Scope is narrow: no overload selection proof, generic inference, JPMS, platform
binding, full access-control, malformed-buffer semantic equivalence, or Java
conformance claim. Review golden changes against Java semantics independently;
do not rewrite expectations just because the native implementation disagrees.

## Multi-declarator baseline

Before the native fix, all seven files compiled and javac selected all 40 query
positions. The first comparator failure was `second_field: Unresolved` instead
of `Resolved`. Eleven supported probes failed, including scalar siblings after
array declarators and imported source-field chains. The existing named-marker
association handles javac's overlapping comma-declaration tree ranges without
weakening checks: the actual compiler-selected element name and unique matching
marker inside that tree are still required. The Java helper was unchanged.

## Method-invocation namespace baseline

`app.Calls` and `lib.CallBase` add 23 hand-authored probes separating method
invocation names from same-spelled fields, parameters and block locals. The same
names in arguments, values and receiver positions must still bind as values.
Inherited methods and calls from a nested class into the enclosing class and its
superclass are checked across source files. `choose(int)` is independently
selected by javac but remains an explicit native `OverloadNotResolved` boundary.
Before the native change, all nine files compiled and javac selected all 63
queries. The comparator failed at `parameter_name_call`: native selected the
parameter `m` at bytes 504–505 instead of the method declaration. That baseline
used a preserved native binary built from c8e47a99, not generated expectations.

The separate invalid `NoCallable.java` fixture proves that field, parameter,
local and class names, and methods found only in an unrelated class, do not
supply an unqualified callable target. Javac must reject that compilation; the
five native probes must independently report `Unresolved` with zero targets.
These invalid probes are not counted as successful compiler binding matches.
Additional wrong-target controls substitute same-spelled local, parameter and
field declarations for actual method targets, including a nested-class field.
The compiler element mapping and range checks are unchanged.

After the native fix the comparator passes 59 supported declaration bindings
and four explicit unsupported boundaries (63 valid-source probes in total),
plus all three rejected compiler fixtures and the five no-callable refusals.
This original source-only suite does not establish overload selection, binary
method resolution, or general Java invocation legality. Binary method namespace
regressions remain native tests; the separate mixed suite below adds narrow
source-over-binary override evidence.

## Mixed source/class-file signature suite

`mixed-v1` is a separate, fixed-inventory fixture: four handwritten dependency
sources compile into four temporary class files; only the two consumer sources
are registered as source inputs in either analyzer. The optional comparator uses
the same pinned javac with `--release 21`, `-proc:none`, `-implicit:none`, UTF-8,
and empty controlled classpath/sourcepath to generate these dependency bytes.
It never executes fixture programs. Consumer analysis uses only that generated
class directory as its classpath and an empty sourcepath. The native endpoint
reads the class directory as data via `Project::add_class_directory`; it never
starts Java. No generated class files are committed or required by Cargo tests.

`@Override` annotations independently require javac to verify the matching
consumer methods really override inherited declarations. Five probes compare
compiler-selected source overrides against inherited binary
signatures: explicitly imported versus binary-qualified reference types,
primitive parameters, reference arrays, varargs versus arrays, and postfix
multidimensional primitive arrays. Exact compiler-selected source declaration
paths, owners, kinds, names, and UTF-8 ranges are still checked through Trees.
Three distinct signatures (same simple type name in different packages,
`int` versus `long`, and different array dimensions) remain native
`OverloadNotResolved`. Javac chooses the consumer declaration for each, but this
is recorded as a refusal, never native overload-selection success. A type-variable
signature retains `SignatureNotCompared`; no generic substitution is claimed.
All four refusals require exactly the intended source declaration and inherited
binary declaration candidates, including class-file path, owner, name, and the
native zero-range convention. A native run deliberately omitting binary inputs
must change the distinct-package probe to one source candidate and must fail the
comparison. Javac must also fail when its dependency classpath is omitted,
preventing accidental discovery of dependency sources. Wrong-source-target
controls also remain mandatory.

Before signature normalization, the nine consumer queries all compiled and
selected the intended source methods in javac, but the native baseline returned
`SignatureNotCompared` with both source and binary candidates for every query.
The first failing expectation was `binary_scalar_override`, which requires the
unique consumer source override. The baseline endpoint was built before core
changes at commit `10a03e7` and preserved separately during validation.

This adds genuine binary-input evidence, not compiler-selected binary-target
identity evidence: every javac-selected declaration is still a consumer source
method. Binary candidate checks are native provenance checks, not fabricated
Trees source positions for binary declarations. Binary-selected target identity,
method descriptors, inherited bridge methods, generic substitution, unresolved
source types, and broader overload applicability remain covered only by separate
native regressions or remain unsupported. The original nine-file suite and its
59 matched declarations, four refusals, compiler-error and wrong-target controls
remain independently required.

After normalization, the mixed gate passes five matched source-over-binary
bindings and four explicit refusals, separately from the unchanged original
59 matches and four refusals. Both omission controls and the exact refusal
candidate checks pass. These are nine additional valid-source probes, not nine
additional successful semantic matches.
