# Source type-parameter declaration identity

Version 0.6.0 adds source definition targets for type parameters without treating
them as classes or inferring their substituted types.

## Identity and wire contract

The public Rust enum appends `TargetKind::TypeParameter`. CLI declaration
responses use `kind: "TypeParameter"`, the actual source path and exact identifier
byte range, `owner: ""`, and no binary identity. Empty owner is not a class name:
source declaration identity includes path and identifier range. Two same-named
formals in different declarations remain different targets.

The editor adapter and LSP definition endpoint use the existing source location
and UTF-16 conversion. Response structures, protocol versions and source attachment
matching do not change. Type parameters do not enter binary attachment lookup.

This is a source-breaking exhaustive-enum change for Rust Git dependencies.
Update matches over TargetKind for the new variant when moving from 0.5 to 0.6;
do not map it to Class, a value Parameter or a binary symbol. JSON consumers with
a closed kind allowlist also need to accept TypeParameter. The crate remains
publish=false; this is a Git-dependency migration, not a registry release.

## Supported lexical identity

Class and callable formal lists share declaration-record extraction with existing
generic-name guards. Ordinary type-use positions and declaration identifiers can
navigate to their exact formal declaration. Self and mutually referencing bounds,
method return/parameter/throws positions and same-owner generic declarations are
covered. Class-header/bound scope is distinguished from the class body.

An own callable formal shadows outer names. A class's own formal takes precedence
over an inherited member type, while its directly declared member type can shadow
the formal within the body, not in the header/bounds. Local-type shadow
records remain terminal to nominal lookup; 0.7 can expose their bounded lexical
[declaration identity](LOCAL_TYPE_IDENTITY.md) rather than replacing them by an outer T.
Unsupported class-formal uses across static boundaries, incomplete/duplicate
groups and unsupported cross-owner outer formal lookup are conservative refusals, not permission to borrow a same-spelled
class or package. Nearer supported declarations and unrelated names keep their
existing paths. Anonymous methods can expose their own formal names without
opening outer captures or general anonymous receiver typing.

Only structural type-name roles admit these targets. Value references retain
their own namespace; imports and package names do not acquire a formal identity.
A nominal receiver declared T cannot borrow the unrelated class T's members.
T.Inner does not reinterpret T as a package. This is declaration navigation,
not a full type checker: bound-member lookup, generic invocation inference,
substitution-based receiver typing, class-literal support and positive inherited
member-type integration remain outside this change.

## State and work

The immutable snapshot stores formal groups and relevant class/static boundaries.
Unrelated methods do not create formal inventory. Inner-first preparation avoids
per-query sorting and candidate allocations. Strict nominal consumers pay before each scope range check and before the
parameter/member/local-type scans actually visited. Unrelated scope parameters
and scopes beyond an already determined result are not charged or scanned.
Empty formal inventory adds no work; budget exhaustion remains refusal. There is
no persistent cache or raised proof budget.

## Observed validation

On the pinned original Gson/Commons type-name census, 3,393 queries yielded 3,289
new TypeParameter targets: 2,437 type uses independently matched javac declaration
identifier ranges, and 852 declaration clicks matched their CST identifier
inventory. The other 104 complete results were unchanged. These are corpus
observations, not a claim of complete Java scope coverage.

Seven original source positions passed CLI checks and fourteen LSP checks covered
original text and an unsaved Unicode edit with exact UTF-16 locations. The existing
427-query and 848-query suites were unchanged, as were eighteen anonymous/local
declarations and the historical outer-capture refusals. Focused tests cover legal
collisions, invalid member access, scope barriers, duplicate/malformed formals,
incremental/fresh/undo/old snapshots, identity and exact budget exhaustion.
