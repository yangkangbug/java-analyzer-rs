# Bounded reference instance-call selection

This is a declaration-navigation proof for explicit reference values, not complete
Java overload resolution or semantic diagnostics. It complements the
[primitive-call proof](PRIMITIVE_CALLS.md). Production parsing, type lookup,
class/JAR/JMOD reading, conversions and selection run in Rust without a JVM.
Only the optional development comparator executes Java.

## Positive boundary

A direct qualified invocation can select an exact source declaration when all
of the following are proved:

- The receiver is a local or parameter with an explicit, nongeneric, non-array,
  top-level source class type. Parentheses around a supported receiver or argument
  are allowed. Types come from declarations, not initializers or legacy navigation.
- Arguments additionally support plain `new Type()` for an accessible nongeneric
  top-level concrete source class. Its complete declaration inventory must contain
  exactly one correctly named, public, nongeneric, zero-parameter constructor with
  a valid header, real body, no parser errors and no throws clause. Constructor
  annotations are permitted. A same-named method is not constructor evidence.
  This checks construction headers and argument identity, not constructor-body
  semantics, implicit super calls or whole-program instantiation validity.
  Implicit defaults, additional constructors and throws clauses are conservative
  exclusions for this slice, not claims that those Java constructs are invalid.
- Arguments also support ordinary closed string literals with validated simple and
  octal escapes, including `\s`. They use the exact supplied public final,
  nongeneric, top-level binary `java.lang.String`, independently of lexical names
  or imports. Its complete ancestry and native-readable Object root are required.
  Literal byte inspection consumes the shared proof budget. Text blocks, any raw
  backslash-u substring, malformed escapes and physical line breaks are refused;
  Unicode preprocessing is not implemented by this slice. These are conservative
  syntax limits, not claims that every refused Java string is invalid.
- The call belongs to a supported direct, nongeneric source method. Unsupported
  scopes, local-type declarations and parser errors in that caller cannot provide
  the proof. This is not general expression typing or complete Java scope analysis.
- Every same-name declaration is considered before access filtering. Selection
  requires direct, public, nongeneric, fixed-arity source instance methods and
  supported scalar reference formal types. Any inherited same-name, static,
  generic or varargs competitor prevents selection in this slice.
- Class and formal identities are resolved in their owning compilation/file and
  declaration context. Both declaration-site and use-site access are checked.
  Nested type access and protected member rules are conservatively unsupported.
- The relevant inheritance inventories are complete and unambiguous, terminating
  at supplied native-readable binary Object data. Missing parents are unknown, not proof
  that no competitor exists. Cycles, duplicate/illegal edges and unsupported or
  malformed declaration headers cannot establish a target.
- Strict identity/reference widening makes an applicable candidate, and exactly
  one formal vector is most specific. There is no declaration-order or numeric
  conversion-score tie-breaker.

Source and binary nongeneric types can be formal endpoints. Conversion follows
only proved nongeneric edges. A raw generic ancestor inventory may establish
that it contributes no same-name method, as with JsonArray's Iterable ancestor;
it does not perform substitution or generic conversion. Binary receiver/method
selection, overrides and default-method merging remain outside this proof.

Actual JDK String has a generic Signature attribute. Existing binary edge
handling conservatively marks all its edges as lacking plain conversion evidence.
The string slice therefore proves exact String formal identity on that platform,
including mixed argument lists, but does not establish String-to-Object widening
or resolve String/Object and String/unrelated overload sets. Supplying literal
type evidence does not weaken that existing conversion boundary. Final class-file
metadata is a prerequisite, not JVM bytecode verification or proof that every
class-file flag combination is legal.

Binary identities otherwise remain compilation-specific. Only a validated string
literal may cross to an exact String formal in another compilation after both
complete ancestry inventories match one-to-one as binary inputs, with the same
origins and all retained class-file signature metadata. Source replacements,
different origins or different/missing ancestor evidence refuse. This does not
merge ordinary parameter/local types across compilations or enable cross-scope
widening. Shared immutable signature records avoid repeated equality scans;
nonshared equality checks must fit the same bounded proof budget.



The private proof has a shared 32-level/4,096-work-unit budget for lookup,
hierarchy/member discovery, arguments and selection. Exceeding it refuses rather
than using unique-name success. These are conservative analysis limits, not Java
language limits, latency guarantees or bounds on parsing/index construction.
Constructor inventory and retained constructor source bytes also charge this same
work budget; even a large otherwise-valid constructor may be refused.
Private name-location indexes avoid repeatedly scanning every platform class;
they are not persistent symbol IDs or incremental query caches.

## Refusals and compatibility

Within the guarded local/parameter source invocation path, an incomplete proof
cannot fall back to unique-name success. CallNotResolved retains a single
candidate as a hint, not a selected declaration. Overload and unsupported
signature results preserve discovered candidate identities, including inherited
and inaccessible competitors. A bounded discovery failure can return no or only
discovered candidates; it must not claim a complete inventory or select one.

Qualified source guards also refuse a known wrong fixed arity and an instance
method invoked through a modeled type qualifier, without requiring Object data.
They do not infer a literal's type merely to count arguments. Parenthesized
receivers cannot turn the selector into an unrelated field name. These guards do
not cover arbitrary field receivers, binary calls, new-expression receivers or call chains.
Known type-variable and inherited member-type shadows also refuse rather than
choosing an unrelated top-level type. A surviving static-qualified Resolved
result is still legacy name navigation, not static-call applicability proof;
missing Object alone does not create a new refusal for that legacy mode.

One intentional compatibility tightening is recorded in the independent oracle:
the original no-platform Value parameter's read() call now retains its exact
candidate as CallNotResolved because Object's inventory is absent. All original
fixture bytes and goldens remain unchanged, with one explicit correction for
that input. Supplying pinned Object data makes the full original 63-query batch
pass its original expectations again. Ordinary field queries, method references
and the separately bounded primitive proof retain their own contracts.
The native namespace regression likewise retains the exact receiver.m(1)
candidate as CallNotResolved when its package method, literal argument and
missing Object fall outside the reference proof; its other namespace assertions
remain unchanged.

Legacy navigation paths still exist outside these guarded calls. Resolved is not
a universal argument-applicability or Java-validity guarantee. No definite
assignment, checked-exception, return-body or whole-program validity analysis is
added. The CLI schema stays at version 1 and advertises the additive limited
capabilities source_reference_instance_calls and qualified_source_contradiction_guard.
General applicability and semantic diagnostics remain false.

ClassSignature now retains the class-file ACC_FINAL bit as public is_final
metadata so illegal extension of a final binary class cannot prove widening.
Consumers constructing that public Rust struct with a literal must initialize
the added field. Class-file reading still does not execute or load Java code.

## Evidence and remaining gaps

The [handwritten compiler oracle](../dev-tools/oracle/reference-v1/README.md)
compares actual javac Trees declaration identities with exact native source
targets: 15 selections (13 newly selectable and two controls), nine valid but
unsupported cases, and 11 independently compiler-invalid cases. Mutation,
completeness and omitted-dependency controls are separate from the native proof.

The [additive pinned Gson gate](../examples/second_project/REFERENCE_CALLS.md)
checks two JsonArrayTest add(a) calls, where a is explicitly JsonPrimitive,
against JsonArray.add(JsonElement). It retains the original 16 queries, five
local-type queries, all 226 upstream source hashes, and the complete parser-report
record hash. No source is repaired or generated to manufacture success. The
manual project lacks generated GsonBuildConfig, so this is not whole-Gson javac
equivalence or compatibility certification.

Native LSP subprocess tests independently exercise exact overload switching across
two dirty files, CRLF/astral UTF-16 ranges, missing-parent refusal, old/equal
versions and repair. A construction-argument regression additionally changes an unsaved
constructor from public to private and its class from concrete to abstract,
verifies refusal and repair, and preserves dirty-target UTF-16 ranges.
An ordinary-string regression changes unsaved literals and formal types, checks
refusal/repair and stale versions, and keeps exact dirty UTF-16 target ranges.
They are protocol evidence, not OpenIDEA compatibility.

The separate [string-literal oracle](../dev-tools/oracle/string-v1/README.md)
uses independently expected declaration markers and actual checksum-verified JDK
String signatures. The [pinned Gson string gate](../examples/second_project/STRING_CALLS.md)
adds exact queries without changing upstream source, historical queries, or parser
records. Neither gate claims whole-Gson compilation or general overload coverage.
The separate real Neovim gate remains a client regression check.

Beyond the explicit zero-argument construction boundary above, new-expression
typing remains unsupported: no new receivers, enclosing-instance construction,
arrays, anonymous classes, diamond or constructor type/argument inference.
Null and non-string literal typing, field arguments, generic inference, general
call results, boxing/unboxing, expanded varargs, nested-class types and inherited
or binary overload selection remain unsupported. Real generic fromJson and
chained-call cases are not solved. The next integration milestone is safe
OpenIDEA client stale-result protection and actual end-to-end validation, not
replacing its build/debug services with this declaration-only proof.
