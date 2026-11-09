# Native macOS source-build gate

This gate tests the library and experimental CLI on GitHub-hosted macOS machines.
It does not use the developer's Mac, publish a binary, create an archive, install
anything globally, sign/notarize a product, or change the Linux delivery route.
The workflow definition alone is not a passing result. Cite successful jobs for
the exact source commit before reporting either architecture as tested.

## Hosts and scope

`.github/workflows/macos.yml` uses two standard hosted runners:

- `macos-15`: native Apple Silicon, `aarch64-apple-darwin`.
- `macos-15-intel`: native Intel, `x86_64-apple-darwin`.

The gate checks the actual Darwin host, machine architecture, GitHub runner
architecture, and Rust 1.96.0 host triple. It does not cross-compile an Intel
binary on ARM and call that an Intel runtime pass. The release file must have a
thin 64-bit Mach-O executable header matching the native host, not an ELF,
universal binary or dylib. An unavailable runner or failed job is not a pass.
The runner OS version and Rust identity are retained in the job logs.

GitHub's [runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
lists these standard labels and states that standard hosted runner use is free
for public repositories. This repository is public. No `large`/`xlarge` runner is
selected. Recheck billing before applying this workflow to a private repository.
The [runner image catalog](https://github.com/actions/runner-images) also records
label changes and retirements; `macos-latest` is intentionally not used.

Rust 1.96.0 supplies both native host toolchains through rustup. Its
[target support record](https://github.com/rust-lang/rust/blob/1.96.0/src/doc/rustc/src/platform-support.md)
classifies ARM64 macOS as Tier 1 with host tools and Intel macOS as Tier 2 with
host tools. This workflow adds project-specific tests on both. Rust's own
minimum deployment targets do not certify this project's behavior on older
macOS versions. The selected image supplies Apple's native linker/SDK.

## Checks and exclusions

Each host runs format/strict Clippy checks, default and all-feature Cargo tests,
documentation tests, and builds the examples. The project/scopes/resolve
examples run with an empty environment and `PATH=/nonexistent`. Debug CLI smoke
checks source and class-file declarations with the same runtime restriction.
The native release build then receives two independent checks:

1. `scripts/check-macos-runtime.py` validates the executable header and actual
   `/usr/bin/otool -L` and `-l` output. Only the explicit system install names in
   that script are accepted. It rejects unknown dependencies (including JVM
   libraries), embedded library search paths, custom dynamic linkers and dyld
   environment commands. It does not require system dylibs to be separate files:
   Apple's dyld shared cache may provide them. An unexpected dependency fails
   closed and requires investigation, not an automatic allowlist expansion.
2. The existing CLI behavior fixture runs the absolute executable from an
   unrelated working directory with only `PATH=/nonexistent` in its environment.
   It checks version, compilation visibility, source ranges, explicit overload
   refusal, parser classification and rejection of an invisible source.

Hosted images may contain Java tooling. These checks neither remove it nor run
it. Cleared Java-home variables, an unusable executable search path, inspected
linkage, and actual CLI results support the exercised JVM-free runtime path.
They do not prove that arbitrary future code cannot launch an absolute Java
path or dynamically load a library. This is not a security sandbox.

The Linux `/proc` supervisor-death test remains Linux-only. The portable lease
closure, timeout, cancellation, crash and output-limit tests still run on macOS.
The Linux-specific SIGABRT spelling assertion is excluded on macOS; the native
crash-containment test itself still runs. Unix symlink tests remain enabled.

The existing Linux workflow continues to run all Python preview-tooling tests
and Linux packaging. Resource measurements remain a separate Linux-only task.
This Mac job
does not run `package-cli.py`, the ELF checker, Linux `/proc`/wait4 measurements,
or second-project input publication. Those tools have separate platform
requirements. It calls `check-native-runtime.sh` without an optional JAR, so the
optional GNU `sha256sum`/`realpath` branch is not exercised. No JDK/corpus download
or optional javac oracle is added to the Mac gate.

## Reproduce from a trusted source checkout

On a Mac with Rust/Cargo 1.96.0, Python 3.11+ and Apple's command-line tools:

```sh
# Choose the pair matching the native Mac, not a cross-compilation target:
TARGET=aarch64-apple-darwin; RUNNER_ARCH=ARM64
# Intel instead: TARGET=x86_64-apple-darwin; RUNNER_ARCH=X64
python3 -m unittest discover -s scripts -p 'test_macos_runtime.py' -v
python3 scripts/check-macos-runtime.py --host-only --target "$TARGET" --runner-arch "$RUNNER_ARCH"
cargo build --locked --release --features cli --bin java-analyzer
python3 scripts/check-macos-runtime.py --binary target/release/java-analyzer --target "$TARGET" --runner-arch "$RUNNER_ARCH"
```

This builds only this repository and its locked Rust dependencies. An uncached
build may download registry dependencies; `--locked` does not mean offline.
The checker executes a trusted local binary and must not be used to vet an
arbitrary download. Python/otool/Rust are check/build tools, not analyzer runtime
dependencies. This focused reproduction is not the complete CI test suite.

A successful job supports only the tested source revision, architecture and
hosted OS image. It does not establish a Mac downloadable release, universal
binary, signing/notarization, Gatekeeper installation flow, older-OS support,
the user's own machine, editor/LSP integration, Java semantic conformance, or
project-scale performance. Adding the workflow also does not configure branch
protection to require its checks.
