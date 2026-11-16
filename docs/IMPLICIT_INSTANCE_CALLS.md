# Own-class implicit instance calls

A bare `helper(value)` in an ordinary nonstatic method can now use the same
strict argument/conversion/most-specific selector as a qualified invocation.
A strictly selected helper also supplies its declared result to nested arguments
and receiver chains. This closes an entry-point gap, not a second type system.

## Evidence and boundaries

The caller must be in a top-level nongeneric source class, with the existing
ordinary-method syntax/local-scope proof. The exact current class owns the whole
same-name method family, and the complete ancestor graph must have no method of
that name. All candidates must be supported fixed-arity nongeneric instance
methods; unknown competitors are never silently removed. Declaration-context
lookup, exact compilation identity, duplicate signatures, strict applicability,
specificity and the shared work/depth budget remain unchanged.

Private, protected and package methods are accessible here because caller and
selected declaration are proved to be the same class. This does not relax access
for explicit receivers or other classes. Method names use the method namespace:
a same-spelled parameter, local or field does not capture `helper(...)`.
Instance descriptors retain the 254-parameter-slot bound, including competitors
of a different arity. Slot counts use two slots for scalar long/double.

Static/mixed families, same-name inherited methods, imported methods, generic
callers, constructors, nested/local/anonymous classes and lambdas are outside
this new path. Existing supported invocation arguments/returns are reused;
there is no new boxing, generic substitution, field-value inference, array typing
or control-flow analysis. Qualified `this`/`super` are not introduced.

Direct unsupported bare calls retain existing name-navigation fallback, including
some legacy singleton `Resolved` results. Such hints are never return-type
evidence. Nested calls succeed only through strict selection. The older static
primitive-call proof remains for its existing supported subset.

[JLS 6.5.7.1](https://docs.oracle.com/javase/specs/jls/se25/html/jls-6.html#jls-6.5.7.1)
and [15.12.1–15.12.3](https://docs.oracle.com/javase/specs/jls/se25/html/jls-15.html#jls-15.12.1)
separate the method-name search, applicability and invocation-context checks.
Finding a same-class family does not permit falling through to an import when
its candidates are inapplicable.

## Validation

Native regressions cover access levels, same-name values, reference/primitive
overloads, nested returns, unknown/mixed/generic/varargs competitors, inherited
names, ambiguous null, caller context, duplicate signatures and shared budgets.
Five historical negative inputs retain their original source but now assert
exact justified targets: make().next(), mixed pick(give(), a), and the
primitive/null-suite pick(give()) and reference-suite m(give()) cases. The null-suite helper
has a declared reference return; this is not null-value inference.

A development-only javac comparison checks six exact source targets, including
private reference-return composition, and three invalid compiler controls.
The runtime and native tests remain JVM-free. No new parser stage or acceptance
framework is required.

On the unchanged pinned Gson sources, all 12 `getAsSingleElement().getter()`
expressions in JsonArray now navigate from the private helper's declared
JsonElement return to the exact JsonElement getter. These are original source
expressions, not a project-wide coverage percentage; Gson itself was not built.
The existing 577 fixed and 30 parameterized-receiver query records remain exact.
The optional local evidence is retained separately from the repository tests.
