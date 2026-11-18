# Complete import evidence

An import is now one private record with ordinary/static namespace, named,
on-demand or unknown selection, and complete or malformed target evidence.
Namespace indices store record IDs in source order, including damaged records.
They are access paths over one inventory, not separately parsed truth or
per-name caches.

Admission checks the whole ImportDecl and its Name structure, including the
closing semicolon. It never erases damaged segments into a shorter canonical
target. Unknown means the name or target is incomplete in the namespace already
identified by the syntax; it does not guess which namespace an edit may become.

## Lookup behavior

Legacy type lookup, signature lookup, strict reference lookup, and static
field/method entry points consume this inventory. Complete candidate views
exclude malformed targets, but malformed records remain competing evidence.

A malformed named import blocks unique selection for that name. Missing closing
syntax or an unknown terminal cannot disappear and permit fallback. Malformed
on-demand imports block demand-stage conclusions, not stronger same-package
selection. Locals, fields and member types retain their established precedence;
same-file top-level declarations still participate in existing explicit-import
conflicts. Repeated valid imports retain declaration-identity deduplication.

Refusal uses existing statuses, not a fabricated semantic ambiguity or invented
target. Damaged code that previously navigated through a shortened import now
intentionally refuses. This is not general Java import legality checking or new
static member-type/on-demand navigation support.

## Cost and compatibility

No public API or parser changes. There is one record plus one namespace index
entry per import; duplicated static-name strings are removed.
Ordinary helpers may scan that namespace up to four times. This is an acknowledged
constant-factor scan cost, not a performance improvement.

Strict ordinary queries retain existing record/class/local-type budget units and
do not scan or charge unrelated static imports. Static proofs retain record and
owner/member string charges, including nonmatching records, and account for
malformed syntax when traversed. Exhaustion remains refusal, never absence.
These are existing proof-budget units, not measured CPU work.

## Validation and remaining boundaries

Controls cover malformed and complete ordinary/static imports, named/demand/
unknown forms, mixed valid and invalid competition, lexical precedence, duplicate
imports, budgets, and edit/fresh/undo/old snapshots. The fixed real comparisons
preserve 427 complete result records, 18 anonymous targets and capture refusals;
these are regression evidence, not language-coverage percentages.

Malformed package and heritage facts, declarations lost during parser recovery,
and contextual `var` remain separate work. The paused `var` prototype is not
included in this change. Existing cursor-prefix navigation is not a certificate
that an entire import is complete.
