# Optional handwritten constructor-argument compiler oracle

This Apache-2.0 handwritten fixture is independent development evidence for a
bounded source-construction argument proof. It is not Gson source, a production
Java dependency, Java conformance, or a claim that upstream Gson compiles.

Run after building this repository's native oracle endpoint:

```sh
python3 scripts/check-constructor-oracle.py \
  --jdk /path/to/checksum-verified-temurin-21.0.13+9 \
  --native target/debug/examples/oracle_bindings
python3 -m unittest discover -s scripts -p 'test_constructor_oracle.py' -v
```

Only the optional first command invokes Java. It compiles the trusted
`BindingOracle.java` helper, never fixture programs. Fixture attribution uses
javac Trees with release 21, UTF-8, `-proc:none`, `-implicit:none`, explicit
classpath and empty sourcepath. Native queries run with `PATH=/nonexistent`;
Object class data comes from the independently checksum-checked JMOD.
Mandatory native and Python tests require no JVM. The optional extended workflow
runs this suite separately from the older unchanged compiler suites.

## Independent expected identities

The fixed inventory contains 17 source files, 11 declaration markers and 19
query markers. Six positive queries cover identity, source widening, interface
widening, most-specific overload selection, annotated class/constructor headers,
and a qualified source type. The `expected.tsv` declaration markers are explicit
human expectations, not generated from analyzer results. Compiler source path,
method kind, owner, name and uniquely containing declaration range must agree.
Native declarations must match exact UTF-8 name ranges. An astral character before
the queries checks UTF-16/UTF-8 conversion.

Thirteen compiler-valid refusals cover implicit constructors, additional
constructors, throws clauses, generic classes, generic constructors, varargs-only
constructors, package-private and protected constructors, anonymous classes,
abstract anonymous classes, nested types, nonzero arguments and arrays. These
are deliberate native proof limits, not assertions that the Java is invalid.
Five isolated compiler-invalid cases cover private constructor access, abstract
instantiation, missing zero-argument constructor, duplicate constructors and an
uncaught checked exception. Every invalid fixture must produce compiler errors,
no compiler binding output, and the exact two native refusal candidates.

Accepted construction requires a complete inventory with exactly one explicit,
public, nongeneric, fixed-zero-argument constructor, matching class name, valid
header and real body, and no throws clause, on an accessible ordinary concrete
nongeneric top-level source class. This proof does not validate constructor
bodies, implicit superclass constructor calls, checked-exception flow or whole
compilation units. Missing Object inventory must prevent positive selections.
Parser recovery, malformed constructor/header and work-budget boundaries are
native regressions rather than claims about compiler-valid Java fixtures.

Offline checker controls reject wrong same-named overloads, compiler identity or
range mutations, missing/extra files/results, incomplete counts and false success
on every valid unsupported fixture. The shared strict reference comparator also
has its own exact-candidate mutation tests; its original fixtures and goldens
are preserved, including its implicit-constructor refusal.

## Frozen baseline and genuine Gson limitation

`--baseline` checks the exact historical refusal candidates for all 19 valid
queries, including all six newly selected calls. It is not candidate acceptance.
Baseline outputs, fixture hashes and preserved native executables were captured
before candidate validation in the separate local validation artifacts.

The unchanged pinned Gson corpus was attempted separately with 79 original main
sources, original JsonArrayTest and MoreAsserts, and already-pinned JUnit and
Hamcrest. The existing compiler oracle returned exit 2 with eight missing
GsonBuildConfig diagnostics and no binding output. No generated configuration,
source rewriting, dependency download or upstream build script was used.
Consequently the Gson `a.add(new JsonObject())` correction is an exact original
native-query/source-preservation acceptance gate, not a successful javac
comparison of upstream Gson. Its original 16-query goldens remain immutable;
the reviewed correction is maintained separately in the real-project gate.
