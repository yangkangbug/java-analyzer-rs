"""Offline macOS gate regressions. No Mac binary, Apple tools or Java is run."""
import importlib.util
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("macos_runtime", Path(__file__).with_name("check-macos-runtime.py"))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

ARM = "aarch64-apple-darwin"
INTEL = "x86_64-apple-darwin"
SYSTEM = "/usr/lib/libSystem.B.dylib"


def rustc(target=ARM, version="1.96.0"):
    return f"rustc {version} (fixture)\nhost: {target}\nrelease: {version}\n"


def header(cpu=0x0100000c, magic=0xfeedfacf, kind=2):
    return struct.pack("<8I", magic, cpu, 0, kind, 2, 80, 0, 0)


def linked(binary, libraries=(SYSTEM,)):
    return str(binary) + ":\n" + "".join(
        f"\t{item} (compatibility version 1.0.0, current version 1351.0.0)\n" for item in libraries)


def commands(binary, libraries=(SYSTEM,), extra="", loader="/usr/lib/dyld"):
    output = f"{binary}:\nLoad command 0\n          cmd LC_LOAD_DYLINKER\n      cmdsize 32\n         name {loader} (offset 12)\n"
    for index, item in enumerate(libraries, 1):
        output += f"Load command {index}\n          cmd LC_LOAD_DYLIB\n      cmdsize 56\n         name {item} (offset 24)\n"
    return output + extra


class HostTests(unittest.TestCase):
    def test_exact_native_hosts(self):
        self.assertEqual(m.validate_host(ARM, "Darwin", "arm64", "ARM64", rustc()), "arm64")
        self.assertEqual(m.validate_host(INTEL, "Darwin", "x86_64", "X64", rustc(INTEL)), "x86_64")

    def test_wrong_os_architecture_runner_target_or_toolchain_fails(self):
        cases = [
            (ARM, "Linux", "arm64", "ARM64", rustc()),
            (ARM, "Darwin", "x86_64", "ARM64", rustc()),
            (ARM, "Darwin", "arm64", "X64", rustc()),
            (ARM, "Darwin", "arm64", "ARM64", rustc(INTEL)),
            (ARM, "Darwin", "arm64", "ARM64", rustc(version="1.97.0")),
            ("aarch64-unknown-linux-gnu", "Darwin", "arm64", "ARM64", rustc()),
            (ARM, "Darwin", "arm64", "ARM64", rustc() + "host: x86_64-apple-darwin\n"),
        ]
        for args in cases:
            with self.subTest(args=args), self.assertRaises(m.CheckError):
                m.validate_host(*args)


class MachOTests(unittest.TestCase):
    def test_both_thin_executable_headers(self):
        m.validate_header(header(), "arm64")
        m.validate_header(header(cpu=0x01000007), "x86_64")

    def test_wrong_cpu_elf_universal_truncated_or_not_executable_fails(self):
        for data in [header(cpu=0x01000007), b"\x7fELF" + bytes(28),
                     header(magic=0xcafebabe), header(kind=6), header()[:31]]:
            with self.subTest(data=data), self.assertRaises(m.CheckError):
                m.validate_header(data, "arm64")

    def test_system_libraries_need_not_exist_outside_dyld_cache(self):
        binary = Path("/build/java-analyzer")
        libs = [SYSTEM, "/usr/lib/libiconv.2.dylib", "/usr/lib/libresolv.9.dylib"]
        self.assertEqual(m.validate_libraries(linked(binary, libs), binary), libs)
        m.validate_load_commands(commands(binary, libs), binary, libs)

    def test_jvm_non_system_relative_and_rpath_libraries_fail(self):
        binary = Path("/build/java-analyzer")
        for dependency in ["/Library/Java/JavaVirtualMachines/jdk/lib/server/libjvm.dylib",
                           "/usr/lib/libjvm.dylib", "/opt/homebrew/lib/libunknown.dylib",
                           "@rpath/libjvm.dylib", "@loader_path/libnative.dylib", "libSystem.B.dylib"]:
            with self.subTest(dependency=dependency), self.assertRaises(m.CheckError):
                m.validate_libraries(linked(binary, [SYSTEM, dependency]), binary)

    def test_missing_malformed_duplicate_or_different_binary_linkage_fails(self):
        binary = Path("/build/java-analyzer")
        for output in [str(binary) + ":\n", linked(binary, [SYSTEM, SYSTEM]),
                       linked(Path("/other/binary")), linked(binary) + "unrecognized line\n",
                       linked(binary, ["/usr/lib/libiconv.2.dylib"])]:
            with self.subTest(output=output), self.assertRaises(m.CheckError):
                m.validate_libraries(output, binary)

    def test_rpath_environment_custom_loader_and_disagreeing_commands_fail(self):
        binary = Path("/build/java-analyzer")
        extras = ["Load command 2\n          cmd LC_RPATH\n         path /tmp/libs (offset 12)\n",
                  "Load command 2\n          cmd LC_DYLD_ENVIRONMENT\n         name DYLD_INSERT_LIBRARIES=/tmp/x (offset 12)\n"]
        outputs = [commands(binary, extra=extra) for extra in extras]
        outputs += [commands(binary, loader="/tmp/dyld"), commands(binary, [SYSTEM, "/usr/lib/libjvm.dylib"]),
                    str(binary) + ":\n", commands(Path("/other/binary"))]
        for output in outputs:
            with self.subTest(output=output), self.assertRaises(m.CheckError):
                m.validate_load_commands(output, binary, [SYSTEM])

    def test_invalid_linkage_is_rejected_before_any_native_smoke(self):
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory).resolve() / "java-analyzer"
            binary.write_bytes(header())
            def tool(*args):
                if args == ("rustc", "-vV"):
                    return rustc()
                if args == ("/usr/bin/otool", "-L", str(binary)):
                    return linked(binary, [SYSTEM, "/usr/lib/libjvm.dylib"])
                self.fail(f"unexpected command: {args}")
            with patch.object(m.platform, "system", return_value="Darwin"), \
                 patch.object(m.platform, "machine", return_value="arm64"), \
                 patch.object(m, "tool_output", side_effect=tool), \
                 patch.object(m.check, "smoke") as smoke, self.assertRaises(m.CheckError):
                m.verify_native(binary, Path(directory), ARM, "ARM64")
            smoke.assert_not_called()

    def test_valid_preflight_completes_before_shared_behavior_smoke(self):
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory).resolve() / "java-analyzer"
            binary.write_bytes(header())
            calls = []
            def tool(*args):
                calls.append(args)
                return {("rustc", "-vV"): rustc(),
                        ("/usr/bin/otool", "-L", str(binary)): linked(binary),
                        ("/usr/bin/otool", "-l", str(binary)): commands(binary)}[args]
            def smoke(actual_binary, fixture, version):
                self.assertEqual(actual_binary, binary)
                self.assertEqual(fixture, Path(directory))
                self.assertTrue(version)
                self.assertEqual(calls[-1], ("/usr/bin/otool", "-l", str(binary)))
            with patch.object(m.platform, "system", return_value="Darwin"), \
                 patch.object(m.platform, "machine", return_value="arm64"), \
                 patch.object(m, "tool_output", side_effect=tool), \
                 patch.object(m.check, "smoke", side_effect=smoke) as smoke_mock:
                facts = m.verify_native(binary, Path(directory), ARM, "ARM64")
            smoke_mock.assert_called_once()
            self.assertEqual(facts["native_smoke"], "passed")
            self.assertFalse(facts["distribution_artifact"])


if __name__ == "__main__":
    unittest.main()
