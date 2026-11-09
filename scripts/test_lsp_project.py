"""Offline tests of the wire harness and independent oracle, not Java semantics."""
import copy
import importlib.util
import os
from pathlib import Path
import signal
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('lsp_project', Path(__file__).with_name('check-lsp-project.py'))
check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check)


class CalculationTests(unittest.TestCase):
    def test_independent_utf16_line_and_strict_boundaries(self):
        data = 'a😀中\r\nx\ry\n'.encode('utf-8')
        self.assertEqual(check.position(data, 5), {'line': 0, 'character': 3})
        self.assertEqual(check.position(data, 10), {'line': 1, 'character': 0})
        self.assertEqual(check.position(data, len(data)), {'line': 3, 'character': 0})
        for byte in (2, 9, -1, len(data) + 1, True):
            with self.subTest(byte=byte), self.assertRaises(ValueError):
                check.position(data, byte)

    def test_exact_location_does_not_use_engine_coordinates(self):
        source = '/* 😀 */ class Target {}'.encode('utf-8')
        target = check.location('file:///Target.java', source, 17, 23, b'// prefix\r\n')
        self.assertEqual(target, {'uri': 'file:///Target.java', 'range': {
            'start': {'line': 1, 'character': 15}, 'end': {'line': 1, 'character': 21}}})
        with self.assertRaises(ValueError):
            check.location('file:///Target.java', source, 23, 17)

    def test_wrong_target_range_truncation_and_false_refusal_fail(self):
        expected = {'uri': 'file:///Target.java', 'range': {
            'start': {'line': 1, 'character': 6}, 'end': {'line': 1, 'character': 12}}}
        response = {'jsonrpc': '2.0', 'id': 'q', 'result': expected}
        check.check_result(response, 'q', expected)
        wrong = [dict(response, result=None), dict(response, result=[expected]),
                 dict(response, id='other'), dict(response, error={'code': -1}),
                 {'jsonrpc': '2.0', 'id': 'q'}]
        for key in ('start', 'end'):
            for coordinate in ('line', 'character'):
                value = copy.deepcopy(response)
                value['result']['range'][key][coordinate] += 1
                wrong.append(value)
        value = copy.deepcopy(response)
        value['result']['uri'] = 'file:///Wrong.java'
        wrong.append(value)
        value = copy.deepcopy(response)
        value['result']['range']['start']['line'] = True
        wrong.append(value)
        for value in wrong:
            with self.subTest(value=value), self.assertRaises(ValueError):
                check.check_result(value, 'q', expected)
        with self.assertRaises(ValueError):
            check.check_result(response, 'q', None)

    def test_all_java_paths_and_hashes_detect_edits_additions_and_removal(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'Excluded.java'
            source.write_bytes(b'class Excluded {}')
            baseline = check.java_inventory(root)
            source.write_bytes(b'class Changed {}')
            self.assertNotEqual(check.java_inventory(root), baseline)
            source.write_bytes(b'class Excluded {}')
            self.assertEqual(check.java_inventory(root), baseline)
            (root / 'Added.java').write_bytes(b'class Added {}')
            self.assertNotEqual(check.java_inventory(root), baseline)
            source.unlink()
            self.assertNotEqual(check.java_inventory(root), baseline)

    def test_desynchronized_refusal_does_not_accept_unrelated_failures(self):
        check.check_desynchronized({'jsonrpc': '2.0', 'id': 'q', 'result': None}, 'q')
        error = {'jsonrpc': '2.0', 'id': 'q', 'error': {
            'code': -32602, 'message': 'workspace has desynchronized buffers; repair them or restart'}}
        check.check_desynchronized(error, 'q')
        for mutation in (lambda value: value['error'].update(code=-32601),
                         lambda value: value['error'].update(message='invalid source position'),
                         lambda value: value.update(result=None),
                         lambda value: value.update(id='wrong')):
            wrong = copy.deepcopy(error)
            mutation(wrong)
            with self.assertRaises(ValueError):
                check.check_desynchronized(wrong, 'q')
        with self.assertRaises(ValueError):
            check.check_desynchronized({'jsonrpc': '2.0', 'id': 'q', 'result': {'uri': 'stale'}}, 'q')

    def test_closed_query_requires_the_specific_protocol_refusal(self):
        error = {'jsonrpc': '2.0', 'id': 'q', 'error': {
            'code': -32602, 'message': 'definition requires an open synchronized owned document'}}
        check.check_closed(error, 'q')
        for wrong in ({'jsonrpc': '2.0', 'id': 'q', 'result': None},
                      dict(error, error={'code': -32602, 'message': 'invalid position'}),
                      dict(error, error={'code': -32601, 'message': error['error']['message']}),
                      dict(error, id='wrong'), dict(error, result=None)):
            with self.assertRaises(ValueError):
                check.check_closed(wrong, 'q')


class FrameTests(unittest.TestCase):
    def test_fragmented_coalesced_and_unicode_byte_lengths(self):
        first = {'jsonrpc': '2.0', 'id': '😀', 'result': None}
        second = {'jsonrpc': '2.0', 'id': -3, 'result': {'text': '中'}}
        frames = check.Frames()
        wire = check.encode(first) + check.encode(second)
        for byte in wire:
            frames.feed(bytes([byte]))
        frames.eof()
        self.assertEqual(list(frames.messages), [first, second])
        header, body = check.encode(first).split(b'\r\n\r\n', 1)
        self.assertEqual(int(header.split(b':')[1]), len(body))
        self.assertGreater(len(body), len(body.decode('utf-8')))

    def test_malformed_and_truncated_frames_do_not_pass(self):
        examples = [b'Content-Length: 0\r\n\r\n', b'X: 1\r\n\r\n',
                    b'Content-Length: 2\r\ncontent-length: 2\r\n\r\n{}',
                    b'Content-Length: -1\r\n\r\n', b'Content-Length: 999999999\r\n\r\n',
                    b'Content-Length: 1\r\n\r\n\xff', b'Content-Length: 2\r\n\r\n[]',
                    b'Content-Length: 3\r\n\r\n{}', b'Content-Length: 3\r\n',
                    b'h' * (check.MAX_HEADER + 1)]
        for wire in examples:
            with self.subTest(wire=wire[:80]), self.assertRaises(ValueError):
                frames = check.Frames()
                frames.feed(wire)
                frames.eof()

    def test_output_and_message_queue_bounds(self):
        wire = check.encode({'jsonrpc': '2.0', 'id': 1, 'result': None})
        with patch.object(check, 'MAX_OUTPUT', len(wire) - 1), self.assertRaises(ValueError):
            check.Frames().feed(wire)
        with patch.object(check, 'MAX_MESSAGES', 2), self.assertRaises(ValueError):
            check.Frames().feed(wire * 3)


class FixedLifecycleTests(unittest.TestCase):
    """Replies are literal coordinates, never derived by the checker under test."""
    CASE = {'query_uri': 'file:///Caller.java', 'target_uri': 'file:///Target.java',
            'query_source': b'// disk\r\nclass Caller { Target x; }\r\n',
            'target_source': b'// disk\r\nclass Target {}\r\n',
            'query_byte': 24, 'target_start': 15, 'target_end': 21}
    STAGES = [('disk_bytes_baseline', 1, 1), ('two_unsaved_opens', 2, 3),
              ('negative_nonconsecutive_versions', 3, 5), ('dependency_only_change', 3, 7),
              ('duplicate_version_refused', 3, None), ('valid_full_replacement_recovers', 3, 9),
              ('target_close_reads_disk', 3, 1), ('closed_query_refused', 1, None),
              ('reopened_disk_bytes_restore_baseline', 1, 1)]

    class Client:
        def __init__(self, owner, mutation=None):
            self.owner, self.mutation = owner, mutation
            self.stages = iter(owner.STAGES)
            self.events = []
            self.finished = False

        def request(self, request_id, method, params):
            self.events.append((method, params))
            result = None
            if method == 'initialize':
                result = {'capabilities': {'positionEncoding': 'utf-16', 'definitionProvider': True,
                                          'textDocumentSync': {'openClose': True, 'change': 1}}}
            elif method == 'textDocument/definition':
                stage, query_line, target_line = next(self.stages)
                self.owner.assertEqual(request_id, stage)
                self.owner.assertEqual(params, {'textDocument': {'uri': 'file:///Caller.java'},
                                               'position': {'line': query_line, 'character': 15}})
                if target_line is not None:
                    result = {'uri': 'file:///Target.java', 'range': {
                        'start': {'line': target_line, 'character': 6},
                        'end': {'line': target_line, 'character': 12}}}
                if self.mutation and stage == 'two_unsaved_opens':
                    self.mutation(result)
                if stage == 'closed_query_refused':
                    return {'jsonrpc': '2.0', 'id': request_id, 'error': {
                        'code': -32602, 'message': 'definition requires an open synchronized owned document'}}
            else:
                self.owner.assertEqual(method, 'shutdown')
            return {'jsonrpc': '2.0', 'id': request_id, 'result': result}

        def notify(self, method, params=None):
            self.events.append((method, params))

        def finish(self):
            self.finished = True

    def test_frozen_lifecycle_with_literal_oracle(self):
        client = self.Client(self)
        results = check.exercise(client, self.CASE)
        self.assertEqual(len(results), 9)
        self.assertTrue(client.finished)
        self.assertEqual([params['textDocument']['version'] for method, params in client.events
                          if method in ('textDocument/didOpen', 'textDocument/didChange')],
                         [-100, -11, -23, -7, -3, 100, 100, 104, -101])
        self.assertEqual([params['textDocument']['uri'] for method, params in client.events
                          if method == 'textDocument/didClose'],
                         ['file:///Caller.java', 'file:///Target.java', 'file:///Caller.java', 'file:///Caller.java'])
        opens = [params['textDocument'] for method, params in client.events if method == 'textDocument/didOpen']
        self.assertEqual(opens[0]['text'].encode(), self.CASE['query_source'])
        self.assertEqual(opens[-1]['text'].encode(), self.CASE['query_source'])
        self.assertEqual(results[0]['result'], results[-1]['result'])

    def test_mutated_source_target_or_range_cannot_self_validate(self):
        for mutation in (lambda value: value.update(uri='file:///Wrong.java'),
                         lambda value: value['range']['start'].update(character=7),
                         lambda value: value['range']['end'].update(line=2)):
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                check.exercise(self.Client(self, mutation), self.CASE)

    def test_explicit_desynchronization_error_is_recorded_and_recovers(self):
        client = self.Client(self)
        original = client.request

        def request(request_id, method, params):
            reply = original(request_id, method, params)
            if request_id == 'duplicate_version_refused':
                del reply['result']
                reply['error'] = {'code': -32602, 'message': 'workspace has desynchronized buffers'}
            return reply

        client.request = request
        results = check.exercise(client, self.CASE)
        self.assertEqual(results[4]['error']['code'], -32602)
        self.assertNotIn('result', results[4])
        self.assertEqual(results[5]['result']['range']['start'], {'line': 9, 'character': 6})


@unittest.skipUnless(os.name == 'posix' and hasattr(os, 'WNOWAIT'), 'POSIX bounded pipe harness')
class ProcessTests(unittest.TestCase):
    def session(self, script, timeout=2):
        return check.Session([sys.executable, '-c', script], Path(tempfile.gettempdir()),
                             timeout=timeout, total_timeout=timeout + 1)

    def test_empty_path_and_clean_exit(self):
        script = "import os\nassert os.environ.get('PATH') == ''\nassert 'JAVA_HOME' not in os.environ\n"
        with self.session(script) as session:
            session.finish()
        self.assertEqual(session.process.returncode, 0)

    def test_timeout_and_blocked_writer_are_bounded(self):
        for write in (False, True):
            started = time.monotonic()
            with self.subTest(write=write), self.assertRaisesRegex(ValueError, 'timeout'):
                with self.session('import time; time.sleep(30)', timeout=0.15) as session:
                    if write:
                        session.send({'jsonrpc': '2.0', 'method': 'test', 'params': 'x' * 1000000})
                    else:
                        session.response(1)
            self.assertLess(time.monotonic() - started, 3)
            self.assertIsNotNone(session.process.returncode)

    def test_duplicate_response_and_stderr_limits(self):
        wire = check.encode({'jsonrpc': '2.0', 'id': 1, 'result': None})
        script = f'import os,time; os.write(1,{wire!r}*2); time.sleep(30)'
        with self.session(script) as session:
            self.assertIsNone(session.response(1)['result'])
            with self.assertRaisesRegex(ValueError, 'duplicate'):
                session.response(2)
        with patch.object(check, 'MAX_STDERR', 100), self.assertRaisesRegex(ValueError, 'stderr limit'):
            with self.session("import os; os.write(2,b'x'*101)") as session:
                session.response(1)

    def test_group_cleanup_precedes_reap_even_after_leader_exit(self):
        calls = []
        real_kill, real_wait, real_observe = os.killpg, os.waitpid, os.waitid

        def kill(*args):
            calls.append('kill')
            self.assertEqual(args[1], signal.SIGKILL)
            return real_kill(*args)

        def wait(*args):
            calls.append('reap')
            return real_wait(*args)

        def observe(*args):
            self.assertTrue(args[2] & os.WNOWAIT)
            calls.append('observe')
            return real_observe(*args)

        with patch.object(os, 'killpg', side_effect=kill), patch.object(os, 'waitpid', side_effect=wait), \
                patch.object(os, 'waitid', side_effect=observe):
            with self.session('pass') as session:
                session.finish()
        self.assertIn('observe', calls)
        self.assertLess(calls.index('kill'), calls.index('reap'))

    @unittest.skipUnless(sys.platform == 'linux', 'Linux descendant state observation')
    def test_timeout_cleans_descendant_retaining_pipe(self):
        with tempfile.TemporaryDirectory() as temporary:
            record = Path(temporary) / 'child-pid'
            script = (f'import os,time\npid=os.fork()\nif pid:\n'
                      f' open({str(record)!r},"w").write(str(pid)); os._exit(0)\n'
                      'time.sleep(30)')
            with self.assertRaisesRegex(ValueError, 'timeout'):
                with self.session(script, timeout=0.2) as session:
                    session.response(1)
            pid = int(record.read_text())
            deadline = time.monotonic() + 2
            while time.monotonic() < deadline:
                try:
                    state = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[0]
                except FileNotFoundError:
                    break
                if state == 'Z':  # Orphan reaping belongs to the container/OS.
                    break
                time.sleep(0.01)
            else:
                self.fail('descendant remained running after harness cleanup')


if __name__ == '__main__':
    unittest.main()
