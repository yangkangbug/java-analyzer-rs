# Method signature identity: native navigation foundation

Source spellings and JVM descriptors are different representations of method
parameters. Comparing only spelling can merge unrelated imported types; refusing
every mixed representation prevents navigation to ordinary source overrides of
library methods. This slice normalizes supported parameter types before deciding
whether one declaration hides another.

## Type identity and context

- Primitive types have distinct primitive identities. A class-file reference
  descriptor remains a declared type even if its class name resembles a primitive.
- Arrays retain their rank. Prefix/suffix dimensions and varargs are normalized
  without borrowing brackets from type arguments or initializer expressions.
- Declared types resolve where the method was declared: source imports, enclosing
  types and compilation visibility, or the binary owner's linking context.
- Class identity includes owning compilation, origin, qualified name and source
  declaration range. It is private, snapshot-local identity, not a stable public
  symbol ID. Identical names linked against different classpaths are not merged.
- Ambiguous/missing types, type variables, parameterized types and unsupported
  generic signatures do not become equality merely because their spelling matches.
  Inherited member-type and generic contexts remain conservative when not modeled.

JVM descriptors are parsed completely and validated before their return component
is excluded from signature comparison. Java method signatures do not include the
return type. This is not return-type compatibility or invocation type checking.

## Comparing and hiding declarations

Supported parameter lists can be equal, provably different or not comparable.
Different arity or a known different parameter can prove distinctness even when
another parameter is unknown. This does not choose an overload from arguments.
Uncertain equality retains `SignatureNotCompared` and its candidates.

Equal signatures hide a declaration only through a justified declaring-type
relationship, not because an unrelated interface happens to be farther away in
the traversal. An interface's synthetic Object superclass is not such an override
edge. Full class/interface/default-method precedence is not implemented; unresolved
conflicts remain explicit rather than selecting an interface incorrectly.

Same-owner duplicate parameter signatures, including bridge/covariant-return
variants, are not distinguished by return type to manufacture separate overloads.
They can remain `Ambiguous`; bridge selection is not implemented.

A lone syntactically valid method may still be a `Resolved` name-navigation target
with unknown parameter types. This does not prove applicability. Malformed source
signatures or invalid descriptors remain input errors or explicit refusals.
The separate [primitive call proof](PRIMITIVE_CALLS.md) can now select supported
source overloads and refuse singleton invocation contradictions/unknowns. It does
not change the distinction between declaration identity and call applicability.

## API and execution boundaries

`ClassSignature.is_interface` records the actual class-file `ACC_INTERFACE` flag.
Code manually constructing a `ClassSignature` struct literal must supply this new
field. This signature-identity change preserved resolve statuses/target fields;
the later primitive-call slice adds `ResolveStatus::CallNotResolved`, requiring
exhaustive client matches to add a case. Public target fields remain unchanged.

The library and normal Rust tests do not require Java. Native tests construct
signature-only class bytes. A separate optional compiler oracle can compile
handwritten dependency classes into a temporary directory and compare consumer
source overrides against those real bytes. Fixture programs are never executed.
Compiler-selected binary targets are not represented as fake source declarations.

This is one foundation for project definition/hover and later type services.
Generic substitution, general overload applicability, static/instance legality, complete
access rules, stable symbol IDs, incremental invalidation, references, diagnostics,
completion and refactoring remain separate milestones. Normalization is currently
performed during lookup; prepared handles are not a new semantic-result cache.
