# Direct nested-type evidence

## Class-file facts

`ClassFileData.inner_classes` retains every `InnerClasses` attribute as a separate
ordered group of entries. Each entry retains the original binary class name,
optional outer binary class name, optional inner simple name and original u16
access flags. Absent, empty, duplicate and contradictory groups are not collapsed.
`has_enclosing_method` records attribute presence only, not a complete local or
anonymous class binding.

Names and constant-pool validation remain cafebabe's responsibility. The existing
bounded raw-layout walk additionally retains class-attribute payloads so unknown
flags cannot be silently truncated. Entry count and exact `2 + count * 8` payload
length must agree with parsed attributes. Malformed layout fails parsing rather
than publishing partially trusted metadata. No JVM or new dependency is added.

These are facts, not a certified relationship: entries may describe other classes,
zero outer/name indices occur for local/anonymous classes, and arbitrary `$` in a
binary name does not prove nesting. The legacy `dotted_name`/`nesting` projection
is unchanged. The signature path described below checks actual direct ownership,
complete intermediate candidates, contradictory flags and local/anonymous
exclusions. These facts alone are not a complete Java access or validity proof.

Native regressions use handwritten class bytes and cover group/entry order,
absence vs empty attributes, duplicates, unknown/conflicting flags, zero indices,
EnclosingMethod and malformed/truncated payloads. A separate optional javac check
verifies real public/static/private/package member, local, anonymous and legal
`$`-named top-level class files without executing fixture programs.

## Source compatibility

`ClassSignature` keeps its existing fields and struct-literal construction.
`parse_class_bytes`, `read_jar`, `read_jmod` and `Project::class_signature` retain
their existing signature-only return types. The additive `parse_class_data` API
returns `ClassFileData`, containing that signature and the new metadata.

Internal directory/archive loading and immutable payload sharing retain the
wrapper in the same read. No global sidecar, duplicate I/O, repeated class parse,
serialized workspace schema, dependency version or classpath policy is added.


## Existing signature paths: each intermediate owner matters

The existing `signature_nested` path now follows one direct member at a time
instead of joining the first declaration's dotted name with the final suffix.
A missing, ambiguous or contradictory intermediate step stops the proof. Source
children must belong to the actual owner's file, compilation and range; a child
under a different same-spelled declaration cannot fill the gap.

Binary children require matching owner/child `InnerClasses` entries, one
attribute group, unique self entries, compatible known flags and no enclosing
method. Loaded child-side claims cannot disappear merely because the parent
omits them. `$` in a binary name alone does not prove a member relationship;
conversely a certified relationship does not require concatenating binary names.
Access flags retain competitors rather than filtering ambiguity to success.

Every selected step retains same-JVM-identity competitors visible in the original
query compilation, including when the first source owner belongs to a dependency
compilation. Revisited declaration identities refuse cyclic ownership. The path
is iterative over the provided suffix: no new inheritance walk, recursive lookup,
cache, parser, runtime dependency or public API break is introduced.

The existing first-segment scope/import rules and callers are unchanged. This is
signature-identity evidence used by existing method comparison/selection paths,
not general member-type inheritance, access checking or ordinary `Outer.Inner`
field-type navigation. Fully qualified fallback and other legacy lookup paths
are not comprehensively redesigned by this change.

Native tests demonstrate that a valid override still selects the child method,
while an ambiguous intermediate parameter type preserves both method candidates
as `SignatureNotCompared`. Separate compiler controls cover valid source and
class-file chains, duplicate/missing intermediate types and legal `$` names.
The optional real-binary check feeds compiler-created bytes to the native engine;
compilation is not part of the analysis runtime or mandatory tests.
