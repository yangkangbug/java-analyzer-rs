"""Offline falsifying controls for supplemental LSP timing; never executes Java."""
import copy
import importlib.util
import json
import math
import os
from pathlib import Path
import tempfile
import sys
import threading
import unittest
from unittest.mock import patch, Mock
from types import SimpleNamespace

spec = importlib.util.spec_from_file_location('measure_lsp_reload', Path(__file__).with_name('measure-lsp-reload.py'))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class Measurements(unittest.TestCase):
    def test_finite_and_integer_limits(self):
        m.validate_options(7, 60, 600, 5)
        for index, bad in ((0, True), (0, 0), (0, 31), (0, 7.5),
                           (1, float('nan')), (1, float('inf')), (1, 0), (1, 121),
                           (2, float('inf')), (2, 0), (2, 7201),
                           (3, float('nan')), (3, 0), (3, .1), (3, 101)):
            values = [7, 60, 600, 5]
            values[index] = bad
            with self.subTest(values=values), self.assertRaises(ValueError):
                m.validate_options(*values)
        with self.assertRaises(ValueError):
            m.validate_options(7, 60, 30, 5)

    def test_schedule_is_complete_and_alternates_paired_order(self):
        order = m.schedule(7)
        self.assertEqual(len(order), 8 * 2 * 2 * 2)
        for iteration in range(8):
            rows = [r for r in order if r[0] == iteration]
            for project in m.PROJECTS:
                for work in m.WORKLOADS:
                    versions = [r[3] for r in rows if r[1:3] == (project, work)]
                    self.assertEqual(versions, ['baseline', 'candidate'] if iteration % 2 == 0 else ['candidate', 'baseline'])

    def test_receipt_binary_and_source_checks_are_separate_from_tooling(self):
        receipt = {'binary_sha256': 'a' * 64, 'git_revision': 'b' * 40,
                   'rust_build_inputs_sha256': 'c' * 64, 'source_dirty': False,
                   'build_profile': 'release', 'target': 'x86_64-unknown-linux-gnu',
                   'private_command': '/private/name/build'}
        for bad in ([], None, 42, 'bad'):
            with self.subTest(receipt=bad), self.assertRaises(ValueError):
                m.portable_receipt(bad, 'a' * 64)
        result = m.portable_receipt(receipt, 'a' * 64)
        self.assertNotIn('private_command', result)
        self.assertNotIn('/private', json.dumps(result))
        for key, value in [('binary_sha256', 'd' * 64), ('git_revision', '/private'),
                           ('rust_build_inputs_sha256', 'bad'), ('source_dirty', 'false'),
                           ('build_profile', 'debug'), ('target', 'other')]:
            mutated = dict(receipt, **{key: value})
            with self.subTest(key=key), self.assertRaises(ValueError):
                m.portable_receipt(mutated, 'a' * 64)
        m.check_candidate_source(result, {'git_revision': 'b' * 40, 'rust_build_inputs_sha256': 'c' * 64})
        with self.assertRaises(ValueError):
            m.check_candidate_source(result, {'git_revision': 'b' * 40, 'rust_build_inputs_sha256': 'e' * 64})

    def test_existing_output_or_protocol_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            out = root / 'result.json'
            proto = root / 'result.protocol.json'
            for path in (out, proto):
                path.write_text('existing')
                with self.assertRaises(ValueError):
                    m.output_paths(out)
                self.assertEqual(path.read_text(), 'existing')
                path.unlink()
            if hasattr(os, 'symlink'):
                out.symlink_to(root / 'missing')
                with self.assertRaises(ValueError):
                    m.output_paths(out)

    def test_published_results_do_not_contain_local_uris(self):
        case = {'target_uri': 'file:///private/user/Target.java', 'target_relative': 'src/Target.java'}
        response = {'jsonrpc': '2.0', 'id': 'q', 'result': {'uri': case['target_uri'], 'range': {
            'start': {'line': 1, 'character': 2}, 'end': {'line': 1, 'character': 3}}}}
        clean = m.portable_response(response, case)
        self.assertEqual(clean['result']['target'], 'src/Target.java')
        self.assertNotIn('/private', json.dumps(clean))
        response['result']['uri'] = 'file:///another/private/Bad.java'
        self.assertTrue(m.portable_response(response, case)['result']['unexpected_uri'])
        response = {'id': 'q', 'error': {'code': -1, 'message': '/private/secret'}}
        self.assertNotIn('/private', json.dumps(m.portable_response(response, case)))

    def sample_rows(self, repetitions=2):
        return [{'iteration': i, 'project': p, 'workload': w, 'variant': v,
                 'warmup': i == 0, 'wall_ms': 999 if i == 0 else i,
                 'correct': True, 'proc_sampling': {'available': True, 'peak_sum_rss_kib': 100 + i}}
                for i, p, w, v in m.schedule(repetitions)]

    def test_summary_requires_exact_samples_correctness_and_memory(self):
        rows = self.sample_rows()
        summary = m.summarize(rows, 2)
        item = summary['gson']['same_generation_definition']['baseline']
        self.assertEqual(item['wall_ms'], {'min': 1, 'median': 1.5, 'max': 2})
        self.assertEqual(item['raw_wall_ms'], [1, 2])
        self.assertNotIn('p95', json.dumps(summary))
        for mutation in ('missing', 'duplicate', 'wrong', 'memory', 'nan', 'warmup', 'rss_nan'):
            changed = copy.deepcopy(rows)
            if mutation == 'missing':
                changed.pop()
            elif mutation == 'duplicate':
                changed.append(changed[-1])
            elif mutation == 'wrong':
                changed[-1]['correct'] = False
            elif mutation == 'memory':
                changed[-1]['proc_sampling']['available'] = False
            elif mutation == 'warmup':
                changed[-1]['warmup'] = True
            elif mutation == 'rss_nan':
                changed[-1]['proc_sampling']['peak_sum_rss_kib'] = math.nan
            else:
                changed[-1]['wall_ms'] = math.nan
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                m.summarize(changed, 2)

    def test_cleanup_attempts_every_client_after_one_error(self):
        seen = []
        class Client:
            def __init__(self, key): self.key = key
            def close(self):
                seen.append(self.key)
                if self.key == 1: raise OSError('failure')
        with self.assertRaises(ValueError):
            m.close_clients([Client(1), Client(2), Client(3)])
        self.assertEqual(seen, [1, 2, 3])

    def test_sampler_stops_on_body_exception_and_reports_sampling_failure(self):
        class Sampler:
            def sample(self, _): raise OSError('unavailable')
            def result(self): return {'available': False, 'peak_sum_rss_kib': None}
        with self.assertRaisesRegex(ValueError, 'sampling'):
            with m.Sampling(1, .001, factory=lambda *args: Sampler()) as sampler:
                sampler.thread.join(1)
        self.assertFalse(sampler.thread.is_alive())
        class GoodSampler:
            def sample(self, _): pass
            def result(self): return {'available': True, 'peak_sum_rss_kib': 1}
        with self.assertRaisesRegex(RuntimeError, 'body'):
            with m.Sampling(1, .001, factory=lambda *args: GoodSampler()) as sampler:
                raise RuntimeError('body')
        self.assertFalse(sampler.thread.is_alive())

    def test_independent_target_oracle_rejects_wrong_utf16_range(self):
        lsp = m.helpers()['lsp']
        expected = {'uri': 'file:///tmp/T.java', 'range': {'start': {'line': 2, 'character': 3},
                                                         'end': {'line': 2, 'character': 4}}}
        response = {'jsonrpc': '2.0', 'id': 'q', 'result': copy.deepcopy(expected)}
        lsp.check_result(response, 'q', expected)
        response['result']['range']['start']['character'] = 2
        with self.assertRaises(ValueError):
            lsp.check_result(response, 'q', expected)
        response['result'] = None
        with self.assertRaises(ValueError):
            lsp.check_result(response, 'q', expected)

    def test_partial_session_constructor_is_cleaned(self):
        for failure in (OSError, KeyboardInterrupt):
            with self.subTest(failure=failure):
                process = Mock(pid=123456)
                class BrokenSession:
                    def __init__(self, *args, **kwargs):
                        self.process = process
                        raise failure('selector initialization failed')
                with patch.object(m, 'helpers', return_value={'lsp': SimpleNamespace(Session=BrokenSession)}), \
                        patch.object(m.os, 'killpg') as kill:
                    with self.assertRaises(failure):
                        m.start_client([], Path('/tmp'), 1, 2)
                kill.assert_called_once_with(123456, m.signal.SIGKILL)
                process.wait.assert_called_once_with(timeout=5)
                for stream in (process.stdin, process.stdout, process.stderr):
                    stream.close.assert_called_once()

    @unittest.skipUnless(sys.platform == 'linux', 'Linux resource harness')
    def test_protocol_precedes_spawn_and_initialization_failure_is_retained(self):
        self.check_initialization_failure(OSError)

    @unittest.skipUnless(sys.platform == 'linux', 'Linux resource harness')
    def test_interruption_retains_failed_status_and_cleans_client(self):
        self.check_initialization_failure(KeyboardInterrupt)

    def check_initialization_failure(self, failure):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binary = root / 'native'
            binary.write_bytes(b'not executed')
            binary.chmod(0o700)
            receipt = {'binary_sha256': m.digest(binary), 'git_revision': 'b' * 40,
                       'rust_build_inputs_sha256': 'c' * 64, 'source_dirty': False,
                       'build_profile': 'release', 'target': 'x86_64-unknown-linux-gnu'}
            receipt_path = root / 'receipt.json'
            receipt_path.write_text(json.dumps(receipt))
            args = SimpleNamespace(baseline=binary, candidate=binary, baseline_receipt=receipt_path,
                candidate_receipt=receipt_path, repetitions=1, timeout_seconds=1,
                session_timeout_seconds=2, sample_interval_ms=5, output=root / 'result.json')
            identity = {'git_revision': 'b' * 40, 'rust_build_inputs_sha256': 'c' * 64}
            closed = []
            class Client:
                def request(self, *args): raise failure('/private/should-not-be-published')
                def close(self): closed.append(True)
            def start(*args, **kwargs):
                self.assertTrue((root / 'result.protocol.json').is_file())
                return Client()
            cases = {name: {'manifest': root / name} for name in m.PROJECTS}
            with patch.object(m, 'rust_source_identity', return_value=identity), \
                    patch.object(m, 'prepare_cases', return_value=(cases, {}, {}, {})), \
                    patch.object(m, 'start_client', side_effect=start):
                with self.assertRaises(failure): m.run(args)
            result = json.loads(args.output.read_text())
            self.assertEqual(result['status'], 'failed')
            self.assertEqual(result['failure_category'], failure.__name__)
            self.assertEqual(result['phase'], 'initialization')
            self.assertEqual(result['samples'], [])
            self.assertEqual(closed, [True])
            self.assertNotIn('/private', args.output.read_text())
            self.assertTrue((root / 'result.protocol.json').is_file())

    def test_wrong_definition_retains_failure_and_stops_sampling(self):
        case = {'query_uri': 'file:///private/Q.java', 'target_uri': 'file:///private/T.java',
                'query_relative': 'Q.java', 'target_relative': 'T.java',
                'query_source': b'class Q { T x; }', 'target_source': b'class T {}',
                'query_byte': 10, 'target_start': 6, 'target_end': 7}
        state = {'project': 'gson', 'version': 1, 'prefix': b''}
        class Client:
            process = SimpleNamespace(pid=1)
            def request(self, request_id, *args):
                return {'jsonrpc': '2.0', 'id': request_id, 'result': {'uri': case['target_uri'],
                    'range': {'start': {'line': 0, 'character': 5}, 'end': {'line': 0, 'character': 7}}}}
        sampler = Mock()
        sampler.__enter__ = Mock(return_value=sampler)
        sampler.__exit__ = Mock(return_value=None)
        sampler.result.return_value = {'available': True, 'peak_sum_rss_kib': 10}
        with patch.object(m, 'Sampling', return_value=sampler):
            row = m.measure_definition(Client(), state, case, 1, 'same_generation_definition', 'baseline', .005)
        self.assertFalse(row['correct'])
        self.assertIn('failure_category', row)
        self.assertEqual(row['expected']['range']['start']['character'], 6)
        self.assertEqual(row['actual']['result']['range']['start']['character'], 5)
        self.assertNotIn('/private', json.dumps(row))
        sampler.__exit__.assert_called_once()

    def test_source_digest_ignores_tools_but_includes_new_rust_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'src').mkdir()
            for name in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'src/lib.rs'):
                (root / name).write_text(name)
            with patch.object(m, 'git_bytes', return_value=b'a' * 40):
                first = m.rust_source_identity(root=root)
                (root / 'tooling.py').write_text('new tooling')
                self.assertEqual(m.rust_source_identity(root=root), first)
                (root / 'src/new.rs').write_text('new Rust')
                second = m.rust_source_identity(root=root)
            self.assertNotEqual(first['rust_build_inputs_sha256'], second['rust_build_inputs_sha256'])
            self.assertEqual(first['rust_build_inputs_sha256'], m.canonical_hash(first['rust_build_input_files']))
            self.assertIn('src/new.rs', second['rust_build_input_files'])

    def test_cleanup_continues_after_interruption_and_rethrows(self):
        closed = []
        class Client:
            def __init__(self, key): self.key = key
            def close(self):
                closed.append(self.key)
                if self.key == 1: raise KeyboardInterrupt()
        with self.assertRaises(KeyboardInterrupt):
            m.close_clients([Client(1), Client(2), Client(3)])
        self.assertEqual(closed, [1, 2, 3])

    def test_receipt_read_is_bounded_regular_non_symlink_and_hashes_captured_bytes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            receipt = root / 'receipt.json'
            payload = b'{"field": "value"}\n'
            receipt.write_bytes(payload)
            value, checksum = m.read_receipt(receipt)
            self.assertEqual(value, {'field': 'value'})
            self.assertEqual(checksum, m.hashlib.sha256(payload).hexdigest())
            with self.assertRaises(ValueError): m.read_receipt(root)
            link = root / 'alias.json'
            link.symlink_to(receipt)
            with self.assertRaises(ValueError): m.read_receipt(link)
            parent = root / 'alias'
            parent.symlink_to(root, target_is_directory=True)
            with self.assertRaises(ValueError): m.read_receipt(parent / 'receipt.json')
            if hasattr(os, 'mkfifo'):
                fifo = root / 'fifo'
                os.mkfifo(fifo)
                with self.assertRaises(ValueError): m.read_receipt(fifo)
            for payload in (b' ' * 65537, b'[]', b'null', b'42', b'"text"', b'{"x":1,"x":2}'):
                receipt.write_bytes(payload)
                with self.subTest(payload=payload[:20]), self.assertRaises(ValueError):
                    m.read_receipt(receipt)

    def test_receipt_growth_after_descriptor_metadata_cannot_bypass_bound(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'receipt.json'
            path.write_bytes(b'{}')
            actual_fstat = os.fstat
            def grow(fd):
                before = actual_fstat(fd)
                with path.open('ab') as stream: stream.write(b' ' * 70000)
                return before
            with patch.object(m.os, 'fstat', side_effect=grow), self.assertRaises(ValueError):
                m.read_receipt(path)

    def test_body_interruption_stops_sampler(self):
        class Sampler:
            def sample(self, _): pass
            def result(self): return {'available': True, 'peak_sum_rss_kib': 1}
        with self.assertRaises(KeyboardInterrupt):
            with m.Sampling(1, .001, factory=lambda *args: Sampler()) as sampler:
                raise KeyboardInterrupt()
        self.assertFalse(sampler.thread.is_alive())

    def test_initialize_null_or_malformed_shapes_are_explicit_failures(self):
        good = {'jsonrpc': '2.0', 'id': 'initialize', 'result': {'capabilities': {
            'positionEncoding': 'utf-16', 'definitionProvider': True,
            'textDocumentSync': {'change': 1, 'openClose': True}}}}
        m.check_initialize(good)
        for wrong in (None, [], {'jsonrpc': '2.0', 'id': 'initialize', 'result': None},
                      {'result': {'capabilities': None}},
                      {'result': {'capabilities': {'textDocumentSync': None}}}):
            with self.subTest(response=wrong), self.assertRaises(ValueError):
                m.check_initialize(wrong)

    def test_input_metadata_describes_only_measured_lsp_query_and_edit(self):
        gson = {'project': 'Gson', 'source_count': 211, 'query_count': 16}
        commons = {'name': 'Commons', 'source_count': 249, 'query_count': 45,
                   'candidate_count': 70, 'requests_sha256': 'unused',
                   'edited_requests_sha256': 'unused', 'edit': {'prefix': 'unused CLI edit'}}
        cases = {name: {'query_relative': 'Q.java', 'target_relative': 'T.java',
                        'query_byte': 10, 'target_start': 6, 'target_end': 7} for name in m.PROJECTS}
        result = m.input_metadata(gson, commons, cases, {name: {'Q.java': 'a'} for name in m.PROJECTS})
        for name in m.PROJECTS:
            self.assertEqual(result[name]['measured_query_count'], 1)
            self.assertEqual(result[name]['definition']['target_byte_range'], [6, 7])
            self.assertEqual(result[name]['definition']['target_prefix_scheme'], m.PREFIX_TEMPLATE)
        for unused in ('query_count', 'candidate_count', 'requests_sha256', 'edited_requests_sha256', 'edit'):
            self.assertNotIn(unused, result['commons'])
            self.assertNotIn(unused, result['gson'])
        self.assertNotIn('unused', json.dumps(result))


if __name__ == '__main__':
    unittest.main()
