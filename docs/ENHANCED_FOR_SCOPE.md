# Enhanced-for variable scope

An enhanced-for header variable is visible in its loop body, not in the iterable
expression. For example, in `for (C C : C.values)`, the iterable's `C` may
denote an imported class, while the body uses the loop variable.

The index derives that boundary from direct `ForStmt` children, so parentheses
in calls and colons in conditional expressions cannot be mistaken for header
delimiters. Only the exact header declaration receives the body-start visibility
boundary. Its declaration identity and enclosing scope interval remain unchanged.
If the header/body boundary is incomplete, use visibility is withheld while
declaration-name navigation remains available. Basic-for variables, catches,
resources and unrelated body declarations retain their existing rules.

This is a private indexing change: no public API, syntax kind, version or parser
contract change. It does not broaden strict invocation proofs. The existing
conservative enhanced-header `direct_caller` guard remains. The separate
final-prefixed damaged-declaration recovery experiment is **not included**.

## Evidence and limits

Six legal compiler comparisons verify imported-type and outer-field lookup in
the iterable, the loop declaration in the body, and outer lookup after the loop.
Native tests additionally cover incomplete boundaries, nested/unbraced bodies,
lambda/anonymous ownership and incremental edits with fresh/old snapshots.

The fixed 427 and 848 navigation sets, 18 anonymous targets and historical capture
refusals are unchanged. Gson retains 30 parser reports and Commons 0.
A targeted CST census of the original 460 files found 180 enhanced-for headers
but no affected same-name variable reference in their iterable expressions.
Its only same-spelling token was a method selector and remained unchanged.
The demonstrated benefit is legal-source correctness in independent compiler
fixtures, not an additional navigation target in those two original projects.

