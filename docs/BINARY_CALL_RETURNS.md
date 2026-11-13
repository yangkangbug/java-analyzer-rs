# Proven binary invocation results

This slice composes existing strict binary invocation proofs with the existing
qualified expression proof. A selected binary method may supply its declared
primitive scalar or plain nongeneric reference result to an outer receiver or
argument. It does not infer values from method bodies or execute Java.

## Architecture and proof boundary

The inner invocation must first pass the same complete candidate inventory,
argument applicability and unique most-specific selection used for navigation.
A legacy `Resolved` name result is not type evidence. Return types never discard
an overload or decide which method wins. Source invocation result typing retains
its existing path; no parallel resolver, public API or CLI schema is introduced.

The complete JVMS descriptor parser retains the return shape instead of discarding
it. It validates the entire descriptor, including trailing bytes and names that
may legally contain `)`. All eight primitive scalar tags can supply existing
strict arguments. `void` and arrays cannot. A reference result is resolved in the
selected declaration's linking context, not the consuming caller's similarly
named scope. Exact raw JVM identity, visibility, nongeneric declaration evidence
and the complete bounded ancestor graph must all be established.

Ordinary explicit local/parameter-rooted qualified chains may compose multiple
proved calls. Fields, constructors, implicit calls, arithmetic, static calls,
generic substitution, nested/dollar return types, array expressions and default
interface selection gain no new support. An otherwise navigable inner method
can still have an unsupported result, in which case it supplies no outer type.
Shared 4096-work and 32-depth limits remain unchanged, including nested calls.

## Evidence

The preimplementation executable is frozen separately from candidate results.
The optional `binary-return-v1` compiler oracle compares complete binary declaring
owners and JVM descriptors, positive predictions, conservative refusals and
invalid-source controls. Its programs are never executed; javac is test-only.
The oracle freezes 20 queries: 14 supported predictions, six conservative
refusals and two separately compiler-rejected source controls. The fixed Commons
gate covers five new selections, two retained refusals and one preserved inner
target, checking complete default/detailed records and all 249 source hashes. These bounded cases are not a whole-project correctness percentage or
a performance comparison with another Java engine.

## Design reference

At IntelliJ reference commit `840d3be11dfdeada533d05052008b48816b6cc4e`,
[PsiMethodCallExpressionImpl.TypeEvaluator](https://github.com/JetBrains/intellij-community/blob/840d3be11dfdeada533d05052008b48816b6cc4e/java/java-psi-impl/src/com/intellij/psi/impl/source/tree/java/PsiMethodCallExpressionImpl.java)
obtains resolved method candidates before deriving declared return types with
their substitution context. This implementation borrows that separation only:
it requires a uniquely proved candidate and does not port PSI, generic capture,
least-upper-bound inference, getClass special typing or result caches.
See also [JLS 15.12.2.6](https://docs.oracle.com/javase/specs/jls/se25/html/jls-15.html#jls-15.12.2.6).

The frozen older binary-instance contract remains byte-for-byte intact. A separate
current-stage projection upgrades only queries 24 and 49, one per linking
context, for the already javap-selected `Methods.token(Token).id()` chain. It
checks both old compiler calls and the exact original query before predicting
`dep/Token.id:()I`; every other complete record remains unchanged.
