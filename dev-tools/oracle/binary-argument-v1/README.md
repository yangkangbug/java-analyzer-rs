# Exact binary argument identity across compilations

This is an optional developer oracle, not a Java analysis runtime dependency. It
first verifies the pinned official JDK archive and installed tool bytes. Only
explicit handwritten dependency sources and the trusted BindingOracle helper are
compiled. No fixture program, external build or application is executed.

The native side uses the shipped **java-analyzer CLI**, not the single-root
oracle_bindings example. Every case has separate lib/main and app/main compilation
configurations with a directed source dependency. Three complete native query rows
cover String, an ordinary non-String dep.Token and unsupported cross-scope widening
to Object. The compiler independently identifies the three source Target methods
for the positive Java program. Compiler validity is not claimed to reject every
native conservative identity case.

Seven native configurations exercise:

1. The same selected artifact and complete binary hierarchy in both scopes.
2. Byte-identical Token/Base files at different origins.
3. The same Token origin but byte-identical Base selected at a different origin.
4. Caller source Token shadowing the binary declaration.
5. A missing Base ancestor.
6. One multi-release JAR selected at releases 17 and 21 with different Token entries.
7. Missing Object platform input.

Actual inspect inventories assert both compilation identities, source roots and
selected origins, including exact MR entry paths. Source inputs are hash-pinned
and checked unchanged. Query/candidate metadata, origin, compilation, UTF-8 and
UTF-16 ranges are compared exactly. Baseline mode checks the old refusal records
and demonstrates that they fail the new positive contract; it is not old-or-new
acceptance.

A separate compiler-invalid crossed-specificity fixture uses cross(Token,int)
versus cross(Base,byte), called with Token_app and byte. Javac rejects it as
ambiguous. Native must retain both candidates as OverloadNotResolved, not discard
an unproved cross-scope widening conversion to select the exact-root overload.

```sh
python3 scripts/check-binary-argument-oracle.py \
  --jdk /path/to/verified/jdk --jdk-archive /path/to/pinned/archive \
  --native target/release/java-analyzer
python3 -m unittest discover -s scripts -p 'test_binary_argument_oracle.py'
```

## Explicit coverage limits

These CLI cases do not independently switch archive caches off or inject different
signatures under the same origin into separate in-memory Projects. Those cases and
work/depth-budget exhaustion belong to native Rust regressions. This oracle does
not relabel same-bytes/different-origin tests as same-origin/different-signature
coverage. It proves a bounded flat-classpath modeled identity policy, not raw
class-file equality, JPMS identity, class-loader behavior or general cross-scope
subtyping. Declaration selection is still a source method; binary method selection
has not been added.

## Explicit v2 bounded-proof revision

The original fixture manifest is unchanged. `contract-v2.json` pins the original
forecast script, first real candidate failure, and measured budget diagnosis under
`history/`. Directory-loaded String's two independently parsed modeled signatures
require at least 11,588 member-text units, exceeding the 4,096 proof budget. Its
exact refusal is therefore a budget boundary, not type incompatibility. The old
Resolved forecast is rejected by the current checker, never accepted as an alternative.

The new eighth configuration shares the verified complete java.base.jmod and a
handwritten Token jar between two real compilation scopes. String and Token must
both resolve there. The original small Token directory positive remains, along
with every original origin/ancestor/MR/shadow/missing/cross-specificity negative.
Sharing affects what can be proven within the finite budget; this is not a claim
of cache-independent coverage. Baseline remains strict refusal for all positives.
