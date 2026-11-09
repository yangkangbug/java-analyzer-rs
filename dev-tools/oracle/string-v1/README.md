# Optional bounded string-literal comparator

These original handwritten Apache-2.0 fixtures are independent of the native
implementation. They are not copied from a JDK or upstream project. The standard
native Cargo tests and analysis runtime do not use this comparator or a JVM.

From the repository root, after building the native examples:

```sh
python3 -m unittest discover -s scripts -p 'test_string_oracle.py' -v
python3 scripts/check-string-oracle.py --jdk /path/to/pinned/jdk \
  --native target/debug/examples/oracle_bindings
```

The first command is entirely offline and never starts Java or native programs.
The second is explicitly opt-in. It compiles only the trusted existing
`BindingOracle.java`, with release 21, explicit empty classpath/sourcepath,
`-proc:none` and `-implicit:none`. That helper only parses and attributes the
handwritten fixtures under the same compiler restrictions; it neither generates
nor executes fixture programs. Nothing downloads inputs or runs upstream build
scripts. The pinned JDK is the optional acceptance input, not a dependency of the
library. `--baseline` checks the pre-string-proof refusal expectations, rather
than treating their absence of new resolutions as success for the candidate.

The existing SHA-256 verifier checks `java.base.jmod` before the checker extracts
actual class bytes as data. The explicitly supplied complete String ancestry is
Object, Serializable, Comparable, CharSequence, Constable and ConstantDesc. No
fake String replaces the JDK type. In particular, the actual generic Signature
remains present: exact String identity is supported, but Object widening and
String/Object or String/unrelated overload sets stay refused.

The 19 valid queries have independent fixed native statuses and exact candidate
sets. Nine resolve: ordinary/empty/Unicode strings, audited escapes and octal,
multiple String parameters, one- and two-arity alternatives, and a mixed reference
argument. The source package contains its own `String`; that lexical shadow must
not become the literal's type. Astral Unicode prefixes occur in both queried and
declaration files so compiler UTF-16 positions must convert to UTF-8 byte ranges.
The other ten queries record the deliberately unsupported boundaries: Object
conversion, two overload sets, null, cast, concatenation, field, result, Unicode
escape preprocessing and text block. All compiler-selected source declarations,
owners, method names and exact full declaration ranges are checked independently;
native records must have exact name ranges, statuses, counts and candidates.

Four separate invalid compilations check illegal escapes, wrong arity, inaccessible
methods and ambiguity. Each must emit its specific compiler diagnostic, exit 2,
and emit no successful attribution records. The native result must retain the
specified refusal and candidate identity. Seven independent negative controls
remove String or each of its ancestors in turn; every query must refuse. Offline
mutations cover missing/extra fixture and evidence inventories, duplicate queries,
compiler identity/range fields, native success/refusal changes, wrong targets,
incomplete and duplicate candidate records, and checksum-gate ordering.

This is a bounded handwritten comparison, not a general Java equivalence claim.
