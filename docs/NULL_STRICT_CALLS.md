# Plain null in strict source-instance arguments

This additive slice gives the exact `null` literal its own `StrictActual::Null`
category. It is neither `Object` nor a missing/unknown class identity. Bounded
parentheses and trivia are allowed; casts, conditional expressions, calls,
fields, arrays and arbitrary expression inference gain no new support.

JLS 5.3 permits a value of the null type to convert to any reference type in an
invocation context. This is described here as the null conversion, not the
widening-reference conversion of JLS 5.1.5:
https://docs.oracle.com/javase/specs/jls/se21/html/jls-5.html#jls-5.3

Only already-supported, fully validated scalar reference formals are accepted.
Primitive formals do not accept null. The complete candidate inventory, access,
header, declaration-context type lookup, hierarchy and duplicate-signature
checks run unchanged, including same-name competitors whose arity or other slots
would not apply. Missing platform or type evidence is still a refusal.

Most-specific selection still compares entire formal vectors with the existing
reference/primitive relations. A derived reference formal can dominate its base;
unrelated or crossed vectors remain ambiguous. No conversion scoring, boxing,
unboxing, narrowing, generic inference, array applicability, expanded varargs or
inherited method selection is introduced. Binary class metadata can still prove
a supported reference formal, but binary method selection is not enabled.

Null recognition shares the existing work/depth budget with reference and scalar
primitive classification. Failed proof never becomes null evidence. It is kept
out of general expression and receiver typing. A proven `box.inner(null)` may
naturally feed the already-supported one-layer reference-return receiver proof;
this does not enable null receivers, deeper chains or call-result arguments.

## Evidence and history

`tests/null_strict_calls.rs` records three real RED failures before implementation:
reference specificity, mixed argument vectors and one-layer return composition.
It checks declaration ordering, ambiguity, primitive refusals, unsupported
competitors, unknown/generic/duplicate formals, incomplete platform, bounded
parentheses, exact spelling, inherited refusals and unsupported expression forms.
Prior plain-null refusal tests are explicitly advanced to exact winner checks;
cast-null negative controls remain, and primitive-slot null still fails.

The additive real-Gson plan freezes the `66486e21` predecessor. The **only new
real-source success** is `JsonArrayTest` byte 2527 (`set(0, null)`), selecting
`JsonArray.set` at 4463..4466.

The [original frozen V1 plan](../examples/second_project/null-actual-probes.json)
also predicted `JsonObjectTest` byte 2139 (`add(propertyName, null)`) would select
`JsonObject.add` at 2345..2348. That prediction failed and is not counted as a
success: the ordinary `String` local `propertyName` belongs to the test
compilation, while the formal String belongs to the main compilation. Existing
ordinary reference identity remains scope-local; the separately verified
String-literal identity bridge does not apply to locals. The caller's annotation
and throws clause were unchanged in diagnostic probes and are not the blocker.
Even the original no-null `add(propertyName, value)` at byte 1653 is refused.

The [explicit V2 scope revision](../examples/second_project/null-actual-probes-v2.json)
keeps byte 2139 as an exact deferred `CallNotResolved` record, rather than silently
removing it or accepting either result. The
[pinned diagnosis](../examples/second_project/null-actual-deferred-diagnosis.json)
records exact input hashes and same-length diagnostic replacements: replacing
only `propertyName` by a String literal or null makes the call resolve, without
changing annotations, owner inventory or the null implementation. Those altered
sources are diagnostic evidence, not additional real-source acceptance wins.
The original V1 plan and first failed comparison are preserved. General binary
reference identity across compilation scopes is deferred, not added to this
plain-null slice. The field-argument control at `JsonObjectTest` byte 2489 also
stays refused. Historical goldens and parser contracts remain unchanged.

The independent implementation uses the null-type / TypeConversionUtil behavior
in the existing Apache-2.0 IntelliJ Community reference tree at commit
`840d3be11dfdeada533d05052008b48816b6cc4e` as a design reference, not a port of
its general assignability, PSI or inference implementation. No JVM or IDE runtime
dependency is added. Optional javac comparisons remain separate from native tests.
