#!/usr/bin/env python3
"""Fetch and safely unpack exact official Neovim dev inputs. Never runs them.

No system installation, project execution, Java, plugins, or user config access.
Only the checked-in manifest selects downloads. Use --offline for verified cache.
"""
import argparse
import ctypes
import gzip
import hashlib
import http.client
import importlib.util
import json
import os
from pathlib import Path
import platform
import posixpath
import re
import shutil
import sys
import tarfile
import tempfile
import urllib.parse
import urllib.request
import zlib


_spec = importlib.util.spec_from_file_location(
    'acceptance_input_common', Path(__file__).with_name('fetch-acceptance-inputs.py'))
_common = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_common)
InputError = _common.InputError
verify = _common.verify
reject_symlink_path = _common.reject_symlink_path
ALLOWED_HOSTS = {'github.com', 'release-assets.githubusercontent.com'}
PLATFORMS = {'linux-x86_64', 'macos-arm64', 'macos-x86_64'}


def validate_manifest(manifest):
    if (manifest.get('format_version') != 1 or manifest.get('version') != '0.11.5'
            or set(manifest.get('platforms', {})) != PLATFORMS
            or manifest.get('source_commit') != '9fa7c0a04638ee3d5fefbdcec347b1c07a5ce9ce'):
        raise InputError('unsupported Neovim manifest')
    for name, spec in manifest['platforms'].items():
        expected = f'https://github.com/neovim/neovim/releases/download/v0.11.5/nvim-{name}.tar.gz'
        if spec.get('url') != expected or spec.get('root') != 'nvim-' + name:
            raise InputError('unexpected official asset URL/root')
        if not re.fullmatch('[0-9a-f]{64}', spec.get('sha256', '')):
            raise InputError('invalid pinned checksum')
        for key, cap in [('size', 16777216), ('max_entries', 10000),
                         ('max_file', 33554432), ('max_expanded', 134217728)]:
            if type(spec.get(key)) is not int or not 0 < spec[key] <= cap:
                raise InputError('invalid input budget: ' + key)


def check_download_url(url):
    parsed = urllib.parse.urlsplit(url)
    if (parsed.scheme != 'https' or parsed.hostname not in ALLOWED_HOSTS
            or parsed.username or parsed.password or parsed.port not in (None, 443)):
        raise InputError('only official credential-free HTTPS release hosts are allowed')


class OfficialRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        check_download_url(newurl)
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def fetch(spec, cache, offline=False):
    reject_symlink_path(cache)
    cache.mkdir(parents=True, exist_ok=True)
    path = cache / spec['sha256']
    if path.exists() or path.is_symlink():
        verify(path, spec)
        return path
    if offline:
        raise InputError('offline cache miss: ' + spec['sha256'])
    check_download_url(spec['url'])
    with tempfile.NamedTemporaryFile(dir=cache, prefix='.partial-', delete=False) as stream:
        temporary = Path(stream.name)
        try:
            opener = urllib.request.build_opener(OfficialRedirects())
            with opener.open(spec['url'], timeout=60) as response:
                check_download_url(response.url)
                length = response.headers.get('Content-Length')
                if length is not None and int(length) != spec['size']:
                    raise InputError('unexpected download Content-Length')
                total = 0
                while block := response.read(1024 * 1024):
                    total += len(block)
                    if total > spec['size']:
                        raise InputError('download size budget exceeded')
                    stream.write(block)
            stream.flush()
            verify(temporary, spec)
            try:
                os.link(temporary, path)
            except FileExistsError:
                verify(path, spec)
        finally:
            temporary.unlink(missing_ok=True)
    return path


def safe_name(name):
    result = _common.safe_name(name)
    if name.rstrip('/') != result or any(ord(c) < 32 for c in name):
        raise InputError('noncanonical archive member')
    return result


def extract(path, spec, destination):
    """Extract data only; internal file links become ordinary copies, never links."""
    destination.mkdir(parents=True, exist_ok=True)
    seen, regular, links = set(), set(), []
    materialized = 0
    with gzip.open(path, 'rb') as compressed:
        bounded = _common.BoundedReader(compressed, spec['max_expanded'])
        with tarfile.open(fileobj=bounded, mode='r|') as archive:
            for index, entry in enumerate(archive):
                name = safe_name(entry.name)
                if (index >= spec['max_entries'] or name in seen
                        or name.split('/')[0] != spec['root']):
                    raise InputError('duplicate/member-count/unexpected-root failure')
                seen.add(name)
                if entry.size < 0 or entry.size > spec['max_file']:
                    raise InputError('archive member size budget exceeded')
                target = destination / name
                if entry.isdir():
                    target.mkdir(parents=True, exist_ok=True)
                elif entry.isfile():
                    materialized += entry.size
                    if materialized > spec['max_expanded']:
                        raise InputError('materialized output budget exceeded')
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with archive.extractfile(entry) as source, target.open('xb') as out:
                        shutil.copyfileobj(source, out, 1024 * 1024)
                    if target.stat().st_size != entry.size:
                        raise InputError('truncated archive member')
                    target.chmod(0o755 if entry.mode & 0o111 else 0o644)
                    regular.add(name)
                elif entry.issym() or entry.islnk():
                    link = entry.linkname
                    if not link or link.startswith('/') or '\\' in link or ':' in link:
                        raise InputError('unsafe archive link')
                    resolved = safe_name(posixpath.normpath(posixpath.join(
                        posixpath.dirname(name) if entry.issym() else '', link)))
                    if resolved.split('/')[0] != spec['root']:
                        raise InputError('escaping archive link')
                    links.append((name, resolved))
                else:
                    raise InputError('special archive member forbidden')
        while bounded.read(1024 * 1024):
            pass
    for name, resolved in links:
        if resolved not in regular:
            raise InputError('archive links must target direct regular files')
        materialized += (destination / resolved).stat().st_size
        if materialized > spec['max_expanded']:
            raise InputError('materialized output budget exceeded by link copies')
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(destination / resolved, target)
        target.chmod((destination / resolved).stat().st_mode & 0o777)
    root = destination / spec['root']
    if not (root / 'bin/nvim').is_file() or not (root / 'share/nvim/runtime/doc/uganda.txt').is_file():
        raise InputError('missing executable or upstream licensing documentation')
    return root


def publish_stage(stage, output):
    """Atomic publication without overwriting even an empty output directory."""
    libc = ctypes.CDLL(None, use_errno=True)
    if sys.platform.startswith('linux'):
        rename = getattr(libc, 'renameat2', None)
        if rename is None:
            raise InputError('renameat2 unavailable')
        rename.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int,
                           ctypes.c_char_p, ctypes.c_uint]
        rename.restype = ctypes.c_int
        rc = rename(-100, os.fsencode(stage), -100, os.fsencode(output), 1)
    elif sys.platform == 'darwin':
        rename = getattr(libc, 'renamex_np', None)
        if rename is None:
            raise InputError('renamex_np unavailable')
        rename.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]
        rename.restype = ctypes.c_int
        rc = rename(os.fsencode(stage), os.fsencode(output), 4)  # RENAME_EXCL
    else:
        raise InputError('only Linux and macOS dev inputs are supported')
    if rc:
        error = ctypes.get_errno()
        raise OSError(error, os.strerror(error), str(output))


def host_platform():
    system, machine = platform.system(), platform.machine()
    value = ('macos-' if system == 'Darwin' else 'linux-') + machine
    if system not in ('Linux', 'Darwin') or value not in PLATFORMS:
        raise InputError('unsupported host: ' + system + ' ' + machine)
    return value


def setup(manifest, name, cache, output, offline=False):
    validate_manifest(manifest)
    reject_symlink_path(output)
    if output.exists() or output.is_symlink():
        raise InputError('output must not already exist')
    spec = manifest['platforms'][name]
    archive = fetch(spec, cache, offline)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.neovim-', dir=output.parent) as td:
        stage = Path(td) / 'stage'
        root = extract(archive, spec, stage)
        license_path = Path(__file__).resolve().parent.parent / 'editors/neovim/licenses/Neovim-0.11.5-LICENSE.txt'
        license_data = license_path.read_bytes()
        if hashlib.sha256(license_data).hexdigest() != manifest['license_sha256']:
            raise InputError('checked-in upstream license checksum mismatch')
        (root / 'LICENSE.txt').write_bytes(license_data)
        provenance = {'manifest': manifest, 'platform': name,
                      'binary_sha256': hashlib.sha256((root / 'bin/nvim').read_bytes()).hexdigest()}
        (root / 'INPUT.json').write_text(json.dumps(provenance, indent=2) + '\n', encoding='utf-8')
        reject_symlink_path(output)
        publish_stage(root, output)
    return output / 'bin/nvim'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--platform', choices=sorted(PLATFORMS))
    parser.add_argument('--offline', action='store_true')
    args = parser.parse_args()
    try:
        manifest = json.loads(Path(__file__).with_name('neovim-inputs.json').read_text())
        binary = setup(manifest, args.platform or host_platform(), args.cache, args.output, args.offline)
        print(f'VERIFIED Neovim development input: {binary}; no software executed')
    except (OSError, ValueError, tarfile.TarError, EOFError, http.client.HTTPException,
            RuntimeError, zlib.error) as error:
        parser.exit(1, f'NEOVIM INPUT SETUP FAILED: {error}\n')


if __name__ == '__main__':
    main()
