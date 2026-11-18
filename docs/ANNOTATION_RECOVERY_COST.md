# Nameless annotation-prefix recovery cost

A statement sequence can reuse an already failed classification for consecutive
nameless `@` tokens. It still invokes the original recovery once per token,
retaining individual error nodes, text, trivia and ranges. The last `@` is
classified normally. A preceding dot excludes reuse because local-type lookahead
depends on that context.

All old annotation/modifier lookahead and declaration/container decisions remain
unchanged. Only `parse_stmts` consumes the private replay-eligibility result;
single-statement if/for/while/label callers do not. The result is not parse
success. This introduces no token index, memoization, budget or API change.

The first attempted fail-closed annotation lookahead implementation was rejected:
it changed malformed for/lambda containment and could expose header names as
outer locals. It is not part of this change.

## Evidence and limits

The original 11,000-token CLI output-limit fixture is unchanged. On one Debian
host, serial same-debug-profile runs changed approximately 7.97 seconds to
0.023 seconds while retaining `output_limit`, empty stdout and every error node.
This is local diagnostic evidence, not an ARM performance guarantee.

Independent checks compare 300 context/prefix cases, 17 earlier cases and both
for/lambda regressions using complete CST and target records; all are identical.
The pinned 460 original Java files produce identical 973,270-line CST dumps
including diagnostics/trivia. All 1,297 existing full query records are unchanged.
Focused edits compare fresh parsing, undo and retained snapshots.

This is a bounded grammar-specific recovery optimization, not globally linear
parsing. Long named annotation prefixes without declarations and repeated
unclosed named annotation arguments retain their existing complexity.
Timeouts, output budgets, test assertions and fixture sizes are not relaxed.
The historical ARM failure at commit 529fb28 remains a failure; a new commit must
be executed and checked on ARM before claiming it is fixed there.

Evidence: `evidence/annotation-sequence-recovery-20261118` and
`evidence/annotation-lookahead-independent-20261118/SEQUENCE_REVIEW.md`.
