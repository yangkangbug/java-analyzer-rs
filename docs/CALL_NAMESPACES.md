# Invocation names are not variable names

In Java, `m()` names a method even if a local, parameter or field is also named
`m`. This binder now chooses the method namespace for the direct invocation name
before consulting lexical values. Ordinary expressions remain unchanged:

```java
int m(int value) { return value; }
int use(int m) { return m(m); }
```

The callee `m` navigates to the method; the argument `m` navigates to the parameter.
Value receivers such as `box.m(m)` likewise retain their declaration context.
Nested calls, array index arguments, `this` calls and inheritance are covered.

## Supported boundary

The existing method search walks enclosing classes and supertypes. Access checks,
source/class-file provenance, compilation visibility and prepared-handle behavior
are retained. Missing methods do not fall through to fields, locals or classes.
Unsupported static imports, multiple overload candidates and incomparable source/
binary signatures keep their explicit refusal statuses.

This changes neither syntax trees nor call-position propagation. Only the direct
callee is marked as an invocation name. Arguments and receiver prefixes still
use the value namespace, including cases with the same spelling at both positions.

## Evidence

Eight native regressions cover local/parameter/catch/resource/lambda collisions,
value/receiver/index contexts, inherited and binary methods, mixed-signature and
overload refusals, and prepared/compilation-scoped views. The optional
javac oracle independently checks valid declaration identity and rejects same-name
wrong-target substitutions. Separate invalid fixtures require no callable target.

The fixed Commons Lang corpus contains twenty observed wrong variable targets
among 3,030 direct unqualified call names before this fix. All twenty now yield
method candidates or a unique method, never a variable. With platform data,
only three become uniquely resolved; nine remain overload refusals and eight
remain source/binary signature refusals. The exact source-grounded comparison is
under `examples/real_source/`. This directly improves navigation soundness while
identifying signature normalization as the next semantic dependency.

## Not implemented

This is not overload applicability, argument-type checking, generic inference,
static/instance legality, constructor selection or complete Java access semantics.
Method-reference selectors and some receiver-expression forms have separate
pre-existing limitations and are not certified by this invocation fix. The
production analyzer never invokes Java; compiler comparison is development-only.
