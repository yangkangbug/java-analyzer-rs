#!/usr/bin/env python3
"""Offline fixed Gson LSP wire acceptance. No downloads, Java or project builds.

This is a handwritten protocol harness, not a real editor client test. Semantic
expectations come only from the previously reviewed second_project query plan.
The existing check-second-project.py owns the reviewed 16-query CLI oracle and
parser baseline. Its declaration oracle is reused for separate before/after disk
batches here; this wire check does not rerun or claim the parser-report result.
"""
import argparse
from collections import deque
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import selectors
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
QUERY_PREFIX = '// Unsaved Gson query 😀\r\n'.encode('utf-8')
TARGET_PREFIX = '// Unsaved Gson target 🐬\r\n// 第二行\r\n'.encode('utf-8')
MAX_HEADER = 8192
MAX_BODY = 16 * 1024 * 1024
MAX_OUTPUT = 32 * 1024 * 1024
MAX_STDERR = 1024 * 1024
MAX_MESSAGES = 128
QUERY_PATH = 'extras/src/main/java/com/google/gson/typeadapters/PostConstructAdapterFactory.java'
TARGET_PATH = 'gson/src/main/java/com/google/gson/TypeAdapterFactory.java'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def helpers():
    spec = importlib.util.spec_from_file_location('lsp_second_project', ROOT / 'scripts/check-second-project.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def position(data, byte):
    """Compute LSP UTF-16 lines independently from pinned UTF-8 source bytes."""
    require(type(byte) is int and 0 <= byte <= len(data), 'byte outside source')
    require(not (byte and data[byte - 1:byte + 1] == b'\r\n'), 'inside CRLF')
    parts = re.split(r'\r\n|\r|\n', data[:byte].decode('utf-8'))
    return {'line': len(parts) - 1,
            'character': len(parts[-1].encode('utf-16-le')) // 2}


def location(uri, data, start, end, prefix=b''):
    require(0 <= start < end <= len(data), 'invalid reviewed declaration range')
    return {'uri': uri, 'range': {
        'start': position(prefix + data, start + len(prefix)),
        'end': position(prefix + data, end + len(prefix))}}


def check_result(response, request_id, expected):
    require(isinstance(response, dict) and response.get('jsonrpc') == '2.0', 'invalid JSON-RPC response')
    require(type(response.get('id')) is type(request_id) and response['id'] == request_id,
            'wrong response ID')
    require('error' not in response and 'result' in response, 'request failed or omitted its result')
    if expected is not None:
        actual = response['result']
        require(isinstance(actual, dict) and set(actual) == {'uri', 'range'} and
                isinstance(actual['uri'], str) and isinstance(actual['range'], dict) and
                set(actual['range']) == {'start', 'end'}, 'invalid source Location')
        for point in actual['range'].values():
            require(isinstance(point, dict) and set(point) == {'line', 'character'} and
                    all(type(value) is int and value >= 0 for value in point.values()),
                    'invalid UTF-16 Position')
    require(response['result'] == expected, 'wrong exact source URI/UTF-16 range or refusal')


def check_desynchronized(response, request_id):
    """A known bad sync state must refuse navigation, not return a stale target.

    The fixed plan does not prescribe null versus an explicit invalid-state
    error. Only the documented desynchronization error is accepted here; an
    arbitrary failure cannot turn an unrelated regression into a passing test.
    """
    if isinstance(response, dict) and 'error' in response:
        require(response.get('jsonrpc') == '2.0' and type(response.get('id')) is type(request_id)
                and response['id'] == request_id and 'result' not in response, 'invalid refusal response')
        error = response['error']
        require(isinstance(error, dict) and type(error.get('code')) is int and error['code'] == -32602
                and isinstance(error.get('message'), str) and 'desynchron' in error['message'].lower(),
                'expected explicit desynchronized-buffer refusal, not an unrelated failure')
    else:
        check_result(response, request_id, None)


def check_closed(response, request_id):
    require(isinstance(response, dict) and response.get('jsonrpc') == '2.0'
            and type(response.get('id')) is type(request_id) and response['id'] == request_id
            and 'result' not in response, 'closed query did not produce an explicit refusal')
    error = response.get('error')
    require(isinstance(error, dict) and type(error.get('code')) is int and error['code'] == -32602
            and isinstance(error.get('message'), str)
            and 'open synchronized owned document' in error['message'].lower(),
            'expected closed-query refusal, not an unrelated failure')


def encode(value):
    body = json.dumps(value, ensure_ascii=False, separators=(',', ':'), allow_nan=False).encode('utf-8')
    require(len(body) <= 2 * 1024 * 1024, 'outgoing test request limit')
    return f'Content-Length: {len(body)}\r\n\r\n'.encode('ascii') + body


class Frames:
    """Bounded byte parser; malformed/truncated stdout can never be a pass."""
    def __init__(self):
        self.buffer = bytearray()
        self.length = None
        self.messages = deque()
        self.total = 0

    def feed(self, block):
        self.total += len(block)
        require(self.total <= MAX_OUTPUT, 'aggregate LSP stdout limit')
        self.buffer.extend(block)
        while True:
            if self.length is None:
                boundary = self.buffer.find(b'\r\n\r\n')
                if boundary < 0:
                    require(len(self.buffer) <= MAX_HEADER, 'LSP response header limit')
                    return
                require(boundary + 4 <= MAX_HEADER, 'LSP response header limit')
                lengths = []
                for line in bytes(self.buffer[:boundary]).split(b'\r\n'):
                    name, colon, value = line.partition(b':')
                    require(colon and name and name.isascii(), 'invalid LSP response header')
                    if name.lower() == b'content-length':
                        value = value.strip()
                        require(re.fullmatch(rb'[0-9]{1,9}', value) is not None, 'invalid Content-Length')
                        lengths.append(int(value))
                require(len(lengths) == 1 and 0 < lengths[0] <= MAX_BODY, 'missing/duplicate/oversized Content-Length')
                self.length = lengths[0]
                del self.buffer[:boundary + 4]
            if len(self.buffer) < self.length:
                return
            body = bytes(self.buffer[:self.length])
            del self.buffer[:self.length]
            self.length = None
            # Explicit UTF-8 decoding disallows JSON's UTF-16/32 autodetection.
            value = json.loads(body.decode('utf-8'), parse_constant=lambda _: (_ for _ in ()).throw(ValueError('nonfinite JSON')))
            require(isinstance(value, dict) and value.get('jsonrpc') == '2.0', 'invalid framed JSON-RPC object')
            require(len(self.messages) < MAX_MESSAGES, 'queued LSP response limit')
            self.messages.append(value)

    def eof(self):
        require(self.length is None and not self.buffer, 'truncated LSP response')


class Session:
    """POSIX harness containment, distinct from the server's own guarantees.

    All pipes are nonblocking, output and time are bounded, and failed runs kill
    their entire private process group before reaping its leader. WNOWAIT keeps
    the process-group identity reserved until cleanup, avoiding PID reuse races.
    """
    def __init__(self, command, cwd, timeout=60, total_timeout=240):
        require(os.name == 'posix' and hasattr(os, 'waitid') and hasattr(os, 'WNOWAIT'),
                'LSP harness requires POSIX waitid(WNOWAIT) process cleanup')
        require(all(math.isfinite(value) and value > 0 for value in (timeout, total_timeout)),
                'timeouts must be finite and positive')
        self.timeout = timeout
        self.deadline = time.monotonic() + total_timeout
        self.frames = Frames()
        self.stderr = bytearray()
        self.completed = set()
        self.notifications = 0
        self.closed = False
        self.process = subprocess.Popen([str(arg) for arg in command], cwd=cwd,
                                        env={'PATH': ''}, stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                        start_new_session=True, bufsize=0)
        self.selector = selectors.DefaultSelector()
        for name in ('stdout', 'stderr'):
            pipe = getattr(self.process, name)
            os.set_blocking(pipe.fileno(), False)
            self.selector.register(pipe, selectors.EVENT_READ, name)
        os.set_blocking(self.process.stdin.fileno(), False)

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()

    def until(self, seconds=None):
        return min(self.deadline, time.monotonic() + (self.timeout if seconds is None else seconds))

    def pump(self, deadline, pending=None):
        remaining = deadline - time.monotonic()
        require(remaining > 0, 'bounded LSP IO timeout')
        events = self.selector.select(remaining)
        require(events, 'bounded LSP IO timeout')
        written = 0
        for key, _ in events:
            if key.data == 'stdin':
                try:
                    written += os.write(key.fd, pending[:65536])
                except BlockingIOError:
                    pass
                continue
            try:
                block = os.read(key.fd, 65536)
            except BlockingIOError:
                continue
            if not block:
                self.selector.unregister(key.fileobj)
                if key.data == 'stdout':
                    self.frames.eof()
                continue
            if key.data == 'stdout':
                self.frames.feed(block)
            else:
                require(len(self.stderr) + len(block) <= MAX_STDERR, 'LSP stderr limit')
                self.stderr.extend(block)
        return written

    def send(self, value):
        pending = memoryview(encode(value))
        deadline = self.until()
        self.selector.register(self.process.stdin, selectors.EVENT_WRITE, 'stdin')
        try:
            while pending:
                pending = pending[self.pump(deadline, pending):]
        finally:
            self.selector.unregister(self.process.stdin)

    def notify(self, method, params=None):
        self.send({'jsonrpc': '2.0', 'method': method, 'params': params})

    def response(self, request_id):
        deadline = self.until()
        while True:
            while self.frames.messages:
                value = self.frames.messages.popleft()
                if 'id' not in value:
                    require(value.get('method') == 'window/logMessage' and
                            isinstance(value.get('params'), dict), 'unexpected server notification')
                    self.notifications += 1
                    require(self.notifications <= MAX_MESSAGES, 'aggregate notification limit')
                    continue
                require(type(value['id']) is type(request_id) and value['id'] == request_id,
                        'unexpected/duplicate terminal response')
                require(request_id not in self.completed, 'duplicate terminal response')
                self.completed.add(request_id)
                return value
            require(any(key.data == 'stdout' for key in self.selector.get_map().values()),
                    f'LSP exited before response; stderr={bytes(self.stderr[:2000])!r}')
            self.pump(deadline)

    def request(self, request_id, method, params):
        self.send({'jsonrpc': '2.0', 'id': request_id, 'method': method, 'params': params})
        return self.response(request_id)

    def finish(self):
        self.process.stdin.close()
        deadline = self.until(10)
        while self.selector.get_map():
            self.pump(deadline)
        while os.waitid(os.P_PID, self.process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is None:
            require(time.monotonic() < deadline, 'LSP exit timeout')
            time.sleep(0.005)
        require(not self.frames.messages, 'unexpected trailing LSP message')
        require(not self.stderr, f'LSP wrote unexpected stderr: {bytes(self.stderr[:2000])!r}')
        self.close()
        require(self.process.returncode == 0, f'LSP exit code {self.process.returncode}')

    def close(self):
        if self.closed:
            return
        self.closed = True
        try:
            # Do not poll/wait/reap before signaling this group.
            try:
                os.killpg(self.process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            self.process.wait(timeout=5)
        finally:
            self.selector.close()
            for pipe in (self.process.stdin, self.process.stdout, self.process.stderr):
                pipe.close()


def reviewed_case(fixture):
    """Freeze the original semantic assertion, without consulting engine output."""
    query, expected = fixture.plan[0], fixture.expected[0]
    require(query['id'] == 'cross_interface' and query['file'] == QUERY_PATH and query['byte'] == 1082
            and query['spelling'] == 'TypeAdapterFactory'
            and query['compilation'] == {'project': 'extras', 'source_set': 'main'}, 'reviewed query changed')
    require(expected == {'status': 'Resolved', 'targets': [{
        'origin': 'source', 'path': TARGET_PATH, 'kind': 'Class',
        'owner': 'com.google.gson.TypeAdapterFactory', 'name': 'TypeAdapterFactory',
        'start': 6402, 'end': 6420}]}, 'reviewed semantic target changed')
    require(fixture.owner[QUERY_PATH] == 'extras/main' and fixture.owner[TARGET_PATH] == 'gson/main',
            'query/target must have different reviewed source owners')
    source, target = fixture.sources[QUERY_PATH], fixture.sources[TARGET_PATH]
    require(source[1082:1100] == target[6402:6420] == b'TypeAdapterFactory', 'reviewed source spelling changed')
    return {'query_uri': (fixture.source_root / QUERY_PATH).as_uri(),
            'target_uri': (fixture.source_root / TARGET_PATH).as_uri(),
            'query_source': source, 'target_source': target,
            'query_byte': 1082, 'target_start': 6402, 'target_end': 6420}


def exercise(client, case):
    initialize = client.request('initialize', 'initialize', {
        'processId': None, 'rootUri': None,
        'capabilities': {'general': {'positionEncodings': ['utf-16']}}})
    caps = initialize.get('result', {}).get('capabilities', {})
    require(caps.get('positionEncoding') == 'utf-16' and caps.get('definitionProvider') is True,
            'missing UTF-16 definition capability')
    require(caps.get('textDocumentSync', {}).get('change') == 1
            and caps['textDocumentSync'].get('openClose') is True, 'missing full document synchronization')
    require(not any(name in caps for name in ('completionProvider', 'renameProvider', 'diagnosticProvider')),
            'preview advertised unsupported semantic features')
    client.notify('initialized', {})
    results = []

    def query(stage, query_prefix=b'', target_prefix=b'', refused=None):
        point = position(query_prefix + case['query_source'], len(query_prefix) + case['query_byte'])
        expected = None if refused else location(case['target_uri'], case['target_source'],
                                                 case['target_start'], case['target_end'], target_prefix)
        response = client.request(stage, 'textDocument/definition', {
            'textDocument': {'uri': case['query_uri']}, 'position': point})
        if refused == 'closed':
            check_closed(response, stage)
        elif refused == 'desynchronized':
            check_desynchronized(response, stage)
        else:
            check_result(response, stage, expected)
        # Record the actual wire outcome, including a possible explicit refusal.
        results.append({'stage': stage, 'position': point,
                        **{key: response[key] for key in ('result', 'error') if key in response}})
        return response.get('result')

    def open_document(which, version, prefix):
        client.notify('textDocument/didOpen', {'textDocument': {
            'uri': case[which + '_uri'], 'languageId': 'java', 'version': version,
            'text': (prefix + case[which + '_source']).decode('utf-8')}})

    def change(which, version, prefix):
        client.notify('textDocument/didChange', {
            'textDocument': {'uri': case[which + '_uri'], 'version': version},
            'contentChanges': [{'text': (prefix + case[which + '_source']).decode('utf-8')}]})

    # The preview only accepts requests from a synchronized open query buffer.
    # Baseline/recovery open the exact disk bytes, with the target still closed.
    open_document('query', -100, b'')
    baseline = query('disk_bytes_baseline')
    client.notify('textDocument/didClose', {'textDocument': {'uri': case['query_uri']}})
    open_document('query', -11, QUERY_PREFIX)
    open_document('target', -23, TARGET_PREFIX)
    query('two_unsaved_opens', QUERY_PREFIX, TARGET_PREFIX)
    change('query', -7, QUERY_PREFIX * 2)
    change('target', -3, TARGET_PREFIX * 2)
    query('negative_nonconsecutive_versions', QUERY_PREFIX * 2, TARGET_PREFIX * 2)
    change('target', 100, TARGET_PREFIX * 3)
    query('dependency_only_change', QUERY_PREFIX * 2, TARGET_PREFIX * 3)
    change('target', 100, TARGET_PREFIX * 4)
    query('duplicate_version_refused', QUERY_PREFIX * 2, refused='desynchronized')
    change('target', 104, TARGET_PREFIX * 4)
    query('valid_full_replacement_recovers', QUERY_PREFIX * 2, TARGET_PREFIX * 4)
    client.notify('textDocument/didClose', {'textDocument': {'uri': case['target_uri']}})
    query('target_close_reads_disk', QUERY_PREFIX * 2)
    client.notify('textDocument/didClose', {'textDocument': {'uri': case['query_uri']}})
    query('closed_query_refused', refused='closed')
    open_document('query', -101, b'')
    require(query('reopened_disk_bytes_restore_baseline') == baseline, 'closed overlays were retained')
    client.notify('textDocument/didClose', {'textDocument': {'uri': case['query_uri']}})
    shutdown = client.request('shutdown', 'shutdown', None)
    check_result(shutdown, 'shutdown', None)
    client.notify('exit')
    client.finish()
    return results


def java_inventory(root):
    return {path.relative_to(root).as_posix(): digest(path) for path in root.rglob('*.java')}


def run(native, inputs):
    helper = helpers()
    with tempfile.TemporaryDirectory(prefix='java-lsp-gson-') as scratch:
        # A distinct cwd also catches accidental manifest-relative path rebasing.
        fixture = helper.prepare_fixture(inputs, Path(scratch) / 'fixture')
        sources = java_inventory(fixture.source_root)
        pinned = {key: digest(path) for key, path in fixture.inputs.items()}
        require(len(sources) == 226 and len(pinned) == 6, 'complete input inventory changed')
        case = reviewed_case(fixture)
        native_hash = digest(native)
        try:
            baseline_cli = helper.successful(native, fixture, 'declaration')
            helper.check_response('declaration', baseline_cli, fixture)
            require('overlay_state' not in baseline_cli['result'], 'CLI disk baseline gained overlay metadata')
            with Session([native, 'lsp', '--manifest', fixture.manifest], Path(scratch)) as client:
                results = exercise(client, case)
            restored_cli = helper.successful(native, fixture, 'declaration')
            helper.check_response('declaration', restored_cli, fixture)
            require('overlay_state' not in restored_cli['result'], 'CLI disk restoration retained overlays')
            require(baseline_cli['result']['queries'] == restored_cli['result']['queries'],
                    'independent CLI disk declaration batch changed')
        finally:
            require(java_inventory(fixture.source_root) == sources, 'disk Java paths or hashes changed')
            require({key: digest(path) for key, path in fixture.inputs.items()} == pinned,
                    'pinned external input hashes changed')
        require(digest(native) == native_hash, 'native binary changed during acceptance')
        return {'status': 'passed', 'project_commit': fixture.metadata['commit'],
                'native_sha256': native_hash, 'source_count': len(fixture.sources),
                'all_java_paths_and_hashes_unchanged': len(sources),
                'pinned_input_sha256_unchanged': pinned,
                'query_plan_sha256': digest(helper.CORPUS / 'query-plan.json'),
                'reviewed_expectations_sha256': digest(helper.CORPUS / 'expected.json'),
                'lsp_query': 'cross_interface', 'query_owner': 'extras/main', 'target_owner': 'gson/main',
                'baseline_query_buffer': 'Opened original disk bytes; target remains closed and reads disk',
                'unsaved_files': 2, 'query_prefix_utf8_bytes': len(QUERY_PREFIX),
                'target_prefix_utf8_bytes': len(TARGET_PREFIX), 'stages': results,
                'runtime': 'native_rust_no_jvm', 'child_path': '', 'upstream_code_executed': False,
                'scope': 'Handwritten LSP wire proof, not an actual editor client or new semantic oracle',
                'separate_cli_disk_evidence': {'baseline_queries': len(baseline_cli['result']['queries']),
                    'restored_queries': len(restored_cli['result']['queries']),
                    'overlay_metadata_absent': True, 'reviewed_query_results_exactly_equal': True},
                'parser_evidence': 'Not rerun here; check-second-project.py retains the parser-report baseline',
                'cleanup': 'Bounded harness process-group cleanup, not a server descendant-reaping proof'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('inputs_dir', type=Path)
    parser.add_argument('--native', type=Path, default=ROOT / 'target/release/java-analyzer')
    args = parser.parse_args()
    print(json.dumps(run(args.native.resolve(strict=True), args.inputs_dir.resolve(strict=True)), indent=2))


if __name__ == '__main__':
    main()
