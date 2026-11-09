# Install and try the native CLI preview

The supported delivery slice is **Linux x86_64 GNU**. It is a one-shot JSON CLI
for explicitly configured, trusted local projects, not an editor language server
or a full Java compiler. No Java, JVM, JNI, JDT LS, Maven or Gradle process is
required or started. Explicit class/JAR/JMOD files are data only.

There is no crates.io package, signed binary release, automatic updater or global
installer. The source installation below and the local archive builder are the
supported routes. macOS, Windows, ARM and musl are not certified by this work.
The Linux binary is **dynamically linked**, not a static universal executable.

## Install from a trusted source checkout

Prerequisites: Rust/Cargo **1.96.0** (selected by `rust-toolchain.toml`) and a
native Linux linker/toolchain. Cargo may download locked registry dependencies
on the first build. `--locked` does not make an uncached build offline. Review
and obtain this repository through a trusted source; do not run scripts from an
arbitrary Java project to use the analyzer.

From the repository root, choose a new installation directory that you own:

```sh
INSTALL_ROOT="$HOME/java-analyzer-preview"
cargo install --path . --locked --features cli --bin java-analyzer --root "$INSTALL_ROOT"
"$INSTALL_ROOT/bin/java-analyzer" --version
"$INSTALL_ROOT/bin/java-analyzer" inspect --manifest examples/cli_project/project.json
"$INSTALL_ROOT/bin/java-analyzer" declaration --manifest examples/cli_project/project.json --requests examples/cli_project/requests.json
"$INSTALL_ROOT/bin/java-analyzer" parser-report --manifest examples/cli_project/project.json
```

Use an absolute `INSTALL_ROOT`. Cargo installs an optimized release executable
and its install bookkeeping under this selected directory; it does not change
your shell configuration. The explicit binary path avoids selecting a different
version from `PATH`. No `sudo`, global install, shell startup edit or `--force` is
needed. Do not reuse a root containing another installation unless you intend
Cargo's documented install/update behavior. Append `--offline` to the Cargo
command only when the pinned dependencies and toolchain are already cached.

The three declaration results are `Resolved`, `Resolved`, and
`OverloadNotResolved`. The first two identify `value` in
`examples/cli_project/lib/main/lib/Library.java`, bytes `52..57`; the last keeps
two candidates instead of guessing an overload. The parser report has three
sources and zero reports. These outcomes do not establish Java compilation
correctness or general method applicability.

Optional installed-binary smoke validation needs Python **3.11+** and GNU
`readelf` (binutils), only for the checker, not for analysis:

```sh
python3 scripts/check-cli-package.py --binary "$INSTALL_ROOT/bin/java-analyzer" --fixture examples/cli_project
```

The smoke runs the installed binary by absolute path in an unrelated working
directory with an otherwise empty environment and `PATH=/nonexistent`, without
`JAVA_HOME` or `JDK_HOME`. It checks inspection, main/test visibility, exact
source declaration ranges, an explicit overload refusal, parser classification,
and rejection of a test source from main. It reads ELF linkage and permits only
the documented native OS libraries. This checks the exercised native path; it is
not a security sandbox or a certification for arbitrary inputs.

To uninstall this Cargo-installed binary, explicitly target the same root:

```sh
cargo uninstall java-engine --root "$INSTALL_ROOT"
```

The checkout/sample project is separate from this installation. Run against your
own project by creating the explicit manifest described in [CLI.md](CLI.md).
No build files, project scripts, dependency download or JDK discovery is inferred.

## Build a local archive with a checksum

Run this only in a reviewed checkout. Required tools: Git, Rust/Cargo 1.96.0,
Python 3.11+, a native linker and GNU `readelf`. The Rust installation must retain
its standard-library copyright and license files under `share/doc/rust`; these
are included with the binary. The builder refuses other Rust versions/hosts and
rejects environment overrides for compiler flags and compiler wrappers.

```sh
python3 scripts/package-cli.py --output target/cli-dist
# With cached dependencies, add --offline.
```

The builder:

1. Requires a clean Git checkout by default. It builds this source itself using
   `cargo build --locked --release --features cli --bin java-analyzer --target x86_64-unknown-linux-gnu --target-dir target/cli-package-build`.
2. Checks the actual release executable and reads its ELF interpreter, native
   library dependencies and greatest required glibc symbol version. Unexpected
   libraries and embedded library search paths fail packaging.
3. Includes the executable, checked-in main/test sample, CLI docs, complete project
   notices/licenses and the toolchain's Rust standard-library notices/licenses.
   It includes no JDK, JVM, third-party corpus or project build scripts.
4. Writes `BUILD.json` with the exact source commit/tree, dirty flag, build-input
   hashes, bundled-input and packager hashes, pinned tool versions, target, build
   command, binary SHA-256 and linkage.
   It does not record usernames, hostnames, absolute build paths or environment
   values. `SHA256SUMS` covers every other file in the bundle.
5. Normalizes archive ordering, permissions, owners and timestamps, checks the
   actual serialized archive through bounded safe extraction, and smokes its
   extracted executable with no Java environment before emitting local files.

Output names contain the package version, 12-character source revision and target,
ending in `.tar.gz` and `.tar.gz.sha256`. The selected output directory must not
already exist. The complete verified pair is staged beside it, then published
together by a no-replacement atomic directory rename on Linux. Write/publish
failures clean up that staging directory and leave no partial final pair.
Choose a new output directory for another build. Nothing is published, tagged,
uploaded, installed globally or copied to another computer.

`--allow-dirty` explicitly enables a **developer bundle**, visibly marked `-dirty`
in the filename and receipt. Its source/build-input hashes are retained, but it
must not be represented as the exact published commit. Commit the reviewed changes
and rebuild without that option for a commit-corresponding delivery. Git-ignored
files under source/docs/licenses/sample trees are rejected even in developer mode;
only Git-indexed documentation, notices and sample files enter a package. Stage
reviewed new files explicitly before using dirty mode; other untracked files are
never bundled, and untracked source files are rejected. Source and bundled input
bytes are checked again after the packaged smoke, so changes during
building/packaging fail even if a file keeps the same modified Git status.

## Verify and run a trusted archive

A checksum detects changed bytes, **not who produced them**. Obtain the archive
and checksum through a trusted channel. A malicious binary accompanied by its own
matching hash is still malicious. The checker eventually executes the binary;
do not use it to test arbitrary untrusted downloads.

Use the checker from your reviewed source checkout. Replace the two paths below
with the exact local filenames printed by the builder:

```sh
python3 scripts/check-cli-package.py --archive /path/to/package.tar.gz --sha256-file /path/to/package.tar.gz.sha256
```

This verifies the outer hash, rejects path traversal, duplicate entries,
file/directory collisions, symlinks/hardlinks, special files, extended tar headers,
unexpected metadata and expansion limits, then extracts into a fresh temporary
directory. It verifies every bundled file and receipt, runs the native smoke and
removes only its own temporary directory. It does not install anything. A modified
or missing file, extra file, changed binary hash or different linkage fails.

For ordinary use after verification, extract the trusted archive into a **new,
empty directory you choose** using your archive utility. From its package root:

```sh
./bin/java-analyzer --version
./bin/java-analyzer inspect --manifest examples/cli_project/project.json
./bin/java-analyzer declaration --manifest examples/cli_project/project.json --requests examples/cli_project/requests.json
./bin/java-analyzer parser-report --manifest examples/cli_project/project.json
python3 check-cli-package.py --bundle .
```

The binary alone needs the host's native OS libraries, but no Rust, Cargo,
Python, Java or internet connection. Retain the accompanying licenses/notices
when copying or redistributing it. The bundle includes `docs/CLI_PREVIEW.md` and
all indexed project documentation, but some README/documentation links lead to
source-only tests or examples that are not in the runtime archive. Follow those
in the source repository at the exact `BUILD.json` revision. Removal is local: delete the archive's
user-selected extracted directory when you no longer need it. This is separate
from Cargo's installation bookkeeping.

## Platform and reproducibility boundaries

The validation host is Linux x86_64 GNU. Inspect `BUILD.json` for **that binary's**
`required_glibc_symbol_version_max`, `needed_libraries` and `elf_interpreter`.
The initial local build required glibc symbols through **2.39**, `libgcc_s.so.1`,
`libc.so.6` and `/lib64/ld-linux-x86-64.so.2`. This is a necessary dynamic-link
requirement, not a promise that every distribution with glibc 2.39+ works.
Build on the target host if its OS libraries differ; a macOS or Windows machine
cannot run this Linux ELF. No older-glibc, musl or cross-platform certification is
implied. No native OS libraries are bundled.

Keep three different claims separate:

- **Repeatable source recipe:** the source revision, Cargo.lock, pinned Rust,
  explicit target and build command identify what to build. Registry cache,
  linker, system libraries, Cargo configuration and host can still matter.
- **Deterministic archive format:** tests require identical archive bytes for the
  same bundled input bytes/metadata under the same Python/zlib implementation.
  Sorting and normalized tar/gzip metadata remove accidental file-order/time
  variation. This does not rebuild the native executable.
- **Bit-for-bit native build reproducibility:** not certified. Two different
  toolchains, linker/libc environments or source locations may produce different
  executables. SHA-256 identifies the delivered artifact; it does not prove a
  reproducible or hermetic build. No signature or supply-chain attestation is
  issued by these scripts.

This delivery slice does not close the persistent-server, transactional reload,
unsaved-overlay, editor/LSP, semantic-conformance or project-performance gates in
[PREVIEW_PLAN.md](PREVIEW_PLAN.md). See [CLI_EXECUTION.md](CLI_EXECUTION.md) for
wall-clock supervision, memory and output-backpressure limitations.
