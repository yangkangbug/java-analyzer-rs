# Expression-qualified class creation (0.5.0)

The parser represents expressions such as `outer.new Inner(value)` with
`QualifiedNewExpr`, distinct from ordinary `NewExpr`. It preserves the
qualifying expression, optional explicit constructor type arguments, annotations,
the single member-class identifier and its type arguments or diamond, argument
list, and optional anonymous class body. Basic-for initialization/update lists
accept this class-creation statement expression. Ordinary class and array
creation keep their existing syntax.

This is structural support, **not constructor or result-type inference**.
The created member name is relative to an expression whose type is not proved:
it cannot fall back to an unrelated lexical class named `Inner`. Independent
type-argument names such as `String` in `outer.new Inner<String>()` retain their
ordinary type navigation. Qualifier and argument names likewise retain their
own contexts. Anonymous bodies use the existing private lexical-owner inventory;
their own locals/parameters/fields do not leak into the surrounding class.

A result such as `outer.new Inner().field` is refused rather than reinterpreting
`field` as a surrounding variable. This remains true through parentheses, casts,
index receivers, call-result chains, and conditional/assignment result branches.
A creation occurring only in an unrelated call argument or index operand does
not become receiver provenance. The provenance walk uses a bounded iterative
stack; exhausted work is unknown and conservatively refused, not proof of absence.
No new cache, platform-identity relation, inherited member-type lookup, or
invocation applicability proof is introduced.

## Scope and limitations

- No jump to the created member class or constructor is promised.
- No new qualified-creation receiver type is propagated to fields or methods.
- Generic bounds, static-inner legality, constructor applicability, and complete
  Java type validity are not certified merely because syntax is represented.
- Existing capture limitations remain. This does not implement inherited
  `Outer.Inner` lookup.
- Existing ordinary literal/cast/new/index receiver-erasure gaps, including
  `1..x`, remain separate work. They are not fixed by a new-node blacklist.
- The new expression has no independent fragment reparser. Existing enclosing
  block/body/file fallback and anonymous-class body reparsing remain available;
  local edit/fresh/undo/old-snapshot comparisons cover this boundary.

## Observed original-source evidence

The pinned Gson corpus has seven creation occurrences in
`FieldExclusionTest.java` and `ObjectTest.java`. Their 14 recovery diagnostics
disappear (10 and 4 respectively), changing the current Gson report count from
44 to 30. Commons remains at zero. Historical parser contracts and source hashes
are retained; the current entry applies an exact record delta, not a count-only
allowlist.

All 19 selected creation-context queries keep their prior results. Eleven
resolved qualifier/argument names match independently selected javac declaration
identifier ranges. Seven creator-name queries and one captured outer local
remain refused. This is not a claim of seven newly supported constructor jumps.

The existing 427-query comparison has four additional name-level results:
`org.junit.Assert.fail` calls elsewhere in `ObjectTest.java` become available
when that file's syntax errors disappear. Independent javac analysis agrees on
binary origin, method kind, owner, and name. Their native precision remains
`origin_kind_owner_name_only`: this is **not** descriptor-level or strict
overload/applicability verification. The other 423 results, the original 18
anonymous-owner targets and historical capture refusals stay unchanged.

A separate 848-query original complex-receiver/method-reference comparison is
unchanged, including the 14 previously confirmed cast-field declaration targets.
These observations are regression evidence, not whole-language completeness or
a performance claim.

## Migration from 0.4.x Git dependencies

`SyntaxKind::QualifiedNewExpr` is a new public enum variant appended after the
existing variants. Exhaustive downstream matches must add it or use an
appropriate fallback. This is a source-breaking API change under the project's
pre-1.0 version policy; the package version is now **0.5.0**.

The package remains `java-engine` (Rust import `java_engine`) and
`publish = false`. No crates.io release is implied. Git-dependent consumers
must update their pinned revision/lockfile intentionally, check any version
constraint such as `version = "0.4"`, and recompile. Syntax consumers must not
assume the new node has ordinary `NewExpr`'s leading-`new` layout, or derive
constructor/receiver binding from its presence. No public target schema changes
are introduced.
