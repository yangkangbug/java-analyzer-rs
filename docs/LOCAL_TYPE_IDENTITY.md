# Local-type declaration identity

Version 0.7.0 adds lexical source definition targets for recognizable local class,
enum and record declarations. Their bodies remain opaque: this is declaration
identity, not implementation of local-class members, captures or record synthesis.

## API and scope
The public Rust enum appends TargetKind::LocalType. CLI JSON uses
kind:"LocalType", owner:"", the actual source path and exact name-token byte range,
with no binary identity. Empty owner is not a top-level or fabricated class name.
Different declarations with the same spelling have different source ranges.

This is a source-breaking exhaustive-enum change for Rust Git dependencies.
Update matches and closed JSON kind allowlists when moving 0.6 to 0.7. Do not map
LocalType to nominal Class or infer a qualified name. The package is still
publish=false; this is not a registry release. No JSON/LSP structure or protocol
version changes. Source attachments do not admit LocalType; LSP uses existing
source URI and UTF-16 conversion.

Supported queries are exact declaration identifiers and existing structured
type-use roles outside opaque declarations, within supported lexical ownership.
A local type shadows earlier imports, enclosing nominal types and method/class
formals after its declaration. Disjoint blocks restore their outer meanings.
Same-owner overlapping duplicate declarations refuse a unique type-use target,
including a second declaration later in the current block. Exact declaration
clicks identify their own token rather than certifying legality.

An anonymous method's own formal can win over an outer local type. Unsupported
cross-owner lookup refuses rather than assuming inherited names are absent.
Local.Inner and values declared with local types cannot borrow unrelated nominal
members. Value, import and package namespaces do not gain local-type targets.

## Shape evidence is not Java validation
Private complete evidence means a recognizable class/enum/record name and a
closed, balanced opaque body. It does NOT validate the remaining header.
For example, class T extends {} may retain a declaration identity without being
legal Java. Local interface/@interface and known-name incomplete declarations
stay unavailable; their same-name barrier is preserved. A missing actual name
does not invent a declaration or shadow every unrelated type outside its opaque
node. This retains the existing recovery boundary, not a claim that malformed
source has an unambiguous intended interpretation.

Headers and bodies remain opaque except exact declaration-name clicks. No new
constructor selection, class-literal support, enclosing capture, field/method
lookup, inherited member-type integration, or receiver/inference capability is
provided. The thirty original unsupported-local-type parser reports remain.

## Shared state and bounded work
Local records retain exact name ranges, lexical block ranges and source owner
identity. A preparation-only map groups record IDs by scope; it is not a query
cache. Relevant local-only nested owner boundaries are retained independently
of formal groups. The existing shared charged lexical visitor selects formal,
local, absent or unknown evidence in scope order. Nominal consumers treat local
identity as terminal. Legacy negative guards on non-type-role entry paths remain
to preserve their unsupported-prefix behavior. No proof budget is increased.

## Observed validation
On the pinned 460-file Gson/Commons census, 94 local-related identifiers produced
57 new targets: 30 exact declaration clicks and 27 references independently checked
against javac declaration identifier ranges. The other 37 full query results were
unchanged. All 3,393 prior formal queries were unchanged. Five original CLI and ten
LSP checks covered original and unsaved Unicode source with exact UTF-16 locations.

The existing 427 and 848 query sets, 18 local/anonymous declarations and historical
outer-capture refusals were unchanged. Native controls cover declaration order,
scope/owner crossings, overlapping duplicates, illegal/incomplete declarations,
missing names, value/type isolation, Unicode and edit/fresh/undo/old snapshots.
These are bounded observations, not a Java conformance or overall-completion rate.

The historical local-type-v1 fixture/golden stays immutable. Its current oracle
checks one explicit change from Unresolved to exact LocalType, including empty
owner and marker range; wrong kind/owner/range mutations must fail. The other
three opaque-body refusals remain.
