#!/usr/bin/env python3
"""Build and package this trusted local checkout. No publishing or project-script execution."""
import argparse
import ctypes
import gzip
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib

spec = importlib.util.spec_from_file_location("cli_package_check", Path(__file__).with_name("check-cli-package.py"))
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)
ROOT = Path(__file__).resolve().parent.parent


def encoded(value):
    return (json.dumps(value, sort_keys=True, indent=2) + "\n").encode("utf-8")


def archive_bytes(files, prefix):
    """Deterministic format for identical bytes/metadata under the same Python/zlib."""
    check.safe_name(prefix)
    output = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=output, mtime=0, compresslevel=9) as zipped:
        with tarfile.open(fileobj=zipped, mode="w", format=tarfile.USTAR_FORMAT) as archive:
            for name, data in sorted(files.items()):
                check.safe_name(name)
                info = tarfile.TarInfo(prefix + "/" + name)
                info.size = len(data)
                info.mode = 0o755 if name == "bin/java-analyzer" else 0o644
                info.uid = info.gid = info.mtime = 0
                info.uname = info.gname = ""
                archive.addfile(info, io.BytesIO(data))
    return output.getvalue()


def write_staged(path, data):
    with path.open("xb") as destination:
        destination.write(data)
        destination.flush()
        os.fsync(destination.fileno())


def rename_without_replacement(source, destination):
    # Linux-only distribution route. Unlike os.rename, this cannot replace even
    # an empty destination directory created by another process after our check.
    libc = ctypes.CDLL(None, use_errno=True)
    rename = libc.renameat2
    rename.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    rename.restype = ctypes.c_int
    if rename(-100, os.fsencode(source), -100, os.fsencode(destination), 1) != 0:
        error = ctypes.get_errno()
        raise OSError(error, os.strerror(error))


def publish_directory(output, files):
    """Publish the complete pair together; never overwrite a chosen destination."""
    check.require(not output.exists() and not output.is_symlink(), "output directory already exists; select a new --output")
    output.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=".java-analyzer-package-", dir=output.parent))
    try:
        for name, data in files.items():
            check.require(len(check.safe_name(name).parts) == 1, "invalid output filename")
            write_staged(staging / name, data)
        rename_without_replacement(staging, output)
    finally:
        if staging.exists():
            shutil.rmtree(staging)


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def repository_bytes(name):
    check.safe_name(name)
    check.require(not any((ROOT / parent).is_symlink() for parent in Path(name).parents),
                  "symlink ancestor in repository inputs")
    return check.regular_bytes(ROOT / name)


def source_inputs():
    check.require((ROOT / "src").is_dir() and not (ROOT / "src").is_symlink(), "source root must be a real directory")
    paths = [ROOT / name for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml")]
    paths += sorted((ROOT / "src").rglob("*"))
    return {path.relative_to(ROOT).as_posix(): check.sha256(repository_bytes(path.relative_to(ROOT).as_posix()))
            for path in paths if not path.is_dir() or path.is_symlink()}


def collect_tree(files, directory, prefix):
    check.require(directory.is_dir() and not directory.is_symlink(), "package root must be a real directory")
    for path in sorted(directory.rglob("*")):
        check.require(not path.is_symlink(), "symlink in package inputs")
        if not path.is_dir():
            name = prefix + "/" + path.relative_to(directory).as_posix()
            check.safe_name(name)
            files[name] = check.regular_bytes(path)


def tracked_paths():
    return {name for name in command("git", "ls-files", "--cached", "-z").split("\0") if name}


def collect_repository_tree(files, prefix, tracked):
    directory = ROOT / prefix
    check.require(directory.is_dir() and not directory.is_symlink(), "package root must be a real directory")
    for name in sorted(tracked):
        if name.startswith(prefix + "/"):
            files[name] = repository_bytes(name)


def reject_ignored_inputs():
    ignored = command("git", "ls-files", "--others", "--ignored", "--exclude-standard", "-z", "--",
                      "src", "docs", "licenses", "examples/cli_project")
    check.require(not ignored, "Git-ignored files in source/package input trees; use a clean checkout without local leftovers")
    untracked = command("git", "ls-files", "--others", "--exclude-standard", "-z", "--", "src")
    check.require(not untracked, "untracked source inputs; explicitly stage reviewed source changes before packaging")


def package_inputs(rust_docs):
    files = {}
    tracked = tracked_paths()
    for name in ("LICENSE", "NOTICE", "PROVENANCE.md", "THIRD_PARTY_LICENSES.md", "Cargo.lock", "rust-toolchain.toml", "README.md", "ROADMAP.md", "CONTRIBUTING.md"):
        check.require(name in tracked, "mandatory package input is not Git-indexed: " + name)
        files[name] = repository_bytes(name)
    collect_repository_tree(files, "licenses", tracked)
    collect_repository_tree(files, "docs", tracked)
    collect_repository_tree(files, "examples/cli_project", tracked)
    for name in ("docs/CLI_INSTALL.md", "docs/CLI_PREVIEW.md", "scripts/check-cli-package.py", "scripts/package-cli.py"):
        check.require(name in tracked, "mandatory package input is not Git-indexed: " + name)
    files["check-cli-package.py"] = repository_bytes("scripts/check-cli-package.py")
    # Include the embedded Rust standard library's own complete attribution.
    files["licenses/rust-1.96.0/COPYRIGHT-library.html"] = check.regular_bytes(rust_docs / "COPYRIGHT-library.html")
    check.require((rust_docs / "licenses/MIT.txt").is_file() and (rust_docs / "licenses/Apache-2.0.txt").is_file(),
                  "Rust toolchain license texts are missing")
    collect_tree(files, rust_docs / "licenses", "licenses/rust-1.96.0/licenses")
    return files


def verify_inputs(before, bundled_before, rust_docs, packager_hash):
    reject_ignored_inputs()
    check.require(source_inputs() == before, "source content changed during packaging")
    check.require(package_inputs(rust_docs) == bundled_before, "package input content changed during packaging")
    check.require(check.sha256(repository_bytes("scripts/package-cli.py")) == packager_hash,
                  "packager content changed during packaging")


def package(output, allow_dirty, offline):
    check.require(sys.platform == "linux", "packaging is validated only on Linux x86_64 GNU")
    check.require(not output.exists() and not output.is_symlink(), "output directory already exists; select a new --output")
    manifest = tomllib.loads(repository_bytes("Cargo.toml").decode("utf-8"))
    version = manifest["package"]["version"]
    revision = command("git", "rev-parse", "HEAD")
    tree = command("git", "rev-parse", "HEAD^{tree}")
    status = command("git", "status", "--porcelain", "--untracked-files=normal")
    dirty = bool(status)
    check.require(not dirty or allow_dirty, "checkout is dirty; commit reviewed changes or explicitly use --allow-dirty for a developer bundle")
    rustc = command("rustc", "-vV")
    check.require("release: 1.96.0" in rustc.splitlines() and
                  "host: " + check.TARGET in rustc.splitlines(), "requires Rust 1.96.0 on " + check.TARGET)
    cargo = command("cargo", "--version")
    check.require(cargo.startswith("cargo 1.96.0 "), "requires Cargo 1.96.0")
    for name in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
        check.require(not os.environ.get(name), "unset build override " + name + " for this packaging route")
    reject_ignored_inputs()
    before = source_inputs()
    rust_docs = Path(command("rustc", "--print", "sysroot")) / "share/doc/rust"
    bundled_before = package_inputs(rust_docs)
    packager_hash = check.sha256(repository_bytes("scripts/package-cli.py"))
    build_args = ["cargo", "build", "--locked", "--release", "--features", "cli", "--bin", "java-analyzer",
                  "--target", check.TARGET, "--target-dir", "target/cli-package-build"]
    if offline:
        build_args.append("--offline")
    subprocess.run(build_args, cwd=ROOT, check=True)
    check.require(source_inputs() == before and command("git", "rev-parse", "HEAD") == revision,
                  "source inputs changed while building; retry from a stable checkout")
    binary = ROOT / "target/cli-package-build" / check.TARGET / "release/java-analyzer"
    check.smoke(binary, ROOT / "examples/cli_project", version)
    linkage = check.elf_details(binary)
    files = {**bundled_before, "bin/java-analyzer": check.regular_bytes(binary)}
    receipt = {
        "schema_version": 1, "version": version, "git_revision": revision, "git_tree": tree,
        "source_dirty": dirty, "rust_build_input_files": before,
        "rust_build_inputs_sha256": check.sha256(json.dumps(before, sort_keys=True, separators=(",", ":")).encode()),
        "target": check.TARGET,
        "packager_sha256": packager_hash,
        "package_input_files": {name: check.sha256(data) for name, data in sorted(bundled_before.items())},
        "rustc_version": rustc, "cargo_version": cargo, "build_command": " ".join(build_args),
        "binary_sha256": check.sha256(files["bin/java-analyzer"]), "linkage": linkage,
        "runtime": {"java_required": False, "native_os_libraries_required": True},
        "reproducibility": "locked source build; normalized archive metadata; bit-for-bit binary reproducibility not certified",
    }
    check.require(command("git", "status", "--porcelain", "--untracked-files=normal") == status,
                  "checkout status changed while packaging; retry from a stable checkout")
    files["BUILD.json"] = encoded(receipt)
    files["README.txt"] = ("Native Java analyzer CLI preview\n\n"
        "Linux x86_64 GNU only; dynamic glibc/libgcc/loader requirements are in BUILD.json.\n"
        "Not macOS, Windows, musl, a language server, or a full Java compiler.\n"
        "No Rust/Python/Java is required to run the binary. Python 3.11+ and readelf\n"
        "are needed only for the supplied integrity/smoke checker. Use trusted inputs.\n\n"
        "From this directory:\n"
        "  ./bin/java-analyzer --version\n"
        "  ./bin/java-analyzer inspect --manifest examples/cli_project/project.json\n"
        "  ./bin/java-analyzer declaration --manifest examples/cli_project/project.json --requests examples/cli_project/requests.json\n"
        "  ./bin/java-analyzer parser-report --manifest examples/cli_project/project.json\n"
        "  python3 check-cli-package.py --bundle .\n\n"
        "See docs/CLI_INSTALL.md for install/uninstall and checksum trust boundaries.\n"
        "See docs/CLI.md, docs/CLI_EXECUTION.md, docs/CLI_PREVIEW.md and docs/PREVIEW_PLAN.md for limits.\n"
        "README.md and docs retain some links to source-only tests/examples not in this runtime bundle.\n"
        "For those links, use the source repository at the exact BUILD.json revision.\n"
        "Source: https://github.com/yangkangbug/java-analyzer-rs at the BUILD.json revision.\n"
        "A dirty developer bundle is not an exact published-revision artifact.\n").encode("utf-8")
    files["SHA256SUMS"] = "".join(f"{check.sha256(data)}  {name}\n" for name, data in sorted(files.items())).encode("ascii")
    prefix = f"java-analyzer-{version}-{revision[:12]}{'-dirty' if dirty else ''}-{check.TARGET}"
    payload = archive_bytes(files, prefix)
    check.require(len(payload) <= check.MAX_ARCHIVE, "archive exceeds package size limit")
    archive = output / (prefix + ".tar.gz")
    checksum = output / (archive.name + ".sha256")
    check.require(not archive.exists() and not archive.is_symlink() and not checksum.exists() and not checksum.is_symlink(),
                  "output already exists; select a fresh --output directory (nothing overwritten)")
    # Verify the exact serialized archive before publishing either local output.
    with tempfile.TemporaryDirectory(prefix="java-analyzer-package-") as directory:
        staged = Path(directory) / archive.name
        staged.write_bytes(payload)
        staged_sum = Path(directory) / checksum.name
        digest = f"{check.sha256(payload)}  {archive.name}\n"
        staged_sum.write_text(digest, encoding="ascii")
        check.check_archive(staged, staged_sum)
    # Recheck content, not just porcelain status: an already modified file may
    # change a second time without changing its Git status line.
    verify_inputs(before, bundled_before, rust_docs, packager_hash)
    check.require(command("git", "rev-parse", "HEAD") == revision and
                  command("git", "status", "--porcelain", "--untracked-files=normal") == status,
                  "checkout revision/status changed during packaging")
    publish_directory(output, {archive.name: payload, checksum.name: digest.encode("ascii")})
    print(archive)
    print(checksum)
    print("PASS: locked native release build, packaged-binary smoke and normalized archive; no publication")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="new user-selected local directory; archive and checksum published together without replacement")
    parser.add_argument("--allow-dirty", action="store_true", help="explicitly allow a marked, non-release developer bundle")
    parser.add_argument("--offline", action="store_true", help="require already cached Cargo dependencies")
    args = parser.parse_args()
    try:
        package(args.output.absolute(), args.allow_dirty, args.offline)
    except (check.PackageError, OSError, EOFError, ValueError, KeyError, AttributeError, tarfile.TarError, subprocess.SubprocessError) as error:
        print("FAIL: " + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
