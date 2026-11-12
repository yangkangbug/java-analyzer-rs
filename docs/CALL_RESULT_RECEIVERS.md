# One-layer source call-result receivers

This bounded extension composes two existing strict source-instance selections.
For `local.inner(arguments).outer(arguments)`, the inner receiver must be an
explicit local or parameter with existing declaration evidence. Both selections
share one proof budget. Parentheses are bounded; no general expression inference
is added.

A selected source method may expose an independently validated, nongeneric,
scalar reference return spelling. The full method header is checked, including
trailing tokens and throws placement. Return names are resolved in the declaring
file, compilation scope and imports, not the caller's same-spelled types. Caller
accessibility and complete hierarchy evidence remain required. This does not
reuse lossy field/local declared-type metadata or inspect method bodies.

The outer call uses the declared compile-time return class, never a guessed
runtime subclass. An abstract return class is permitted when its directly
declared method satisfies the existing strict rules. Inherited same-name
competitors, binary/interface selection, static methods, generics/substitution,
varargs, boxing, arrays, primitive/void returns and unknown metadata remain
unsupported. No call-result arguments, casts, conditional inference, assignment
inference, arbitrary-depth chains or method-reference typing are enabled.

When any call-result receiver cannot be proved, resolution terminates with
`ReceiverNotBound`; it must not discard the receiver and bind the selector to a
same-named local or field. The guard follows receiver/result origins, not call
arguments, conditional conditions, index operands or plain assignment targets. Compound assignments inspect both
value origins; this remains a refusal guard, not expression typing.

## Evidence

`tests/call_result_receivers.rs` starts with two recorded failures: a valid outer
call was unresolved, while an unproved inner call incorrectly fell back to a
same-named parameter. It covers exact declaration ranges, abstract declared
return vs runtime override, declaration imports, caller accessibility, malformed
and unsupported returns, unknown competitors, scope, depth/work limits, throws,
method references and ordinary argument isolation. Existing primitive/mixed
Rust regressions explicitly advance their historical outer-refusal assertions
to exact declaration targets rather than deleting the checks.

Real-source acceptance is additive to the frozen mixed-strict stage. In pinned
Gson, `array.remove(1).getAsString()` at byte 1986 and
`array.get(0).getAsString()` at byte 2489 must select the direct
`JsonElement.getAsString` declaration at 6434..6445, not a subclass override.
Historical goldens remain immutable; versioned plans/checkers distinguish the
baseline from current proofs. Parser contracts remain independent.

## Design reference

JLS 15.12.2.6 defines method invocation type separately from choosing a method:
https://docs.oracle.com/javase/specs/jls/se21/html/jls-15.html#jls-15.12.2.6

The design reference is `PsiMethodCallExpressionImpl` at the pinned IntelliJ
community commit `840d3be11dfdeada533d05052008b48816b6cc4e`:
https://github.com/JetBrains/intellij-community/blob/840d3be11dfdeada533d05052008b48816b6cc4e/java/java-psi-impl/src/com/intellij/psi/impl/source/tree/java/PsiMethodCallExpressionImpl.java

Its file-level Apache-2.0 license governs that source. This is an independent,
restricted Rust implementation, not a port of PSI, substitutors or Java inference.
No IDE dependency or JVM is added to analysis or mandatory tests.
