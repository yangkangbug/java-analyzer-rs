# Multiple variable declarations: supported boundary

Java permits several variables in one field or local declaration. The shared
type does not make their identities or postfix array dimensions interchangeable:

```java
Payload first[], second, third[][];
Payload[] fourth[], fifth;
```

Each name has its own source path, owner, kind and exact UTF-8 name range.
`first`, `third`, `fourth` and `fifth` are arrays; `second` is scalar. Prefix
dimensions belong to the shared type, and suffix dimensions belong only to the
preceding name. The parser preserves its existing lossless tree layout, including
comments and initializers. Generic arguments and initializer array accesses do
not supply dimensions to the declared variable.

## Binding behavior

- All sibling fields are indexed, rather than just the first name. The field's
  modifiers, declaring class/import context and compilation ownership are retained.
- All sibling locals were already indexed. They continue to become visible from
  their own declaration through the recorded lexical scope, including later
  initializers in the same declaration. Nested block/for scopes do not leak.
- Each initializer remains an expression subtree. Commas/names in calls or array
  initializers never become additional variable declarations.
- A scalar source-field receiver resolves its declared type where that field was
  written, and member access is checked at the caller. This includes later fields
  inherited from another source file whose imports differ from the caller's.
- A named array remains navigable as a declaration, but member lookup through it
  returns `ReceiverNotBound` instead of pretending it is a scalar element object.
  Indexed-array receiver expressions remain unsupported and may be `Unresolved`.

No public API type changes are required. Legacy, prepared and compilation-scoped
queries retain the same target conventions. Prepared handles still borrow an
immutable index; edited/cloned inputs require new preparation.

## Evidence and limits

Focused native regressions cover exact sibling targets, mixed dimensions,
initializer nesting, imports/access, inheritance/shadowing, Unicode names,
malformed recovery, edits and scope visibility. The independent optional javac
oracle checks valid source cases and rejects intentionally wrong sibling or
wrong-package targets. Compiler execution remains outside the Rust library and
default native test path.

The pinned Commons Lang corpus has six multi-local declarations but no multi-field
declarations in this syntax inventory. Its additional exact-target queries are
regressions for existing behavior, not claimed new functionality on that project.

This slice does not implement expression typing, inferred `var`, array indexing,
array `length`/`clone`, definite assignment or illegal-forward-reference diagnostics.
Recovery tests can intentionally use invalid Java and do not prove its legality.
Postfix type annotations, full Java scope/access rules, stable symbol identities,
method-reference semantics, generic inference and overload selection remain
separate work. Unqualified invocation namespaces are covered by the subsequent
[call-namespace slice](CALL_NAMESPACES.md). No parser-error count or prepared/rebuild agreement establishes
complete Java correctness.
