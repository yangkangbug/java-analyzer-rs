# Non-pattern switch syntax and declaration navigation

Version 0.8.0 adds lossless SwitchExpr, SwitchRule and YieldStmt syntax. This is a
bounded syntax and declaration-navigation slice, not switch type checking.

## Supported structure

Colon-label and arrow-rule switch statements and expressions accept constant-label
syntax, including comma-separated labels. All labels in a switch must use the same
delimiter style. Arrow bodies are a block, throw statement, or semicolon-terminated
expression. In a statement switch the last form must be a statement expression
(call, class creation, assignment, increment or decrement), not a bare value.
Constant evaluation is not performed. Null, pattern and guarded labels remain
opaque unsupported syntax. Selector, label and rule-delimiter commit failures
roll back the entire switch; ordinary errors inside a parsed block retain their
diagnostics and recovery structure.

Yield is contextual. A YieldStmt belongs to the nearest surrounding SwitchExpr;
an intervening lambda, callable or type body prevents capturing an outer switch.
A nested statement switch does not prevent yielding to an outer expression.
Ordinary variable/declaration names such as int yield, yield = n and yield++
retain identifier tokens and lexical navigation. Unqualified yield(...) outside
a switch expression keeps a diagnostic, whereas this.yield(...) remains a
qualified call. Restricted type-name roles do not gain a type named yield.
No new inference is provided for a contextual identifier used in strict proofs.

## Scope and value boundaries

SwitchBody retains a shared lexical scope for colon groups. Each explicit arrow
Block has its normal separate scope; locals cannot leak into sibling rules or
past the switch. Labels remain non-binding: clicking a label does not borrow an
ordinary same-named local or field. Selector, body and yield operands retain
ordinary supported declaration navigation; the yield keyword itself has no target.

SwitchExpr results remain unknown in primitive/reference value proofs. No arm
type, common type, overload conversion, exhaustiveness, duplicate-value validation,
definite assignment, reachability or completion rule is inferred. An empty rule
block may therefore parse even when javac rejects it for failing to yield.

The existing per-callable switch inventory now includes expressions as well as
statements. Parsing success does not authorize stricter invocation flow proofs.
An unrelated method can recover from old whole-file parser errors without making
a switch-containing caller newly eligible.

The existing bounded unsupported-result receiver check explicitly includes
SwitchExpr alongside qualified creation. A switch result under parentheses,
casts, calls, indices, assignment RHS or conditional value branches cannot
disappear and borrow a lexical member. Call arguments and index operands are
not receiver provenance; their presence does not alone taint an unrelated base.
This does not introduce general receiver typing, change unknown field-chain
fallbacks or add ancestor-member inference.

## Incremental parsing

No descendant of SwitchExpr or SwitchRule is independently reparsed. Edits
recheck the nearest enclosing context-independent body or the whole file. This
retains yield context and rechecks a rule's structural commit boundary.
Traditional colon statement blocks retain their existing independent path.
There is no hidden fragment parser flag, extra result cache or new snapshot API.

## Compatibility

Cargo remains publish=false; this is a Git-dependency migration from 0.7.x to
0.8.0. Exhaustive downstream matches on public SyntaxKind must add SwitchExpr,
SwitchRule and YieldStmt arms. Existing variants and their order are unchanged;
the three variants are appended. These nodes are not independent reparse roots.
CLI/LSP schemas and whole-file source-attachment diagnostic gates are unchanged.

## Verification boundaries

Native tests cover grammar modes, label isolation, shared/separate scopes,
contextual yield and callable barriers, unsupported-result receiver refusal,
operand non-contamination, retained overload candidates and Unicode incremental
edit/undo equality with immutable old snapshots. Historical traditional-switch
tests retain unsupported pattern/null/guard controls and now assert exact local
navigation in expressions that this slice supports.

Optional compiler fixtures are separate from the native runtime. Passing syntax
tests is not Java semantic conformance or a performance claim. Real-source
attachment benefits must be checked by exact descriptor/source range and UTF-16
navigation, not inferred solely from a reduced parser-report count.

## Observed fixed-source verification

With the supplied fixed JDK source, Math's two switch-expression parser reports
become zero without altering the source-attachment diagnostic gate. Twelve
already-selected min/max/abs overloads retain exact primitive JVM descriptors
and now navigate to their exact source-name ranges. Twenty-four LSP checks
cover those overloads before and after a same-session emoji/CRLF source refresh,
including UTF-16 coordinates. The source association remains explicit_unverified;
this does not prove that every JDK version has identical source.

The existing complete 1,297-query source comparison is unchanged, including all
14 independently compiler-confirmed cast-field declaration targets. These are
bounded regression observations, not complete Java semantic support.
