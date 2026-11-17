# Field member navigation

Field lookup builds each type's effective members along its direct parent edges,
then applies caller access and reports zero, one or multiple candidates. It no
longer chooses the closest traversal depth or removes inaccessible declarations
before they can hide ancestor fields. This follows [JLS 8.3](https://docs.oracle.com/javase/specs/jls/se25/html/jls-8.html#jls-8.3),
[JLS 9.3](https://docs.oracle.com/javase/specs/jls/se25/html/jls-9.html#jls-9.3) and
[JLS 15.11.1](https://docs.oracle.com/javase/specs/jls/se25/html/jls-15.html#jls-15.11.1).

## Direct-edge members before caller access

An own declaration replaces the inherited same-name set, even when inaccessible
to the final caller. A private declaration is not inherited; a package-private
field must remain eligible at every edge and cannot reappear after a foreign
package interrupts its inheritance. Public and protected fields propagate before
the existing caller-access policy is applied. Source interface fields with no
explicit access modifier have their implicit public access recorded in the index.

Each direct parent's member set is merged separately. In particular, if `J`
redeclares `I.x`, but `K` inherits `I.x`, a class implementing both `J` and `K`
still receives both fields. Global declaring-type ancestry is not a substitute
for the member-set construction. Repeated paths to the same declaration are
coalesced using snapshot-local owner identity and member ordinal, while malformed
duplicate declarations remain distinct.

The query-local memo stores member sets, not caller-filtered results. Cycles and
ambiguous parent declarations remain errors even when an own field exists.
Missing supertypes retain the existing navigation behavior and contribute no
members. Interfaces do not inherit fields from a class-file Object superclass.

## Known protected instance-field receivers

After existing caller visibility, ordinary cross-package field navigation rejects
an instance-field candidate only when the known receiver type cannot be the
accessing subclass (or one of its subclasses). All lexically enclosing classes
participate, so a nested class may use its enclosing subclass's access. Static
fields and same-package accesses retain their existing rules. See
[JLS 6.6.2.1](https://docs.oracle.com/javase/specs/jls/se25/html/jls-6.html#jls-6.6.2.1).

This is a negative guard, not a general permission framework. It reuses existing
validated source-field metadata and known legal binary field flags. Missing source
facts, contradictory flags or incomplete parent graphs cannot justify deleting a
candidate. An already known ancestry relation stays positive; a negative needs a
complete graph, including registered Object. Unknown candidates remain alongside
public competitors rather than silently making them unique. The existing walker
now reports completeness separately; other callers retain their previous behavior.

Method navigation, strict proofs, private nest access and unsupported `super`
expressions are unchanged. In particular, navigation of annotated/unsupported
source fields or incomplete projects may still retain the older target hints;
this guard does not turn such hints into verified Java access.

## Contract and limits

This changes navigation targets for incorrectly resolved or inaccessible fields;
it does not change the public result schema. Binary field target provenance keeps
its previous path/kind/owner representation. Contradictory access candidates now
retain `NotDetermined` even alongside known-access candidates, instead of being
silently discarded to manufacture uniqueness. Unknown access is not a semantic
certificate.

Method collection and strict method/field type proofs are separate and unchanged.
A navigation success does not certify a field's type or legal use in every Java
context. Protected method access, unknown protected-field facts, private nest access, static
context checking, `super` field expressions, malformed declaration validation and
missing-parent completeness are not implemented by this change. Inherited fields
do not become new strict invocation actuals or receivers.

## Validation

Eleven native groups cover hidden inaccessible declarations, unequal-depth
ambiguity, alternate inheritance paths, diamonds, package boundaries, final caller
access, source/binary hiding parity, contradictory access, duplicate declarations,
cycles, ambiguous parent lookup and edit/fresh/old-snapshot behavior. Tests do not
start a JVM. Separate pinned javac checks compare five legal declaration targets
and six rejected cases; ten cases also use real compiled dependency class files
and compare native source/binary outcomes. This is a correctness repair, not a
full Java conformance or coverage-rate claim.

Eight additional native groups cover known protected-field receiver restrictions,
static/same-package/implicit access, enclosing subclasses, missing ancestry,
unknown source facts and binary flags, competing public fields, `super` refusal
and edit/fresh/old-snapshot parity. A separate compiler comparison checks eight
legal exact targets and three illegal controls; all eleven also match native
results using real compiled dependency classes. No method applicability expansion
or strict field-type coverage increase is claimed.
