# Explicit reference-array elements

An explicitly declared one-dimensional plain reference array local or parameter
can now supply its component type through `values[index]`. The same expression
proof feeds strict invocation arguments, direct method receivers and nested call-result
composition. For example, `items[i].get()` and `sink.take(items[i])` need no
special rule for the component class or library.

## Safety boundary

Strict reference declaration metadata preserves rank separately from its nominal
class name. Prefix brackets and brackets belonging to each individual declarator
are checked, including mixed scalar/array siblings. Scalar reference lookup still
requires rank zero. Only a proved index into rank one produces a scalar reference;
`A[]` is never interpreted as `A` merely because its base class name is known.

The array base must be a visible, unique explicit local/parameter declaration.
The component uses its declaration's compilation/import context and existing
access, identity and complete hierarchy proof. The index must have an already
proved scalar byte, short, char or int type. The existing primitive array operand
structure check is shared, and all recursive work uses the existing proof budget.
No new public type API, parser stage or runtime dependency is introduced.

[JLS 15.10.3](https://docs.oracle.com/javase/specs/jls/se25/html/jls-15.html#jls-15.10.3)
requires an array base and an index promotable to int; the expression has the
component type after capture conversion. This slice deliberately excludes capture
and unboxing. [JLS 10.2](https://docs.oracle.com/javase/specs/jls/se25/html/jls-10.html#jls-10.2)
accounts for dimensions on both the common type and each variable declarator.

Still unsupported here: generic or annotated component types, type-variable
components, varargs declarations, multidimensional reference indexing, field or
method-result arrays, new-array expression typing, array methods/length, array
argument conversion and covariance. There is no bounds, nullness or initializer
inference. An initialized or null-filled array does not change its declared
component type. Existing caller-context and overload-selection limits remain.

## Validation

Eight native groups cover exact source/binary targets, strict arguments and
composed calls, prefix/postfix dimensions and sibling declarators, scalar/array
separation, invalid/unsupported indices, type identity, scopes, catches and budgets.
A prior `A[] nulls` regression retains the same source and now selects `pick(A)`
for `nulls[0]`; this is component-type evidence, not a null literal shortcut.
An optional javac comparison checks six exact source targets and five invalid
controls. Native tests and the library never invoke Java.

On unchanged pinned Commons sources, seven ArrayUtils wrapper-array value calls
and three StringUtils String-array length calls resolve to exact platform targets.
An isolated javac check of the same expressions under their explicit array/index
declarations confirms owner/name/descriptor. This is not a complete Commons build
or a project-wide coverage measurement. Full native records retain platform
origin and linking scope. Local evidence is kept separately from repository tests.

The prior 577-query regression set has two additional independently checked
successes (Object-array equals argument and toString receiver) and one refusal
classification change for a still-unsupported varargs receiver. All other full
records, including the separate 30 parameterized-receiver queries, are unchanged.
