# Final-prefixed local declaration recovery

In local declaration positions, a leading `final` commits to the existing local
declaration parser even if strict type lookahead fails. For example,
`final p.C<,> C=null` retains the actual declaration name `C`. Its damaged
type supplies no receiver-type fact, so a later `C.x` cannot silently borrow an
imported class named `C`.

This reuses existing type and declarator recovery. It does not guess a missing
name or add a second type parser. Local types retain their separate path;
invalid labeled declarations remain rejected. General type lookahead,
cast probes, catch/header recovery probes and resource parsing are unchanged.
The published [enhanced-for scope](ENHANCED_FOR_SCOPE.md) boundary keeps a recovered
enhanced-for variable out of its iterable expression and limits it to the actual body.

Existing error recovery can finish a malformed annotation-like prefix and reach
a subsequent real `final` token. Such a declaration can now be recovered too,
with diagnostics retained. This is not complete annotation syntax support or
annotation-skipping admission. Tested internal annotation-argument `final`
tokens do not export a declaration; unknown malformed contexts are not generally
solved.

Missing type/name and ordinary declarations without leading `final` remain
separate recovery gaps. In particular, `p.C<,> C=null` may still fail to form
a declaration and allow the old erroneous import fallback. Navigation of a
recovered name does not certify compilation legality or infer its type.

## Validation boundary

Thirteen focused tests cover actual name ranges, withheld type facts, the prior
enhanced-for regression, labels/local types, malformed arrays, multiple names,
missing delimiters and following declarations, annotation recovery, and
incremental fresh/undo/old-snapshot agreement. A 14-case javac matrix independently
checks legal and illegal syntax; fixture programs are not executed.

The original 427 and 848 navigation results, 18 anonymous targets and historical
capture refusals are unchanged. Gson keeps 30 parser reports and Commons 0.
This is damaged-editor-state correctness, not a demonstrated new target in those
original projects. No public API, version or parser contract change is required.
