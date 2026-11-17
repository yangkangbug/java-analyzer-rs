# Explicit invocation type arguments

The parser now retains qualified invocation type arguments such as
`api.<String>method()` and `Collections.<String>emptyList()` instead of treating
the angle brackets as an unrelated expression. This is syntax support, **not
generic method inference or applicability verification**.

The existing `CallExpr` contains a `FieldExpr` callee. That callee now may contain
receiver, dot, `TypeArgList`, and method-name token. Existing public enum variants
and API signatures are unchanged, but downstream CST consumers must handle this
additional child rather than assuming every field expression has only a receiver
and selector.

## Parsing and recovery

The existing type parser and type-argument lookahead are reused. A role-specific
structural check follows only type grammar edges, not expressions inside type-use
annotation values. Invocation lists must be nonempty and cannot have top-level
wildcards or trailing commas. Nested generic lists may have wildcards, but cannot
use diamond, bare primitive or void arguments. Primitive arrays are reference
types and are accepted. These checks do not validate referenced type declarations,
generic arity or type-variable bounds.

This remains a syntax subset: type-use annotations on array dimensions and other
previously unsupported type forms may still diagnose. Constructor type arguments
and generic method-reference semantics are not added. Incomplete lists use the
existing recovery path. A completed typed selector without invocation parentheses
is diagnosed, without intentionally consuming the following declaration.

## Binding boundary

Navigation does not erase explicit type arguments to manufacture a method target.
The member-chain collector propagates an unavailable result through all outer
suffixes, so malformed `obj.<T>method.field` cannot turn into `obj.field`.
Existing strict call-shape checks also refuse these expressions as argument or
return-type evidence. Their receiver and type-name subnodes can still be queried
independently. Ordinary supported neighboring calls are not disabled merely
because the file contains this valid syntax.

Legacy single-name navigation elsewhere remains separate from strict invocation
proof; a unique outer method name is not evidence that an explicit generic
argument expression was typed successfully.

## Evidence and limits

Tests cover positive and negative grammar, nested closers, annotation-expression
isolation, malformed suffixes, fresh/incremental parity, and supported source and
binary calls contrasted with their explicit-type-argument versions. Optional
javac controls check 13 legal and 12 illegal grammar cases without executing
fixture programs.

On unchanged pinned sources, this removes erroneous parser recovery around five
Commons invocations and 28 Gson invocations across 12 Gson files. The frozen
current parser contract retains exact before/after records and source identity;
historical contracts are unchanged. The Commons unqualified-call audit explicitly
removes only the five formerly misclassified typed invocations from its frozen
3077 records; the other 3072 records are unchanged. The pre-existing 45 Commons navigation
records remain identical. In a selected diagnostic set of 393 Gson `fail`
queries, 81 ordinary calls newly use the existing static-import proof; the other
312 records are identical. This is scoped differential evidence, not overall
project coverage or support for the explicit generic calls themselves.
