# Declaring-aware inherited source invocation

This bounded extension collects all same-name source methods along a complete
plain nongeneric source superclass chain, then applies the existing strict
argument classification and formal-vector specificity. It supports a unique
inherited declaration and different-signature overloads across declaring classes.
It does not prefer the nearest class or assign conversion scores.

Every candidate is validated before arity filtering, including inaccessible,
static, varargs, generic and unknown declarations. Parameters resolve in each
method's declaring file and compilation, not in the receiver class. A selected
method carries its declaring class into ordinary navigation, outer call-result
navigation and inner call-result return-type lookup. Original declaration paths,
ranges and context remain authoritative.

Historical inherited-only boundary (superseded for proven plain source pairs by [source override selection](SOURCE_OVERRIDE_CALLS.md)): same-signature override was not implemented. In the inherited branch, two equal
arity formal vectors are different only when at least one position is provably
different. Same logical reference names in different compilation scopes are
unknown rather than distinct merely because their internal identities differ.
Equal or unknown potential override signatures refuse the whole selection.
All formal metadata is still validated even if another position or arity differs.

New inherited paths reject generic or nonplain superclass edges and binary
superclass selection. Ancillary interfaces, including generic ones, are used
only as negative evidence: the complete transitive graph and every inventory
must be valid, and no reachable interface may declare the queried method name.
Any such name, missing ancestor or incomplete inventory refuses the selection;
no interface method or generic substitution is selected. Supplied binary Object may close the complete graph
but any same-name binary method is an unsupported competitor. Public instance
source methods and fixed arity remain required; package-private declaring owners
may be conservatively refused. Direct-only invocation retains its existing proof
boundary. The shared work and depth budgets remain in force. No global type
identity, new conversion, override substitution, interface-default handling or
runtime dispatch prediction is added.

## Evidence

The frozen predecessor is `39fa8de8`. The additive 35-query Gson plan preserves
29 previous records and adds two expected selections: JsonArrayTest byte 5305
(unique inherited getter) and functional/InheritanceTest byte 2883 (parent no-arg
getter versus child String-argument overload). Both select JsonElement's
getAsJsonObject declaration at 3012..3027. Four explicit deepCopy/toString
same-name override controls remain refusals; no override progress is claimed.

Rust RED covers inherited primitive calls, different arities, declaring imports,
return-context propagation, parent updates and old snapshots. Adversarial tests
cover cross-scope potential override identity, crossed formal vectors, inaccessible
or unknown parent competitors, interface/generic paths and hierarchy budgets.
Earlier inherited-refusal assertions for reference, null, String and construction
arguments are advanced to exact declaration positives only where this new proof
applies. Initial failed regression outputs remain in local evidence. Native
analysis and mandatory tests remain JVM-free.
