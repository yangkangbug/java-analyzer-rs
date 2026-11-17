
# Calls inside array dimensions

The existing caller model also recognizes complete array-creation dimension
expressions, in ordinary methods and the [constructor contexts](CONSTRUCTOR_CALL_CONTEXTS.md) admitted by the core.
A call in `new int[values.size()]` uses the same enclosing callable and strict
selector as `int n = values.size()`. This is a syntactic position rule, not array
value inference, constructor selection or validation of dimension value types.

Only the existing `NewExpr` layout `new Type [expression] ... [] ...` is admitted,
with the queried call inside a nonempty dimension. Every enclosing creation must
qualify. Ordinary constructor arguments, anonymous bodies, array initializers,
lambdas and other existing ownership barriers stay outside this slice. Incomplete
brackets, missing creator types, recovery errors, a sized dimension after an empty
one and mixed dimension/initializer layouts do not grant this caller context.
The scan is bounded by existing caller work; both reference and primitive proof
entry points deduct it, including the platform-independent negative guard.

Native tests exercise overload identity, single/multiple/trailing-empty dimensions,
nested creations, comments/Unicode, methods/static methods/constructors, ownership
barriers, malformed edits, shadowing and shared work exhaustion. Optional javac
comparison confirms eight source declaration targets and rejects four malformed
array controls. One retained Gson Collection call inside an array dimension now
resolves; the other eleven retained sites and the fixed 577/30 diagnostic records
are unchanged. These observations are not full Java or project acceptance.
