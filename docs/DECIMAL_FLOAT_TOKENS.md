# Decimal floating-point token boundaries

Decimal floating literals may end their fractional part immediately after the
dot. The lexer now recognizes `1.`, `1.f`, `1.d` and `1.e+2` through the
same decimal scanning path as existing fractional and exponent forms.

Longest matching remains lexical, not a guess about intended member access:
`1..x` begins with the double token `1.`; `1.foo` consumes the legal float
suffix in `1.f` before the remaining text. Adjacent dots, comments, operators
and suffixes retain their exact source ranges and lossless text.

## Separator validity and scope

A shared decimal digit-span rule checks that separators occur between digits,
including multiple internal underscores. An empty fractional span is allowed.
Malformed decimal floating candidates such as `1._2`, `1_.`, `1.e_2` and
`1.2_` produce ErrorToken rather than silently becoming valid floating literals.
This intentionally tightens some previously accepted damaged floating tokens.

Integer, hexadecimal and binary branches are unchanged. Exponents with no digits
retain the existing rollback behavior. This is not complete numeric legality,
overflow/range evaluation or hexadecimal-floating support.

No public enum or API shape changes. Token classification changes where the old
lexer split valid trailing-dot literals, and for the damaged decimal separator
cases described above. The strict invocation paths still do not prove general
float/double applicability: a correct floating token must not be mistaken for
an int argument.

## Real-source and edit evidence

The original Commons JavaVersion condition `v - 1. < 1.` now has the correct
arithmetic/comparison structure with two double literals rather than field
selections. Its two parser reports disappear; the fixed Commons input now has
zero reports, while Gson remains at 44. This is an exact corpus observation,
not full Java syntax support.

Fourteen original variable targets remain unchanged, including eleven reference
uses independently checked with javac. Existing 427 result records, 18 anonymous
targets and capture refusals are preserved. Native token/range tests and local
edits compare against fresh parsing, undo and old snapshots, including Unicode
prefixes.

## Known remaining boundary

The existing binder may still misinterpret `1..x` and borrow a same-named field
after losing the literal receiver. Baseline and candidate reproductions are
retained; this lexical change does not fix that binding issue. The related
`1.x` case now refuses rather than inheriting the previous incorrect split.
Do not treat correct tokenization as a guarantee of complete malformed-expression
navigation.
