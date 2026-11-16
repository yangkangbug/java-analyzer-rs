# Ordinary constructor call contexts

Strict invocation analysis can now use parameters, explicit locals and supported
own fields/helpers inside a top-level nongeneric class's ordinary constructor.
The same receiver, argument, selection and result-composition rules apply as in
an ordinary instance method. No constructor selection algorithm is added.

## Separate caller evidence

The caller certificate records syntax, range and whether the caller is a method
or constructor. A constructor must have a matching enclosing-class name, legal
header, no constructor/class type parameters, and no syntax errors or local-type
boundary. Constructor parameters and locals already had indexed declaration
scopes; this reuses those identities rather than inferring initialization values.

A Constructor does **not** acquire the `source_call` or return-type metadata of
an ordinary method. Current-instance permission is checked separately against
the exact enclosing owner and caller. A constructor cannot become an ordinary
method call or supply a result through `Calls().next()`.

The first slice deliberately refuses a whole constructor containing explicit
`this(...)` or `super(...)` delegation, including qualified/type-argument forms.
It scans the whole body, not merely the statements before the query. This avoids
mistaking delegation arguments or a newer constructor prologue for an initialized
instance context. Ordinary implicit-super bodies are admitted. See
[JLS 21 8.8.7](https://docs.oracle.com/javase/specs/jls/se21/html/jls-8.html#jls-8.8.7)
and [8.8.7.1](https://docs.oracle.com/javase/specs/jls/se21/html/jls-8.html#jls-8.8.7.1).
This is not support for flexible/early-construction bodies in newer Java versions.

## Deliberate limits

Generic constructors/owners, nested/local/anonymous classes, records, initializers
and queries inside lambdas keep their prior boundaries. Other receiver/type
limits still apply. There is no definite-assignment checking, field-initializer
inference, implicit-super accessibility proof, delegation-cycle validation or
whole-body semantic validation. A method target is not a claim that the entire
class compiles or that invoking it at runtime is safe.

The added delegation scan is bounded and its consumed work is deducted from the
same reference-call proof budget. Existing method callers spend no work on this
new scan. This does not claim a total resource bound for the parser or preexisting
caller-location/header checks.

## Validation

Seven native groups cover ordinary parameters/locals/own fields/helpers, access,
shadowing, explicit delegation shapes, malformed names/modifiers, generic/nested
contexts, constructor-versus-method identity and proof budgets. The original
source-field constructor fixture retains its input and now asserts an exact A
method target; its other refusal cases remain unchanged.

Local optional javac evidence independently checks eleven exact native/source
targets, eight compiler-valid deferred contexts and five compiler-invalid controls.
Native tests never invoke Java. The fixed 577-query and 30-query survey records
are unchanged: this is a reusable caller capability, not a measured project-wide
coverage increase.
