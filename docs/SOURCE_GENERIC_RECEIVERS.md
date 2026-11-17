# Explicit source generic receivers

A top-level source class `Box<T>` can now use the existing strict invocation
selector through an explicitly declared `Box<Item>` receiver. Direct public
instance methods substitute owner parameters in their scalar parameter and return
types. A selected `get(): T` can supply `Item` to an outer call or argument;
`set(T): void` can be navigated without inventing an expression value.
This is member substitution, not generic-method or constructor inference.

## Declaration evidence, not erased-name guessing

The source adapter reads the existing CST and declaration metadata. It never
manufactures a JVM Signature string or resolves declaration names using caller
imports. Owner formals retain ordered slots; plain nominal types resolve in the
method declaration's file and compilation. Substituted values preserve the actual
argument's identity. The same candidate selector, conversions, budgets and public
target representation remain in use.

The owner must be an ordinary top-level class with no explicit `extends` or
`implements` clause. Its complete graph must contain only itself and the supplied
binary `java.lang.Object` root. Each formal has an implicit Object bound or an
explicit bound resolving to that exact Object identity. A bound spelled `Object`
which actually refers to a formal or another class is not accepted. Formal lists
are limited to 32 parameters and 4096 UTF-8 bytes.

Direct member types can legally hide a class parameter: in
`class Box<T> { class T {} T get() { return null; } }`, `get` returns the member
class, not the formal. Such owners are refused rather than substituted by spelling.
A formal with the same name as its enclosing class is not itself an error.
Qualified formal uses such as `T.Inner` remain unsupported.

Every same-name method must provide complete supported evidence, even when its
arity would not apply. Parameters and returns may be owner slots, ordinary plain
references or scalar primitives; scalar void is an explicit result category.
Unknown returns are not interpreted as void. Generic methods, static competitors,
varargs, arrays, parameterized types and other unsupported declarations refuse the
whole family. Supported throws syntax is retained, but checked-exception analysis
is not added.

Erased parameter identities are checked separately from instantiated parameter
identities. `m(T)` and `m(Object)` clash under an Object bound. `m(T)` and `m(Item)`
can have different erasures yet collide after substitution; the existing selector
still refuses ambiguous selection. Return types never eliminate an overload.

## Scope and remaining limits

Receivers use the existing explicit local/parameter and supported own-field
paths. This does not substitute generic field members, add inherited-field lookup,
or infer types from initializers. Source interfaces, nested owners, explicit
superclasses, generic inheritance, wildcard/unknown actuals, bounded capture,
parameterized actuals/results, boxing and method-type inference remain outside
this slice. Existing ordinary caller-context and access limits still apply.

The declaration's Object bound is a real identity requirement. Source arguments
owned by a dependency compilation can work when their graph reaches that owner's
Object. A caller-local argument linked to another compilation's Object does not
become interchangeable merely because both platform inputs have the same path,
bytes or name. This conservative boundary can leave normal multi-module Java
calls unresolved; no graph/identity rules were relaxed for this feature.

## Validation boundary

Native regressions cover source/binary paired declarations, return composition,
nominal import context, void, complete competitors, erasure versus substitution
collisions, formal/member shadowing, bounds, limits, shared budgets and edited /
fresh / old prepared results. Independent javac comparison checks exact source
declarations and invalid programs; test fixtures are not executed.

Real-project diagnostic probes are reported separately from unchanged upstream
queries. A new probe in an existing compilation is not a claim that the original
project's tests, cross-compilation calls or general generics now work. There is no
new public API, schema, runtime JVM dependency or broad semantic-completeness claim.

For the pinned Gson inputs, four original TypeAdapterTest calls retain their
complete prior results. A separately added diagnostic caller in the existing
`gson/main` compilation newly selects TypeAdapter.toJson(T) with a String actual;
the original 226 upstream files stay byte-for-byte unchanged. The corresponding
fromJson(String/Reader) family and its outer chain remain unproved, as does boxing
for TypeAdapter<Integer>. These are explicitly retained limitations, not hidden
by the new source-template capability.
