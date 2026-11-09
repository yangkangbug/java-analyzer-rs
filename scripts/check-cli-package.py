#!/usr/bin/env python3
"""Verify a trusted local CLI bundle and run its native smoke fixture. No downloads."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import re
import stat
import subprocess
import sys
import tarfile
import tempfile

TARGET = "x86_64-unknown-linux-gnu"
MAX_ARCHIVE = 32 * 1024 * 1024
MAX_TOTAL = 64 * 1024 * 1024
MAX_FILE = 16 * 1024 * 1024
MAX_ENTRIES = 1000
NATIVE_LIBRARIES = {
    "libgcc_s.so.1", "libc.so.6", "ld-linux-x86-64.so.2", "libm.so.6",
    "libpthread.so.0", "libdl.so.2", "librt.so.1",
}


class PackageError(Exception):
    pass


def require(condition, message):
    if not condition:
        raise PackageError(message)


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def safe_name(name):
    require(isinstance(name, str) and name and "\\" not in name and ":" not in name,
            "unsafe package path")
    require(not any(ord(char) < 32 or ord(char) == 127 for char in name), "control in package path")
    path = PurePosixPath(name)
    require(not path.is_absolute() and all(p not in ("", ".", "..") for p in name.split("/")),
            "noncanonical package path")
    return path


def regular_bytes(path, limit=MAX_FILE):
    info = path.lstat()
    require(stat.S_ISREG(info.st_mode), "not a regular file: " + path.name)
    require(info.st_size <= limit, "file exceeds package limit: " + path.name)
    return path.read_bytes()


def unpack(archive_path, destination):
    """Extract only our bounded, normalized regular-file format, never extractall."""
    require(not any(destination.iterdir()), "extraction destination must be empty")
    compressed = regular_bytes(archive_path, MAX_ARCHIVE)
    with gzip.GzipFile(fileobj=io.BytesIO(compressed), mode="rb") as zipped:
        expanded = zipped.read(MAX_TOTAL + 1)
    require(len(expanded) <= MAX_TOTAL, "expanded tar exceeds package limit")
    with tarfile.open(fileobj=io.BytesIO(expanded), mode="r:") as archive:
        members, names, roots, total = [], set(), set(), 0
        for member in archive:
            require(len(members) < MAX_ENTRIES, "too many package entries")
            header = expanded[member.offset:member.offset + 512]
            require(header[257:265] == b"ustar\x0000" and member.offset_data == member.offset + 512,
                    "unexpected tar format or extension header")
            path = safe_name(member.name)
            require(len(path.parts) >= 2, "entries must be under one package directory")
            require(member.name not in names, "duplicate package path")
            require(member.isfile() and not member.sparse and not member.pax_headers,
                    "unexpected archive type or metadata")
            require(member.mode == (0o755 if path.parts[1:] == ("bin", "java-analyzer") else 0o644),
                    "unexpected package mode")
            require(member.uid == member.gid == member.mtime == 0 and not member.uname and not member.gname,
                    "unexpected package ownership or timestamp")
            require(0 <= member.size <= MAX_FILE, "package member exceeds size limit")
            total += member.size
            require(total <= MAX_TOTAL, "package expansion exceeds limit")
            names.add(member.name)
            roots.add(path.parts[0])
            members.append(member)
        require(len(roots) == 1, "expected one package root")
        end = max(member.offset_data + ((member.size + 511) // 512) * 512 for member in members)
        require(len(expanded) - end >= 1024 and not any(expanded[end:]),
                "unexpected trailing archive data or missing end markers")
        for name in names:
            require(not any(str(parent) in names for parent in PurePosixPath(name).parents),
                    "file/directory package collision")
        # No writes occur until all names and types pass. The new temp directory is ours.
        for member in members:
            target = destination.joinpath(*PurePosixPath(member.name).parts)
            target.parent.mkdir(parents=True, exist_ok=True)
            data = archive.extractfile(member).read(MAX_FILE + 1)
            require(len(data) == member.size, "short or oversized package member")
            with target.open("xb") as output:
                output.write(data)
            target.chmod(member.mode)
    return destination / next(iter(roots))


def check_checksums(bundle):
    receipt = json.loads(regular_bytes(bundle / "BUILD.json"))
    require(receipt.get("schema_version") == 1 and receipt.get("target") == TARGET,
            "unsupported build receipt")
    require(receipt.get("binary_sha256") == sha256(regular_bytes(bundle / "bin/java-analyzer")),
            "binary does not match build receipt")
    require(receipt.get("runtime", {}).get("java_required") is False, "missing runtime boundary")
    listed = {}
    for line in regular_bytes(bundle / "SHA256SUMS").decode("ascii").splitlines():
        require(bool(re.fullmatch(r"[0-9a-f]{64}  .+", line)), "invalid SHA256SUMS record")
        digest, name = line.split("  ", 1)
        safe_name(name)
        require(name not in listed and name != "SHA256SUMS", "duplicate/self checksum entry")
        listed[name] = digest
    actual = {}
    for path in bundle.rglob("*"):
        require(not path.is_symlink(), "symlinks are not allowed in bundle")
        if path.is_dir():
            continue
        name = path.relative_to(bundle).as_posix()
        if name != "SHA256SUMS":
            actual[name] = sha256(regular_bytes(path))
    require(actual == listed, "package contents differ from SHA256SUMS")
    for name in ("LICENSE", "NOTICE", "THIRD_PARTY_LICENSES.md", "PROVENANCE.md",
                 "docs/CLI_INSTALL.md", "docs/CLI.md", "docs/CLI_EXECUTION.md", "docs/CLI_PREVIEW.md",
                 "examples/cli_project/project.json", "examples/cli_project/requests.json",
                 "licenses/rust-1.96.0/COPYRIGHT-library.html", "check-cli-package.py"):
        require(name in actual, "missing required bundle file: " + name)
    return receipt


def elf_details(binary):
    def readelf(*args):
        result = subprocess.run(["readelf", *args, str(binary)], check=True, capture_output=True,
                                text=True, env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"}, timeout=15)
        return result.stdout
    header = readelf("-h")
    require("ELF64" in header and "Advanced Micro Devices X86-64" in header,
            "expected Linux x86_64 ELF")
    dynamic = readelf("-d")
    needed = sorted(re.findall(r"\(NEEDED\).*\[([^]]+)\]", dynamic))
    require(needed and set(needed) <= NATIVE_LIBRARIES, "unexpected native library dependency")
    require("(RPATH)" not in dynamic and "(RUNPATH)" not in dynamic, "embedded library search path")
    interpreter = re.search(r"Requesting program interpreter: ([^]]+)\]", readelf("-l"))
    require(interpreter and interpreter.group(1) == "/lib64/ld-linux-x86-64.so.2",
            "unexpected ELF interpreter")
    versions = sorted(set(re.findall(r"\bGLIBC_(\d+(?:\.\d+)+)\b", readelf("--version-info"))),
                      key=lambda value: tuple(map(int, value.split("."))))
    require(versions, "no glibc version requirements found")
    return {"needed_libraries": needed, "elf_interpreter": interpreter.group(1),
            "required_glibc_symbol_version_max": versions[-1]}


def smoke(binary, fixture, version=None):
    binary, fixture = binary.resolve(strict=True), fixture.resolve(strict=True)
    regular_bytes(binary)
    with tempfile.TemporaryDirectory(prefix="java-analyzer-smoke-") as unrelated:
        def run(*args, failure=None):
            result = subprocess.run([str(binary), *args], cwd=unrelated,
                                    env={"PATH": "/nonexistent"}, stdin=subprocess.DEVNULL,
                                    capture_output=True, timeout=15, check=False)
            require(result.returncode == (2 if failure else 0), "unexpected CLI exit status")
            require(not (result.stdout if failure else result.stderr), "unexpected CLI output stream")
            data = json.loads(result.stderr if failure else result.stdout)
            require(data["schema_version"] == 1, "unexpected CLI schema")
            if failure:
                require(data["error"]["code"] == failure, "unexpected CLI failure")
            else:
                require(data["capabilities"]["runtime"] == "native_rust_no_jvm", "missing native boundary")
            return data
        info = run("--version")
        require(version is None or info["version"] == version, "binary and source versions differ")
        manifest = str(fixture / "project.json")
        inventory = run("inspect", "--manifest", manifest)["result"]["compilations"]
        require([c["id"] for c in inventory] == [
            {"project": "app", "source_set": "main"},
            {"project": "app", "source_set": "test"},
            {"project": "lib", "source_set": "main"}], "unexpected sample inventory")
        require([len(c["source_paths"]) for c in inventory] == [1, 1, 1] and
                [len(c["visible_source_paths"]) for c in inventory] == [2, 3, 1],
                "sample source-set visibility differs")
        batch = run("declaration", "--manifest", manifest, "--requests", str(fixture / "requests.json"))["result"]
        require(batch["index_builds"] == 1 and [q["status"] for q in batch["queries"]] ==
                ["Resolved", "Resolved", "OverloadNotResolved"], "sample query statuses differ")
        expected = str(fixture / "lib/main/lib/Library.java")
        for query in batch["queries"][:2]:
            require(len(query["targets"]) == 1, "unexpected declaration candidates")
            target = query["targets"][0]
            require(target["path"] == expected and target["name"] == "value" and
                    target["origin"] == "source" and target["source_available"] and
                    target["range"] == {"start_byte": 52, "end_byte": 57, "start_utf16": 52, "end_utf16": 57},
                    "sample declaration provenance/range differs")
        require(len(batch["queries"][2]["targets"]) == 2, "refused overload lost candidates")
        reports = run("parser-report", "--manifest", manifest)["result"]
        require(reports["classification"] == "parser_limit_or_invalid_syntax" and
                reports["java_semantic_diagnostics"] is False and reports["source_count"] == 3 and
                reports["report_count"] == 0, "sample parser-report boundary differs")
        run("declaration", "--manifest", manifest, "--project", "app", "--source-set", "main",
            "--file", "app/test/app/AppTest.java", "--byte", "69", failure="invisible_source")
    return info["version"]


def check_bundle(bundle):
    require(not bundle.is_symlink() and bundle.is_dir(), "bundle must be a real directory")
    receipt = check_checksums(bundle)
    require(elf_details(bundle / "bin/java-analyzer") == receipt["linkage"], "ELF receipt differs")
    smoke(bundle / "bin/java-analyzer", bundle / "examples/cli_project", receipt["version"])
    return receipt


def check_archive(archive, checksum):
    text = regular_bytes(checksum, 4096).decode("ascii")
    require(text == f"{sha256(regular_bytes(archive, MAX_ARCHIVE))}  {archive.name}\n",
            "archive checksum mismatch")
    with tempfile.TemporaryDirectory(prefix="java-analyzer-package-check-") as directory:
        bundle = unpack(archive, Path(directory))
        return check_bundle(bundle)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--archive", type=Path, help="trusted local archive; verified before safe temporary extraction")
    group.add_argument("--bundle", type=Path, help="already unpacked trusted package directory")
    group.add_argument("--binary", type=Path, help="trusted installed binary to smoke with --fixture")
    parser.add_argument("--sha256-file", type=Path)
    parser.add_argument("--fixture", type=Path)
    args = parser.parse_args()
    try:
        if args.archive:
            require(args.sha256_file is not None and args.fixture is None, "archive mode requires only --sha256-file")
            check_archive(args.archive, args.sha256_file)
        elif args.bundle:
            require(args.sha256_file is None and args.fixture is None, "bundle mode takes no extra options")
            check_bundle(args.bundle)
        else:
            require(args.fixture is not None and args.sha256_file is None, "binary mode requires only --fixture")
            elf_details(args.binary)
            smoke(args.binary, args.fixture)
    except (PackageError, OSError, EOFError, ValueError, KeyError, TypeError, tarfile.TarError,
            subprocess.SubprocessError) as error:
        print("FAIL: " + str(error), file=sys.stderr)
        return 1
    print("PASS: package/install integrity, native linkage, inventory, declarations, refusal, parser reports and source-set isolation; no Java environment")
    return 0


if __name__ == "__main__":
    sys.exit(main())
