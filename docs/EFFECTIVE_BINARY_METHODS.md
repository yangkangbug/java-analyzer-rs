# Effective binary method declarations

This bounded slice extends the existing strict instance-call selector, not the
expression language or a second resolver. It can choose an inherited binary
class declaration, prove an exact binary override, call a public abstract
interface declaration, and choose a class declaration that implements an
abstract interface in its own proven ancestry.

The target is the compile-time declaring method, not the receiver class or an
assumed runtime implementation. For example, a Child inheriting Base.pick has
Base as the target. Default CLI v1 serialization is unchanged; explicit
`--target-details` continues to expose the raw JVM descriptor and linking context.

## Evidence and selection

1. Prove the complete bounded ancestor graph using exact raw JVM edge identity.
2. Retain every same-name declaration. Validate all candidate access, flags,
   generic metadata, descriptors and parameter types before removing anything.
3. Compare parameter identities using the existing selector. An equal-signature
   binary pair can hide its parent only when it has a real ancestor relation,
   the same linking scope, identical complete descriptors and a non-final parent.
   Every pair is checked, including already-hidden intermediate declarations.
4. Apply the existing strict applicability and most-specific rules to the
   effective declarations.

Descriptor equality is a sufficient, deliberately narrow return-compatibility
proof in this common linking scope. There is no covariance or generic erasure matching. Return-value propagation is
a separate [binary invocation result proof](BINARY_CALL_RETURNS.md); a terminal
target alone does not supply a type for a chained call.

Only public abstract interface candidates are supported. A generic ancestor
without any declaration of the queried name remains part of graph inventory,
but is not a method candidate. Thus String's unrelated Comparable<String> edge
does not hide its length/charAt implementation.

## Refusal boundaries

Same-name default, static, private/protected, bridge, synthetic, variable-arity or
generic declarations remain reasons to refuse, even when another candidate
looks applicable. Arrays and unsupported formal types retain the prior boundary.
Missing ancestors, raw-name collisions, conflicting same-owner declarations,
final override conflicts, different return descriptors and exhausted shared work
budgets refuse rather than manufacture a winner.

Unrelated interfaces with identical signatures are not arbitrarily merged.
A class method inherited from Base does not automatically win over a sibling
interface introduced only by Child: the declaration's own proven ancestry must
establish the override relation. Interface receiver calls with an Object
same-name declaration are also deferred; the synthetic class-file Object edge
is not ordinary Java interface inheritance. Mixed source/binary overriding and
cross-scope binary overriding remain unsupported. Existing source-only
override behavior is unchanged.

The proof retains its 4096 work budget and 32 depth bound. The receiver's
already-complete local-declaration scan also collects its fixed caller subset.
Ordinary name hints retain the innermost-scope policy; strict lookup separately
rechecks overlap, declaration visibility and scope on every use. This is
proof-local declaration reuse, not cached type evidence or a persistent cache.

## Frozen checks

- Native tests construct class bytes in Rust, with inherited/overridden targets,
  overloads, final ancestors, interface conflicts, generic nonparticipants,
  raw identities, exact reference returns and diamond declaration identity.
- The optional `effective-binary-v1` javac oracle compiles handwritten binary
  dependencies, then obtains the compiler's declaring element and JVM
  descriptor. It freezes 20 complete queries: 11 supported predictions and
  9 intentional native refusals. Two illegal sources are independently rejected.
  It never executes fixture programs and is not a runtime dependency.
- The independent Commons forecast retains the prior complete records and
  predicts exactly three String charAt/length selections. A separate
  `--capture-effective-binary-baseline` mode verifies the exact old executable;
  current and historical stages cannot accept a union of their expectations.
- The 577-query survey is a deterministic, biased discovery sample, not a
  correctness percentage. Any changed records require separate interpretation.

## Design references

The separation between parameter signatures, overriding and return
substitutability follows the Java language rules:
[JLS 8.4.2](https://docs.oracle.com/javase/specs/jls/se25/html/jls-8.html#jls-8.4.2),
[JLS 8.4.8](https://docs.oracle.com/javase/specs/jls/se25/html/jls-8.html#jls-8.4.8),
[JLS 9.2](https://docs.oracle.com/javase/specs/jls/se25/html/jls-9.html#jls-9.2), and
[JLS 15.12.2.5](https://docs.oracle.com/javase/specs/jls/se25/html/jls-15.html#jls-15.12.2.5).

IntelliJ's [MethodSignatureUtil](https://github.com/JetBrains/intellij-community/blob/840d3be11dfdeada533d05052008b48816b6cc4e/java/java-psi-api/src/com/intellij/psi/util/MethodSignatureUtil.java)
at the pinned reference revision separates signature/subsignature tests from
return-type substitutability. This implementation uses that architectural
separation; it does not port PSI, substitution, general covariance or an IDE
dependency. Passing these bounded gates is not full Java conformance.


### Historical binary-instance acceptance

The frozen binary-instance-v1 contract remains unchanged. Its two linking-context
queries for `Child.inherited(int)` now select `dep/Child/(I)I` rather than refusing
the inherited `Parent.inherited(long)` competitor. This is inherited overload
selection, not exact-descriptor override elimination. The current-stage harness
projects only these two outcomes, checks the pre-existing independent javap call,
and preserves all remaining query and target metadata exactly.
