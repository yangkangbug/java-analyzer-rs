# Cast-null developer oracle

Optional verified JDK compiler comparison; no fixture program is executed.
Native argument is the `oracle_bindings` example, not the CLI.

Frozen hand-written inputs distinguish a Parent-typed null from plain null and
a Child-typed null. Nested casts, calls, deep parentheses, array/generic types
and unresolved types do not expand this feature. String's generic Comparable
ancestor prevents excluding a nongeneric Number competing overload; native conservatively
retains both exact candidates although javac selects String. Compiler-invalid
primitive-null and missing-type examples are separate from compiler-valid native
refusals. No runtime Java dependency or generalized cast conversion is added.

`--baseline` verifies pre-feature results exactly, including existing plain-null
selection, and requires failure against the new selected target expectations.
