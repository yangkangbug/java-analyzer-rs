# Effective binary declarations: frozen compiler oracle

Optional developer-only evidence, separate from the JVM-free production runtime.
The helper is adapted from this repository's BindingOracle scanner; it emits
javac's actual declaring TypeElement and erased JVM descriptor, not a constant
pool invocation owner. It analyzes caller sources against separately compiled
handwritten dependencies. No fixture program, dependency build script, Maven
or Gradle process is executed.

The verified pinned JDK compiles 21 dependency files and the helper. Twenty
queries compare exact binary declarations. Eleven candidate predictions cover
class inheritance, exact class/interface overrides, retained overloads, and
String/CharSequence length and charAt. Nine valid-Java cases remain deliberate
native refusals: covariance/bridge, default methods, generic bridge competitors,
unrelated abstract interfaces, superclass/interface cross-branch implementation,
interface Object members, and static, varargs or private same-name competitors.

The unrelated-interface compiler target is a pinned-javac observation, not a
portable promise that every compiler must choose that particular declaration.
Two separate illegal source fixtures verify compiler rejection of a final
override and incompatible primitive return.

## Frozen evidence

Base: main 5563da7a (local equivalent 11c08a6).
The exact pre-implementation executable hash is recorded in the script and
contract. Predictions were written before compiler execution; javac independently
confirmed them. The full baseline native result and candidate transformation
were then frozen before native implementation began. Negative candidate rows
are exactly the baseline rows; positive rows change only status and one exact
binary target. Paths alone are normalized; identity, descriptor, context,
status, query order and multiplicity remain exact.

Run the optional candidate check with the verified JDK and already-built native
CLI:

    python3 scripts/check-effective-binary-oracle.py --jdk /path/to/pinned-jdk \
      --jdk-archive /path/to/pinned-archive --native target/release/java-analyzer

Use --baseline only with the exact recorded pre-implementation executable.
--capture-baseline is a one-time development operation and refuses to overwrite
the existing contract. Six adversarial comparator mutations and additional
offline unit tests guard omissions, duplicated targets, owner/descriptor/context
changes, status changes, query order and bool-as-integer fields.

The semantic boundary follows the separation of signatures, overriding and
return compatibility in JLS 8.4.2, 8.4.8 and 9.4.1. The native subset is narrower:
descriptor-identical overrides only; no generic substitution or covariant
return proof is claimed.

- https://docs.oracle.com/javase/specs/jls/se25/html/jls-8.html#jls-8.4.8
- https://docs.oracle.com/javase/specs/jls/se25/html/jls-9.html#jls-9.4.1

This small fixture is not Java conformance or project-scale coverage evidence.

