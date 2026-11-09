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

The original source-only suite is narrow: it gives no overload selection proof,
generic inference, JPMS, platform binding, full access-control, malformed-buffer
semantic equivalence, or Java conformance claim. The separate strict primitive
suite below adds only its explicitly bounded source overload evidence. Review golden changes against Java semantics independently;
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

## Strict primitive source-call suite

`primitive-v1` is a separate fixed three-file handwritten suite. The existing
source-only 59 matches/four refusals and mixed five matches/four refusals remain
independently required, with their original comparison rules unchanged.
The Java helper is unchanged: every compiler-selected target is a real source
method, identified by `Trees.getElement`, `Trees.getPath`, and a unique name marker
inside its actual declaration-tree range. An astral character before queries
continues to distinguish UTF-16 compiler positions from UTF-8 byte offsets.

The new gate requires twelve source bindings: ten newly selectable overload
queries, plus a correctly typed singleton and a zero-argument singleton. It checks
primitive identity, widening specificity, differing arity with known argument
types, and primitive-array elements indexed by declared `int` names. A nested
inner call can resolve while the outer call refuses because return-expression
inference is not implemented. No positive probe depends on literal typing; `pick(array[0], value)` explicitly
refuses while named `int` indices can support selection. A separate
`chain(f).other(d)` probe requires the inner `chain(float)` target rather than
`chain(double)`: a surrounding call must not replace the inner argument list.
Both overloads return the same source class, and the outer instance call remains
`Unresolved` with no target, matching its pre-slice unsupported boundary.
Return types do not choose the overload. An enhanced-for scope probe has a
static double field and a same-name float loop variable: the loop variable is not
in scope in its own iterable. Javac selects the double overload there; native
lookup must refuse the unsupported field argument, not select the float overload.
Twenty valid Java queries require explicit native refusals, including generic
competitors, boxing/unboxing, null/reference selection, variable-arity invocation,
unknown argument expressions, receivers, instance methods and inheritance.
Four singleton unknown-applicability cases use `CallNotResolved` with their
original candidate retained. This is neutral: the compiler accepts these calls,
and the native refusal is not a Java-invalidity diagnostic.

The target sets in `expected.tsv` are hand-authored declaration markers plus
owners. Every field of every native candidate is checked, including duplicates
and exact source name ranges. Refusals are not accepted merely for having two
same-name candidates. Wrong same-name overloads, wrong owners/ranges, corrupted
compiler ranges, missing/extra queries, duplicate targets and deleted refusal
candidates must fail the checker. These negative mutations test the comparison,
not additional successful native bindings.

The suite supplies only the real `java/lang/Object.class` platform declaration,
extracted as data from the pinned JDK's `jmods/java.base.jmod` after checking the
JMOD checksum against `scripts/acceptance-inputs.json`. It never executes a JMOD
tool or embeds platform bytes in the repository. This explicit input closes the
ordinary source class's superclass inventory; missing parents must not silently
be treated as empty. Omitting Object must prevent the ten newly supported
selections and fail the comparison. The default native Rust regressions construct
their own signature bytes and still need neither a JDK nor Java execution.

Sixteen invalid-source fixtures are analyzed separately, one at a time. Each
must produce javac errors and no successful binding output, while native lookup
preserves the exact refused candidates. They cover an arity-compatible but
wrongly typed argument, primitive narrowing, char-to-short conversion,
primitive-array invariance, long/float indices, crossed most-specific candidates,
instance calls from static contexts, missing names/parents, and malformed argument
lists. Three singleton regressions forbid `Resolved` for `m(1)` when only `m()`
exists, for boolean passed to int, or for double passed to float, both with
and without Object input. The native
`CallNotResolved` result preserves the candidate without claiming compiler-like
diagnostics. These sixteen cases are not counted as compiler binding matches.

Before implementation, a native endpoint built from commit `1dfa0576` failed the original
nine new overload selections with `OverloadNotResolved`. The three invalid
singletons instead incorrectly reported `Resolved`. The integrated gate failed
with `single_primitive_mismatch: native status Resolved, expected CallNotResolved`,
after the unchanged original and mixed gates had passed. Independent baseline
probes also verified valid singleton boxing and varargs calls, preventing a fix
that labels all unsupported conversions as invalid Java.

### Optional four real source selections

Add the existing checksum-pinned Commons Lang source archive to the same optional
development comparator:

```
python3 scripts/check-semantic-oracle.py --jdk /path/to/verified/jdk \
  --sources-jar /path/to/commons-lang3-3.17.0-sources.jar
```

This optional branch verifies the archive hash and exact 249-file/3,676,819-byte
Java inventory, extracts the unmodified sources only into a temporary directory,
and asks javac to parse/analyze all files with annotation processing and implicit
compilation disabled. It never generates or executes upstream application
classes. No source archive or upstream implementation is redistributed.
The existing [source provenance and licenses](../../examples/real_source/README.md)
continue to apply.

`primitive-v1/real-expected.tsv` independently pins four actual use sites in
`IEEE754rUtils.java`: the double/float calls to `max` at bytes 1820/3435 and `min`
at 5044/6661. Javac selects source name ranges 2155..2158, 3768..3771,
5380..5383 and 6995..6998 respectively. The gate checks compiler kind, owner,
selected declaration tree, primitive method header, exact name range and every
native target field. A wrong same-name sibling and omitted Object input must
fail. All four were six-candidate `OverloadNotResolved` results on the preserved
baseline. Four successful source selections are not full-project equivalence,
reference/generic/boxing/varargs support, or binary overload identity evidence.

Binary methods with identical owner/name currently share a public zero range;
this suite does not claim to distinguish those overloads. Production and ordinary
Cargo tests remain JVM/JNI/JDT-LS-free. Only this opt-in comparator executes Java.
