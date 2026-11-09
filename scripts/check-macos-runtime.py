#!/usr/bin/env python3
"""Check a trusted, locally built macOS CLI. No downloads, packaging or Java."""
import argparse
import importlib.util
import json
from pathlib import Path
import platform
import re
import stat
import struct
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {"aarch64-apple-darwin": ("arm64", "ARM64", 0x0100000c),
           "x86_64-apple-darwin": ("x86_64", "X64", 0x01000007)}
# Exact native OS install names, not arbitrary /usr/lib or framework prefixes.
# Libraries in Apple's dyld shared cache need not exist as separate disk files.
NATIVE_LIBRARIES = {"/usr/lib/libSystem.B.dylib", "/usr/lib/libiconv.2.dylib",
                    "/usr/lib/libresolv.9.dylib"}
DYLIB_COMMANDS = {"LC_LOAD_DYLIB", "LC_LOAD_WEAK_DYLIB", "LC_REEXPORT_DYLIB",
                  "LC_LAZY_LOAD_DYLIB", "LC_LOAD_UPWARD_DYLIB"}
spec = importlib.util.spec_from_file_location("cli_smoke", ROOT / "scripts/check-cli-package.py")
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)


class CheckError(Exception):
    pass


def require(condition, message):
    if not condition:
        raise CheckError(message)


def validate_host(target, system, machine, runner_arch, rustc):
    require(target in TARGETS, "unsupported requested macOS target")
    expected_machine, expected_runner, _ = TARGETS[target]
    require(system == "Darwin", "native macOS execution required")
    require(machine == expected_machine and runner_arch == expected_runner,
            "host or GitHub runner architecture differs from requested native target")
    for field, expected in (("host", target), ("release", "1.96.0")):
        values = [line[len(field) + 2:] for line in rustc.splitlines() if line.startswith(field + ": ")]
        require(values == [expected], "unexpected or duplicate rustc " + field)
    return expected_machine


def validate_header(data, architecture):
    require(len(data) == 32, "truncated Mach-O 64-bit header")
    magic, cpu, _, kind, *_ = struct.unpack("<8I", data)
    expected = {values[0]: values[2] for values in TARGETS.values()}
    require(magic == 0xfeedfacf and kind == 2,
            "expected a thin little-endian 64-bit Mach-O executable, not ELF, a universal file or dylib")
    require(architecture in expected and cpu == expected[architecture], "Mach-O CPU differs from native host")


def validate_libraries(output, binary):
    lines = output.splitlines()
    require(lines and lines[0] == str(binary) + ":", "unexpected otool binary header")
    libraries = []
    for line in lines[1:]:
        match = re.fullmatch(r"\s+(\S+) \(compatibility version [0-9.]+, current version [0-9.]+\)", line)
        require(match is not None, "unrecognized otool library record")
        name = match.group(1)
        require(name in NATIVE_LIBRARIES, "unapproved dynamic dependency: " + name)
        require(name not in libraries, "duplicate dynamic dependency")
        libraries.append(name)
    require("/usr/lib/libSystem.B.dylib" in libraries, "missing native system library")
    return libraries


def validate_load_commands(output, binary, libraries):
    blocks = re.split(r"(?m)^Load command [0-9]+\n", output)
    require(blocks[0] == str(binary) + ":\n" and len(blocks) > 1, "unexpected otool load-command header")
    loaded, linkers = [], []
    for block in blocks[1:]:
        commands = re.findall(r"(?m)^\s*cmd (LC_[A-Z0-9_]+)\s*$", block)
        require(len(commands) == 1, "missing or ambiguous load command")
        command = commands[0]
        require(command not in {"LC_RPATH", "LC_DYLD_ENVIRONMENT", "LC_ID_DYLIB"},
                "unapproved dynamic loader command: " + command)
        if command in DYLIB_COMMANDS or command == "LC_LOAD_DYLINKER":
            names = re.findall(r"(?m)^\s*name (\S+) \(offset [0-9]+\)\s*$", block)
            require(len(names) == 1, "missing or ambiguous loader name")
            (linkers if command == "LC_LOAD_DYLINKER" else loaded).append(names[0])
    require(linkers == ["/usr/lib/dyld"], "unexpected native dynamic linker")
    require(loaded == libraries, "load commands disagree with linked-library inventory")


def tool_output(*args):
    result = subprocess.run(args, check=True, capture_output=True, text=True, timeout=30)
    require(not result.stderr, "unexpected tool stderr")
    return result.stdout


def host_facts(target, runner_arch):
    architecture = validate_host(target, platform.system(), platform.machine(), runner_arch,
                                 tool_output("rustc", "-vV"))
    return {"target": target, "architecture": architecture, "rustc_release": "1.96.0",
            "macos_version": platform.mac_ver()[0]}


def verify_native(binary, fixture, target, runner_arch):
    facts = host_facts(target, runner_arch)
    require(stat.S_ISREG(binary.lstat().st_mode), "native binary must be a regular file, not a symlink")
    binary = binary.resolve(strict=True)
    with binary.open("rb") as source:
        validate_header(source.read(32), facts["architecture"])
    libraries = validate_libraries(tool_output("/usr/bin/otool", "-L", str(binary)), binary)
    validate_load_commands(tool_output("/usr/bin/otool", "-l", str(binary)), binary, libraries)
    version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    # Shared behavioral assertions only. Do not call the Linux ELF/package checker.
    # Every CLI subprocess has an unrelated cwd and env={"PATH": "/nonexistent"}.
    check.smoke(binary, fixture, version)
    return {**facts, "version": version, "linked_libraries": libraries,
            "native_smoke": "passed", "distribution_artifact": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, choices=TARGETS)
    parser.add_argument("--runner-arch", required=True, choices=("ARM64", "X64"))
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--host-only", action="store_true")
    group.add_argument("--binary", type=Path, help="trusted local release binary, never an untrusted download")
    parser.add_argument("--fixture", type=Path, default=ROOT / "examples/cli_project")
    args = parser.parse_args()
    try:
        facts = (host_facts(args.target, args.runner_arch) if args.host_only else
                 verify_native(args.binary, args.fixture, args.target, args.runner_arch))
    except (CheckError, check.PackageError, OSError, ValueError, KeyError, TypeError,
            subprocess.SubprocessError) as error:
        print("FAIL: " + str(error), file=sys.stderr)
        return 1
    print(json.dumps(facts, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
