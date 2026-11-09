"""Offline archive-format/integrity regressions; no Java, Cargo build or downloads."""
import gzip
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("cli_packager", Path(__file__).with_name("package-cli.py"))
p = importlib.util.module_from_spec(spec)
spec.loader.exec_module(p)
c = p.check


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.archive = self.root / "test.tar.gz"
        self.destination = self.root / "unpacked"
        self.destination.mkdir()

    def unpack(self, members, format=tarfile.USTAR_FORMAT):
        with tarfile.open(self.archive, "w:gz", format=format) as archive:
            for name, kind, payload in members:
                info = tarfile.TarInfo(name)
                info.mode = 0o644
                if kind == "file":
                    info.size = len(payload)
                    archive.addfile(info, io.BytesIO(payload))
                else:
                    info.type = kind
                    info.linkname = "outside"
                    archive.addfile(info)
        return c.unpack(self.archive, self.destination)

    def test_deterministic_archive_bytes_and_metadata(self):
        files = {"docs/example.md": b"example\n", "bin/java-analyzer": b"binary"}
        first = p.archive_bytes(files, "package")
        second = p.archive_bytes(dict(reversed(list(files.items()))), "package")
        self.assertEqual(first, second)
        self.assertEqual(first[4:8], b"\0" * 4)  # gzip mtime
        self.archive.write_bytes(first)
        bundle = c.unpack(self.archive, self.destination)
        self.assertEqual((bundle / "docs/example.md").read_bytes(), b"example\n")
        self.assertEqual((bundle / "bin/java-analyzer").stat().st_mode & 0o777, 0o755)

    def test_atomic_publish_and_no_replacement(self):
        output = self.root / "published"
        files = {"package.tar.gz": b"archive", "package.tar.gz.sha256": b"checksum"}
        p.publish_directory(output, files)
        self.assertEqual({path.name: path.read_bytes() for path in output.iterdir()}, files)
        with self.assertRaises(c.PackageError):
            p.publish_directory(output, {"new": b"replacement"})
        self.assertEqual({path.name: path.read_bytes() for path in output.iterdir()}, files)

    def test_staging_write_failure_leaves_no_output_pair(self):
        output = self.root / "published"
        original = p.write_staged
        def fail_second(path, data):
            if path.name == "second":
                raise OSError("injected write failure")
            original(path, data)
        with patch.object(p, "write_staged", side_effect=fail_second), self.assertRaises(OSError):
            p.publish_directory(output, {"first": b"complete", "second": b"never published"})
        self.assertFalse(output.exists())
        self.assertEqual(list(self.root.glob(".java-analyzer-package-*")), [])

    def test_publish_failure_cleans_staging_without_final_output(self):
        output = self.root / "published"
        with patch.object(p, "rename_without_replacement", side_effect=OSError("injected publish failure")), self.assertRaises(OSError):
            p.publish_directory(output, {"first": b"a", "second": b"b"})
        self.assertFalse(output.exists())
        self.assertEqual(list(self.root.glob(".java-analyzer-package-*")), [])

    def test_publish_race_does_not_replace_even_empty_directory(self):
        output = self.root / "published"
        original = p.rename_without_replacement
        def race(source, destination):
            destination.mkdir()
            original(source, destination)
        with patch.object(p, "rename_without_replacement", side_effect=race), self.assertRaises(OSError):
            p.publish_directory(output, {"first": b"a", "second": b"b"})
        self.assertEqual(list(output.iterdir()), [])
        self.assertEqual(list(self.root.glob(".java-analyzer-package-*")), [])

    def test_ignored_package_inputs_fail_even_without_dirty_status(self):
        repo = self.root / "repository"
        repo.mkdir()
        subprocess.run(["git", "init", "-q", str(repo)], check=True)
        (repo / ".gitignore").write_text("*.pyc\n")
        subprocess.run(["git", "-C", str(repo), "add", ".gitignore"], check=True)
        subprocess.run(["git", "-C", str(repo), "-c", "user.name=Package Test", "-c",
                        "user.email=package-test@example.invalid", "-c", "commit.gpgsign=false",
                        "commit", "-qm", "fixture"], check=True)
        (repo / "docs").mkdir()
        (repo / "docs/private.pyc").write_bytes(b"private leftover")
        self.assertEqual(subprocess.check_output(["git", "-C", str(repo), "status", "--porcelain"]), b"")
        with patch.object(p, "ROOT", repo), self.assertRaises(c.PackageError):
            p.reject_ignored_inputs()
        (repo / "docs/private.pyc").unlink()
        (repo / "docs/intended.md").write_text("intentional developer addition")
        with patch.object(p, "ROOT", repo):
            p.reject_ignored_inputs()

    def test_repository_collection_excludes_nonignored_untracked_files(self):
        repo = self.root / "repository"
        repo.mkdir()
        subprocess.run(["git", "init", "-q", str(repo)], check=True)
        (repo / "docs").mkdir()
        (repo / "docs/reviewed.md").write_bytes(b"reviewed staged addition")
        (repo / "docs/private.md").write_bytes(b"untracked private leftover")
        subprocess.run(["git", "-C", str(repo), "add", "docs/reviewed.md"], check=True)
        with patch.object(p, "ROOT", repo):
            files = {}
            p.collect_repository_tree(files, "docs", p.tracked_paths())
        self.assertEqual(files, {"docs/reviewed.md": b"reviewed staged addition"})

    def test_indexed_input_symlink_ancestor_rejected(self):
        repo = self.root / "repository"
        (repo / "docs").mkdir(parents=True)
        outside = self.root / "outside"
        outside.mkdir()
        (outside / "file.md").write_bytes(b"private")
        (repo / "docs/linked").symlink_to(outside, target_is_directory=True)
        with patch.object(p, "ROOT", repo), self.assertRaises(c.PackageError):
            p.collect_repository_tree({}, "docs", {"docs/linked/file.md"})

    def test_mandatory_script_symlink_ancestor_rejected(self):
        repo = self.root / "repository"
        repo.mkdir()
        outside = self.root / "outside"
        outside.mkdir()
        (outside / "check-cli-package.py").write_bytes(b"private")
        (repo / "scripts").symlink_to(outside, target_is_directory=True)
        with patch.object(p, "ROOT", repo), self.assertRaises(c.PackageError):
            p.repository_bytes("scripts/check-cli-package.py")

    def test_untracked_source_input_rejected(self):
        repo = self.root / "repository"
        repo.mkdir()
        subprocess.run(["git", "init", "-q", str(repo)], check=True)
        (repo / "src").mkdir()
        (repo / "src/untracked.rs").write_bytes(b"unreviewed source")
        with patch.object(p, "ROOT", repo), self.assertRaisesRegex(c.PackageError, "untracked source inputs"):
            p.reject_ignored_inputs()

    def test_package_input_directory_symlink_rejected(self):
        actual = self.root / "actual"
        actual.mkdir()
        (actual / "file").write_bytes(b"outside contents")
        link = self.root / "link"
        link.symlink_to(actual, target_is_directory=True)
        with self.assertRaises(c.PackageError):
            p.collect_tree({}, link, "docs")

    def test_content_mutation_fails_independently_of_porcelain_status(self):
        original_source = {"src/lib.rs": "hash"}
        original_bundle = {"docs/CLI.md": b"first modification"}
        with patch.object(p, "reject_ignored_inputs"), patch.object(p, "source_inputs", return_value=original_source), \
             patch.object(p, "package_inputs", return_value={"docs/CLI.md": b"second modification"}), \
             self.assertRaisesRegex(c.PackageError, "package input content changed"):
            p.verify_inputs(original_source, original_bundle, self.root, "packager hash")
        with patch.object(p, "reject_ignored_inputs"), patch.object(p, "source_inputs", return_value={"src/lib.rs": "new hash"}), \
             self.assertRaisesRegex(c.PackageError, "source content changed"):
            p.verify_inputs(original_source, original_bundle, self.root, "packager hash")

    def test_unsafe_names(self):
        for name in ("../x", "/tmp/x", "p/../x", "p/./x", "p//x", "C:/x", "p\\x", "p/x\n", "", "."):
            with self.subTest(name=name), self.assertRaises(c.PackageError):
                c.safe_name(name)

    def test_bad_paths_are_not_written(self):
        for bad in ("../outside", "/tmp/outside", "p/../outside", "p//outside"):
            with self.subTest(path=bad), self.assertRaises(c.PackageError):
                self.unpack([("p/good", "file", b"x"), (bad, "file", b"x")])
            self.assertEqual(list(self.destination.iterdir()), [])

    def test_special_entries_rejected_before_extraction(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.DIRTYPE, tarfile.FIFOTYPE, tarfile.CHRTYPE):
            with self.subTest(kind=kind), self.assertRaises(c.PackageError):
                self.unpack([("p/good", "file", b"x"), ("p/special", kind, b"")])
            self.assertEqual(list(self.destination.iterdir()), [])

    def test_duplicate_collision_and_multiple_roots(self):
        for members in ([('p/a', 'file', b'a'), ('p/a', 'file', b'b')],
                        [('p/a', 'file', b'a'), ('p/a/b', 'file', b'b')],
                        [('p/a', 'file', b'a'), ('q/b', 'file', b'b')]):
            with self.subTest(members=members), self.assertRaises(c.PackageError):
                self.unpack(members)
            self.assertEqual(list(self.destination.iterdir()), [])

    def test_archive_limits(self):
        with patch.object(c, "MAX_ENTRIES", 1), self.assertRaises(c.PackageError):
            self.unpack([('p/a', 'file', b'a'), ('p/b', 'file', b'b')])
        with patch.object(c, "MAX_FILE", 1), self.assertRaises(c.PackageError):
            self.unpack([('p/a', 'file', b'ab')])
        with patch.object(c, "MAX_TOTAL", 100), self.assertRaises(c.PackageError):
            self.unpack([('p/a', 'file', b'a')])
        with patch.object(c, "MAX_ARCHIVE", 1), self.assertRaises(c.PackageError):
            self.unpack([('p/a', 'file', b'a')])

    def test_nonempty_destination_and_symlink_archive(self):
        (self.destination / "keep").write_text("keep")
        with self.assertRaises(c.PackageError):
            self.unpack([('p/a', 'file', b'a')])
        self.assertEqual((self.destination / "keep").read_text(), "keep")
        self.archive.unlink()
        self.archive.symlink_to(self.destination / "keep")
        with self.assertRaises(c.PackageError):
            c.regular_bytes(self.archive)

    def test_extended_formats_rejected(self):
        for format in (tarfile.GNU_FORMAT, tarfile.PAX_FORMAT):
            with self.subTest(format=format), self.assertRaises(c.PackageError):
                self.unpack([('p/' + 'x' * 110, 'file', b'a')], format=format)

    def test_bad_mode_and_metadata(self):
        for field, value in (("mode", 0o4777), ("uid", 1), ("mtime", 1), ("uname", "someone")):
            with tarfile.open(self.archive, "w:gz", format=tarfile.USTAR_FORMAT) as archive:
                info = tarfile.TarInfo("p/a")
                info.mode = 0o644
                setattr(info, field, value)
                archive.addfile(info)
            with self.subTest(field=field), self.assertRaises(c.PackageError):
                c.unpack(self.archive, self.destination)

    def test_concatenated_archives_and_trailing_data_rejected(self):
        original = gzip.decompress(p.archive_bytes({"a": b"data"}, "p"))
        for tail in (b"unexpected", original):
            self.archive.write_bytes(gzip.compress(original + tail))
            with self.subTest(tail_size=len(tail)), self.assertRaises(c.PackageError):
                c.unpack(self.archive, self.destination)

    def test_corrupt_gzip(self):
        payload = bytearray(p.archive_bytes({"a": b"data"}, "p"))
        payload[-8] ^= 0xff
        self.archive.write_bytes(payload)
        with self.assertRaises((OSError, EOFError)):
            c.unpack(self.archive, self.destination)

    def minimal_bundle(self):
        names = ("LICENSE", "NOTICE", "THIRD_PARTY_LICENSES.md", "PROVENANCE.md",
                 "docs/CLI_INSTALL.md", "docs/CLI.md", "docs/CLI_EXECUTION.md", "docs/CLI_PREVIEW.md",
                 "examples/cli_project/project.json", "examples/cli_project/requests.json",
                 "licenses/rust-1.96.0/COPYRIGHT-library.html", "check-cli-package.py", "bin/java-analyzer")
        files = {name: b"fixture" for name in names}
        files["BUILD.json"] = json.dumps({"schema_version": 1, "target": c.TARGET,
            "binary_sha256": c.sha256(b"fixture"), "runtime": {"java_required": False}}).encode()
        files["SHA256SUMS"] = "".join(f"{c.sha256(data)}  {name}\n" for name, data in sorted(files.items())).encode()
        bundle = self.root / "bundle"
        for name, data in files.items():
            target = bundle / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        return bundle

    def test_bundle_integrity_and_extra_missing_tampered_files(self):
        bundle = self.minimal_bundle()
        c.check_checksums(bundle)
        extra = bundle / "extra"
        extra.write_bytes(b"new")
        with self.assertRaises(c.PackageError):
            c.check_checksums(bundle)
        extra.unlink()
        (bundle / "LICENSE").write_bytes(b"tampered")
        with self.assertRaises(c.PackageError):
            c.check_checksums(bundle)
        (bundle / "LICENSE").unlink()
        with self.assertRaises(c.PackageError):
            c.check_checksums(bundle)

    def test_checksum_file_cannot_choose_another_archive(self):
        self.archive.write_bytes(b"archive")
        checksum = self.root / "sum"
        checksum.write_text(c.sha256(b"archive") + "  another.tar.gz\n")
        with self.assertRaises(c.PackageError):
            c.check_archive(self.archive, checksum)

    def test_bundle_symlink_and_duplicate_manifest(self):
        bundle = self.minimal_bundle()
        sums = bundle / "SHA256SUMS"
        original = sums.read_bytes()
        sums.write_bytes(original + original.splitlines(keepends=True)[0])
        with self.assertRaises(c.PackageError):
            c.check_checksums(bundle)
        sums.write_bytes(original)
        (bundle / "alias").symlink_to(bundle / "LICENSE")
        with self.assertRaises(c.PackageError):
            c.check_checksums(bundle)


if __name__ == "__main__":
    unittest.main()
