# Exact binary field declaration identity (0.9.0)

Already modeled binary Field targets now retain their raw JVM field descriptor,
original binary owner name, and declaration linking context in Target.binary.
The selected origin remains Target.path and the member kind/name remain in Target.
The same record still carries binary method identity. Descriptor grammar depends
on Target.kind: a field descriptor is not a method descriptor.

This is metadata preservation, not a new field resolver. Selection status,
candidate order and multiplicity, access checks, inherited lookup and type/value
proofs are unchanged. Ambiguous candidate declarations retain their own identities;
metadata does not claim that one of them was selected. Generic Signature text is
not interpreted: the existing raw field descriptor remains authoritative.
Classes and source declarations retain binary=None. No field value, constant,
receiver, ancestor, generic or array-type inference is added.

## Public compatibility

The crate is now 0.9.0 (publish=false), a conservative pre-1.0 Git-dependency
migration for the expanded public contract. Rust struct fields are unchanged,
but consumers must no longer assume Some(Target.binary) means Method. Branch on
Target.kind before interpreting the descriptor. Derived equality now distinguishes
binary field declaration contexts as well as descriptors. This is snapshot-local
declaration identity, not runtime class-loader or persistent symbol identity.

CLI declaration --target-details now advertises target_details_version: 2 and
capability scope binary_methods_and_fields. Binary methods and fields expose the
same descriptor/context precision label; classes/source targets remain unchanged.
Default schema-v1 result/query/target projections remain unchanged. The global
capability advertisement deliberately reports v2 even without opting in, so the
entire response is not byte-identical to the prior release.

The opt-in serializer checks the member in the correct fields or methods table
of the exact recorded declaration context. It does not guess from loaded_contexts
or from the caller's scope. No new field source attachment is enabled: attachment
admission still accepts Class/Method only. A binary field has no fabricated source
range, and enriching its identity does not make source_available true.

## Validation and historical evidence

Native tests cover primitive/reference/multirank raw descriptors, generic field
metadata without supplied component types, ambiguous candidates, unchanged source
fields/methods, direct/prepared queries, edits/old snapshots, shared binary origins
in distinct compilations and a dependency's declaration context.

Frozen oracle JSON and historical v1 prediction helpers remain unchanged.
The current oracle adds a separate v2 projection with independently frozen
class-file field metadata and explicit fixture-specific declaration scopes.
Current-only adapter fixtures move to v2; they do not rewrite historical captures.
Wrong versions, missing descriptors and incorrect declaration contexts must fail.

## Independently checked real-source preservation

The four original Gson JsonWriterTest Math.PI/Math.E witnesses were already
Resolved before this increment. Their default result is unchanged; opt-in
details add only raw descriptor D, the declaration context and the identity
precision marker, together with the v2 envelope. Independent javap inspection
of the same-byte JMOD confirmed D. These are not four newly resolved queries.

With real Math source attachments configured, all four field queries still
refuse field source navigation. The existing 12 Math method witnesses / 24 LSP
checks, all 1297 real-source queries, and 14 cast checks were independently
verified as preserved. This targeted evidence does not claim completion of the
full gate rerun: its first round exposed a historical capture adapter requiring
v2 from a SHA-pinned v1 binary. Both SHA-pinned historical capture entries now
select v1 explicitly; current acceptance remains strictly v2. The original
failure evidence is retained.

After that adapter correction, the complete final 52-gate rerun passed. The
verified release binary remained byte-identical to the real-source candidate.
