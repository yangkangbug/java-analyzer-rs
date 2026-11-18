# Contextual var identifiers

The parser distinguishes ordinary identifier positions from type identifiers and
the existing local-inference marker. The spelling `var` can be an ordinary
variable, parameter, field, method, label, annotation-element or package-segment
name where that grammar role is supported. It cannot become a type declaration
name, type parameter, terminal type name or a type segment after an established
type boundary.

Parser and lookahead share the role tests. Package, type, ordinary import and
static import names have explicit roles. Thus `var.pkg.C` can be a qualified
type through a package prefix, while `C<T>.var` cannot be admitted as a type.
Static import owner and member positions remain distinct.

## Lexing and CST compatibility

Public lexer output is unchanged: the spelling still lexes as `SyntaxKind::Var`.
When consumed in an ordinary identifier position, its CST token is now
`SyntaxKind::Ident` with exactly the original text, trivia and source widths.
Consumers inspecting CST token kinds should use that role distinction rather
than assume every spelling `var` remains a keyword token.

Inference positions retain `Var`: `var var = ...` has a marker followed by a
name; `var.getName()` is an ordinary reference. Untyped lambda names and typed
lambda inference positions are likewise distinct. No public variants, fields
or method signatures change.

This does not add type inference, generic applicability, or complete semantic
validation of inference declarations. Other contextual keywords have separate
rules and are not generally admitted by this change. Existing unsupported
qualified type-annotation grammar remains unsupported; the already supported
catch grammar retains its type-boundary rules.

## Correctness and editing

The [complete import model](IMPORT_EVIDENCE.md) and
[declared-name checks](DECLARED_NAME_EVIDENCE.md) remain unchanged.
Complete Object-graph controls retain refusal for both short and long malformed
static-import prefixes; valid static imports remain navigable. Legal package
names containing `var` retain their actual canonical identities, not phantom
default-package identities.

Local edits between contextual names, ordinary names and qualified prefixes
compare against fresh parsing and navigation, including undo and old snapshots.
The existing damaged declaration that never forms a LocalVar remains a separate
parser-recovery gap; it is not counted as fixed here.

## Observed original-project results

Nine name positions in original Gson and Commons sources now navigate to their
exact variable declarations; six reference uses were independently checked
against javac. This does not claim that following `getName()` or other calls now
have a supported invocation proof.

Gson parser reports fall from 47 to 44 in TypeToken; Commons reports fall from
31 to 2 in TypeUtils. The exact current parser contract preserves all other
diagnostic records and source hashes. Existing 427 complete result records,
18 anonymous targets and capture refusals remain unchanged in the fixed
comparison sets. These counts are not an overall completion percentage.
