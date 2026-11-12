# Plain source override selection

Public instance fixed-arity methods on a complete nongeneric source superclass
chain can now select a proven overriding declaration. Every equal-signature pair
is checked before hiding any ancestor, including intermediate final or incompatible
return declarations. Same-owner duplicates and unproved cross-scope formal identity
remain refusals. Abstract declarations are valid compile-time targets.

Override return evidence is independent of general expression/return propagation:
void and primitive results must match exactly; references must be proven identical
or follow a plain nongeneric source class superclass path with complete graphs.
No interface or generic covariance, binary override, boxing or throws/native method
support is added. All other candidates still receive complete validation.

Declaration annotations are accepted only in structural marker or empty forms,
including qualified names. This does not validate annotation types or annotation
target semantics, and the name `Override` itself proves nothing. Any annotation
arguments, including malformed trailing commas, refuse this proof. The parser and
general annotated-method return propagation are unchanged.

## Real project evidence and deferred target

The original frozen plan predicted four Gson deepCopy selections. Actual candidate
validation proves three: JsonArrayTest 3001, JsonObjectTest 6712 and
JsonPrimitiveTest 10817, each selecting its exact source subclass declaration.
JsonNullTest 1261 remains the exact two-target Ambiguous result: its receiver local
has `@SuppressWarnings("deprecation")`, and existing local reference typing rejects
annotated declarations before override selection. This slice does not broaden
local annotation typing to recover that separate limitation.

The original [36-query forecast](../examples/second_project/source-override-probes.json)
and first candidate failure are retained. The explicit
[V2 contract](../examples/second_project/source-override-probes-v2.json) records
three promotions and the deferred JsonNull result; the
[diagnosis](../examples/second_project/source-override-deferred-diagnosis.json)
preserves its cause. Binary Object/toString also remains refused. Neither deferred
case counts as a new successful navigation.
