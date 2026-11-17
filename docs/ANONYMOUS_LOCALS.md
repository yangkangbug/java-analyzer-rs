# Anonymous-method local declaration navigation

An anonymous class created with `new Type(...) { ... }` has a private lexical
owner in the source index. This is a snapshot-local source identity, not a
synthetic Java name or an exported class. Anonymous members never enter the
canonical type directory or the enclosing named class's member inventory.

## Supported slice

Inside an admitted anonymous method, declaration navigation supports its own
parameters, ordinary local variables, catch parameters and try resources using
the same declaration extraction, scope intervals, visibility and selection rules
as named methods. Duplicate candidates remain ambiguous; pattern refusal guards
are not evidence of a declaration.

Each anonymous body establishes a boundary. Constructor arguments retain their
creating context. A nested anonymous method can resolve its own locals but cannot
borrow those of the enclosing anonymous or named method. A variable used as a
receiver can navigate to its local declaration; this does not resolve the called
member or infer the receiver's type.

Local targets retain the existing empty `Target.owner` and exact source name
range. No public target field or synthetic fully qualified name is added.

## Deliberate refusals

An anonymous body with any parser diagnostic remains wholly unsupported,
including valid Java constructs that this parser does not yet model. Unrelated
errors outside an intact anonymous body do not by themselves disable it.

Anonymous fields, initializers, lambdas, nested named declarations and enum
constant-specific bodies remain opaque. Queries not resolved to the current
anonymous method's own local inventory terminate without falling back to outer
captures, fields, imports or types. Method selection, inheritance, generic
substitution, effectively-final capture validation and strict invocation proofs
are not enabled by this slice. Earlier unsupported outer captures remain
unsupported.

## Index and snapshot boundaries

Owner records and local ownership are rebuilt from each current syntax tree.
No owner number is a persistent symbol identity. One local inventory is stably
partitioned so legacy named-method consumers scan and charge for precisely their
original inventory; anonymous records do not consume an unrelated invocation's
proof budget. The final storage is linear in owners and locals, without a copy
of all locals per owner or a new proof cache.

## Verification

Native tests cover own declarations, shadowing, nested bodies versus constructor
arguments, resource/catch scope, duplicates, opaque regions and damaged edits,
fresh-index parity and old prepared snapshots. Fifteen unchanged Gson parameter,
local and catch references are checked against independently compiler-selected
declarations and exact name ranges; three anonymous-field references remain
refused. These checks demonstrate a bounded capability, not anonymous-class
semantic completeness or a project-wide coverage percentage.
