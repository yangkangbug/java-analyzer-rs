"""Offline standard-library security regressions; no Java/network required."""
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

spec = importlib.util.spec_from_file_location('fetch_inputs', Path(__file__).with_name('fetch-acceptance-inputs.py'))
f = importlib.util.module_from_spec(spec)
spec.loader.exec_module(f)


class InputSecurity(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.out = self.root / 'out'
        self.out.mkdir()
        self.spec = dict(root='jdk', max_entries=20, max_expanded=20000,
                         max_file=1024, jmod_sha256=hashlib.sha256(b'jmod').hexdigest())

    def tar(self, members):
        path = self.root / 'archive.tar.gz'
        with tarfile.open(path, 'w:gz') as archive:
            for name, data, kind in members:
                info = tarfile.TarInfo(name)
                if kind == 'file':
                    info.size = len(data)
                    archive.addfile(info, io.BytesIO(data))
                else:
                    info.type = tarfile.SYMTYPE if kind == 'link' else tarfile.FIFOTYPE
                    info.linkname = data if kind == 'link' else ''
                    archive.addfile(info)
        return path

    def extract(self, members):
        return f.extract_jdk(self.tar(members), self.spec, self.out, True)

    def test_names(self):
        for name in ('../x', '/tmp/x', 'a/../x', 'a\\x', 'C:/x', '.', ''):
            with self.subTest(name=name), self.assertRaises(f.InputError):
                f.safe_name(name)

    def test_good_tar_and_materialized_internal_link(self):
        self.extract([('jdk/jmods/java.base.jmod', b'jmod', 'file'),
                      ('jdk/legal/LICENSE', b'license', 'file'),
                      ('jdk/legal/sub/LICENSE', '../LICENSE', 'link')])
        link = self.out / 'jdk/legal/sub/LICENSE'
        self.assertFalse(link.is_symlink())
        self.assertEqual(link.read_bytes(), b'license')

    def test_bad_tar_entries(self):
        cases = [[('../outside', b'x', 'file')],
                 [('other/a', b'x', 'file')],
                 [('jdk/a', '/outside', 'link')],
                 [('jdk/a', '../../outside', 'link')],
                 [('jdk/a', 'missing', 'link')],
                 [('jdk/a', b'', 'fifo')],
                 [('jdk/a', b'a', 'file'), ('jdk/a', b'b', 'file')],
                 [('jdk/a', b'x' * 1025, 'file')],
                 [('jdk/a', 'b', 'link'), ('jdk/a/x', b'x', 'file')]]
        for members in cases:
            with self.subTest(members=members), self.assertRaises((f.InputError, OSError)):
                self.extract(members)

    def test_tar_count_and_expansion(self):
        self.spec['max_entries'] = 1
        with self.assertRaises(f.InputError):
            self.extract([('jdk/a', b'a', 'file'), ('jdk/b', b'b', 'file')])
        self.spec['max_entries'] = 20
        self.spec['max_expanded'] = 100
        with self.assertRaises(f.InputError):
            self.extract([('jdk/a', b'a', 'file')])

    def test_corrupt_gzip(self):
        path = self.tar([('jdk/a', b'a', 'file')])
        data = bytearray(path.read_bytes())
        data[-8] ^= 0xff
        path.write_bytes(data)
        with self.assertRaises((OSError, EOFError, tarfile.TarError)):
            f.extract_jdk(path, self.spec, self.out)

    def test_zip_traversal_symlink_and_expansion(self):
        for name, mode, payload in [('../x', 0, b'x'), ('link', 0o120777, b'target'),
                                    ('large', 0, b'x' * 1025)]:
            path = self.root / 'source.zip'
            with zipfile.ZipFile(path, 'w') as archive:
                info = zipfile.ZipInfo(name)
                info.external_attr = mode << 16
                archive.writestr(info, payload)
            with self.subTest(name=name), self.assertRaises(f.InputError):
                f.check_sources(path, self.spec, self.out)

    def test_zip_notices_and_crc(self):
        path = self.root / 'source.zip'
        with zipfile.ZipFile(path, 'w') as archive:
            archive.writestr('META-INF/LICENSE.txt', b'license')
            archive.writestr('META-INF/NOTICE.txt', b'notice')
        f.check_sources(path, self.spec, self.out)
        self.assertEqual((self.out / 'NOTICE.txt').read_bytes(), b'notice')
        path.write_bytes(path.read_bytes().replace(b'license', b'corrupt', 1))
        with self.assertRaises(zipfile.BadZipFile):
            f.check_sources(path, self.spec, self.out)

    def test_cache_reverified_and_offline_missing(self):
        data = b'input'
        spec = dict(size=len(data), sha256=hashlib.sha256(data).hexdigest(), url='https://example.invalid')
        path = self.root / spec['sha256']
        with self.assertRaises(f.InputError):
            f.fetch(spec, self.root, True)
        path.write_bytes(data)
        self.assertEqual(f.fetch(spec, self.root, True), path)
        path.write_bytes(b'wrong')
        with self.assertRaises(f.InputError):
            f.fetch(spec, self.root)
        path.unlink()
        path.symlink_to(self.root / 'absent')
        with self.assertRaises(f.InputError):
            f.fetch(spec, self.root)

    def test_symlink_ancestors_rejected(self):
        link = self.root / 'alias'
        link.symlink_to(self.out, target_is_directory=True)
        with self.assertRaises(f.InputError):
            f.reject_symlink_path(link / 'nested' / 'result')
        with self.assertRaises(f.InputError):
            f.fetch(dict(sha256='missing'), link, True)

    def test_default_extraction_does_not_materialize_tools(self):
        path = self.tar([('jdk/jmods/java.base.jmod', b'jmod', 'file'),
                         ('jdk/legal/LICENSE', b'license', 'file'),
                         ('jdk/bin/java', b'executable', 'file')])
        f.extract_jdk(path, self.spec, self.out)
        self.assertEqual(set(p.name for p in self.out.iterdir()),
                         {'java.base.jmod', 'jdk-legal'})

    def test_http_and_redirect_rejected(self):
        spec = dict(size=1, sha256='x', url='http://example.invalid/file')
        with self.assertRaises(f.InputError):
            f.fetch(spec, self.root)
        with self.assertRaises(f.InputError):
            f.HTTPSOnly().redirect_request(None, None, 302, '', {}, 'http://example.invalid')

    def test_download_bounds_and_cleanup(self):
        class Response(io.BytesIO):
            url = 'https://example.invalid/file'
            headers = {}
        spec = dict(size=1, sha256=hashlib.sha256(b'a').hexdigest(), url=Response.url)
        for payload in (b'ab', b'', b'b'):
            with patch.object(f.urllib.request, 'build_opener') as opener:
                opener.return_value.open.return_value = Response(payload)
                with self.assertRaises(f.InputError):
                    f.fetch(spec, self.root)
            self.assertEqual(list(self.root.iterdir()), [self.out])


if __name__ == '__main__':
    unittest.main()
