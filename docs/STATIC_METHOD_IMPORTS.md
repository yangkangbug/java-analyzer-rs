# Explicit static method import navigation

Direct calls with an explicit single static import can use the existing strict
method selector. For example, `import static org.junit.Assert.fail;` followed
by `fail()` may resolve to the same binary declaration as `Assert.fail()`.

## Evidence and ordering

This is a direct-callee navigation extension, not a second overload algorithm.
Existing lexical method lookup retains priority. If it finds nothing, a bounded
complete ancestor graph must prove absence of **every raw same-named method**.
Missing ancestors, cycles, duplicate identities, inaccessible declarations and
unsupported members cannot be discarded to manufacture that absence.

All static imports are scanned under the same work budget used for ancestor
closure and selection. Repeated identical imports are allowed; different owners
for the queried member, or any static on-demand import, refuse this new path.
An owner is looked up by its canonical name, not by expression receiver rules:
locals and ordinary imported types cannot reinterpret the import. Its unique
top-level package/simple name and raw binary name must match the written name.
Normal access and complete graph checks still apply.

The existing TypeStatic selector retains its whole candidate family. Only the
final winner can be static. This is deliberately more conservative than Java
static-import overload lookup when instance methods have the same name: an
instance competitor can cause refusal even though javac chooses an imported
static method. No instance member becomes an imported result.

## Scope and remaining refusals

- Ordinary existing direct caller contexts only: top-level nongeneric classes,
  supported method/constructor bodies, valid syntax and declaration inventory.
- Public own methods of accessible top-level source/binary classes, with the
  selector's existing supported arguments, access and nongeneric signatures.
- No inherited static families, interface or nested/default-package owners,
  on-demand imports, or competing explicit owners.
- No imported fields/types, import-token navigation, or imported call-result
  evidence for an enclosing expression. An inner call can independently navigate
  without making `pick(pick(1))` a proven outer call.
- Existing unsupported generic, variable-arity, unknown argument and incomplete
  caller cases remain refusals. A resolved declaration is not whole-file Java
  validity or full static-import support.

Source edits rebuild import records with the file index. Prepared old versions
retain their own records. No public API or JVM runtime dependency is introduced.

## Validation

Native tests cover exact source/binary targets, repeated imports, lexical
method precedence, value/method namespaces, missing and cyclic ancestors,
missing or competing imports, canonical names, access, unsupported competitors,
mixed static/instance families, edits and separate compilation identities.
An internal proof test checks complete ancestor absence and the single shared
discovery/selection budget. Optional javac controls remain development evidence,
not a requirement of the analysis runtime.

A local diagnostic over pinned unmodified Gson sources found actual test calls
benefiting from this path, alongside unchanged refusals. Such selected probes
are not a whole-project navigation success rate or full project acceptance.
