# Direct binary instance method declarations

The native strict selector now admits public, nongeneric, fixed-arity methods
on a proven direct binary receiver. Existing strict actual classification and
formal-to-formal partial ordering are reused. There is no second overload
resolver and no conversion-cost scoring.

Receivers include explicitly typed local variables/parameters and existing
bounded, proven **source** invocation reference results. A source factory's
binary reference return may therefore supply the terminal receiver. Binary
invocation return values are not propagated as receivers or arguments.

## Evidence and refusal boundaries

Nested binary receivers remain unbound. The existing descriptor parser retains
raw binary parameter names separately from the legacy dotted projection.
Dollar-containing binary parameter names remain unsupported and must not
substitute a same-dotted-name top-level class. A parenthesis within a legal JVM
class name is not treated as the end of the parameter list. Return descriptors
do not undergo this parameter lookup. Caller-visible failure hints use the same
bounded graph/work proof as strict selection; protected access cannot escape
that budget through the legacy hierarchy walker.

Every same-name declaration is validated before arity/applicability. An inherited
or interface same-name competitor, generic signature, nonpublic/static/varargs/
bridge/synthetic method, array formal, unknown method flag, illegal access or
abstract flag combination (including ACC_STRICT outside class-file major 46..60) refuses the proof rather than being filtered out.
Native, abstract, final and synchronized flags individually permit declaration
selection; this does not execute a body or constitute full JVM verification.
Method flags retain their original u16 bits. The primary class-file parser first
validates the file; a checked framing walk recovers bits that its flags wrapper
would otherwise truncate. Counts align before class-initializer filtering.

The complete JVM descriptor is checked. A syntactically legal void or array
return is allowed for **declaration selection only**, not return inference.
Methods differing only in return descriptor cannot win an overload selection.
All existing graph, accessibility, shared work and depth limits remain in force.

A failed explicit binary-instance proof is terminal: a single matching method
name does not justify `Resolved`. This intentionally contracts old successful
`String.length`/`charAt` navigation when an interface contributes the same name,
as well as fixing erroneous legacy success for wrong arity/types. The original Commons forecast of six selections did not all pass. Three
ClassUtils queries remain deferred: its unrelated `Comparator<Class<?>>`
`comparator` method fails the existing generic-header inventory proof, so its
lexical type-shadow absence cannot be certified. This slice does not loosen that
inventory boundary. Two StringUtils substring calls exposed duplicate whole-file
local scans for primitive actuals; classification now shares one bounded visible
local lookup instead of increasing the work budget. The revised target set is
those two substring calls and the existing String-local startsWith selection.
Three length/charAt controls now preserve both String and CharSequence candidates
as `Ambiguous`; the new strict hint path does not apply legacy ancestor-signature
hiding. This is not an identity deduplication change. compareTo remains refused.
Original forecasts, first failures and precise subsequent contracts remain
available; final real-project verification is reported in acceptance logs.

## Exact method identity and CLI projection

`Target.binary: Option<BinaryTargetIdentity>` is present only for binary methods.
It carries original JVM binary name (including `$`), full descriptor including
return, and explicit declaration context: `Standalone` or the workspace's
`CompilationId`. `Target.path` supplies the selected origin. Prepared and direct
queries use the same context mapping. This is snapshot/model identity, not a
persistent symbol ID, runtime class loader identity, JPMS identity or byte hash.
Classes, fields and source targets do not receive fabricated method descriptors.
Binary methods still have no source ranges.

The default CLI schema-v1 target projection retains its previous fields,
`origin_kind_owner_name_only` precision and null linking context. Opt in with
`declaration --target-details`; the result advertises `target_details_version: 1`.
Binary methods then expose `descriptor`, the explicit `linking_context`, and
`identity_precision: origin_binary_name_descriptor_linking_context`. Other kinds
retain their old precision. Candidate multiplicity is retained even when two
candidates have identical default projections. LSP continues using the default
projection and does not acquire binary source navigation.

See [API migration](MIGRATION.md) for public Rust construction/equality changes.
