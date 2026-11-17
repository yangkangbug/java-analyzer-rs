# Unsupported type-pattern binding guards

This is a correctness guard, not flow-sensitive pattern-variable support.

The existing type grammar can recognize an instanceof type followed by a binding
name, including ordinary leading modifiers/annotations, generic types and plain
array types. It is represented by TypePattern and still reports:
'type pattern flow scope is unsupported'.

Previously, a legal pattern variable could be mistaken for an outer same-named
field, local or type. Recognized pattern names now produce private guard-only
records in the existing local-binding collection. A guard is uncertainty, never
proof that a local is absent and never a supported declaration target.

## Boundaries and usability

The guard begins after the pattern name and ends at its syntax execution unit:
method, constructor, field initializer declaration, initializer or enum constant.
It is rebuilt from the current tree at indexing time, not cached on a parser
ancestor. All matching guards are checked before ordinary smallest-scope lookup.
Caller subsets retain covering guards from outside the caller; primitive/reference
proofs cannot use them as type evidence or fall back to a field/type.

Only the affected bare value/receiver name is guarded; the method namespace is
not hidden. Other names, explicit this.field access
and neighboring methods retain ordinary navigation. This does not promise that
strict invocation proofs succeed inside unsupported syntax: their existing
parser-error checks still apply.

This is not exact Java flow scope. A later bare same-named field reference may be
refused even when the pattern variable is no longer in scope. Patterns inside a
lambda can conservatively guard that name until the containing execution unit
ends. No Boolean-flow, abrupt-exit or loop scope is inferred.

Record/deconstruction patterns and type syntax outside the existing parser
subset are not covered. In particular, array-dimension annotations such as
String @A [] value and all qualified-segment annotation forms are not recognized
by this guard. It does not eliminate wrong-binding risks in those unsupported
forms. No general pattern support or Java-conformance claim is
made. Source attachments continue to refuse files with unsupported-pattern reports.

## Compatibility

Version 0.3.0 adds TypePattern to the public exhaustive SyntaxKind enum.
Downstream exhaustive matches must add an explicit case or deliberate fallback.
Visitors must not interpret its binding name as an ordinary reference or as a
proven local variable. No public struct fields or method signatures change.
The package remains publish=false.

## Evidence

A javac-valid same-name field example reproduced the original wrong jump.
Native regression tests cover Boolean/exit/loop/ternary uses, captures, receivers,
static-type fallback, explicit-field and neighboring-method controls, guard-only
proof behavior, deletion/undo and retained prepared versions.

A parse-only javac inventory of 475 checksum-pinned Commons/Gson source files
found no type patterns. The checked JDK String source has five type patterns and
11 later same-name identifier positions. This bounds candidate guard impact in
those inputs; it is not a resolution-success rate or broad coverage measurement.
