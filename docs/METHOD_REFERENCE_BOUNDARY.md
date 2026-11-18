# Method-reference terminal boundary

Method references do not use the ordinary field or local-variable namespace.
The engine does not yet prove method-reference applicability and selection, so
a terminal selector such as `obj::m`, `this::m`, `Type::m` or `(obj)::m`
now refuses with ReceiverNotBound instead of borrowing a same-named field,
local or parameter. The constructor keyword in `Type::new` retains
ConstructorRefNotBound.

Receiver and type prefixes are still visited independently before terminal
dispatch. Existing supported prefix targets remain available; this does not add
method-reference type/value disambiguation. For example, bare `obj::m` may still
have an unresolved prefix where the parser represents it as a type name, while
the reference inside `(obj)::m` retains its existing parameter navigation.
Punctuation, comments and whitespace do not produce terminal targets.

No public API shape, parser, field-chain extraction or receiver-type proof is
changed. This is a correctness restriction, not new method-reference resolution.
Valid Java method references remain unsupported at their terminal selector.

## Evidence and compatibility

Native collision controls cover same-named fields, locals and parameters,
constructor references, explicit type arguments, token positions and local
edit/fresh/undo/old-snapshot behavior. Independent compiler controls select
methods where the old engine borrowed non-method declarations.

The unchanged real-source census covers 553 Gson and 295 Commons selectors,
including 81 method references. Gson results are unchanged. Commons has one
incorrect parameter target removed at `StringJoiner::toString`; 77 other changes
only normalize refusal labels with empty target sets. Fourteen compiler-confirmed
cast-field targets are unchanged. Existing 427 complete results, 18 anonymous
targets and capture refusals also remain unchanged in their comparison sets.

## Deliberately separate receiver work

The broader receiver-chain prototype is not part of this change. Literal and
complex field receivers can still lose their receiver context, including the
known `1..x` case and the original `((CEO) result).assistant` wrong-local target.
Fixing those paths without retracting correct cast-field navigation requires
separate receiver and field evidence work. This terminal fix neither adds a
literal blacklist nor claims those remaining errors are solved.
