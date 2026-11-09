#!/usr/bin/env python3
"""Optional network setup, never invoked by Cargo. Never executes Java.

Only the checked-in manifest selects URLs. Cache entries are SHA-256 addressed
and reverified on every use. Outputs must be fresh; temporary partial outputs
are removed on failure. No upstream artifacts are intended for redistribution.
"""
import argparse
import gzip
import hashlib
import http.client
import json
import os
from pathlib import Path, PurePosixPath
import posixpath
import shutil
import stat
import tarfile
import tempfile
import urllib.parse
import urllib.request
import zipfile
import zlib


class InputError(ValueError):
    pass


def safe_name(name):
    p = PurePosixPath(name)
    if (not name or name.startswith('/') or '\\' in name or ':' in name
            or '\x00' in name or '..' in p.parts or str(p) == '.'):
        raise InputError(f'unsafe archive name: {name!r}')
    return str(p)


def reject_symlink_path(path):
    absolute = path.absolute()
    if any(part.is_symlink() for part in (absolute, *absolute.parents)):
        raise InputError('symlink in input/output/cache path')


def verify(path, spec):
    reject_symlink_path(path)
    if path.is_symlink() or not path.is_file() or path.stat().st_size != spec['size']:
        raise InputError(f'invalid cached size/type: {path.name}')
    with path.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    if digest != spec['sha256']:
        raise InputError(f'checksum mismatch: {path.name}')


class HTTPSOnly(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if urllib.parse.urlsplit(newurl).scheme != 'https':
            raise InputError('refusing non-HTTPS redirect')
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def fetch(spec, cache, offline=False):
    reject_symlink_path(cache)
    cache.mkdir(parents=True, exist_ok=True)
    path = cache / spec['sha256']
    if path.exists() or path.is_symlink():
        verify(path, spec)  # Corruption fails closed, never silently downloads over it.
        return path
    if offline:
        raise InputError(f'offline cache miss: {spec["sha256"]}')
    if urllib.parse.urlsplit(spec['url']).scheme != 'https':
        raise InputError('only HTTPS input URLs are allowed')
    with tempfile.NamedTemporaryFile(dir=cache, prefix='.partial-', delete=False) as out:
        temporary = Path(out.name)
        try:
            opener = urllib.request.build_opener(HTTPSOnly())
            with opener.open(spec['url'], timeout=60) as response:
                if urllib.parse.urlsplit(response.url).scheme != 'https':
                    raise InputError('non-HTTPS response')
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
            os.replace(temporary, path)
        finally:
            temporary.unlink(missing_ok=True)
    return path


def check_sources(path, spec, destination):
    total = 0
    seen = set()
    with zipfile.ZipFile(path) as archive:
        entries = archive.infolist()
        if len(entries) > spec['max_entries']:
            raise InputError('ZIP file count budget exceeded')
        for entry in entries:
            name = safe_name(entry.filename)
            mode = entry.external_attr >> 16
            if name in seen or stat.S_ISLNK(mode) or (stat.S_IFMT(mode) not in (0, stat.S_IFREG, stat.S_IFDIR)):
                raise InputError('duplicate or special ZIP entry')
            seen.add(name)
            total += entry.file_size
            if entry.file_size > spec['max_file'] or total > spec['max_expanded']:
                raise InputError('ZIP expansion budget exceeded')
            if not entry.is_dir():
                with archive.open(entry) as stream:
                    data = stream.read(spec['max_file'] + 1)  # Also verifies CRC.
                if len(data) != entry.file_size:
                    raise InputError('ZIP size mismatch')
                if name in ('META-INF/LICENSE.txt', 'META-INF/NOTICE.txt'):
                    (destination / Path(name).name).write_bytes(data)
    if not {'META-INF/LICENSE.txt', 'META-INF/NOTICE.txt'} <= seen:
        raise InputError('missing source license or notice')


class BoundedReader:
    def __init__(self, stream, limit):
        self.stream, self.remaining = stream, limit

    def read(self, size=-1):
        data = self.stream.read(min(size if size >= 0 else self.remaining + 1,
                                    self.remaining + 1))
        self.remaining -= len(data)
        if self.remaining < 0:
            raise InputError('TAR expansion budget exceeded')
        return data


def extract_jdk(path, spec, destination, include_jdk=False):
    """Never creates archive symlinks: safe internal links become regular copies."""
    with tempfile.TemporaryDirectory(prefix='jdk-', dir=destination.parent) as temp:
        stage = Path(temp)
        seen, regular, links = set(), set(), []
        total = 0
        with gzip.open(path, 'rb') as compressed:
            bounded = BoundedReader(compressed, spec['max_expanded'])
            with tarfile.open(fileobj=bounded, mode='r|') as archive:
                for index, entry in enumerate(archive):
                    name = safe_name(entry.name)
                    if index >= spec['max_entries'] or name in seen:
                        raise InputError('TAR duplicate or file count budget exceeded')
                    seen.add(name)
                    if name.split('/')[0] != spec['root']:
                        raise InputError('unexpected JDK root')
                    if entry.size < 0 or entry.size > spec['max_file']:
                        raise InputError('TAR member size budget exceeded')
                    total += entry.size
                    if total > spec['max_expanded']:
                        raise InputError('TAR expansion budget exceeded')
                    target = stage / name
                    selected = (include_jdk or name.startswith(spec['root'] + '/legal/')
                                or name == spec['root'] + '/jmods/java.base.jmod')
                    if entry.isdir():
                        target.mkdir(parents=True, exist_ok=True)
                    elif entry.isfile():
                        regular.add(name)
                        if not selected:
                            continue
                        target.parent.mkdir(parents=True, exist_ok=True)
                        with archive.extractfile(entry) as source, target.open('xb') as out:
                            shutil.copyfileobj(source, out, 1024 * 1024)
                        if target.stat().st_size != entry.size:
                            raise InputError('truncated TAR member')
                        target.chmod(0o755 if entry.mode & 0o111 else 0o644)
                        regular.add(name)
                    elif entry.issym() or entry.islnk():
                        link = entry.linkname
                        if not link or link.startswith('/') or '\\' in link or ':' in link:
                            raise InputError('unsafe TAR link')
                        resolved = safe_name(posixpath.normpath(posixpath.join(
                            posixpath.dirname(name) if entry.issym() else '', link)))
                        if resolved.split('/')[0] != spec['root']:
                            raise InputError('escaping TAR link')
                        if selected:
                            links.append((name, resolved))
                    else:
                        raise InputError('special TAR entry forbidden')
            # Consume trailer too, verifying gzip CRC and the decompression budget.
            while bounded.read(1024 * 1024):
                pass
        for name, resolved in links:
            if resolved not in regular:
                raise InputError('TAR link must target a regular archive file')
            target = stage / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(stage / resolved, target)
        root = stage / spec['root']
        jmod = root / 'jmods/java.base.jmod'
        with jmod.open('rb') as stream:
            if hashlib.file_digest(stream, 'sha256').hexdigest() != spec['jmod_sha256']:
                raise InputError('java.base.jmod checksum mismatch')
        shutil.copyfile(jmod, destination / 'java.base.jmod')
        shutil.copytree(root / 'legal', destination / 'jdk-legal')
        if include_jdk:
            shutil.move(str(root), destination / 'jdk')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--offline', action='store_true')
    parser.add_argument('--include-oracle-jdk', action='store_true',
                        help='materialize compiler tools, but never execute them')
    args = parser.parse_args()
    specs = json.loads(Path(__file__).with_name('acceptance-inputs.json').read_text())
    try:
        reject_symlink_path(args.output)
        if args.output.exists() or args.output.is_symlink():
            raise InputError('output must not already exist')
        inputs = {key: fetch(spec, args.cache, args.offline) for key, spec in specs.items()}
        args.output.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='.acceptance-', dir=args.output.parent) as temp:
            stage = Path(temp) / 'inputs'
            stage.mkdir()
            check_sources(inputs['sources'], specs['sources'], stage)
            shutil.copyfile(inputs['sources'], stage / 'sources.jar')
            extract_jdk(inputs['jdk'], specs['jdk'], stage, args.include_oracle_jdk)
            stage.rename(args.output)
        print(f'VERIFIED inputs: {args.output}; no Java executed')
    except (OSError, ValueError, tarfile.TarError, zipfile.BadZipFile, EOFError, http.client.HTTPException,
            RuntimeError, zlib.error) as error:
        parser.exit(1, f'INPUT SETUP FAILED: {error}\n')


if __name__ == '__main__':
    main()
