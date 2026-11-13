# Source enum constants and opaque bodies

Source enum constants are indexed as public field navigation targets with the
enum's declared type. The target retains the exact source path, enum owner and
UTF-8 identifier range, including non-ASCII names. Qualified and ordinary
unqualified field lookup reuse existing ambiguity and local-shadowing rules.
Malformed constant subtrees do not publish a constant.

A constant-specific class body is not the enclosing enum's member inventory.
Both that body and anonymous class bodies inside constant arguments are opaque:
their fields, methods and types do not leak into the enum, and queries inside
them return Unresolved rather than borrowing bindings from the outer enum.
Ordinary argument expressions outside those bodies still use existing lookup.
Prepared snapshots and incremental edits recompute the same containment ranges.

## Access boundary

Implicit public constants do not bypass inaccessible source enum owners or
enclosing types. The bounded owner check covers public, package access, private
same-top-level nests, and implicitly public interface member types. Cross-package
protected nested-type access remains conservatively refused. This check applies
only to the newly modeled source enum constants; it is not a rewrite or a claim
of completeness for existing general member/type visibility.

## Deliberately not modeled

- Constant-specific anonymous subclass semantics or navigation inside its body.
- Enum constructor selection, synthetic values/valueOf methods, or enum generic
  superclass semantics.
- Switch-label binding, exhaustiveness, control flow, or enum initialization
  legality. Existing switch labels remain refused.
- General Java expression typing, static import semantics or global semantic
  diagnostics. Syntax recovery fixtures need not be valid Java programs.

## Evidence and reproduction

Native Cargo regressions cover exact constants, Unicode, nested/cross-package
owners, access refusals, malformed recovery, same-name ordinary members,
anonymous constructor-argument bodies, edits and prepared snapshots.
These tests start no JVM.

The optional compiler-only comparator is independent of runtime analysis:

```sh
cargo build --locked --release --example oracle_bindings
python3 scripts/check-source-enum-oracle.py \
  --jdk /path/to/pinned-jdk --jdk-archive /path/to/pinned-archive \
  --native target/release/examples/oracle_bindings
```

It uses 15 frozen queries: 11 exact native targets and 4 intentional refusals.
Compiler targets are checked for every query, including native refusals.
Three invalid sources must be rejected by javac. Six adversarial record
mutations, completeness checks, hashes and an optimized-Python rejection guard
protect the checker. Fixtures are compiled for comparison, never executed.
The optional extended-acceptance CI runs this check separately from native gates.
Offline Python tests verify its contract without Java.

The prior binary and frozen contract retain the actual before-results; use
`--baseline` only with its exact recorded binary hash. A baseline pass is not a
candidate pass. Previously failed fixture/freezing logs are preserved in local
evidence, not overwritten or counted as successful runs.

The representation follows the Java distinction between an enum's implicit
constant fields and constant-specific class bodies. No IntelliJ implementation
or fixtures were copied. These bounded navigation improvements do not establish
full enum conformance or project-wide performance claims.
