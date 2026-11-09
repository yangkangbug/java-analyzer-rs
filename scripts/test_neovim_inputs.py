"""Offline security and pin regressions for optional Neovim development input."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location(
    'neovim_inputs', Path(__file__).with_name('fetch-neovim-input.py'))
MOD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MOD)


class NeovimInputs(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name).resolve()
        self.addCleanup(self.temp.cleanup)

    def archive(self, entries):
        path = self.root / 'input.tar.gz'
        with tarfile.open(path, 'w:gz') as tar:
            for name, value, kind in entries:
                info = tarfile.TarInfo(name)
                if kind == 'file':
                    info.size = len(value)
                    info.mode = 0o755 if name.endswith('/bin/nvim') else 0o644
                    tar.addfile(info, io.BytesIO(value))
                elif kind == 'link':
                    info.type = tarfile.SYMTYPE
                    info.linkname = value
                    tar.addfile(info)
                else:
                    info.type = tarfile.FIFOTYPE
                    tar.addfile(info)
        return path

    def test_checked_in_manifest_exact_official_assets(self):
        manifest = json.loads(Path(__file__).with_name('neovim-inputs.json').read_text())
        MOD.validate_manifest(manifest)
        self.assertEqual(manifest['version'], '0.11.5')
        self.assertEqual(set(manifest['platforms']),
                         {'linux-x86_64', 'macos-arm64', 'macos-x86_64'})
        for spec in manifest['platforms'].values():
            bad = json.loads(json.dumps(manifest))
            next(iter(bad['platforms'].values()))['url'] = 'https://evil.invalid/nvim.tar.gz'
            with self.assertRaises(MOD.InputError):
                MOD.validate_manifest(bad)
            self.assertEqual(len(spec['sha256']), 64)

    def test_redirect_only_to_official_release_asset_host(self):
        for url in ('http://github.com/a', 'https://example.com/a',
                    'https://github.com@evil.invalid/a', 'https://github.com:444/a'):
            with self.assertRaises(MOD.InputError):
                MOD.check_download_url(url)
        MOD.check_download_url('https://release-assets.githubusercontent.com/a?token=x')

    def test_cache_verifies_every_use_and_offline_miss(self):
        cache = self.root / 'cache'
        cache.mkdir()
        spec = {'size': 3, 'sha256': hashlib.sha256(b'abc').hexdigest(),
                'url': 'https://github.com/neovim/neovim/releases/download/v0.11.5/x'}
        with self.assertRaises(MOD.InputError):
            MOD.fetch(spec, cache, offline=True)
        entry = cache / spec['sha256']
        entry.write_bytes(b'abc')
        self.assertEqual(MOD.fetch(spec, cache, offline=True), entry)
        entry.write_bytes(b'xyz')
        with self.assertRaises(MOD.InputError):
            MOD.fetch(spec, cache, offline=True)

    def test_extract_rejects_traversal_duplicate_and_special(self):
        spec = {'root': 'nvim-test', 'max_entries': 100,
                'max_file': 1024, 'max_expanded': 102400}
        for entries in ([('../escape', b'x', 'file')],
                        [('nvim-test/a', b'x', 'file'), ('nvim-test/a', b'y', 'file')],
                        [('nvim-test/fifo', b'', 'fifo')],
                        [('nvim-test/evil', '../../escape', 'link')]):
            with self.subTest(entries=entries):
                path = self.archive(entries)
                with tempfile.TemporaryDirectory(dir=self.root) as td:
                    with self.assertRaises(MOD.InputError):
                        MOD.extract(path, spec, Path(td))
        self.assertFalse((self.root.parent / 'escape').exists())

    def test_extract_budget(self):
        path = self.archive([('nvim-test/a', b'12345', 'file')])
        spec = {'root': 'nvim-test', 'max_entries': 100,
                'max_file': 4, 'max_expanded': 102400}
        with self.assertRaises(MOD.InputError):
            MOD.extract(path, spec, self.root / 'stage')

    def test_link_copies_consume_materialized_output_budget(self):
        path = self.archive([('nvim-test/bin/nvim', b'x' * 16384, 'file'),
                             ('nvim-test/share/nvim/runtime/doc/uganda.txt', b'x', 'file')]
                            + [(f'nvim-test/link{i}', 'bin/nvim', 'link') for i in range(4)])
        spec = {'root': 'nvim-test', 'max_entries': 100,
                'max_file': 32768, 'max_expanded': 32768}
        with self.assertRaisesRegex(MOD.InputError, 'materialized output budget'):
            MOD.extract(path, spec, self.root / 'stage')

    def test_publish_never_replaces_existing_directory(self):
        stage, out = self.root / 'stage', self.root / 'out'
        stage.mkdir()
        out.mkdir()
        (stage / 'new').write_text('new')
        (out / 'keep').write_text('keep')
        with self.assertRaises((OSError, MOD.InputError)):
            MOD.publish_stage(stage, out)
        self.assertEqual((out / 'keep').read_text(), 'keep')
        self.assertFalse((out / 'new').exists())

    def test_publish_fresh(self):
        stage, out = self.root / 'stage', self.root / 'out'
        stage.mkdir()
        (stage / 'new').write_text('new')
        MOD.publish_stage(stage, out)
        self.assertEqual((out / 'new').read_text(), 'new')


if __name__ == '__main__':
    unittest.main()
