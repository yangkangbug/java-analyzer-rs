# Return declaration inventory

Declaration inventory is separate from strict invocation applicability and
return-value typing. A complete, legal type *shape* can contribute a method name
to the inventory even when the engine cannot infer or substitute that type.
This does not validate generic arity, bounds, type availability or an entire
Java compilation unit, and does not type unrelated method bodies.

## Implemented boundary

A single bounded walk over the existing lossless return-Type AST recognizes
ordinary primitive/void/reference returns, arrays, qualified parameterized name
segments, nested type arguments and wildcard arguments with complete reference
bounds. Primitive arrays are reference type arguments; bare primitive or void
arguments are not. Wildcards are only arguments, never standalone returns or
array components. Empty/diamond argument lists, trailing commas, missing bounds,
malformed delimiters and unsupported annotation shapes do not certify inventory.

The walk has the existing 32-level nesting and 4096-unit work limits. It charges
each visited node and its child inventory; exact-boundary tests accompany the
resource-refusal tests. It does not recursively flatten token lists or introduce
a second parser. Parser error nodes remain refusals. The existing parser still
cannot represent every legal Java shape, including primitive-array wildcard
bounds; compiler-valid examples of those remain explicit deferrals.

This proof is used by `reference_method_header`, including headers of the
current caller. A nongeneric caller with a complex declared return may therefore
enter the existing invocation proof. Constructors and parameter checks retain
their earlier rules. The ancestor-specific raw-header proof is unchanged:
this change does not legalize `extends Base<?>` or expand generic ancestry.

## What does not follow

- Generic methods and parameters remain unsupported strict competitors. They
  are retained, not silently dropped to manufacture a unique winner.
- A complex declared return does not become a receiver or argument type merely
  because its method header is complete.
- No generic inference, capture conversion, substitution, new overload phase,
  source discovery, public API or CLI schema is added.
- An inventory refusal is not a semantic diagnostic or a declaration that the
  source is invalid Java.

## Evidence

The frozen `0697877b` native binary establishes the complete old Commons stage.
`header-inventory-probes.json` retains those old default/detailed records and
reuses the original independently frozen binary-method forecast for exactly
three previously deferred ClassUtils queries. The first candidate passed all
three, the other seven binary records, all 45 original queries with the existing
q06 overlay, and four historical source getter records. Parser reports and
source/platform bytes remain unchanged. These are bounded corpus observations,
not whole-project Java support.

Handwritten compiler comparison, malformed-header mutations and resource
refusals are separate from production runtime, which remains JVM-free. Native
unit tests additionally exposed and closed an older permissive-parser case:
a top-level wildcard return could previously certify a class inventory.

## Design reference

The separation follows the design of IntelliJ Java method stubs and TypeInfo,
not a port of its implementation. At fixed commit
`840d3be11dfdeada533d05052008b48816b6cc4e`,
[JavaMethodStubFactory](https://github.com/JetBrains/intellij-community/blob/840d3be11dfdeada533d05052008b48816b6cc4e/java/java-psi-impl/src/com/intellij/psi/impl/java/stubs/factories/JavaMethodStubFactory.java)
collects method name/flags separately from return TypeInfo;
[PsiMethodStubImpl](https://github.com/JetBrains/intellij-community/blob/840d3be11dfdeada533d05052008b48816b6cc4e/java/java-psi-impl/src/com/intellij/psi/impl/java/stubs/impl/PsiMethodStubImpl.java)
retains both; and
[TypeInfo](https://github.com/JetBrains/intellij-community/blob/840d3be11dfdeada533d05052008b48816b6cc4e/java/java-psi-impl/src/com/intellij/psi/impl/cache/TypeInfo.java)
represents generic, inner-generic, wildcard and array structure without first
proving a callable signature. Its permissive stubs are not themselves evidence
for this engine's stronger negative inventory guarantees.
