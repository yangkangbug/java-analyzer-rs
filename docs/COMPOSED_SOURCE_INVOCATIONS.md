# Bounded source invocation values

One strict selector now feeds one invocation-value proof, reused by receiver and
argument consumers. Selected source methods can expose their independently
validated scalar reference or primitive return metadata. Reference results may
be receivers or arguments; primitive results are arguments only. Their declared
type is preserved, never their constant value or runtime implementation type.

Calls may compose in bounded receiver chains and nested arguments. Every branch
uses the same work budget (4096); AST path depth including parentheses, argument
nesting and receiver chains is bounded at 32. Depth counts each recursive proof
step, not merely Java call dots. Sibling argument paths do not add their depths,
but do share remaining work. No budget is reset between inner selection, return
lookup and outer applicability. This is not arbitrary Java expression inference.

Roots remain explicitly typed local/parameter references. Source/public/plain
nongeneric/fixed-arity candidate and complete graph rules remain intact. Return
names use the selected declaring file/import/scope, then caller accessibility.
Void, unsupported annotated return headers, arrays, generic substitution, binary
or static calls, fields/this/super, cast receivers and method references are not
newly accepted. An unsupported call receiver cannot fall back to a same-named
local variable; an unsupported argument call cannot become an unknown wildcard.

Frozen real targets cover eight gaps across four Gson files: JsonArray's
`result.add(element.deepCopy())`, three JsonParserTest chained getAsString calls,
and four JsonArrayTest/JsonObjectTest chained size calls. Exact candidate results
are checked independently against the frozen source-invocation plan. Existing
JsonNull annotated-local deferral is unchanged. Earlier one-layer refusal plans
remain historical evidence rather than being rewritten.
