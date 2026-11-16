# Bounded binary generic receivers

Explicit parameterized locals/parameters can instantiate a bounded method family of
an accessible top-level binary class or interface. For example, `Box<Item>.get()`
can supply `Item` to a following call or argument; a declared `Map<Key, Value>`
can use its own `get(Object)` and `put(K,V)` family. There are no library-name
special cases. This is a bounded member substitution step, not general Java
generic inference or complete collection support.

## Representation and selection

Receiver declarations preserve plain versus applied syntax. An applied receiver
has a declaration-scoped class identity and an ordered list of exact plain
reference argument identities. It does not become a raw scalar argument.
Class Signature templates bind formal indices to that exact owner. Nominal names
in binary method templates resolve in the declaration's linking context; supplied
type arguments retain the receiver declaration's context.

[JLS 4.5.2](https://docs.oracle.com/javase/specs/jls/se25/html/jls-4.html#jls-4.5.2)
describes substitution for members of parameterized types. Here only class formals
with an explicit Object class-file bound are accepted. Each actual must prove a
conversion to the supplied Object identity in the binary owner's scope. Separate
closed hierarchies or matching names are not a cross-scope bound certificate.

The existing selector checks the complete same-name family, substitutes
parameters and return types, and reuses strict applicability and specificity.
The raw hierarchy must be complete. Inherited members require the certificate below.
All same-name methods must fit the slice, even if their arity would not apply.
Flags, slots, access, malformed signatures, duplicate erased parameters and
substitution-collapsed overloads are checked before selecting. No competitor is
silently dropped. Instantiated return evidence flows with the selected method;
`T` is not reread as erased Object. Public targets still identify the original
binary descriptor, origin and linking context. No public API/schema changes.

Class templates validate complete signature syntax against the raw superclass
and interface inventory. Parent argument templates are checked against each
selected parent declaration and its bounds, not used as raw conversions.
Descriptor-only methods retain their
nominal types; an Object descriptor is never guessed to mean a class variable.

## Single-chain inheritance certificate

A receiver may follow one non-Object binary parent at each step. Parent formals
can map to existing formal indices or plain nominal references; every parent
arity, Object bound and linking identity is validated. Synthetic Object edges
and empty nongeneric leaf interfaces are neutral. Multiple paths are rejected,
even when their concrete substitutions agree; raw parents, unsupported bounds
and incomplete inventories do not fall back to erased values.

Overlapping declarations require an additional declaration-level proof, not just
coincidentally equal caller types. Between the two declaring owners every parent
edge must preserve formal indices in order, and normalized method parameter and
return templates must match. Existing scope, ancestor, raw descriptor, access and
flags checks still apply. This deliberately stricter subset of
[JLS 8.4.2](https://docs.oracle.com/javase/specs/jls/se21/html/jls-8.html#jls-8.4.2)
and [9.4.1](https://docs.oracle.com/javase/specs/jls/se21/html/jls-9.html#jls-9.4.1)
rejects erasure clashes, concrete substitutions masking illegal declarations,
and shadowed formal names. Renamed formals at identical indices are accepted.
Constants or reordered parent arguments remain usable for non-overlapping
members; they are not proof of an override.

For example, the JDK List/SequencedCollection/Collection chain can select
`List<E>.add(E)` while keeping `add(int,E)` as a competitor. This does not add
ArrayList's multiple-parent paths, default methods, bridges or covariant returns.
No runtime JVM, build import, general inference or public result schema is added.

## Limits

Only visible, unique, explicitly declared rank-zero locals/parameters (or supported
own source fields) with plain
nongeneric reference type arguments are accepted. All receiver arguments, formals
and nonvoid results must have complete identity/access/hierarchy proof. Public
instance families only; binary interfaces require abstract methods. Existing
caller-context restrictions remain.

Deferred: source generic owners, inherited fields, branching generic member
selection, parameterized arguments/results, generic-method inference, non-Object
bounds, wildcards/capture, nested classes, arrays, annotations, raw/diamond or
varargs receivers, boxing and arbitrary expression inference. A generic method,
bridge, synthetic or varargs same-name competitor refuses the whole family.
These are conservative analysis limits, not claims that such Java is invalid.

Template parsing is nonrecursive with 4096-byte inputs, at most 32 class formals
and parents, and 255 method parameters (the selector also checks JVM slots).
The receiver, substitution and all comparisons share existing proof work/depth
budgets. The owner-plus-arguments environment follows the certified parent chain without
adding a second selection algorithm or changing general reference conversions.

## Validation

Eleven template unit groups cover erasure, scope/formal order, complete/truncated
syntax, duplicate variables, malformed bounds/parents and exact limits. Ten
integration groups cover own class/interface families, instantiated return chains,
List/Map-shaped methods, complete-family rejection, lexical shadows, declaration
rank/varargs, missing platform inputs, cross-compilation bound identity and budgets.
An optional javac check uses genuine compiled generic class files: seven exact
native targets and ten compiler invocations agree on targets and instantiated
results, plus four compiler-invalid controls. The existing frozen binary-instance
fixture also upgrades GenericOwner<Token>.plain(int) in both linking contexts,
with exact old-record and javap assertions; its frozen inputs remain untouched.
Native tests never invoke Java.

In the original own-family slice, unchanged pinned Commons ArrayUtils, `occurrences.get(boxed)` under
`Map<Byte, MutableInt>` now selects the exact platform Map.get declaration. This
single query occurs in both the 577-query regression set and the 30-query
parameterized survey; all other full records were unchanged at that stage. This is not a
project-wide coverage claim or a full Commons build. A separate javac check of
the same declaration/expression shape confirms the Map target and MutableInt
result. Local evidence is separate
from repository tests.

The inheritance extension adds 15 native integration groups and parent-template
unit coverage. Local javac evidence checks six exact inherited/overridden targets
and their instantiated return composition, three compiler-valid deferred shapes,
and three illegal declaration controls. The unchanged 577-query and 30-query
surveys improve one row each: Commons ExceptionUtils List.size and GsonBuilder
List.add. All other full records are unchanged. These are observed examples, not
project-wide correctness/coverage percentages. Native tests do not run javac.
