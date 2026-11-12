# Single-primitive-argument instance-call proof

This is a narrow extension of the [reference instance-call proof](REFERENCE_CALLS.md),
not general Java overload resolution. The receiver and all candidate declarations
retain that proof's source ownership, visibility, complete hierarchy, access,
nongeneric, fixed-arity and shared work/depth-budget requirements.

For exactly one argument, an explicitly declared scalar primitive parameter or
ordinary local can supply its existing type evidence. Parentheses preserve that
evidence. A plain nonnegative decimal `int` literal is also accepted only in the
range `0..2147483647`, with no suffix, underscore, radix prefix or leading zero
except `0` itself. Lexing a token as `IntLiteral` is not by itself a legality proof.

All same-name candidates are validated before choosing a winner. Strict invocation
allows primitive identity/widening for these actuals; it does not box them into
reference parameters. A unique most-specific formal is established by the existing
primitive conversion matrix, not a numeric score, declaration order or the literal's
value. There is no constant narrowing from `1` to `byte` or `short` for invocation.
Reference actuals retain the existing path and its no-unboxing strict-phase rule.

This slice deliberately does not type mixed/multiple primitive arguments, negative
or other unary expressions, fields, arbitrary arithmetic, primitive arrays or call
results. It does not add boxing/unboxing, varargs expansion, generic inference,
inherited/binary method selection or reflection types. Unknown expressions and
incomplete competing declarations cannot be ignored to manufacture a winner.
Supported single-reference and multi-reference calls remain separately governed by
the previous proof; this is not a new restriction on their established cases.

## Frozen real-source target

The additive [Gson plan](../examples/second_project/qualified-primitive-probes.json)
was recorded against `a9353caf5b1e7e1c34b47fb0e9b1c1afc913c798`, before the candidate.
It independently identifies `array.remove(1)` at byte 1976 of `JsonArrayTest.java`
and `JsonArray.remove(int index)` at declaration-name bytes 5494..5500. The intended
change is only that exact target. Existing `remove(a)` reference selection, refusal
of mixed-argument `set(0,b)`, and refusal of outer `getAsString` remain explicit.
Selecting the inner method does not supply a type for its result receiver.

Historical plans and source hashes remain frozen. New baseline checks refer to the
post-class-literal engine with 166 parser reports; they must not be confused with
the older strict-reference stage in which `remove(a)` was still unselected.

## IntelliJ IDEA behavior reference and license

The following official IntelliJ Community files were read at fixed commit
`840d3be11dfdeada533d05052008b48816b6cc4e`. This is behavior/design reference and an
independent Rust implementation, not a translation of the PSI/type system or a Java
runtime dependency. Existing extraction and adapted-code notices remain in NOTICE.

- [PsiLiteralExpressionImpl.getType](https://github.com/JetBrains/intellij-community/blob/840d3be11dfdeada533d05052008b48816b6cc4e/java/java-psi-impl/src/com/intellij/psi/impl/source/tree/java/PsiLiteralExpressionImpl.java#L59)
  separates token-based literal type from value validation.
- [PsiLiteralUtil.parseInteger](https://github.com/JetBrains/intellij-community/blob/840d3be11dfdeada533d05052008b48816b6cc4e/java/java-psi-api/src/com/intellij/psi/util/PsiLiteralUtil.java#L29)
  handles radix/range rules. Its special `2147483648` support serves a unary-minus
  context and is not copied into our positive-literal rule.
- [TypeConversionUtil](https://github.com/JetBrains/intellij-community/blob/840d3be11dfdeada533d05052008b48816b6cc4e/java/java-psi-api/src/com/intellij/psi/util/TypeConversionUtil.java#L94)
  uses explicit primitive widening relationships. Its general `isAssignable`
  permits boxing/unboxing, so it is not used as our strict-conversion contract.
- [JavaMethodsConflictResolver](https://github.com/JetBrains/intellij-community/blob/840d3be11dfdeada533d05052008b48816b6cc4e/java/java-psi-impl/src/com/intellij/psi/scope/conflictResolvers/JavaMethodsConflictResolver.java#L414)
  separates applicability and specificity; its later parameter comparisons inform
  our formal-to-formal ordering rather than a conversion-cost heuristic.
- [MethodCandidateInfo](https://github.com/JetBrains/intellij-community/blob/840d3be11dfdeada533d05052008b48816b6cc4e/java/java-psi-api/src/com/intellij/psi/infos/MethodCandidateInfo.java#L157)
  references the strict invocation phase. The governing language rules are
  [JLS 5.1.2](https://docs.oracle.com/javase/specs/jls/se25/html/jls-5.html#jls-5.1.2),
  [5.3](https://docs.oracle.com/javase/specs/jls/se25/html/jls-5.html#jls-5.3) and
  [15.12.2](https://docs.oracle.com/javase/specs/jls/se25/html/jls-15.html#jls-15.12.2).

These source files carry Apache-2.0 headers; the upstream NOTICE identifies
JetBrains attribution. Upstream root product-build terms are not substituted for
those file-level source licenses. No IntelliJ code, binary or project is required
by the native engine, its mandatory tests or its runtime.

## Validation and remaining limits

The frozen pre-change executable fails the new target expectation and passes the
new slice's explicitly separate baseline mode. The candidate selects only the
specified `remove(int)` declaration; the other three fixed queries retain their
full target/candidate records. Current reference and strict-reference gates apply
this one correction through a digest-pinned additive plan, not by rewriting their
historical goldens. Primitive-selection and parser-contract stages are independent:
the new baseline uses the already improved 166-report parser, whereas older
feature baselines use their frozen 331-report parser. Cross-layer mode tests do
not substitute for executing every historical binary; this pass runs the actual
frozen a935 baseline, not the much older strict-reference binary.

Native tests cover typed scalar widening/specificity, declaration order, exact
literal boundaries, no constant narrowing, lexical visibility, duplicate and
unsupported competitors, incomplete platform data, budgets and unchanged reference
calls. Runtime checks run without a Java search path.

The separate [qualified-primitive oracle](../dev-tools/oracle/qualified-primitive-v1/README.md)
verifies the pinned JDK archive and installed tool bytes before development-only
compiler analysis. Seven native selections match the independently identified
compiler declarations. Seven other compiler-valid cases remain explicit native
refusals; two compiler-invalid cases and exact Object-omission refusals also pass.
No fixture application is run, Gson is not compiled, and Java is not introduced
into the analysis engine or mandatory native tests. This bounded evidence does
not establish full overload resolution, Java conformance or arbitrary-project
compatibility.
