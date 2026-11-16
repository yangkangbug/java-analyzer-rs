# Interface method inheritance in navigation

Static/private interface methods belong to their declaring interface; they are
not inherited by implementing classes or subinterfaces. Navigation now excludes
these declarations at ancestor steps, including bare, class-qualified,
value-qualified and subinterface-qualified method lookups. Direct owner lookup
still uses its previous access and target rules. See
[JLS 9.4.1](https://docs.oracle.com/javase/specs/jls/se21/html/jls-9.html#jls-9.4.1).

## Declaration facts, not type proofs

The source index records the relevant interface modifier fact independently of
`source_call`. Generic, varargs and annotated static declarations must not become
inheritable just because their signatures are outside strict invocation support.
Annotations are separate syntax, while unknown or contradictory modifiers cannot
authorize exclusions. Binary navigation uses the existing class-file flags and
version, preserving unsupported/contradictory flag combinations conservatively.

Both navigation collection paths use one inheritance predicate. It is applied
only to ancestor interface methods, not fields or the directly queried owner.
Abstract/default instance-method targets, interface constants and class-method
behavior keep their prior rules. There is no public API/schema change.

## Raw inventory stays intact

This predicate is **not** an eligibility certificate for the strict selector.
Raw members, graph completeness, override ancestry and strict same-name inventory
are unchanged. In particular, malformed flags, descriptors and Signatures cannot
be removed to manufacture a successful typed result or return chain. Some legal
calls can therefore still be conservatively refused when the strict inventory
contains unsupported ancestor declarations. Fixing that requires its own effective
member design, not reusing a navigation hint as semantic evidence.

This slice does not implement general access checking, field hiding or a full
Java effective-member model. Interface static fields remain inheritable.

## Validation

Eight native groups cover source/binary static-method exclusion, direct owner
preservation, default/abstract/private controls, generic/varargs/annotations,
contradictory declaration flags, strict-chain refusals and field/class boundaries.
Optional pinned javac checks four legal and five illegal invocation contexts;
real compiled interface class files are also compared before/after for all four
illegal navigation forms and three preserved direct/instance forms. Native tests
do not start Java. Correcting an invalid target is not a coverage-rate increase.
