"""Offline input-security tests; never fetch or execute the legacy adapter."""
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location('legacy_adapter_fetch', Path(__file__).with_name('fetch-neovim-legacy-adapter.py'))
MOD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MOD)


class InputTests(unittest.TestCase):
    def test_fixed_immutable_public_source(self):
        self.assertEqual(MOD.COMMIT, 'd30ff6ba4ff67dae4a9862c67507bda4c0bb40ee')
        self.assertEqual(MOD.URL, 'https://raw.githubusercontent.com/yangkangbug/java-analyzer-rs/'
            + MOD.COMMIT + '/editors/neovim/lua/java_analyzer.lua')
        self.assertEqual(MOD.SHA256, 'a8be5c2bd9f85c2428ccc65492bccc31592f2131125412892736639c48e78ab6')
        self.assertEqual(MOD.SIZE, 26196)

    def test_wrong_size_or_hash_rejected(self):
        for data in (b'', b'x' * MOD.SIZE, b'x' * (MOD.SIZE + 1)):
            with self.assertRaises(ValueError): MOD.verify(data)

    def test_no_redirect_even_same_host(self):
        for url in (MOD.URL, 'http://raw.githubusercontent.com/file', 'file:///tmp/file', 'https://evil.example/file'):
            with self.assertRaises(ValueError):
                MOD.NoRedirects().redirect_request(None, None, 302, '', {}, url)

    def test_existing_or_symlink_output_never_replaced(self):
        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(MOD.urllib.request, 'build_opener') as build:
            base = Path(temporary)
            for output in (base, base / 'link'):
                if output != base: output.symlink_to(base / 'missing', target_is_directory=True)
                with self.assertRaises(ValueError): MOD.fetch(output)
            build.assert_not_called()

    def response(self, data):
        response = mock.MagicMock()
        response.status, response.url = 200, MOD.URL
        response.headers = {'Content-Length': str(len(data))}
        response.read.return_value = data
        response.__enter__.return_value = response
        return response

    def test_verified_bytes_only_in_new_owned_directory(self):
        data = b'public test Lua bytes'
        response = self.response(data)
        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(MOD, 'SIZE', len(data)), \
                mock.patch.object(MOD, 'SHA256', hashlib.sha256(data).hexdigest()), \
                mock.patch.object(MOD.urllib.request, 'build_opener') as build:
            build.return_value.open.return_value = response
            output = Path(temporary) / 'adapter'
            MOD.fetch(output)
            self.assertEqual((output / 'lua/java_analyzer.lua').read_bytes(), data)
            response.read.assert_called_once_with(len(data) + 1)
            _, kwargs = build.return_value.open.call_args
            self.assertEqual(kwargs, {'timeout': 20})

    def test_bad_download_creates_no_output(self):
        for changes in ({'status': 302}, {'url': 'https://evil.example/a'},
                {'headers': {'Content-Length': '999'}}, {}):
            response = self.response(b'x' * MOD.SIZE)
            for key, value in changes.items(): setattr(response, key, value)
            with tempfile.TemporaryDirectory() as temporary, mock.patch.object(MOD.urllib.request, 'build_opener') as build:
                build.return_value.open.return_value = response
                output = Path(temporary) / 'adapter'
                with self.assertRaises(ValueError): MOD.fetch(output)
                self.assertFalse(output.exists())


if __name__ == '__main__': unittest.main()
