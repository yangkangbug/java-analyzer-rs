# Complete declared-name evidence

Source declaration type facts must not turn a damaged name into a different,
shorter type. A field or parameter written as `p.C.var`, `p..C`, or with
syntactically damaged type arguments must not supply receiver type `p.C`
and then navigate to `p.C.x`.

A shared private CST check now guards the two existing declaration-fact producers,
`read_type` and `variable_declarators`. It checks contiguous name segments and
syntax errors in that name's own type-argument and annotation subtrees.
Complete annotations before a name segment remain transparent, but do not
complete the segment. It does not scan unrelated file errors or resolve generic
arguments.

## Behavior and compatibility

Already recognized declarations retain their names and shadowing. Only their
unreliable declared receiver-type fact is withheld. Existing valid name-level
behavior, including `C<Missing>`, is not upgraded to a proof that generic
arguments are valid. Existing qualified catch annotations remain supported.
Primitive and binary type paths are unchanged.

The producers cover field declarations, parameters, locals/resources/for
variables, catches, record components and source return metadata. Named and
anonymous field extraction share the declaration helper, but anonymous strict
typing is not added; damaged anonymous bodies retain their existing containment.

Malformed-declaration member queries that previously returned a wrong unique
target intentionally become refusals. No public enum, struct or method signature
changes. Cursor-prefix navigation remains separate: clicking the complete
`C` prefix in `p.C.` retains its existing behavior.

## Explicit remaining gaps

This is not a universal malformed-name fix. Package and heritage inventories
still use their existing paths. Malformed imports are handled by the separate
[complete import evidence](IMPORT_EVIDENCE.md) model, which retains unknown
competition rather than turning a failed import into absence.

Likewise, `p.C<,> C=null` may be recovered as expressions rather than a LocalVar.
There is then no declaration record for this check to preserve, and later
`C.x` can still fall back to an imported class. The original failing case and
baseline CST are retained as a known parser-recovery gap, not counted as fixed.

## Validation boundary

Focused controls cover malformed declarations, same-name shadowing, valid
annotation/generic syntax, and local edit/fresh/undo/old-snapshot behavior.
The fixed real-source comparison preserves 427 existing result records,
18 anonymous declaration targets and historical capture refusals. These
regression sets are not a language-coverage percentage.
