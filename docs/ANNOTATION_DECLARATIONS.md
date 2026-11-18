# Annotation declaration structure

Annotation declarations use distinct `AnnotationType`, `AnnotationBody`,
`AnnotationElement` and `AnnotationDefault` syntax nodes. Element values reuse
the existing annotation-value parser, including nested annotations in arrays;
ordinary expression arrays retain their separate grammar.

This is syntax and declaration inventory, not annotation semantic validation.
It does not evaluate defaults, enforce legal element types or add annotation
processing, generic inference, or full annotation invocation support.

## Binding and conservative boundaries

Named annotation types retain type targets. Their elements are public method
declarations with unknown invocation shapes, not ordinary executable methods.
Return-type references and element declaration targets remain available. Default
subtrees do not export classes, members or locals, and do not produce navigation
targets. Illegal generic or throws headers recover without fabricating fields.

The declared parent is explicitly `java.lang.annotation.Annotation`, not an
Object-only fallback. This does not grant a strict reference-class certificate.
Missing, duplicate, cyclic and same-name parent inventories are covered by native
controls: absence of proof must not become a unique successful invocation.
Local annotation declarations remain behind the unsupported local-type boundary.

## Edits

`AnnotationBody` is not an independently reparseable interface-body fragment.
Edits use the existing safe enclosing scope or whole-file path. Tests compare
incremental edits and fresh parsing, including incomplete defaults, nested
annotations, declaration changes and recovery.

## 0.4.0 migration

This pre-1.0 source-breaking release appends four variants to the public exhaustive
`SyntaxKind` enum. Downstream exhaustive matches must handle them or deliberately
choose a fallback. Visitors must distinguish annotation declarations and elements
from ordinary interfaces and methods; defaults are value subtrees, not method
bodies. Numeric discriminants are not a serialization or ABI contract.
No public struct fields or method signatures change. The package remains
`publish = false`; this version marks a Git dependency migration, not a crates.io
publication.

## Observed evidence

On the fixed original Gson input, parser reports fall from 79 to 47 across
18 to 11 affected files. The removed 32 reports belong to seven annotation
declaration files; all remaining diagnostic records are unchanged. This is
corpus evidence, not a language-conformance percentage.

On the fixed original Commons input, reports fall from 36 to 31 across
7 to 2 affected files. The five removed reports are in
DiffExclude, EqualsExclude, HashCodeExclude, ToStringExclude, and
ToStringSummary annotation declarations. All other parser records remain
unchanged. The current parser contract applies these exact before/after records
alongside the seven Gson changes; historical baseline contracts remain intact.

Seven Gson and five Commons annotation-type targets, and eight Gson element
declaration targets, already existed and remain unchanged. Two original Excluder
element calls remain refused with identical target arrays, but their reason
changes from `CallNotResolved` to `SignatureNotCompared`: the element inventory is
now explicit and untyped. This is not a newly supported call.

Existing anonymous declaration/field navigation and historical capture refusals
remain unchanged in the recorded comparisons. Captures, framework semantics and
general Java type completeness are outside this increment.
