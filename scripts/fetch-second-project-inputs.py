#!/usr/bin/env python3
"""Optional pinned Gson input setup. Never executes Java or project scripts.

Only second-project-inputs.json selects downloads. Supply existing pinned JMOD
data with --platform-dir (for example FIRST_SETUP/jdk/jmods from the separate
first acceptance setup with --include-oracle-jdk). This helper never downloads
a JDK. Every artifact is reverified; --offline forbids network cache misses.
Outputs must be fresh. Sources remain a ZIP, not an executable/extracted tree.
External input bytes are local acceptance data, not package contents.
"""
import argparse
import ctypes
import http.client
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import sys
import tempfile
import urllib.parse
import urllib.request
import zipfile
import zlib


_loader = importlib.util.spec_from_file_location(
    'first_acceptance_inputs', Path(__file__).with_name('fetch-acceptance-inputs.py'))
_common = importlib.util.module_from_spec(_loader)
_loader.loader.exec_module(_common)
InputError = _common.InputError
verify = _common.verify
reject_symlink_path = _common.reject_symlink_path

FILENAMES = {
    'sources': 'gson.zip', 'junit': 'junit.jar', 'hamcrest': 'hamcrest.jar',
    'jsr250': 'jsr250.jar', 'java_base': 'java.base.jmod', 'java_sql': 'java.sql.jmod',
}
DOWNLOADS = ('sources', 'junit', 'hamcrest', 'jsr250')
PLATFORM = ('java_base', 'java_sql')


def https_origin(url):
    parts = urllib.parse.urlsplit(url)
    if parts.scheme != 'https' or not parts.hostname or parts.username or parts.password:
        raise InputError('only credential-free HTTPS input URLs are allowed')
    return parts.hostname, parts.port or 443


class OfficialHTTPSOnly(_common.HTTPSOnly):
    def __init__(self, url):
        self.origin = https_origin(url)

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if https_origin(newurl) != self.origin:
            raise InputError('refusing cross-origin artifact redirect')
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def fetch(spec, cache, offline=False):
    """Same pinned cache contract as the first setup, with same-origin redirects."""
    reject_symlink_path(cache)
    cache.mkdir(parents=True, exist_ok=True)
    path = cache / spec['sha256']
    if path.exists() or path.is_symlink():
        verify(path, spec)
        return path
    if offline:
        raise InputError(f'offline cache miss: {spec["sha256"]}')
    redirects = OfficialHTTPSOnly(spec['url'])
    with tempfile.NamedTemporaryFile(dir=cache, prefix='.partial-', delete=False) as out:
        temporary = Path(out.name)
        try:
            opener = urllib.request.build_opener(redirects)
            with opener.open(spec['url'], timeout=60) as response:
                if https_origin(response.url) != redirects.origin:
                    raise InputError('unexpected artifact response origin')
                length = response.headers.get('Content-Length')
                if length is not None and int(length) != spec['size']:
                    raise InputError('unexpected download Content-Length')
                total = 0
                while block := response.read(1024 * 1024):
                    total += len(block)
                    if total > spec['size']:
                        raise InputError('download size budget exceeded')
                    out.write(block)
            out.flush()
            verify(temporary, spec)
            try:
                os.link(temporary, path)  # Atomic cache install, never overwrites.
            except FileExistsError:
                verify(path, spec)
        finally:
            temporary.unlink(missing_ok=True)
    return path


def safe_name(name):
    """Reject aliases as well as traversal; ZIP names use POSIX separators."""
    result = _common.safe_name(name)
    if name.rstrip('/') != result or any(ord(c) < 32 for c in name):
        raise InputError(f'noncanonical archive name: {name!r}')
    return result


def validate_manifest(manifest):
    """Fail closed before using manifest values as paths or download budgets."""
    if manifest.get('format_version') != 1:
        raise InputError('unsupported input manifest version')
    specs = manifest.get('artifacts', {})
    if set(specs) != set(FILENAMES):
        raise InputError('unexpected artifact set')
    for key, spec in specs.items():
        if spec.get('filename') != FILENAMES[key]:
            raise InputError(f'unexpected artifact filename: {key}')
        if not re.fullmatch('[0-9a-f]{64}', spec.get('sha256', '')):
            raise InputError(f'invalid SHA-256 pin: {key}')
        for field in ('size', 'max_entries', 'max_expanded', 'max_file'):
            if type(spec.get(field)) is not int or spec[field] <= 0:
                raise InputError(f'invalid {field} budget: {key}')
        if spec.get('kind') != ('jmod' if key in PLATFORM else 'zip'):
            raise InputError(f'unexpected archive kind: {key}')
        notices = spec.get('required_notices')
        if not isinstance(notices, list) or len(notices) != len(set(notices)):
            raise InputError(f'invalid notice inventory: {key}')
        for notice in notices:
            if safe_name(notice) != notice:
                raise InputError(f'invalid notice path: {key}')
        if key in DOWNLOADS:
            url = urllib.parse.urlsplit(spec.get('url', ''))
            host = 'codeload.github.com' if key == 'sources' else 'repo.maven.apache.org'
            if (url.scheme != 'https' or url.hostname != host or url.port not in (None, 443)
                    or url.username or url.password or url.query or url.fragment):
                raise InputError(f'unexpected artifact URL: {key}')
        elif 'url' in spec:
            raise InputError('platform modules must be supplied locally, not downloaded')
    root = specs['sources'].get('root', '')
    if safe_name(root) != root or '/' in root:
        raise InputError('invalid source root')
    return specs


def check_archive(path, spec, notices_dir):
    """Validate every member/CRC, extracting only the fixed notice inventory.

    JMODs are prefixed ZIP data. No class, native library, tool, or project source
    is extracted here. Notice writes happen only after the entire archive passes.
    """
    reject_symlink_path(path)
    reject_symlink_path(notices_dir)
    if spec['kind'] == 'jmod':
        with path.open('rb') as stream:
            if stream.read(4) != b'JM\x01\x00':
                raise InputError('unsupported JMOD header')
    required = set(spec['required_notices'])
    notices, seen, files, directories = {}, set(), set(), set()
    total = 0
    with zipfile.ZipFile(path) as archive:
        entries = archive.infolist()
        if len(entries) > spec['max_entries']:
            raise InputError('ZIP file count budget exceeded')
        for entry in entries:
            name = safe_name(entry.orig_filename)
            if entry.orig_filename != entry.filename:
                raise InputError('truncated ZIP name')
            mode = entry.external_attr >> 16
            kind = stat.S_IFMT(mode)
            if name in seen or kind not in (0, stat.S_IFREG, stat.S_IFDIR):
                raise InputError('duplicate or special ZIP entry')
            if ((kind == stat.S_IFDIR and not entry.is_dir())
                    or (kind == stat.S_IFREG and entry.is_dir())):
                raise InputError('inconsistent ZIP entry type')
            if entry.flag_bits & 1 or entry.compress_type not in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED):
                raise InputError('encrypted or unsupported ZIP compression')
            if 'root' in spec and (name.split('/')[0] != spec['root']
                                   or (name == spec['root'] and not entry.is_dir())):
                raise InputError('unexpected source archive root')
            parents = {str(p) for p in PurePosixPath(name).parents if str(p) != '.'}
            if parents & files or (not entry.is_dir() and name in directories):
                raise InputError('ZIP file/directory collision')
            directories.update(parents)
            (directories if entry.is_dir() else files).add(name)
            seen.add(name)
            total += entry.file_size
            if (entry.file_size < 0 or entry.file_size > spec['max_file']
                    or total > spec['max_expanded'] or (entry.is_dir() and entry.file_size)):
                raise InputError('ZIP expansion budget exceeded')
            count, chunks = 0, []
            with archive.open(entry) as stream:
                while block := stream.read(min(1024 * 1024, spec['max_file'] - count + 1)):
                    count += len(block)
                    if count > spec['max_file'] or count > entry.file_size:
                        raise InputError('ZIP member expansion budget exceeded')
                    if name in required:
                        chunks.append(block)
            if count != entry.file_size:
                raise InputError('ZIP size mismatch')
            if name in required:
                if entry.is_dir() or not count:
                    raise InputError('required license/notice is not a nonempty file')
                notices[name] = b''.join(chunks)
    if required != set(notices):
        raise InputError('missing required license/notice entry')
    for name, data in notices.items():
        # Drop only the verified source root; preserve distinct upstream notices.
        relative = name[len(spec['root']) + 1:] if 'root' in spec else name
        target = notices_dir / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        reject_symlink_path(target)
        with target.open('xb') as out:
            out.write(data)


def verified_copy(source, target, spec):
    verify(source, spec)
    # Exclusive destination creation and post-copy hashing avoid trusting a file
    # merely because it was verified earlier in setup.
    with source.open('rb') as src, target.open('xb') as dst:
        remaining = spec['size']
        while block := src.read(min(1024 * 1024, remaining + 1)):
            remaining -= len(block)
            if remaining < 0:
                raise InputError('input changed during copy: size budget exceeded')
            dst.write(block)
    verify(target, spec)


def publish_stage(stage, output):
    """Atomic directory publication which cannot replace an existing output.

    Linux renameat2(RENAME_NOREPLACE) and Windows rename have the required
    semantics. Fail closed on unsupported systems, never use a check/rename race.
    Staging and output are siblings, so publication stays on one filesystem.
    """
    if os.name == 'nt':
        os.rename(stage, output)
    elif sys.platform.startswith('linux'):
        libc = ctypes.CDLL(None, use_errno=True)
        rename = getattr(libc, 'renameat2', None)
        if rename is None:
            raise InputError('atomic no-replace publication requires Linux renameat2 or Windows')
        rename.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p,
                           ctypes.c_uint]
        rename.restype = ctypes.c_int
        if rename(-100, os.fsencode(stage), -100, os.fsencode(output), 1):
            error = ctypes.get_errno()
            raise OSError(error, os.strerror(error), str(output))
    else:
        raise InputError('atomic no-replace publication requires Linux renameat2 or Windows')


def setup_inputs(manifest, cache, platform_dir, output, offline=False):
    specs = validate_manifest(manifest)
    cache, platform_dir, output = map(Path, (cache, platform_dir, output))
    for path in (cache, platform_dir, output):
        reject_symlink_path(path)
    if output.exists() or output.is_symlink():
        raise InputError('output must not already exist')
    if not platform_dir.is_dir():
        raise InputError('--platform-dir must be an existing JMOD directory')
    resolved = [p.resolve() for p in (cache, platform_dir, output)]
    for index, path in enumerate(resolved):
        if any(path == other or path in other.parents or other in path.parents
               for other in resolved[index + 1:]):
            raise InputError('cache, platform, and output directories must be disjoint')
    # Fail before network or output creation when local platform data is wrong.
    inputs = {key: platform_dir / specs[key]['filename'] for key in PLATFORM}
    for key, path in inputs.items():
        verify(path, specs[key])
    inputs.update({key: fetch(specs[key], cache, offline) for key in DOWNLOADS})
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.second-project-', dir=output.parent) as temp:
        stage = Path(temp) / 'inputs'
        stage.mkdir()
        for key, spec in specs.items():
            path = stage / spec['filename']
            verified_copy(inputs[key], path, spec)
            check_archive(path, spec, stage / 'notices' / key)
        (stage / 'notices').mkdir(exist_ok=True)
        (stage / 'notices' / 'provenance.json').write_text(
            json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
        reject_symlink_path(output)
        publish_stage(stage, output)
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache', type=Path, required=True)
    parser.add_argument('--platform-dir', type=Path, required=True,
                        help='existing directory containing the exact pinned JMOD files')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--offline', action='store_true')
    args = parser.parse_args()
    try:
        manifest = json.loads(Path(__file__).with_name('second-project-inputs.json').read_text())
        setup_inputs(manifest, args.cache, args.platform_dir, args.output, args.offline)
        print(f'VERIFIED second-project inputs: {args.output}; no Java executed')
    except (OSError, ValueError, zipfile.BadZipFile, EOFError, http.client.HTTPException,
            RuntimeError, zlib.error) as error:
        parser.exit(1, f'INPUT SETUP FAILED: {error}\n')


if __name__ == '__main__':
    main()
