"""Offline setup security regressions. All artifacts are synthetic ZIP data.

No network, Java executable, upstream build, or external artifact is required.
"""
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import stat
import struct
import tempfile
import unittest
from unittest.mock import patch
import warnings
import zipfile


loader = importlib.util.spec_from_file_location(
    'second_project_inputs', Path(__file__).with_name('fetch-second-project-inputs.py'))
f = importlib.util.module_from_spec(loader)
loader.loader.exec_module(f)


class SecondProjectInputs(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.cache = self.root / 'cache'
        self.platform = self.root / 'platform'
        self.output = self.root / 'output'
        self.cache.mkdir()
        self.platform.mkdir()
        self.manifest = json.loads(Path(__file__).with_name('second-project-inputs.json').read_text())
        self.specs = self.manifest['artifacts']
        self.source_root = 'gson-test'
        self.specs['sources']['root'] = self.source_root
        self.specs['sources']['required_notices'] = [self.source_root + '/LICENSE']
        for key, spec in self.specs.items():
            entries = [(name, b'license', 0) for name in spec['required_notices']]
            entries.append((self.source_root + '/src/Test.java' if key == 'sources'
                            else 'classes/Test.class', b'not executable', 0))
            self.install(key, self.archive(entries, spec['kind'] == 'jmod'))
        self.source_spec = self.specs['sources']

    def archive(self, entries, jmod=False):
        stream = io.BytesIO()
        with warnings.catch_warnings():
            warnings.simplefilter('ignore', UserWarning)
            with zipfile.ZipFile(stream, 'w') as archive:
                for name, data, mode in entries:
                    info = zipfile.ZipInfo(name)
                    info.create_system = 3
                    info.external_attr = mode << 16
                    archive.writestr(info, data)
        return (b'JM\x01\x00' if jmod else b'') + stream.getvalue()

    def install(self, key, data):
        spec = self.specs[key]
        spec.update(size=len(data), sha256=hashlib.sha256(data).hexdigest())
        path = ((self.cache / spec['sha256']) if key in f.DOWNLOADS
                else self.platform / spec['filename'])
        path.write_bytes(data)
        return path

    def setup(self):
        with patch.object(f._common.urllib.request, 'build_opener',
                          side_effect=AssertionError('offline tests must not access network')):
            return f.setup_inputs(self.manifest, self.cache, self.platform, self.output, True)

    def assert_no_partial(self):
        self.assertFalse(self.output.exists())
        self.assertEqual(list(self.root.glob('.second-project-*')), [])
        self.assertEqual(list(self.cache.glob('.partial-*')), [])

    def check_zip(self, entries, **changes):
        spec = dict(self.source_spec, **changes)
        path = self.root / 'test.zip'
        path.write_bytes(self.archive(entries))
        f.check_archive(path, spec, self.root / 'notices')

    def test_canonical_manifest_pins_and_interface(self):
        canonical = json.loads(Path(__file__).with_name('second-project-inputs.json').read_text())
        specs = f.validate_manifest(canonical)
        self.assertEqual(set(specs), set(f.FILENAMES))
        self.assertEqual(specs['sources']['root'], 'gson-' + canonical['project']['commit'])
        self.assertEqual(specs['jsr250']['required_notices'], [])
        self.assertNotIn('url', specs['java_base'])
        self.assertNotIn('url', specs['java_sql'])

    def test_offline_success_preserves_only_artifacts_and_notices(self):
        self.assertEqual(self.setup(), self.output)
        self.assertEqual({p.name for p in self.output.iterdir()}, set(f.FILENAMES.values()) | {'notices'})
        for key, spec in self.specs.items():
            f.verify(self.output / spec['filename'], spec)
        self.assertEqual((self.output / 'notices/sources/LICENSE').read_bytes(), b'license')
        self.assertEqual((self.output / 'notices/java_sql/legal/LICENSE').read_bytes(), b'license')
        self.assertFalse((self.output / 'notices/jsr250').exists())
        self.assertEqual(list(self.output.rglob('*.java')), [])
        self.assertEqual(json.loads((self.output / 'notices/provenance.json').read_text()), self.manifest)
        self.assertEqual(list(self.root.glob('.second-project-*')), [])

    def test_manifest_rejects_unsafe_pins_filenames_urls_and_sets(self):
        mutations = [
            ('sha256', '../cache-outside'), ('size', 0), ('max_entries', True),
            ('max_file', -1), ('filename', '../outside.zip'),
            ('url', 'http://codeload.github.com/google/gson/zip/test'),
            ('url', 'https://attacker.invalid/gson.zip'),
            ('url', 'https://user:password@codeload.github.com/test'),
            ('required_notices', ['../outside']),
            ('required_notices', ['a', 'a']), ('kind', 'tar'),
        ]
        for field, value in mutations:
            manifest = copy.deepcopy(self.manifest)
            manifest['artifacts']['sources'][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(f.InputError):
                f.validate_manifest(manifest)
        for key in ('java_base', 'java_sql'):
            manifest = copy.deepcopy(self.manifest)
            manifest['artifacts'][key]['url'] = 'https://example.invalid/jdk'
            with self.assertRaises(f.InputError):
                f.validate_manifest(manifest)
        manifest = copy.deepcopy(self.manifest)
        del manifest['artifacts']['java_sql']
        with self.assertRaises(f.InputError):
            f.validate_manifest(manifest)

    def test_offline_cache_miss_does_not_create_output(self):
        (self.cache / self.source_spec['sha256']).unlink()
        with self.assertRaisesRegex(f.InputError, 'offline cache miss'):
            self.setup()
        self.assert_no_partial()

    def test_cache_corruption_never_redownloaded_or_overwritten(self):
        path = self.cache / self.source_spec['sha256']
        before = path.read_bytes()
        for data in (b'wrong size', bytes([before[0] ^ 1]) + before[1:]):
            path.write_bytes(data)
            with self.subTest(size=len(data)), self.assertRaises(f.InputError):
                self.setup()
            self.assertEqual(path.read_bytes(), data)
            self.assert_no_partial()

    def test_cache_symlink_and_nonregular_entry_rejected(self):
        path = self.cache / self.source_spec['sha256']
        data = path.read_bytes()
        path.unlink()
        elsewhere = self.root / 'elsewhere'
        elsewhere.write_bytes(data)
        path.symlink_to(elsewhere)
        with self.assertRaises(f.InputError):
            self.setup()
        path.unlink()
        path.mkdir()
        with self.assertRaises(f.InputError):
            self.setup()
        self.assert_no_partial()

    def test_platform_corruption_fails_before_fetch(self):
        path = self.platform / 'java.sql.jmod'
        data = path.read_bytes()
        path.write_bytes(bytes([data[0] ^ 1]) + data[1:])
        with patch.object(f, 'fetch', side_effect=AssertionError('must check platform first')):
            with self.assertRaisesRegex(f.InputError, 'checksum mismatch'):
                self.setup()
        self.assert_no_partial()

    def test_platform_missing_and_symlink_rejected(self):
        path = self.platform / 'java.sql.jmod'
        data = path.read_bytes()
        path.unlink()
        with self.assertRaises(f.InputError):
            self.setup()
        elsewhere = self.root / 'jmod'
        elsewhere.write_bytes(data)
        path.symlink_to(elsewhere)
        with self.assertRaises(f.InputError):
            self.setup()
        self.assert_no_partial()

    def test_output_exists_never_clobbered(self):
        for kind in ('directory', 'file', 'symlink'):
            with self.subTest(kind=kind):
                if kind == 'directory':
                    self.output.mkdir()
                elif kind == 'file':
                    self.output.write_bytes(b'keep')
                else:
                    self.output.symlink_to(self.root / 'absent')
                with self.assertRaises(f.InputError):
                    self.setup()
                self.assertTrue(self.output.exists() or self.output.is_symlink())
                if kind == 'directory':
                    self.output.rmdir()
                else:
                    self.output.unlink()

    def test_symlink_ancestor_rejected_for_all_paths(self):
        alias = self.root / 'alias'
        alias.symlink_to(self.root, target_is_directory=True)
        for attribute in ('cache', 'platform', 'output'):
            original = getattr(self, attribute)
            setattr(self, attribute, alias / original.name)
            with self.subTest(attribute=attribute), self.assertRaises(f.InputError):
                self.setup()
            setattr(self, attribute, original)
        self.assert_no_partial()

    def test_path_overlap_rejected(self):
        for output in (self.cache / 'output', self.platform / 'output'):
            with self.subTest(output=output), self.assertRaisesRegex(f.InputError, 'disjoint'):
                f.setup_inputs(self.manifest, self.cache, self.platform, output, True)
        with self.assertRaisesRegex(f.InputError, 'disjoint'):
            f.setup_inputs(self.manifest, self.output / 'cache', self.platform, self.output, True)
        self.assert_no_partial()

    def test_unsafe_zip_names_rejected(self):
        for name in ('../escape', '/absolute', 'C:/file', 'root\\file', '', '.',
                     'gson-test/../file', 'gson-test//file', 'gson-test/./file',
                     'gson-test/line\nfeed'):
            # zipfile cannot write an empty filename; test the validator directly.
            if not name:
                with self.assertRaises(f.InputError):
                    f.safe_name(name)
                continue
            with self.subTest(name=name), self.assertRaises(f.InputError):
                self.check_zip([(name, b'x', 0)])

    def test_duplicate_and_file_directory_collision_rejected(self):
        names = [
            [('gson-test/a', b'a', 0), ('gson-test/a', b'b', 0)],
            [('gson-test/a', b'a', 0), ('gson-test/a/b', b'b', 0)],
            [('gson-test/a/b', b'b', 0), ('gson-test/a', b'a', 0)],
        ]
        for entries in names:
            with self.subTest(entries=entries), self.assertRaises(f.InputError):
                self.check_zip(entries)

    def test_zip_special_types_and_inconsistent_types_rejected(self):
        for mode in (stat.S_IFLNK, stat.S_IFIFO, stat.S_IFSOCK, stat.S_IFCHR, stat.S_IFBLK, stat.S_IFDIR):
            with self.subTest(mode=mode), self.assertRaises(f.InputError):
                self.check_zip([('gson-test/special', b'target', mode | 0o644)])
        with self.assertRaises(f.InputError):
            self.check_zip([('gson-test/file/', b'', stat.S_IFREG | 0o644)])

    def test_zip_count_file_expansion_and_total_expansion_bounded(self):
        entries = [('gson-test/LICENSE', b'license', 0), ('gson-test/file', b'x' * 20, 0)]
        for changes in ({'max_entries': 1}, {'max_file': 10}, {'max_expanded': 25}):
            with self.subTest(changes=changes), self.assertRaises(f.InputError):
                self.check_zip(entries, **changes)

    def test_missing_empty_or_directory_license_rejected(self):
        for entries in ([], [('gson-test/LICENSE', b'', 0)],
                        [('gson-test/LICENSE/', b'', stat.S_IFDIR | 0o755)]):
            with self.subTest(entries=entries), self.assertRaises(f.InputError):
                self.check_zip(entries)
        self.assertFalse((self.root / 'notices').exists())

    def test_unexpected_source_root_rejected(self):
        with self.assertRaisesRegex(f.InputError, 'source archive root'):
            self.check_zip([('different/LICENSE', b'license', 0)])

    def test_corrupt_nonnotice_entry_crc_blocks_notices_and_output(self):
        key = 'sources'
        original = (self.cache / self.specs[key]['sha256']).read_bytes()
        corrupted = original.replace(b'not executable', b'BAD executable', 1)
        self.install(key, corrupted)  # Isolate archive CRC from the outer pin check.
        with self.assertRaises(zipfile.BadZipFile):
            self.setup()
        self.assert_no_partial()
        self.assertFalse((self.root / 'notices').exists())

    def test_encrypted_zip_and_nul_names_rejected(self):
        data = bytearray(self.archive([('gson-test/LICENSE', b'license', 0)]))
        central = data.index(b'PK\x01\x02')
        for offset in (6, central + 8):
            struct.pack_into('<H', data, offset, 1)
        self.install('sources', data)
        with self.assertRaisesRegex(f.InputError, 'encrypted'):
            self.setup()
        self.assert_no_partial()
        data = self.archive([('gson-test/LICENSE', b'license', 0)])
        data = data.replace(b'gson-test/LICENSE', b'gson-test/LIC\x00NSE')
        self.install('sources', data)
        with self.assertRaises(f.InputError):
            self.setup()
        self.assert_no_partial()

    def test_jmod_header_verified_without_java(self):
        key = 'java_sql'
        path = self.platform / self.specs[key]['filename']
        self.install(key, b'JM\x02\x00' + path.read_bytes()[4:])
        with self.assertRaisesRegex(f.InputError, 'JMOD header'):
            self.setup()
        self.assert_no_partial()

    def test_late_output_arrival_preserved(self):
        check = f.check_archive

        def arrive(*args):
            check(*args)
            if not self.output.exists():
                self.output.mkdir()
                (self.output / 'keep').write_bytes(b'other owner')

        with patch.object(f, 'check_archive', side_effect=arrive):
            with self.assertRaises(FileExistsError):
                self.setup()
        self.assertEqual((self.output / 'keep').read_bytes(), b'other owner')
        self.assertEqual(list(self.root.glob('.second-project-*')), [])

    def test_publish_failure_leaves_no_output_or_stage(self):
        def fail(stage, output):
            self.assertFalse(output.exists())
            self.assertEqual({p.name for p in stage.iterdir()}, set(f.FILENAMES.values()) | {'notices'})
            raise OSError('synthetic publishing failure')

        with patch.object(f, 'publish_stage', side_effect=fail):
            with self.assertRaisesRegex(OSError, 'synthetic publishing failure'):
                self.setup()
        self.assert_no_partial()
        self.assertTrue((self.platform / 'java.base.jmod').exists())

    def test_atomic_publication_refuses_even_empty_existing_directory(self):
        stage = self.root / 'stage'
        stage.mkdir()
        (stage / 'complete').write_text('complete')
        self.output.mkdir()
        with self.assertRaises(FileExistsError):
            f.publish_stage(stage, self.output)
        self.assertEqual(list(self.output.iterdir()), [])
        self.assertTrue((stage / 'complete').is_file())
        self.output.rmdir()
        f.publish_stage(stage, self.output)
        self.assertFalse(stage.exists())
        self.assertEqual((self.output / 'complete').read_text(), 'complete')

    def test_unsupported_atomic_publication_fails_closed(self):
        with patch.object(f.sys, 'platform', 'unsupported'), patch.object(f.os, 'name', 'posix'):
            with self.assertRaisesRegex(f.InputError, 'atomic no-replace publication'):
                f.publish_stage(self.root / 'stage', self.output)
        self.assert_no_partial()

    def test_postcopy_verification_catches_source_changes(self):
        path = self.cache / self.source_spec['sha256']
        verify = f.verify

        def corrupt(source, spec):
            verify(source, spec)
            if source == path:
                data = path.read_bytes()
                path.write_bytes(bytes([data[0] ^ 1]) + data[1:])

        with patch.object(f, 'verify', side_effect=corrupt):
            with self.assertRaisesRegex(f.InputError, 'checksum mismatch'):
                f.verified_copy(path, self.root / 'copy.zip', self.source_spec)

    def test_fetch_download_limits_and_partial_cleanup(self):
        class Response(io.BytesIO):
            url = 'https://repo.maven.apache.org/maven2/example'
            headers = {}

        spec = dict(size=1, sha256=hashlib.sha256(b'a').hexdigest(), url=Response.url)
        for payload in (b'ab', b'', b'b'):
            with patch.object(f._common.urllib.request, 'build_opener') as opener:
                opener.return_value.open.return_value = Response(payload)
                with self.subTest(payload=payload), self.assertRaises(f.InputError):
                    f.fetch(spec, self.cache)
            self.assertEqual(list(self.cache.glob('.partial-*')), [])
            self.assertFalse((self.cache / spec['sha256']).exists())

    def test_redirects_and_responses_must_keep_official_https_origin(self):
        original = 'https://repo.maven.apache.org/maven2/input.jar'
        for destination in ('http://repo.maven.apache.org/input.jar',
                            'https://attacker.invalid/input.jar',
                            'https://repo.maven.apache.org:444/input.jar',
                            'https://user:password@repo.maven.apache.org/input.jar'):
            with self.subTest(destination=destination), self.assertRaises(f.InputError):
                f.OfficialHTTPSOnly(original).redirect_request(None, None, 302, '', {}, destination)

        class Response(io.BytesIO):
            url = 'https://attacker.invalid/file'
            headers = {}

        spec = dict(size=1, sha256=hashlib.sha256(b'a').hexdigest(), url=original)
        with patch.object(f.urllib.request, 'build_opener') as opener:
            opener.return_value.open.return_value = Response(b'a')
            with self.assertRaisesRegex(f.InputError, 'response origin'):
                f.fetch(spec, self.cache)
        self.assertEqual(list(self.cache.glob('.partial-*')), [])


if __name__ == '__main__':
    unittest.main()
