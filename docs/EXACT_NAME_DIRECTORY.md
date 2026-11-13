# Exact-name directory reuse

The prepared binder already owns a scope-qualified directory from dotted class
names to declaration locations. Legacy exact-name lookup now uses that directory
instead of scanning every visible source and platform class and rebuilding its
dotted spelling. This changes lookup mechanics, not Java support.

The directory returns every matching source declaration in original file/class
order, followed by binaries in the querying scope's original order. Invisible
sources and dependency-scope binaries remain excluded. Duplicate declarations,
different origins and legacy dotted-name aliases remain separate candidates.
An index hit is not a resolution proof; callers retain their ambiguity, import,
member, accessibility and unsupported-case decisions.

Only explicit single-import lookup, qualified type-prefix lookup, multi-part type
lookup and named ancestor lookup use this path. Same-package, wildcard import,
package-prefix and nested-member existence scans are unchanged. The stricter
invocation proof keeps its existing lookup and work/depth accounting.

There is no new cache, public API, persistent identity, incremental invalidation
or semantic fast path. Directory locations belong to one immutable Index.
Preparing another snapshot rebuilds its directory; old prepared handles remain
bound to their old inputs.

## Verification

Internal differential tests compare actual declaration identities and order with
the prior complete scan, including visibility, duplicates, nested-name aliases
and context changes. Whole-project and compiler-oracle gates must retain their
existing exact records and refusal boundaries. Timing observations separate
preparation, legacy rebuild-per-query and prepared repeated queries; they are
not complete editor latency, memory accounting or cross-engine comparisons.

## IDEA reference

At IntelliJ commit `840d3be11dfdeada533d05052008b48816b6cc4e`,
[JavaPsiFacadeImpl](https://github.com/JetBrains/intellij-community/blob/840d3be11dfdeada533d05052008b48816b6cc4e/java/java-psi-impl/src/com/intellij/psi/impl/JavaPsiFacadeImpl.java)
separates scope-qualified class lookup from downstream resolution and distinguishes
finding one class from collecting all matching classes. This slice follows that
separation, not its weak caches, extension finders or first-class selection policy.
No IntelliJ implementation is copied.

## Observed fixed-workload measurement

On the Debian Linux x86_64 development host, release binaries before and after
this change ran the same existing Commons Lang corpus (249 original sources,
7,615 platform classes) and all 45 frozen name-binding queries, five rounds per
sample. Six serial samples used order baseline/candidate/candidate/baseline/
baseline/candidate, without concurrent builds or tests. Every invocation checked
the fixed targets and exact prepared/legacy result equality outside query timers.

For 225 prepared queries per sample, baseline totals were 1.484–1.489 seconds;
candidate totals were 52.9–54.2 milliseconds. Preparation remained approximately
104.7–105.3 milliseconds in both. Rebuild-per-query totals were approximately
23.37–23.39 seconds before and 22.58–22.60 seconds after. These are single-host
observations of a fixed mixed query workload, not a general speed multiplier,
new semantic coverage, cold-cache experiment or editor-latency promise. Remaining
collection scans and per-query syntax traversal are not eliminated.

The baseline is source commit 1a73d47f (local equivalent 2136eb9). Binary hashes:
- baseline real_source: c47d34404c037cc018d39ac3997feb770cc848db77a4675cfd1158bff1c5ac54
- candidate real_source: 09ada2b9bea5b09a486486a15b67b6be3d6246bde3802c47d1296a63917348df

The existing `scripts/check-real-source.py` verifies source/platform hashes and
runs the release example with no Java environment. Build each revision separately
and run it with `--profile release --rounds 5`; alternate revisions serially.
Retain preparation and query phases separately rather than timing the whole
checker as though it were warm lookup. Existing full Commons and four-compilation
Gson acceptance contracts remain the correctness criteria.
