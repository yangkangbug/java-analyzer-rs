#!/usr/bin/env python3
"""Fetch one immutable public legacy adapter for the hosted CI negative control.

Network input setup only: never execute Lua, install software, update a checkout,
read credentials, follow redirects, or accept arbitrary URLs/revisions.
"""
import argparse
import hashlib
from pathlib import Path
import sys
import urllib.error
import urllib.request

COMMIT = 'd30ff6ba4ff67dae4a9862c67507bda4c0bb40ee'
URL = ('https://raw.githubusercontent.com/yangkangbug/java-analyzer-rs/'
    + COMMIT + '/editors/neovim/lua/java_analyzer.lua')
SHA256 = 'a8be5c2bd9f85c2428ccc65492bccc31592f2131125412892736639c48e78ab6'
SIZE = 26196


class NoRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError('legacy adapter redirects are forbidden')


def verify(data):
    if len(data) != SIZE or hashlib.sha256(data).hexdigest() != SHA256:
        raise ValueError('legacy adapter bytes do not match the pinned source')


def fetch(output):
    # Resolve only an existing parent (including the hosted macOS /var alias).
    # The output itself must be new and may never be a symlink or replaced.
    if output.is_symlink() or output.exists():
        raise ValueError('legacy adapter output must be new')
    destination = output.parent.resolve(strict=True) / output.name
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirects())
    request = urllib.request.Request(URL, headers={'User-Agent': 'java-analyzer-ci-input/1'})
    with opener.open(request, timeout=20) as response:
        if response.status != 200 or response.url != URL:
            raise ValueError('unexpected legacy adapter response')
        length = response.headers.get('Content-Length')
        if length is not None and length != str(SIZE):
            raise ValueError('unexpected legacy adapter Content-Length')
        data = response.read(SIZE + 1)
    verify(data)
    destination.mkdir(mode=0o700)
    (destination / 'lua').mkdir(mode=0o700)
    with (destination / 'lua/java_analyzer.lua').open('xb') as stream:
        stream.write(data)
    # Only public fixed identity, never user paths, credentials or source text.
    return {'commit': COMMIT, 'sha256': SHA256, 'bytes': SIZE}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    try:
        import json
        print(json.dumps(fetch(parser.parse_args().output)))
    except (OSError, ValueError, urllib.error.URLError):
        parser.exit(1, 'LEGACY ADAPTER INPUT FAILED: pinned input unavailable or output unsafe\n')


if __name__ == '__main__':
    main()
