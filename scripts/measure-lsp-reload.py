#!/usr/bin/env python3
"""Offline Linux before/after LSP reload measurements, never a timing gate.

Only the supplied native analyzer binaries run. Existing repository helpers own
input validation, fixed source target assertions, bounded LSP IO, and /proc RSS
sampling. No downloads, project programs, JVM, Maven or Gradle are executed.
Build receipts are operator declarations, not historical build attestations.
"""
import argparse
from datetime import datetime, timezone
from functools import lru_cache
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import signal
import statistics
import stat
import subprocess
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parent.parent
PROJECTS = ('gson', 'commons')
WORKLOADS = ('same_generation_definition', 'target_edit_definition')
VARIANTS = ('baseline', 'candidate')
PREFIX_TEMPLATE = '// LSP reload measurement {iteration} 😀\r\n'
MAX_RECEIPT_BYTES = 64 * 1024
FIXTURES = (
    'scripts/measure-lsp-reload.py', 'scripts/check-lsp-project.py',
    'scripts/check-second-project.py', 'scripts/fetch-second-project-inputs.py',
    'scripts/second-project-inputs.json', 'scripts/check-cli-acceptance.py',
    'scripts/fetch-acceptance-inputs.py', 'scripts/acceptance-inputs.json',
    'scripts/measure-cli-preview.py', 'examples/second_project/query-plan.json',
    'examples/second_project/expected.json', 'examples/real_source/queries.tsv',
    'examples/real_source/targets.tsv',
)


class MeasurementError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise MeasurementError(message)


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def canonical_hash(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'),
                                     allow_nan=False).encode()).hexdigest()


@lru_cache(maxsize=1)
def helpers():
    result = {}
    for name, filename in [('lsp', 'check-lsp-project.py'), ('gson', 'check-second-project.py'),
                           ('commons', 'check-cli-acceptance.py'), ('measure', 'measure-cli-preview.py')]:
        spec = importlib.util.spec_from_file_location('lsp_measure_' + name, ROOT / 'scripts' / filename)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        result[name] = module
    return result


def git_bytes(*args, root=ROOT):
    completed = subprocess.run(['git', *args], cwd=root, stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15, check=True)
    require(len(completed.stdout) <= 16 * 1024 * 1024, 'git metadata exceeds bounded output')
    return completed.stdout


def rust_source_identity(revision=None, root=ROOT):
    """Exact build-input digest helper, also usable when preparing receipts.

    Inventory is {relative POSIX filename: SHA256(file bytes)}, sorted and encoded
    as json.dumps(inventory, sort_keys=True, separators=(',', ':')).encode(). Its
    SHA256 is rust_build_inputs_sha256. Include src/**/*.rs, Cargo.toml, Cargo.lock,
    rust-toolchain.toml only, never tooling/docs/full checkout dirtiness. With a
    40-hex revision, read that Git tree; without it, include all current Rust
    sources (including newly added files) and report HEAD as the source base.
    A supplied revision labels matching sources, not proof of the binary's build.
    """
    root = Path(root)
    fixed = ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml']
    if revision is not None:
        require(re.fullmatch(r'[a-f0-9]{40}', revision) is not None, 'invalid source revision')
        names = git_bytes('ls-tree', '-r', '-z', '--name-only', revision, '--', 'src', *fixed,
                          root=root).decode().split('\0')
        names = sorted(name for name in names if name in fixed or name.startswith('src/') and name.endswith('.rs'))
        inventory = {name: hashlib.sha256(git_bytes('show', revision + ':' + name, root=root)).hexdigest()
                     for name in names}
    else:
        revision = git_bytes('rev-parse', 'HEAD', root=root).decode().strip()
        paths = sorted(list((root / 'src').rglob('*.rs')) + [root / name for name in fixed])
        inventory = {path.relative_to(root).as_posix(): digest(path) for path in paths}
    require(set(fixed).issubset(inventory) and any(name.startswith('src/') for name in inventory),
            'missing Rust/build source inputs')
    return {'git_revision': revision, 'rust_build_input_files': inventory,
            'rust_build_inputs_sha256': canonical_hash(inventory)}


def read_receipt(path):
    """Validate and capture one regular non-symlink receipt, with a growth bound.

    O_NONBLOCK prevents a regular-file-to-FIFO replacement from blocking open;
    O_NOFOLLOW and descriptor checks refuse a replaced/symlink/nonregular leaf.
    As with other local input readers, ancestor checks are not an OS sandbox.
    The hash always describes the same captured bytes that were decoded.
    """
    path = Path(path).absolute()
    require(not any(part.is_symlink() for part in (path, *path.parents)), 'receipt path is a symlink')
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode), 'receipt must be a regular file')
    require(before.st_size <= MAX_RECEIPT_BYTES, 'build receipt exceeds limit')
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    try:
        opened = os.fstat(fd)
        require(stat.S_ISREG(opened.st_mode) and (opened.st_dev, opened.st_ino) == (before.st_dev, before.st_ino),
                'receipt was replaced or is not regular')
        require(opened.st_size <= MAX_RECEIPT_BYTES, 'build receipt exceeds limit')
        data = bytearray()
        while len(data) <= MAX_RECEIPT_BYTES:
            block = os.read(fd, MAX_RECEIPT_BYTES + 1 - len(data))
            if not block:
                break
            data.extend(block)
        require(len(data) <= MAX_RECEIPT_BYTES, 'build receipt grew beyond limit')
    finally:
        os.close(fd)
    def unique_pairs(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, 'duplicate JSON receipt field')
            result[key] = value
        return result
    receipt = json.loads(data.decode('utf-8'), object_pairs_hook=unique_pairs)
    require(isinstance(receipt, dict), 'build receipt must be a JSON object')
    return receipt, hashlib.sha256(data).hexdigest()


def portable_receipt(receipt, binary_hash):
    require(isinstance(receipt, dict), 'build receipt must be a JSON object')
    required = {'binary_sha256': r'[a-f0-9]{64}', 'git_revision': r'[a-f0-9]{40}',
                'rust_build_inputs_sha256': r'[a-f0-9]{64}', 'build_profile': r'release',
                'target': r'x86_64-unknown-linux-gnu'}
    for key, pattern in required.items():
        require(isinstance(receipt.get(key), str) and re.fullmatch(pattern, receipt[key]) is not None,
                'invalid build receipt field: ' + key)
    require(type(receipt.get('source_dirty')) is bool, 'receipt must state source_dirty')
    require(receipt['binary_sha256'] == binary_hash, 'receipt binary checksum mismatch')
    result = {key: receipt[key] for key in (*required, 'source_dirty')}
    kind = receipt.get('source_revision_kind', 'operator_declared_matching_source')
    require(kind in ('operator_declared_matching_source', 'source_equivalent', 'build_checkout'),
            'invalid source revision kind')
    result['source_revision_kind'] = kind
    if 'git_tree' in receipt:
        require(isinstance(receipt['git_tree'], str) and re.fullmatch(r'[a-f0-9]{40}', receipt['git_tree']),
                'invalid source tree')
        result['git_tree'] = receipt['git_tree']
    return result


def check_candidate_source(receipt, identity):
    for key in ('git_revision', 'rust_build_inputs_sha256'):
        require(receipt[key] == identity[key], 'candidate source receipt mismatch: ' + key)


def validate_options(repetitions, timeout, session_timeout, interval_ms):
    require(type(repetitions) is int and 1 <= repetitions <= 30, 'repetitions must be an integer in 1..30')
    for value, low, high, name in ((timeout, .1, 120, 'request timeout'),
                                  (session_timeout, 1, 7200, 'session timeout'),
                                  (interval_ms, 1, 100, 'sample interval')):
        require(type(value) in (int, float) and math.isfinite(value) and low <= value <= high,
                name + ' outside finite bounds')
    require(session_timeout >= timeout, 'session timeout must cover request timeout')


def output_paths(output):
    output = Path(output).absolute()
    protocol = output.with_suffix('.protocol.json')
    require(output != protocol, 'output must differ from protocol path')
    for path in (output, protocol):
        require(not path.exists() and not path.is_symlink(), 'output/protocol already exists')
        require(not any(parent.is_symlink() for parent in path.parents), 'output parent is a symlink')
    return output, protocol


def schedule(repetitions):
    return [(iteration, project, workload, variant)
            for iteration in range(repetitions + 1)
            for project in PROJECTS for workload in WORKLOADS
            for variant in (VARIANTS if iteration % 2 == 0 else tuple(reversed(VARIANTS)))]


class Sampling:
    """Bounded daemon sampling; always stop/join, including failed LSP operations."""
    def __init__(self, pid, interval, factory=None):
        self.sampler = (factory or helpers()['measure'].ProcessGroupSampler)(pid, interval)
        self.interval = interval
        self.stop = threading.Event()
        self.error = None
        self.thread = threading.Thread(target=self.run, name='lsp-measure-rss', daemon=True)

    def run(self):
        try:
            while not self.stop.is_set():
                self.sampler.sample(time.perf_counter())
                self.stop.wait(self.interval)
        except Exception as error:
            self.error = type(error).__name__

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *_):
        self.stop.set()
        self.thread.join(timeout=1)
        require(not self.thread.is_alive(), 'sampling thread did not stop within one second')
        require(self.error is None, 'sampling failed: ' + str(self.error))

    def result(self):
        facts = self.sampler.result()
        facts['thread_stopped'] = not self.thread.is_alive()
        facts['sampling_failure_category'] = self.error
        return facts


def start_client(command, cwd, timeout, total_timeout):
    # Session construction can fail after Popen but before its selector exists.
    # Keep the partial object so its private process group is still cleaned.
    cls = helpers()['lsp'].Session
    client = cls.__new__(cls)
    try:
        cls.__init__(client, command, cwd, timeout=timeout, total_timeout=total_timeout)
    except BaseException:
        process = getattr(client, 'process', None)
        if process is not None:
            try:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait(timeout=5)
            finally:
                selector = getattr(client, 'selector', None)
                if selector is not None:
                    selector.close()
                for stream in (process.stdin, process.stdout, process.stderr):
                    if stream is not None:
                        stream.close()
        raise
    return client


def close_clients(clients):
    failed = False
    interrupted = None
    for client in clients:
        try:
            client.close()
        except BaseException as error:
            failed = True
            if not isinstance(error, Exception) and interrupted is None:
                interrupted = error
    if interrupted is not None:
        raise interrupted
    require(not failed, 'one or more client cleanup operations failed')


def check_initialize(response):
    require(isinstance(response, dict) and response.get('jsonrpc') == '2.0' and
            response.get('id') == 'initialize' and 'error' not in response and
            isinstance(response.get('result'), dict), 'malformed LSP initialization response')
    caps = response['result'].get('capabilities')
    require(isinstance(caps, dict), 'malformed LSP capabilities')
    sync = caps.get('textDocumentSync')
    require(caps.get('positionEncoding') == 'utf-16' and caps.get('definitionProvider') is True and
            isinstance(sync, dict) and sync.get('change') == 1 and sync.get('openClose') is True,
            'wrong LSP initialization capabilities')


def portable_response(response, case):
    """Keep exact valid coordinates, never an arbitrary local URI/error message."""
    result = {'response_sha256': canonical_hash(response)}
    location = response.get('result')
    if isinstance(location, dict):
        fields = {'unexpected_uri': location.get('uri') != case['target_uri']}
        if not fields['unexpected_uri']:
            fields['target'] = case['target_relative']
        points = location.get('range')
        if isinstance(points, dict) and set(points) == {'start', 'end'} and all(
                isinstance(point, dict) and set(point) == {'line', 'character'} and all(
                    type(value) is int and 0 <= value <= 2**31 - 1 for value in point.values())
                for point in points.values()):
            fields['range'] = points
        result['result'] = fields
    elif location is None and 'result' in response:
        result['result'] = None
    if isinstance(response.get('error'), dict):
        code = response['error'].get('code')
        result['error'] = {'code': code if type(code) is int else None}
    return result


def source_inventory(root):
    return {path.relative_to(root).as_posix(): digest(path) for path in sorted(root.rglob('*.java'))}


def input_metadata(gson, commons, cases, inventories):
    # Do not copy the helpers' unrelated 45/16-query CLI or disk-edit metadata.
    gson_keys = ('project', 'commit', 'source_count', 'source_bytes', 'inputs', 'automatic_build_import',
                 'upstream_build_or_tests_executed', 'missing_generated_source', 'jpms_supported')
    commons_keys = ('name', 'source_count', 'source_bytes', 'platform_classes', 'source_artifact_sha256',
                    'source_artifact_bytes', 'platform_sha256', 'platform_bytes', 'input_provenance',
                    'source_inventory_sha256', 'normalized_manifest', 'normalized_manifest_sha256')
    result = {'gson': {key: gson[key] for key in gson_keys if key in gson},
              'commons': {key: commons[key] for key in commons_keys if key in commons},
              'source_inventory_sha256': {name: canonical_hash(value) for name, value in inventories.items()},
              'source_inventory_count': {name: len(value) for name, value in inventories.items()}}
    for name, case in cases.items():
        result[name]['measured_query_count'] = 1
        result[name]['definition'] = {'query_source': case['query_relative'], 'query_byte': case['query_byte'],
            'target_source': case['target_relative'], 'target_byte_range': [case['target_start'], case['target_end']],
            'query_buffer': 'original disk bytes captured by didOpen',
            'target_buffer': 'original disk bytes plus the latest measured prefix',
            'target_prefix_scheme': PREFIX_TEMPLATE, 'initial_document_version': 1}
    return result


def prepare_cases(stage, args):
    h = helpers()
    fixture = h['gson'].prepare_fixture(args.second_inputs_dir, stage / 'gson')
    gson = h['lsp'].reviewed_case(fixture)
    gson.update(manifest=fixture.manifest, source_root=fixture.source_root,
                query_relative=h['lsp'].QUERY_PATH, target_relative=h['lsp'].TARGET_PATH)
    commons_stage = stage / 'commons'
    commons_stage.mkdir()
    _, commons_metadata = h['measure'].prepare_commons(
        commons_stage, args.sources_jar, args.java_base_jmod, args.baseline, 30000)
    source_root = commons_stage / 'sources'
    query_relative = 'org/apache/commons/lang3/StringUtils.java'
    target_relative = 'org/apache/commons/lang3/ArrayUtils.java'
    # Freeze the existing independently reviewed q01 rather than infer from output.
    query_rows = h['commons'].rows(ROOT / 'examples/real_source/queries.tsv', 6)
    target_rows = h['commons'].rows(ROOT / 'examples/real_source/targets.tsv', 7)
    require(query_rows[0] == ['q01', query_relative, '309172', 'ArrayUtils', 'Resolved', '1'],
            'reviewed Commons query changed')
    require(target_rows[0] == ['q01', target_relative, 'Class', 'org.apache.commons.lang3.ArrayUtils',
                               'ArrayUtils', '2159', '2169'], 'reviewed Commons target changed')
    commons = {'manifest': commons_stage / 'config/project.json', 'source_root': source_root,
               'query_relative': query_relative, 'target_relative': target_relative,
               'query_uri': (source_root / query_relative).as_uri(),
               'target_uri': (source_root / target_relative).as_uri(),
               'query_source': (source_root / query_relative).read_bytes(),
               'target_source': (source_root / target_relative).read_bytes(),
               'query_byte': 309172, 'target_start': 2159, 'target_end': 2169}
    cases = {'gson': gson, 'commons': commons}
    inventories = {name: source_inventory(case['source_root']) for name, case in cases.items()}
    metadata = input_metadata(fixture.metadata, commons_metadata, cases, inventories)
    pins = {args.sources_jar: digest(args.sources_jar), args.java_base_jmod: digest(args.java_base_jmod),
            **{path: digest(path) for path in fixture.inputs.values()}}
    return cases, metadata, inventories, pins


def measure_definition(client, state, case, iteration, workload, variant, interval):
    lsp = helpers()['lsp']
    request_id = f'{iteration}:{workload}:{variant}'
    facts = {'iteration': iteration, 'project': state['project'], 'workload': workload,
             'variant': variant, 'warmup': iteration == 0, 'request_id': request_id, 'correct': False}
    sampling = Sampling(client.process.pid, interval)
    started = None
    try:
        with sampling:
            started = time.perf_counter()
            if workload == 'target_edit_definition':
                state['version'] += 1
                state['prefix'] = PREFIX_TEMPLATE.format(iteration=iteration).encode()
                client.notify('textDocument/didChange', {'textDocument': {
                    'uri': case['target_uri'], 'version': state['version']},
                    'contentChanges': [{'text': (state['prefix'] + case['target_source']).decode()}]})
            response = client.request(request_id, 'textDocument/definition', {
                'textDocument': {'uri': case['query_uri']},
                'position': lsp.position(case['query_source'], case['query_byte'])})
            facts['wall_ms'] = (time.perf_counter() - started) * 1000
        expected = lsp.location(case['target_uri'], case['target_source'], case['target_start'],
                                case['target_end'], state['prefix'])
        facts['expected'] = {'target': case['target_relative'], 'range': expected['range']}
        facts['actual'] = portable_response(response, case)
        facts['query'] = {'source': case['query_relative'], 'position': lsp.position(case['query_source'], case['query_byte'])}
        lsp.check_result(response, request_id, expected)
        facts['correct'] = True
    except Exception as error:
        facts['failure_category'] = type(error).__name__
    finally:
        facts.setdefault('wall_ms', (time.perf_counter() - started) * 1000 if started is not None else None)
        facts['proc_sampling'] = sampling.result()
    return facts


def summarize(samples, repetitions):
    required = schedule(repetitions)
    observed = [(row['iteration'], row['project'], row['workload'], row['variant']) for row in samples]
    require(observed == required, 'missing, duplicate or out-of-order samples')
    require(all(type(row['warmup']) is bool and row['warmup'] == (row['iteration'] == 0) for row in samples),
            'warmup labels must match the frozen schedule')
    require(all(row['correct'] and math.isfinite(row['wall_ms']) and row['wall_ms'] >= 0 for row in samples),
            'incorrect or invalid timing sample')
    require(all(row['proc_sampling']['available'] and
                type(row['proc_sampling']['peak_sum_rss_kib']) in (int, float) and
                math.isfinite(row['proc_sampling']['peak_sum_rss_kib']) and
                row['proc_sampling']['peak_sum_rss_kib'] > 0 for row in samples),
            'memory evidence is incomplete, not a pass')
    result = {}
    for project in PROJECTS:
        result[project] = {}
        for workload in WORKLOADS:
            result[project][workload] = {}
            for variant in VARIANTS:
                rows = [row for row in samples if not row['warmup'] and
                        (row['project'], row['workload'], row['variant']) == (project, workload, variant)]
                require(len(rows) == repetitions, 'wrong measured sample count')
                item = {'n': len(rows)}
                for key, values in [('wall_ms', [row['wall_ms'] for row in rows]),
                                    ('sampled_group_peak_rss_kib', [row['proc_sampling']['peak_sum_rss_kib'] for row in rows])]:
                    item[key] = {'min': min(values), 'median': statistics.median(values), 'max': max(values)}
                    item['raw_' + key] = values
                result[project][workload][variant] = item
    return result


def run(args):
    require(sys.platform == 'linux' and hasattr(os, 'waitid') and hasattr(os, 'WNOWAIT') and Path('/proc').is_dir(),
            'measurement requires Linux /proc and waitid(WNOWAIT)')
    validate_options(args.repetitions, args.timeout_seconds, args.session_timeout_seconds, args.sample_interval_ms)
    output, protocol_path = output_paths(args.output)
    h = helpers()
    result = {'schema_version': 1, 'status': 'preparing', 'started_utc': datetime.now(timezone.utc).isoformat(),
              'phase': 'setup', 'samples': [], 'initialization': []}
    clients = []
    # Reserve this report before preparation; later writes may replace only this
    # run's own inode through the existing exclusive/identity-checked writer.
    h['measure'].write_json(output, result)
    try:
        native = {}
        receipts = {}
        for variant in VARIANTS:
            path = Path(getattr(args, variant)).absolute()
            h['commons'].input_helpers().reject_symlink_path(path)
            require(path.is_file() and os.access(path, os.X_OK), 'native binary is not executable')
            native[variant] = path.resolve()
            receipt_path = Path(getattr(args, variant + '_receipt'))
            captured_receipt, receipt_hash = read_receipt(receipt_path)
            receipt = portable_receipt(captured_receipt, digest(path))
            receipt['receipt_sha256'] = receipt_hash
            receipts[variant] = receipt
        identity = rust_source_identity()
        check_candidate_source(receipts['candidate'], identity)
        candidate_base = rust_source_identity(identity['git_revision'])
        require(receipts['candidate']['source_dirty'] ==
                (identity['rust_build_inputs_sha256'] != candidate_base['rust_build_inputs_sha256']),
                'candidate receipt source_dirty differs from Rust/build inputs')
        baseline_identity = rust_source_identity(receipts['baseline']['git_revision'])
        require(receipts['baseline']['rust_build_inputs_sha256'] == baseline_identity['rust_build_inputs_sha256'],
                'baseline source digest differs from its Git revision')
        fixtures = {name: digest(ROOT / name) for name in FIXTURES}
        with tempfile.TemporaryDirectory(prefix='java-lsp-reload-measure-') as temporary:
            stage = Path(temporary).resolve()
            cases, inputs, inventories, pins = prepare_cases(stage, args)
            protocol = {'schema_version': 1, 'repetitions': args.repetitions, 'warmups_per_variant_workload': 1,
                        'order': 'serial project/workload pairs; baseline/candidate order reverses each round',
                        'schedule': schedule(args.repetitions), 'request_timeout_seconds': args.timeout_seconds,
                        'session_timeout_seconds': args.session_timeout_seconds, 'sample_interval_ms': args.sample_interval_ms,
                        'scope': 'Two successful fixed source definitions and one-line target-buffer edits; not full semantic acceptance.',
                        'clock': 'local request/notification preparation through complete matching response; includes position/prefix/JSON work, IO and concurrent sampler overhead; oracle and cleanup excluded',
                        'memory': 'sampled process-group RSS sum including broker/supervisor/worker when observed; /proc reads are non-atomic, shared pages double-counted, brief peaks/processes may be missed; availability alone does not prove all three were observed; not unique physical memory or a quota',
                        'disk': 'fresh native job per query, likely warm filesystem cache; not incremental query timing',
                        'initialization': 'one initialize-request observation per project/variant after Popen; not full process startup or seven-sample startup timing',
                        'child_environment': {'PATH': ''},
                        'builds': receipts, 'build_provenance_trust': 'Operator declarations cross-checked against binary and matching source bytes; not independent build attestation. git_revision labels matching source, not necessarily actual historical build commit.',
                        'candidate_source': identity, 'baseline_source': baseline_identity, 'inputs': inputs,
                        'harness_and_fixture_sha256': fixtures, 'host': h['measure'].host_identity()}
            result.update(status='running', phase='initialization', protocol=protocol)
            h['measure'].write_json(protocol_path, protocol)
            h['measure'].write_json(output, result)
            states = {}
            for project in PROJECTS:
                case = cases[project]
                for variant in VARIANTS:
                    client = start_client([native[variant], 'lsp', '--manifest', case['manifest']], stage,
                                          args.timeout_seconds, args.session_timeout_seconds)
                    clients.append(client)
                    started = time.perf_counter()
                    response = client.request('initialize', 'initialize', {'processId': None, 'rootUri': None,
                                              'capabilities': {'general': {'positionEncodings': ['utf-16']}}})
                    elapsed = (time.perf_counter() - started) * 1000
                    check_initialize(response)
                    client.notify('initialized', {})
                    for which in ('query', 'target'):
                        client.notify('textDocument/didOpen', {'textDocument': {'uri': case[which + '_uri'],
                            'languageId': 'java', 'version': 1, 'text': case[which + '_source'].decode()}})
                    states[project, variant] = {'client': client, 'project': project, 'version': 1, 'prefix': b''}
                    result['initialization'].append({'project': project, 'variant': variant, 'wall_ms': elapsed})
                    h['measure'].write_json(output, result)
            result['phase'] = 'queries'
            for iteration, project, workload, variant in schedule(args.repetitions):
                state = states[project, variant]
                sample = measure_definition(state['client'], state, cases[project], iteration, workload,
                                             variant, args.sample_interval_ms / 1000)
                result['samples'].append(sample)
                h['measure'].write_json(output, result)
                require(sample['correct'] and 'failure_category' not in sample, 'definition failed its independent oracle')
                require(sample['proc_sampling']['available'], 'memory observation unavailable; evidence incomplete')
                print(f'{project} {workload} {variant} round={iteration} wall_ms={sample["wall_ms"]:.3f}', flush=True)
            result['phase'] = 'shutdown'
            for client in clients:
                h['lsp'].check_result(client.request('shutdown', 'shutdown', None), 'shutdown', None)
                client.notify('exit')
                client.finish()
            result['phase'] = 'identity_verification'
            require(all(digest(path) == expected for path, expected in pins.items()), 'pinned input changed')
            require(all(source_inventory(cases[name]['source_root']) == expected for name, expected in inventories.items()),
                    'source path/hash inventory changed')
            require(all(digest(native[name]) == receipt['binary_sha256'] for name, receipt in receipts.items()),
                    'native binary changed')
            require(all(digest(ROOT / name) == expected for name, expected in fixtures.items()), 'harness/fixture changed')
            require(rust_source_identity() == identity, 'candidate Rust source identity changed')
            result['summary'] = summarize(result['samples'], args.repetitions)
            result['post_run_identity_checks_passed'] = True
            result['status'] = 'passed'
    except BaseException as error:
        result['status'] = 'failed'
        result['failure_category'] = type(error).__name__
        # Do not persist exception text, which may include source/private paths.
        raise
    finally:
        try:
            close_clients(clients)
        except BaseException as error:
            result['status'] = 'failed'
            result['cleanup_failure_category'] = type(error).__name__
            raise
        finally:
            result['finished_utc'] = datetime.now(timezone.utc).isoformat()
            h['measure'].write_json(output, result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for variant in VARIANTS:
        parser.add_argument('--' + variant, type=Path, required=True)
        parser.add_argument('--' + variant + '-receipt', type=Path, required=True)
    parser.add_argument('--sources-jar', type=Path, required=True)
    parser.add_argument('--java-base-jmod', type=Path, required=True)
    parser.add_argument('--second-inputs-dir', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--repetitions', type=int, default=7)
    parser.add_argument('--timeout-seconds', type=float, default=60)
    parser.add_argument('--session-timeout-seconds', type=float, default=600)
    parser.add_argument('--sample-interval-ms', type=float, default=5)
    args = parser.parse_args()
    try:
        run(args)
    except KeyboardInterrupt:
        parser.exit(130, 'MEASUREMENT INTERRUPTED; retained evidence is marked failed.\n')
    except Exception as error:
        parser.exit(1, f'MEASUREMENT FAILED ({type(error).__name__}); inspect local invocation and retained evidence.\n')
    print('PASS fixed target assertions and complete observations; no speed/RSS threshold.')


if __name__ == '__main__':
    main()
