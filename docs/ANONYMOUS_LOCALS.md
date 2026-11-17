# Anonymous-method declaration navigation

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

After local selection has found no declaration, an admitted method can navigate
a uniquely declared field of the same anonymous owner. Local guards or ambiguity
terminate the lookup rather than falling through to a field. Direct `this.field`
selects only that owner's field, independently of a same-named local; other
receivers, `Outer.this` and `super` are not admitted. Duplicate own fields remain
ambiguous. An own field may hide an inherited field without needing to search the
parent for this declaration-only result.

A field used as a receiver may navigate its own name, not the called member.
No anonymous field type enters strict actual/receiver or return-type proofs.
Field declarations supply inventory, but queries at their declarations or
initializers remain outside the method-only query boundary.

## Target compatibility

Local targets retain their existing empty `Target.owner`. Anonymous Field targets
now also use an empty owner: this is a new **Field output semantic**, not a new
struct field. It means no canonical owner name is represented by this interface,
not a top-level field or an unknown named class. Existing named field outputs are
unchanged. No synthetic fully qualified name is invented.

Source declarations are located in their snapshot/compilation context by path
and exact name range; the empty display value is not an identity. Internal
anonymous ownership uses the private owner record and member location, never
display-name reverse lookup. Repository CLI and editor regression tests exercise empty-owner fields with
exact source positions; the LSP source-location adapter has been audited to use
path and range, not the owner display. External
consumers that assumed every Field owner was nonempty must account for this
documented extension.

## Deliberate refusals

An anonymous body with any parser diagnostic remains wholly unsupported,
including valid Java constructs that this parser does not yet model. Unrelated
errors outside an intact anonymous body do not by themselves disable it.

Field initializers, initialization blocks, lambdas, nested named declarations and enum
constant-specific bodies remain opaque. Queries not resolved to the current
anonymous owner's local or own-field inventory terminate without falling back to
outer captures, inherited fields, imports or types. Method selection, inheritance, generic
substitution, effectively-final capture validation and strict invocation proofs
are not enabled by this slice. Earlier unsupported outer captures remain
unsupported.

## Index and snapshot boundaries

Owner records and local ownership are rebuilt from each current syntax tree.
No owner number is a persistent symbol identity. One local inventory is stably
partitioned so legacy named-method consumers scan and charge for precisely their
original inventory; anonymous records do not consume an unrelated invocation's
proof budget. The final storage is linear in owners, locals and own-field records, without a copy
of all locals per owner or a new proof cache.

## Verification

Native tests cover own declarations, shadowing, nested bodies versus constructor
arguments, resource/catch scope, duplicates, opaque regions and damaged edits,
fresh-index parity and old prepared snapshots. Fifteen unchanged Gson parameter,
local and catch references are checked against independently compiler-selected
declarations and exact name ranges. Three unchanged Gson own-field references
now independently match compiler-selected declaration names and ranges; the
historical outer captures remain refused. These checks demonstrate a bounded
capability, not anonymous-class semantic completeness or a project-wide coverage
percentage.
