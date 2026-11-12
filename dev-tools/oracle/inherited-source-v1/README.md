# Inherited source candidate oracle

Additive suite inside check-semantic-oracle.py. Six source selections: unique
parent, parent/child different-signature overloads, two superclass levels, and
cross-package declaring import versus a same-name child-package shadow.
Same-signature override, interface and generic superclass remain exact refusals;
missing superclass and unknown signatures are compiler-invalid separate fixtures.

The manifest pins source bytes and both complete explicit stage contracts.
Baseline mode checks old candidates and requires failure against current winners.
Old oracle goldens are untouched. BindingOracle observes only javac static targets;
no fixture program is run and no Java dependency is added to engine runtime.

## Explicit inherited-overlay revision

The original four-suite forecast and fixture manifest bytes are unchanged.
`prior-inherited-revision.json` pins that forecast and the first real failure,
and activates only qualified-primitive, strict-reference and reference current
stages. The primitive unqualified call remains outside this slice and retains
its original refusal. The previously successful reference_generic_ancestor must
remain successful; an incomplete diagnostic platform that omitted Iterable initially refused it,
which is not an allowed overlay or evidence of product regression.
Historical capture branches never invoke the new current-stage function.
