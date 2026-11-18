# Own source-field type evidence

Class state can now enter the existing strict invocation proof, rather than only
legacy name navigation. In an ordinary instance method, a directly declared
instance field may supply a scalar or explicit-array actual, or a receiver through both `field` and
`this.field`. This connects existing argument selection, call-result composition
and bounded binary receiver instantiation to object state; it adds no separate
method selector or public type API.

## Evidence and scope

A field has dedicated declaration metadata: validated modifiers, static status,
plain/applied reference syntax or primitive type, and per-declarator dimensions.
The old `Member.declared` hint is not proof. Field and local reference declarations
share their type/rank witness; applied receivers share the same exact
owner-and-arguments environment and Object-bound checks.

Bare names first consult the caller's lexical declarations. A lookup distinguishes
absence from ambiguity, unsupported kinds/types and budget failure. Only proved
absence can use an own field. An expired local therefore permits an own field;
an in-scope unknown local does not. Exact `this.field` bypasses local shadowing.
An own field hides a same-named ancestor; an unknown own declaration never borrows
an ancestor's type. Inherited fields themselves remain outside this slice.

The caller must prove an ordinary nonstatic method (or a supported
[ordinary constructor](CONSTRUCTOR_CALL_CONTEXTS.md)) in its own top-level,
nongeneric source class, with the existing complete inventory/hierarchy and
caller-context checks. Access to that class's own private, package, protected or
public field is allowed. Type names resolve at the field declaration, not at the
use or from an initializer. Null initializers do not change declared types.

Plain reference and primitive fields supply strict actuals. Explicit array fields
retain component identity, rank and declaration linking context through the same
private declaration-to-array proof as locals/parameters. Existing indexing
composes naturally: primitive ranks may decrement to a scalar, and the existing
rank-one reference projection remains available as an argument. This does not
add indexed receivers or field-array length/arithmetic evidence. Applied
fields are receivers only, not erased scalar arguments. For example,
`this.box.get().hit()` may use the same bounded binary substitution proof as an
explicit local `Box<Item>`. Existing method-family limits still apply.

[JLS 15.11.1](https://docs.oracle.com/javase/specs/jls/se25/html/jls-15.html#jls-15.11.1)
describes field access using a primary; [JLS 6.5.6.1](https://docs.oracle.com/javase/specs/jls/se25/html/jls-6.html#jls-6.5.6.1)
distinguishes simple expression names and their declarations. This slice does not
attempt the full inheritance, qualified access or capture-conversion rules.

## Preserved boundaries

No strict field evidence for static fields/contexts, unsupported constructors, initializers,
lambdas, nested/anonymous/local classes, `super`, qualified `Outer.this`, arbitrary
`obj.field`, inherited fields, unsupported annotated declarations, field updates, generic
source owners, or initializer inference. Invalid/duplicate field modifiers,
duplicate own fields and incomplete type/hierarchy identities refuse proof.
All scans and expression recursion share the existing work/depth limits.

Existing direct field-chain navigation can still return a name hint when strict
selection is unavailable. Those legacy results are not reused as argument or
return-type evidence. Unrelated `obj.field` navigation is unchanged. No new claim
is made that every public `Resolved` result checks full Java applicability.

## Original scalar-field validation and real-project interpretation

Eight native groups cover reference/primitive actuals, bare/this and applied
receivers, lexical shadowing, own-versus-inherited fields, access, modifier/rank
legality, unsupported contexts and budgets. Nine prior native inputs now assert
precise successful targets rather than former field refusals; original input
text and all remaining negative cases are retained. An optional javac comparison
checks nine exact source targets and five compiler-invalid controls. Native code
and tests do not invoke Java.

Inspection of existing Gson/Commons sources confirms ordinary class-state uses
such as JsonWriter's `Writer out`, FastDateParser's `Locale locale`, and
JsonArray's `ArrayList<JsonElement> elements`. These motivate a common declaration
entry point, not class-specific rules or promised coverage. The last example
still needs inherited generic member handling, and other calls can remain blocked
by unrelated caller, argument or inventory limits.

At the original scalar-field increment, the fixed 577-query and separate
30-query result records were completely unchanged.
This is a structural semantic improvement demonstrated by independent fixtures,
not a measured rise in those projects' coverage. No new survey framework or
acceptance stage was introduced.

## Shared own-field array extension

The existing own-field identity, access, shadowing and caller checks are unchanged.
A reference component is resolved at its declaration and must pass the existing
complete identity/access/graph proof. Unknown locals, duplicate fields, generic
components and exhausted proofs do not fall through to a different field or type.
No new field lookup, cache, budget, stored type model or public API is introduced.

The original nine StrBuilder arraycopy calls were independently checked with
javac. Only getChars (byte 64344) gains the exact System.arraycopy descriptor
(Ljava/lang/Object;ILjava/lang/Object;II)V; the other eight remain refused.
Their scalar-field arithmetic remains outside this extension. Original 1,297
query records and independent 3,393 type-parameter, 94 local-type and 167 array
records are unchanged. Five independent native groups, eight compiler targets
and four disk/unsaved-Unicode LSP checks cover the shared behavior. Two previous
field[0]/this.field[0] argument refusals now assert the complete compiler-exact
Sink.take target; their receiver and other unsupported-array controls remain.

Evidence: evidence/own-field-array-values-20261118 and
evidence/own-field-review-20261118. This is declaration evidence reuse, not general
field expression typing.
