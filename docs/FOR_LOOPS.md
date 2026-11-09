# Basic-for expression lists and edit safety

Basic-for initialization and update positions now accept comma-separated
statement expressions, following the shape of
[JLS §14.14.1](https://docs.oracle.com/javase/specs/jls/se21/html/jls-14.html#jls-14.14.1).
Local-variable declaration initializers and the existing enhanced-for path remain
separate. Nested commas in calls, assignments containing arrays/lambdas, and class
creation belong to those expressions, not to the outer list.

List items are checked by syntax-root form: assignment, pre/post increment or
decrement, invocation, or class-instance creation. Array creation alone, a bare
name/literal, arbitrary binary/conditional expression, a lambda/method reference,
or a parenthesized statement expression does not become a list item. Empty and
trailing items produce diagnostics. The condition remains one expression, and
comma is not introduced as an operator elsewhere in Java.

These checks do not establish assignment-target legality, invocation correctness,
boolean condition type, inference, definite assignment, or control flow. They do
not close every basic/enhanced-for declaration restriction. Incomplete input keeps
its tokens and structural delimiters; recovery is tested on specific cases, not
promised to reconstruct the author's intent for arbitrary malformed text.

## Binding evidence

`tests/for_scope_oracle.rs` has six hand-authored complete expected targets. Each
expected declaration position comes from the fixture text and lexical scope
rules, not from either resolver. Both prepared and rebuild queries must match
those values separately. The cases distinguish loop locals from same-name fields,
restore field binding after the loop, and reuse enclosing locals when the for
initializer is an expression list rather than a declaration.

Two additional pinned Commons Lang positions independently checked against the
unmodified source now bind to the declaration of `j` at bytes 1234..1235 in
CharSequenceUtils: byte 1261 in the update and byte 1376 in the body. Both were
Unresolved in the retained baseline. This is a small lexical-binding oracle,
not a compiler differential or proof of complete Java semantics.

## Incremental text-loss guard

Adversarial edits exposed an inherited defect: a balanced fragment could parse
only part of its tokens and still be spliced into the syntax tree. This could
drop source coverage after editing nested array/lambda expressions. Reparse now
requires the replacement's UTF-8 length to equal the complete candidate slice
before splicing. Otherwise it tries the existing larger-node/full-parse fallback.

Focused red tests demonstrated actual text loss both in a for header and in an
ordinary local array initializer. Independent review then checked 25 grammar
cases, 4,212 insertion edits, 134 deletion edits and UTF-16 emoji edits: ordinary
incremental parsing and verified parsing matched a full parse and retained text
in every exercised case. This is bounded evidence, not a universal incremental
correctness proof or an incremental performance promise.

## Complete published-source comparison

The unchanged Commons Lang 3.17.0 archive still contains 249 Java files and
3,676,819 source bytes. Diagnostics change from 149 to 85 across 28 to 24 files:

- All 70 diagnostics attributed to expression-list recovery disappear from
  ArrayUtils, CharSequenceUtils, StrBuilder and StrMatcher.
- The already unsupported contextual identifier `var` in TypeUtils changes from
  23 to 29 diagnostics because missing-item recovery retains malformed tokens
  instead of consuming them early. That file's total increases from 26 to 32.
  This is reported explicitly, not hidden by an aggregate improvement.
- Switch (27), synchronized statements (10), explicit invocation type arguments
  (10), annotation declarations (5), and labels (4) are unchanged.

The grouping was reviewed against source constructs and retains every diagnostic
range/message in `examples/real_source/for-diagnostic-causes.tsv`. The complete
249-file before/after counts are in `for-parser-comparison.tsv`. Four files improve,
one increases, and 244 are unchanged. All files remain lossless. Diagnostic counts
include recovery cascades and are not feature counts or conformance scores.
