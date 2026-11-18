# Explicit array invocation types

Existing fixed-arity invocation selection now retains array component identity,
rank and declaration linking context for explicit local/parameter arguments and
source/binary formals. Arrays are private type evidence, not synthetic classes or
navigation targets. Version 0.7 and public APIs are unchanged.

## Shared semantics

The same structural array relation serves strict/loose applicability and
most-specific comparison. Primitive elements are invariant; reference elements
use existing declaration conversion proofs. Rank is significant: int[][] can
convert to Object[], while int[] cannot. Null is applicable to proven array
formals. Array components are never boxed/unboxed, and varargs expansion remains
unsupported. Existing primitive-only call proofs retain their narrower identical
primitive-array rule.

The full method family is prepared before selection. Array parameters are not
discarded because another candidate fits. Unknown components and generic template
arrays still prevent proof. Duplicate signatures and inherited comparisons keep
rank and component identity; cross-compilation compatibility does not become
declaration equality or justify hiding a competitor.

## Platform and ownership

Array Object/Cloneable/Serializable conversions use exact configured binary
declarations in the array's declaration linking context. Object must be an
ordinary root with no parent edges; markers must have the standard interface
shape and no extra superinterfaces. Source substitutes, missing or ambiguous
declarations, generic or nonstandard marker shapes remain unknown when needed.
Cross-context compatibility uses existing complete binary closure evidence.

Evidence is demand-driven. A positive Object conversion does not eagerly load
both markers. A negative conversion to an unrelated interface requires both
standard marker shapes, so unexpected marker ancestry cannot be silently ignored.
This is modeled platform evidence, not authentication of a JDK vendor.

All lookups, component graphs, structural rank steps and comparisons use the
existing query budget. No budget increase, cache, global canonicalization or new
ancestor lookup is introduced.

## Boundaries

Initial actual arrays are explicit local/parameter names with transparent
parentheses. Fields, captures, inferred values, new/cast/index/call-result roots,
generic components, local types and array return propagation are not newly
admitted. Runtime non-nullness, store compatibility and execution are not proved.
Ordinary reference components use existing source/binary name/access/graph
proofs, not a new nominal resolver.

The shared source method-header check now distinguishes parameter/body array
brackets from invalid void-result dimensions. Direct malformed headers remain
refused. Consequently some valid non-array invocations in array-using methods
also recover; this is not a corpus-specific exception.

## Validation

The original 167-query public-source survey adds 37 independently compiler-exact
targets: 32 System.arraycopy calls, two source isMatch calls, and three JUnit
assertEquals calls. They comprise 35 exact binary descriptors and two exact source
name ranges. Original primitive boolean[] and int[] arraycopy witnesses improve;
the original generic T[] witness remains refused. Plain reference-array behavior
is covered by independent fixtures, not advertised as a new original-source gain.

The prior 1,297 complete query records and independent 3,393 type-parameter/94
local-type records remain unchanged. Binary/source fixtures cover null,
covariance, multidimensional relations, primitive invariance, complete unknown
competitors, platform faults, cross-scope identity, precise budget boundaries and
edit/fresh/retained snapshots. Historical refusal tests now assert exact known
array-family outcomes while preserving unknown-component and invalid-flag cases.
The inherited-signature test explicitly separates compatible cross-scope arrays
from identity and proves rank/primitive differences cannot be lost.

Evidence: evidence/array-invocation-20261118 and
evidence/array-invocation-independent-20261118. This increment is bounded Java
array invocation support, not a full Java type system.

Historical Oracle inputs remain byte-for-byte pinned. Existing current projections
advance only compiler-confirmed strict_array, null_array and the two linking
contexts of arrayFormal([I)I), plus the original Commons q07 isEmpty(char[])
source target (129587..129594). The original nine q07 golden candidates remain
pinned in both Rust and CLI acceptance; only the current result selects char[]. Exact previous rows, source name ranges or binary
descriptor/context records are asserted; mutation tests reject altered identities.
