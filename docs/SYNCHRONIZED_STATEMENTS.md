# Synchronized statements

The parser represents synchronized (expression) { statements } as a distinct
SynchronizedStmt node. Its required body is an ordinary Block, using existing
lexical scope: body declarations are not visible in the monitor expression or
after the block. Existing ordinary name/call navigation applies in valid contexts.
Synchronized method modifiers remain unchanged.

This adds syntax and navigation, not locking/concurrency analysis or monitor-type
diagnostics. It does not add instanceof pattern-variable flow scopes. String.java
still contains unsupported type patterns; this change alone does not enable its
source-attachment display.

## Incomplete edits

Missing/invalid headers and missing required blocks are marked invalid. A missing
block may recover at most one following statement beneath the invalid node, never
a synthetic block extending across the rest of the method. Enclosing else/catch/
finally/case/default and closing braces remain available to their owners.
Invalid nodes cannot export local declarations or produce reference/call targets.
Valid block contents use existing brace-block reparsing; edit/fresh and undo tests
check both syntax and ordinary name navigation.

## 0.2.0 migration: public enum addition

This pre-1.0 source-breaking release adds SyntaxKind::SynchronizedStmt to the public
exhaustive enum. Downstream exhaustive matches must add a case (or a deliberate
fallback where appropriate). Visitors should traverse its monitor expression and
Block as statement contents, not treat it as a declaration or independent scope.

The variant is appended, but numeric discriminants are not a serialization or ABI
contract. No public struct fields or method signatures change. The package remains
publish=false; this version change documents the Git dependency migration rather
than a crates.io publication.

## Acceptance evidence and current contracts

The fixed Gson input contains nine synchronized statements in five production
files. This slice removes their 18 prior parser reports (132 to 114 reports);
the fixed Commons input has five synchronized statements in four production files,
removing 10 prior reports (56 to 46). All other per-file diagnostic records and
all source hashes stay exact.
This is corpus evidence, not a Java-conformance percentage.

Non-historical acceptance uses the stable latest entry in
scripts/current-parser-contract.py. Historical switch/label contracts remain
unchanged and explicitly selectable. Future syntax changes update this one
current entry rather than proliferating named runtime stages in every caller.
The current delta first verifies exact previous messages, ownership and byte/UTF-16
ranges; it does not filter arbitrary diagnostic text or weaken inventory checks.
