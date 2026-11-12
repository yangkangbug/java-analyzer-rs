# Independent acceptance: current gates and remaining work

## Separate execution boundaries

The default `ci` workflow runs repository tests, compile-fail documentation,
formatting, lint, docs, examples, and the checked-in native-runtime smoke script.
Both default-library and optional-CLI builds are tested. The executable's local
subprocess/runtime checks also require neither a JDK nor external Java artifacts. After the pinned
Rust toolchain and locked Cargo dependencies are available, they can run offline.
Initial CI toolchain/dependency installation still uses the network.

The separate `extended-acceptance` workflow performs explicit network setup,
real-source analysis, and an optional development-only compiler comparison. It
runs manually on main or on trusted main pushes changing its workflow, input
manifest, validation scripts, fixtures, or native acceptance examples. It does
not run for pull requests or arbitrary branches, and core-only edits do not
currently trigger this extended workflow automatically. A default CI success
must not be presented as success of the external corpus/compiler checks.

This workflow uses read-only repository permissions, disables persisted checkout
credentials, pins official actions to commit SHAs, selects Rust 1.96.0, limits
runtime/concurrency, and uploads no upstream archive/JDK data. It does not use
`pull_request_target` or repository secrets. Archive and toolchain pins do not
make the entire hosted runner/dependency graph hermetic or byte-reproducible.
Network/setup failure fails explicitly; skipped or absent extended runs are not
acceptance evidence.

## Fixed inputs and safe preparation

`scripts/acceptance-inputs.json` records official URLs, SHA-256 hashes, compressed
sizes, extraction bounds, and license/provenance for Commons Lang 3.17.0 sources
and Temurin 21.0.13+9 Linux x64. The fetcher rehashes digest-addressed cache entries,
requires HTTPS, bounds downloads/decompression/file counts, rejects unsafe or
duplicate paths and special files, and rejects symlink ancestors of local paths.
Internal archive links are validated and materialized as regular file copies.
Fresh staged output is published only on successful validation.

Default preparation extracts only `java.base.jmod` and legal notices. Compiler
executables require `--include-oracle-jdk`; preparation itself never runs Java.
Both source and JMOD hashes are checked again by the real-source wrapper. The
analyzer runs with only `PATH=/nonexistent` in its environment. The host may still
have Java installed; this is not a claim of a physically JVM-free machine.

Commons Lang is Apache-2.0. Temurin/OpenJDK is GPL-2.0 with Classpath and assembly
exceptions plus component-specific notices. Complete provided legal files are
retained with local inputs. Archives, upstream sources, and the JDK are not
committed, bundled in the source ZIP, or uploaded as CI artifacts. They remain
external validation inputs, not Cargo or production runtime dependencies.

## Reproduce

Build the Rust acceptance endpoints first. Python 3.11+ is needed only for optional
validation/setup scripts. Linux x64 is the pinned compiler distribution's target.
Output directories must not already exist; cache entries are reverified.

```sh
cargo build --locked --features cli --bin java-analyzer --example real_source --example oracle_bindings
python3 -m unittest discover -s scripts -p 'test_fetch_acceptance_inputs.py' -v
python3 scripts/fetch-acceptance-inputs.py --cache /tmp/java-acceptance-cache --output /tmp/java-acceptance --include-oracle-jdk
python3 scripts/check-real-source.py /tmp/java-acceptance/sources.jar /tmp/java-acceptance/java.base.jmod --profile debug --rounds 1
python3 scripts/check-cli-acceptance.py /tmp/java-acceptance/sources.jar /tmp/java-acceptance/java.base.jmod --native target/debug/java-analyzer
jdk_sha="$(python3 -c 'import json; print(json.load(open("scripts/acceptance-inputs.json"))["jdk"]["sha256"])')"
python3 scripts/check-semantic-oracle.py --jdk /tmp/java-acceptance/jdk --jdk-archive "/tmp/java-acceptance-cache/$jdk_sha" --native target/debug/examples/oracle_bindings --native-cli target/debug/java-analyzer --sources-jar /tmp/java-acceptance/sources.jar
```

For no-network setup after filling the cache, pass `--offline` and a fresh output
directory. Omit `--include-oracle-jdk` when only corpus analysis is needed. The
comparator downloads nothing and executes only the supplied compiler/runtime.
The comparator verifies the pinned archive and installed JDK bytes before running
any Java tool; use the matching archive from the verified extraction flow.

## Shipped executable evidence

The CLI acceptance wrapper supplies the same pinned Commons Lang source and JMOD
inputs through a versioned manifest, from a different working directory. It runs
the actual `java-analyzer` executable with no Java environment, checks project
inventory and labeled parser reports, and compares all 45 fixed declaration
queries with their source/binary provenance. Missing-platform and wrong-output
controls test the wrapper itself. These are explicit project configuration and
interface checks, not new whole-project Java semantic-equivalence evidence.
The harness bounds subprocess time and captured output; this does not provide
production CLI cancellation, parser depth limits, or an editor process supervisor.

## Independent compiler evidence

The nine-file handwritten fixture and expectations are versioned under
`dev-tools/oracle`. javac `Trees.getElement` selects each referenced declaration,
then `Trees.getPath` supplies its actual declaration tree and range. Compiler
UTF-16 positions are converted to UTF-8 bytes, including an astral-character
case. The wrapper associates the actual compiler tree with a unique declaration
marker and checks the native target's path, kind, name, owner convention and exact
name-token range. Expectations are not generated from native output.

Fifty-nine supported bindings are checked against this independent evidence. Four
separate cases verify explicit refusal boundaries: javac selects an overload at two query sites,
resolves `super.inherited`, and binds an indexed-array source-field receiver,
while the native engine respectively reports `OverloadNotResolved`,
`ReceiverNotBound`, and `Unresolved`. The last two statuses have no target;
overload refusals retain candidates. These are not semantic-equivalence
passes. Missing-name/incomplete compiler failures, native missing-name behavior,
wrong-golden and corrupted-compiler evidence, and optimized-Python rejection are
negative controls. Both compiler stages disable annotation processing and
implicit compilation, use UTF-8 and release 21, and use empty controlled
classpath/sourcepath. The original source-only suite and mixed-suite consumers are analyzed only.
The mixed suite separately compiles handwritten dependencies into temporary
class files for explicit binary input; no fixture program is ever executed.
Only the development comparator step executes Java. The Rust endpoint does not.

A separate mixed source/binary suite uses four generated dependency classes and
two consumer sources. It checks compiler-selected consumer source overrides,
exact native refusal candidates and binary provenance. Omitting binary inputs
must break both compiler setup and a native candidate comparison. This is not
compiler-selected binary-target identity or descriptor equivalence evidence.

The oracle exposed a real defect: a shadowed `super` field incorrectly resolved
to the subclass field. A conservative syntax-receiver guard now returns no target
instead of silently dropping `super`. Full super binding is still unimplemented.
`this`, ordinary inheritance, locals/parameters, and qualified type-prefix lookup
have separate regression coverage across legacy/prepared/workspace APIs.

The multi-declarator follow-up independently confirms every named sibling field,
local declaration order and block scope, mixed scalar/array declaration names,
and imported field receiver provenance despite a caller-package name collision.
The compiler helper is unchanged. Wrong-sibling and same-name/wrong-package
perturbations must fail even when comma-declaration tree ranges overlap. This is
still binding evidence, not a type-inference or definite-assignment checker.

Invocation namespace cases distinguish identical names at callee, argument,
value and receiver positions, including inherited methods and nested enclosing
classes. Separate invalid sources require five no-callable native refusals and
compiler rejection. The real-source harness also audits all direct unqualified
invocation names for accidental variable/type targets; zero wrong-namespace
results is a negative invariant, not proof that every call resolves correctly.

A separate `primitive-v1` suite originally required twelve strict primitive
source-call bindings and twenty explicit refusals. A reviewed three-row overlay
now requires proved scalar call results at three previously refused positions,
giving fifteen bindings and seventeen refusals without changing original fixture
bytes. These checks retain exact candidate identities and
unchanged Trees-based source declaration evidence. Sixteen isolated invalid
fixtures require compiler errors and native refusal, including unique method
names with incompatible arity or primitive arguments. `CallNotResolved` preserves
a singleton candidate without claiming Java invalidity; valid boxing/varargs
controls also require that neutral refusal. A checksum-verified Object class,
extracted as data from the supplied JMOD, closes the source classes' inventory;
omitting it must block the newly supported overload selections.

The separate `return-v1` suite checks 29 compiler-valid queries: fourteen exact
source matches (including two existing direct-call controls) and fifteen refusals.
Twelve isolated compiler-invalid fixtures require exact native refusal candidates,
including malformed throws headers which the parser can otherwise accept. Inner
parameter selection is checked independently of its declared return type; arrays,
unknown calls and a compiler-valid deep nesting example remain refused.

The optional `--sources-jar` branch checks eight independently compiler-selected
source overloads over all 249 unchanged Commons Lang sources. It verifies the
existing archive hash/inventory and analyzes without generating or executing
upstream classes. Exact source declaration headers and ranges distinguish the
float/double `IEEE754rUtils.max/min` overloads, covering four original loop calls
and four additional outer nested calls. Same-name target mutation and
missing-Object controls remain required. This does not establish binary-selected
method identity: public binary targets still lack overload-distinguishing
source ranges/descriptors. See the complete [oracle contract](../dev-tools/oracle/README.md).

## Remaining coverage limits

The full real-source workload still has known parser/semantic gaps. Its default
forty-five fixed queries and error-node inventory are not whole-project semantic
coverage. Prepared/rebuild agreement proves consistency, not Java correctness.
The independent oracle covers only a closed, same-owner static primitive
source-call selection subset. It does not cover generic inference, general
overload selection, binary-selected platform bindings, JPMS, complete access
control, malformed-buffer equivalence,
or general Java conformance. A compiler oracle cannot replace editor recovery,
cancellation or incremental invalidation tests.

Next steps are broader independently reviewed fixtures, complete diagnostic
comparison, explicit unsupported-result accounting, additional versioned real
projects, and an intentional decision about making extended gates required or
triggering them on every core change. Compiler/Java requirements must stay outside
the production/native analysis core and default native test path.

## Second project and local delivery

The [Gson fixture](../examples/second_project/README.md) adds a distinct manually
configured core/extras main/test graph. Its separate
[`second-project-inputs.json`](../scripts/second-project-inputs.json) fixes four
public archives and two data-only JMOD files. The optional setup retains notices,
requires fresh outputs, verifies hashes/archive bounds and never executes project
scripts. Default CI runs its offline security/validator tests. Extended CI fetches
only those pins and runs the actual CLI with all predeclared outcomes retained.
Gson reports 1,513 parser gaps in 58 selected files; this is not Java invalidity
or complete project support.

Default CI also builds and verifies a local Linux CLI archive without uploading
it. [Installation](CLI_INSTALL.md) and [resource evaluation](CLI_PREVIEW.md) keep
source/build provenance, deterministic packaging, native build reproducibility
and measured runtime observations separate. Full repeated resource measurements
are explicit local runs, not inferred from a green CI status.

### Binary instance-method proof and opt-in identity

The existing Commons CLI gate also reads the checksum-pinned
`examples/real_source/binary-method-probes-v3.json`, explicitly linked to the
unaltered original forecast and topology V2. Its ten complete real records retain
three prospective new selections, three explicitly deferred original positives,
three conservative migrations of previously legacy-resolved
length/charAt calls, and one preserved bridge/interface compareTo refusal.
Ancestor/interface same-name evidence remains outside this direct-declaration
proof, so those three regressions are intentional refusals, not preservation or
new successes. The old 45-query golden is unmodified: its exact q06 String.length
record receives a current-only full-record overlay, preserving both String and
CharSequence declarations as Ambiguous. All other old records remain exact.

The original forecast incorrectly predicted a singleton CallNotResolved for
three length/charAt controls. The first candidate failure is retained and pinned.
V2 changes only those three complete refusal records, never the six positives.
The reason is legacy signature-based ancestor hiding versus strict failure hints
that retain both declarations without implementing binary override; it is not
a claim that the old identity conflated distinct owners. Independent pinned
String/CharSequence class-file evidence and exact mechanism excerpts accompany
the revision. The loader reconstructs the entire permitted revision and rejects
old-or-new unions. V2 itself did not change any positive forecast. The later diagnosis established
that three ClassUtils sites fail before actual classification: the existing
`Comparator<Class<?>> comparator()` return header at byte 5865 invalidates the
enclosing inventory under the current generic-header boundary. V3 explicitly
defers only those three original positives, retaining their complete original
OverloadNotResolved candidate sets in both default and detailed projections.
It pins V2, the first failure, original source bytes, exact diagnostic traces and
independent class-file descriptors. It does not expand generic headers or erase
the original forecasts. The final candidate passed the full Commons CLI gate, including both StringUtils
substring targets and startsWith(local), in default and detailed projections.
These three observed gains are distinct from the three deferred ClassUtils
predictions; V3 itself was not treated as evidence of success. Recovery checks
also restored the original nested-receiver q12 refusal and caller-visible binary
failure hints without changing the frozen records.

The default declaration JSON remains version 1 with its original target shape
and coarse identity precision. A separate `declaration --target-details` query
requires `result.target_details_version: 1` and exact binary-method descriptor,
binary name and declaring compilation context. These details distinguish
same-name overloads without inventing source ranges or persistent symbol IDs.
The gate compares whole records in both modes, not a normalized subset that
could discard wrong identity fields. Class and field targets do not acquire
method descriptors by implication.

`--capture-binary-method-baseline` verifies the exact frozen 650565ff executable
and original full records; it neither invokes the new unsupported details mode
nor counts old results as current acceptance. The native CLI and oracle binaries,
18 exploratory full baseline rows and an independent read of String.class method
names/descriptors/access flags are preserved under
`evidence/binary-method-20261113/`. Frozen predictions are not reported as
candidate successes until the new binary has actually passed them. Optional
handwritten compiler comparison stays in the shared semantic oracle, never in
native runtime or an upstream project build.

### Return declaration inventory stage

The current Commons gate applies `header-inventory-probes.json` on top of the
unchanged binary-method V3 contract. Exactly three deferred ClassUtils records
now require the original independently predicted `String.substring`/`startsWith`
declarations, in both default and detailed projections. All other binary rows,
the 45-query golden plus its existing q06 overlay, parser inventory and four
historical getters remain exact. The earlier V3 deferral above is retained as
history, not erased or relabeled as a success from that earlier binary.

`--capture-header-inventory-baseline` requires the exact frozen `0697877b` native
binary and verifies its complete old default/detailed records. It cannot be
combined with `--capture-binary-method-baseline`, which still belongs to the
older `650565ff` protocol stage. Current acceptance never accepts a union of
old and new outcomes. Offline controls reject changed rows, descriptors,
linking contexts and modified forecasts.

The separate header-inventory compiler oracle checks bounded handwritten valid
and invalid declarations; it does not execute fixture programs or become a
native runtime dependency. See [the design boundary](DECLARATION_INVENTORY.md).
