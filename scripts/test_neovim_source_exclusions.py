"""Offline adversarial checks for independent source-selection evidence."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('source_exclusion_gate', Path(__file__).with_name('check-neovim-source-exclusions.py'))
MOD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MOD)


def process(code=0):
    return {'returncode': code, 'failure': None, 'reaped': True, 'drainers_finished': True, 'group_cleanup': 'signalled'}


def valid_report(scenario='excluded_buffers'):
    with tempfile.TemporaryDirectory() as temporary:
        _, cfg, oracle = MOD.fixture(Path(temporary), scenario)
    terminal = {'phase': 'idle', 'cleanup': 'safe', 'active': False, 'ready': False,
        'pending_start': False, 'restart_blocked': False}
    observed = {'completed': True, 'peak_clients': 0 if scenario == 'validation' else 1,
        'terminal_status': terminal, 'attachments': copy.deepcopy(oracle['attachments']),
        'validations': copy.deepcopy(oracle['validations']), 'navigation': []}
    snapshots = {key: {**value, 'bufnr': i + 1, 'tick': i + 4, 'modified': True, 'fixendofline': False}
        for i, (key, value) in enumerate(oracle['buffers'].items())}
    observed['before'] = copy.deepcopy(snapshots); observed['after'] = copy.deepcopy(snapshots)
    if scenario == 'nested':
        observed['after_refusal'] = copy.deepcopy(snapshots)
        observed['refused_status'] = {**terminal, 'phase': 'failed', 'error': 'native source ownership refused'}
    for location in oracle['navigation']:
        observed['navigation'].append({'result': {'status': 'jumped', 'result': copy.deepcopy(location)},
            'count': 1, 'uri': location['uri'], 'cursor': oracle['cursors'][location['uri']]})
    events = []
    for ordinal, session in enumerate(oracle['sessions']):
        pid = 101 + ordinal
        def wire(method, params=None, request_id=None):
            msg = {'jsonrpc': '2.0', 'method': method}
            if params is not None: msg['params'] = params
            if request_id is not None: msg['id'] = request_id
            events.append({'proxy_pid': pid, 'direction': 'client_to_server', 'message': msg})
        def reply(method, request_id, value, error=False):
            events.append({'proxy_pid': pid, 'direction': 'server_to_client', 'method': method,
                'message': {'jsonrpc': '2.0', 'id': request_id, 'error' if error else 'result': value}})
        events.append({'proxy_pid': pid, 'event': 'spawn', 'environment': {'PATH': '/nonexistent'}})
        wire('initialize', {}, 1)
        if session['refused']:
            reply('initialize', 1, {'code': -32803, 'message': 'duplicate canonical source ownership'}, True)
        else:
            reply('initialize', 1, {'capabilities': {}}); wire('initialized', {})
            for uri, text in session['documents'].items():
                wire('textDocument/didOpen', {'textDocument': {'uri': uri, 'text': text, 'languageId': 'java', 'version': 0}})
            wire('$/setTrace', {'value': 'off'})
            for i, location in enumerate(session['navigation']):
                wire('textDocument/definition', {'textDocument': {'uri': next(iter(session['documents']))}}, i + 2)
                reply('textDocument/definition', i + 2, copy.deepcopy(location))
            wire('shutdown', request_id=99); reply('shutdown', 99, None)
        wire('exit')
        code = 1 if session['refused'] else 0
        events += [{'proxy_pid': pid, 'event': 'native_exit', 'code': code, 'fault': None},
            {'proxy_pid': pid, 'event': 'proxy_exit', 'code': code, 'fault': None, 'errors': []}]
    inventory = {'project.json': 'a' * 64, 'source.java': 'b' * 64}
    return {'scenario': scenario, 'process': process(), 'editor': {'ok': True, 'observed': observed},
        'events': events, 'oracle': oracle, 'fixture_before': inventory, 'fixture_after': dict(inventory),
        'unchanged_executables': True, 'unchanged_scripts': True,
        'native_validation': [{**v, 'json_valid': True, 'error_code': None if v['accepted'] else 'invalid_configuration', 'process': process(0 if v['accepted'] else 2)} for v in oracle['validations']]}


def legacy_report():
    report = valid_report()
    report['identity'] = {'adapter_sha256': MOD.LEGACY_SHA256, 'adapter_override': True}
    report['process']['returncode'] = 1
    report['editor']['ok'] = False
    observed = report['editor']['observed']; observed.pop('completed')
    observed['navigation'] = []
    observed['attachments'] = observed['attachments'][:1]; observed['attachments'][0]['one'] = True
    observed['failure_code'] = 'EXCLUDED_BUFFER_ATTACHED'
    observed['attachment_failure'] = {'key': 'one', 'expected': False, 'actual': True}
    observed['failure_cleanup_status'] = copy.deepcopy(observed['terminal_status'])
    observed['initial_buffers'] = {key: {**value, 'bufnr': i + 1, 'tick': i + 3,
        'modified': True, 'fixendofline': False} for i, (key, value) in enumerate(report['oracle']['initial_buffers'].items())}
    observed['failure_buffers'] = copy.deepcopy(observed['initial_buffers'])
    report['events'] = [e for e in report['events'] if e.get('method') != 'textDocument/definition'
        and e.get('message', {}).get('method') != 'textDocument/definition']
    report['events'].insert(4, {'proxy_pid': 101, 'direction': 'client_to_server', 'message': {
        'method': 'textDocument/didOpen', 'params': {'textDocument': {'uri': report['oracle']['excluded_witness_uri'],
        'text': '\n'.join(report['oracle']['initial_buffers']['one']['lines']) + '\n',
        'languageId': 'java', 'version': 0}}}})
    return report


class EvidenceTests(unittest.TestCase):
    def setUp(self): self.report = valid_report()
    def reject(self, code=None):
        result = MOD.validate(self.report)
        self.assertTrue(result)
        self.assertEqual(result, sorted(set(result)))
        if code: self.assertIn(code, result)
    def events(self, method):
        return [e for e in self.report['events'] if e.get('message', {}).get('method') == method]
    def test_all_four_families_pass_without_mutation(self):
        for scenario in MOD.SCENARIOS:
            with self.subTest(scenario=scenario):
                report = valid_report(scenario); before = copy.deepcopy(report)
                self.assertEqual(MOD.validate(report), [])
                self.assertEqual(report, before)
    def test_malformed_reports_fail_closed(self):
        for value in (None, [], {}, 1, 'text', {'scenario': []}):
            self.assertEqual(MOD.validate(value), ['EVIDENCE_SCHEMA'])
    def test_process_each_cleanup_fact(self):
        for key, value in [('returncode', True), ('returncode', 1), ('failure', 'timeout'),
            ('reaped', False), ('drainers_finished', False), ('group_cleanup', 'failed')]:
            self.report = valid_report(); self.report['process'][key] = value; self.reject('PROCESS_CLEANUP')
    def test_darwin_cleanup_is_explicit(self):
        self.report['process']['group_cleanup'] = 'darwin-confirmed-exited-only'
        self.assertEqual(MOD.validate(self.report), [])
    def test_editor_must_complete(self):
        for field in ('ok', 'completed'):
            self.report = valid_report()
            (self.report['editor'] if field == 'ok' else self.report['editor']['observed'])[field] = 1
            self.reject('EDITOR_ASSERTION')
    def test_fixture_and_executable_identity(self):
        for key in ('unchanged_executables', 'unchanged_scripts'):
            self.report = valid_report(); self.report[key] = False; self.reject('EXECUTABLE_CHANGED')
        self.report = valid_report(); self.report['fixture_after']['source.java'] = 'c' * 64; self.reject('FIXTURE_CHANGED')
    def test_unknown_excluded_didopen_rejected(self):
        event = copy.deepcopy(self.events('textDocument/didOpen')[0]); event['message']['params']['textDocument']['uri'] += '/excluded.java'
        self.report['events'].insert(4, event); self.reject('EXCLUDED_SYNC')
    def test_excluded_change_close_and_definition_rejected(self):
        for method in ('textDocument/didChange', 'textDocument/didClose', 'textDocument/definition'):
            self.report = valid_report()
            self.report['events'].insert(5, {'proxy_pid': 101, 'direction': 'client_to_server', 'message': {
                'method': method, 'params': {'textDocument': {'uri': 'file:///excluded.java'}, 'contentChanges': [{'text': 'dirty'}]}}})
            self.reject('EXCLUDED_SYNC')
    def test_missing_open_rejected(self):
        self.report['events'].remove(self.events('textDocument/didOpen')[0]); self.reject('REPLAY_DOCUMENTS')
    def test_duplicate_open_rejected(self):
        self.report['events'].insert(4, copy.deepcopy(self.events('textDocument/didOpen')[0])); self.reject('REPLAY_DOCUMENTS')
    def test_unsaved_full_text_exact(self):
        self.events('textDocument/didOpen')[0]['message']['params']['textDocument']['text'] = 'disk instead of unsaved'
        self.reject('REPLAY_TEXT')
    def test_bool_document_version_rejected(self):
        self.events('textDocument/didOpen')[0]['message']['params']['textDocument']['version'] = True; self.reject('REPLAY_TEXT')
    def test_missing_flush_rejected(self):
        self.report['events'].remove(self.events('$/setTrace')[0]); self.reject('REPLAY_FLUSH')
    def test_missing_native_exit_rejected(self):
        self.report['events'] = [e for e in self.report['events'] if e.get('event') != 'native_exit']; self.reject('PROXY_CLEANUP')
    def test_proxy_error_rejected(self):
        self.report['events'][-1]['errors'] = ['unfinished thread']; self.reject('PROXY_CLEANUP')
    def test_native_nonzero_exit_rejected(self):
        self.report['events'][-2]['code'] = 1; self.reject('NATIVE_EXIT')
    def test_native_environment_rejected(self):
        self.report['events'][0]['environment']['PATH'] = '/usr/bin'; self.reject('NATIVE_ENVIRONMENT')
    def test_extra_spawn_rejected(self):
        self.report['events'].insert(1, {'event': 'spawn', 'proxy_pid': 999, 'environment': {'PATH': '/nonexistent'}})
        self.reject('PROCESS_OVERLAP')
    def test_peak_client_bool_rejected(self):
        self.report['editor']['observed']['peak_clients'] = True; self.reject('PROCESS_OVERLAP')
    def test_real_native_target_range_rejected(self):
        reply = next(e for e in self.report['events'] if e.get('method') == 'textDocument/definition')
        reply['message']['result']['range']['start']['character'] += 1; self.reject('NAVIGATION')
    def test_editor_cursor_rejected(self):
        self.report['editor']['observed']['navigation'][0]['cursor'] = [1, 0]; self.reject('NAVIGATION')
    def test_editor_false_navigation_rejected(self):
        self.report['editor']['observed']['navigation'][0]['result']['status'] = 'empty'; self.reject('NAVIGATION')
    def test_buffer_modified_tick_text_options_preserved(self):
        for key, value in [('modified', False), ('tick', 1000), ('lines', ['lost edit']), ('bufnr', 999),
            ('fileformat', 'unix'), ('endofline', False), ('fixendofline', True)]:
            self.report = valid_report(); self.report['editor']['observed']['after']['app'][key] = value; self.reject('BUFFER_PRESERVATION')
    def test_matching_forged_snapshots_rejected_against_fixture(self):
        for where in ('before', 'after'): self.report['editor']['observed'][where]['app']['lines'] = ['lost edits']
        self.reject('BUFFER_PRESERVATION')
    def test_attachment_of_excluded_buffer_rejected(self):
        self.report['editor']['observed']['attachments'][0]['one'] = True; self.reject('ATTACHMENT')
    def test_refusal_is_actual_native_error(self):
        self.report = valid_report('nested')
        reply = next(e for e in self.report['events'] if e.get('method') == 'initialize' and 'error' in e['message'])
        reply['message']['error']['code'] = -32603; self.reject('REFUSAL')
    def test_refusal_preserves_all_buffers(self):
        self.report = valid_report('nested'); self.report['editor']['observed']['after_refusal']['app']['lines'] = []; self.reject('BUFFER_PRESERVATION')
    def test_terminal_sticky_block_rejected(self):
        self.report['editor']['observed']['terminal_status']['restart_blocked'] = True; self.reject('TERMINAL_STATE')
    def test_validation_requires_both_client_and_native(self):
        for side in ('client', 'native'):
            self.report = valid_report('validation')
            target = self.report['editor']['observed']['validations'] if side == 'client' else self.report['native_validation']
            target[0]['accepted'] = False; self.reject('VALIDATION')
    def test_validation_native_timeout_and_non_json_rejected(self):
        for key, value in [('json_valid', False), ('process', process(-9))]:
            self.report = valid_report('validation'); self.report['native_validation'][0][key] = value; self.reject('VALIDATION')
    def test_validation_shape_and_unicode_corpus(self):
        report = valid_report('validation'); cases = {v['name']: v['accepted'] for v in report['oracle']['validations']}
        self.assertGreaterEqual(len(cases), 60)
        for key in ('ordinary_unicode', 'max_bytes', 'max_rules', 'literal_bang_dash'): self.assertTrue(cases[key])
        for key in ('object', 'null', 'roots_object', 'compilations_object', 'too_long', 'unicode_too_long', 'absent_roots'):
            self.assertFalse(cases[key])
    def test_definition_must_come_from_each_expected_session(self):
        self.report = valid_report('restart')
        for event in self.report['events']:
            if event.get('method') == 'textDocument/definition' and event['proxy_pid'] == 102:
                event['proxy_pid'] = 101
        self.reject('NAVIGATION')
    def test_duplicate_response_is_not_another_proof(self):
        self.report = valid_report()
        reply = next(e for e in self.report['events'] if e.get('method') == 'textDocument/definition')
        self.report['events'].insert(8, copy.deepcopy(reply)); self.reject('PROTOCOL_ORDER')
    def test_known_document_change_must_preserve_expected_full_text(self):
        event = copy.deepcopy(self.events('textDocument/didOpen')[0])
        doc = event['message']['params']['textDocument']
        event['message'] = {'method': 'textDocument/didChange',
            'params': {'textDocument': {'uri': doc['uri']}, 'contentChanges': [{'text': 'wrong text'}]}}
        self.report['events'].insert(-5, event); self.reject('REPLAY_TEXT')
    def test_physical_case_fixture_has_no_case_collisions(self):
        with tempfile.TemporaryDirectory() as temporary:
            base, _, _ = MOD.fixture(Path(temporary), 'excluded_buffers')
            for path in base.rglob('*'):
                if path.is_dir():
                    names = [p.name.casefold() for p in path.iterdir()]
                    self.assertEqual(len(names), len(set(names)))
    def test_legacy_precise_negative_control_passes(self):
        report = legacy_report(); before = copy.deepcopy(report)
        self.assertEqual(MOD.validate_negative_control(report), [])
        self.assertEqual(report, before)
    def test_legacy_generic_failure_rejected(self):
        for mutate in (
            lambda r: r['editor']['observed'].pop('failure_code'),
            lambda r: r['editor']['observed'].update(failure_code='TIMEOUT'),
            lambda r: r['editor']['observed']['attachment_failure'].update(key='directory'),
            lambda r: r['editor']['observed']['attachment_failure'].update(actual=False),
            lambda r: r['editor']['observed']['attachments'][0].update(one=False),
            lambda r: r['editor'].update(ok=True),
            lambda r: r['process'].update(returncode=0),
            lambda r: r['process'].update(returncode=True),
        ):
            report = legacy_report(); mutate(report)
            self.assertTrue(MOD.validate_negative_control(report))
    def test_legacy_wrong_adapter_rejected(self):
        for key, value in [('adapter_sha256', '0' * 64), ('adapter_override', False)]:
            report = legacy_report(); report['identity'][key] = value
            self.assertIn('LEGACY_IDENTITY', MOD.validate_negative_control(report))
    def test_legacy_needs_exact_excluded_didopen_witness(self):
        report = legacy_report()
        report['events'] = [e for e in report['events'] if e.get('message', {}).get('params', {}).get('textDocument', {}).get('uri')
            != report['oracle']['excluded_witness_uri']]
        self.assertIn('NEGATIVE_CONTROL', MOD.validate_negative_control(report))
    def test_legacy_needs_real_successful_initialize(self):
        report = legacy_report()
        reply = next(e for e in report['events'] if e.get('method') == 'initialize')
        reply['message'].pop('result'); reply['message']['error'] = {'code': -32803}
        self.assertTrue(MOD.validate_negative_control(report))
    def test_legacy_cleanup_failure_never_counts_as_red(self):
        for mutate in (
            lambda r: r['process'].update(failure='timeout'),
            lambda r: r['process'].update(reaped=False),
            lambda r: r['process'].update(group_cleanup='failed'),
            lambda r: r['events'][-1].update(errors=['unfinished']),
            lambda r: r['events'][-2].update(code=1),
            lambda r: r['editor']['observed']['failure_cleanup_status'].update(restart_blocked=True),
        ):
            report = legacy_report(); mutate(report)
            self.assertTrue(MOD.validate_negative_control(report))
    def test_legacy_preserves_dirty_buffers_inputs_and_executables(self):
        for mutate in (
            lambda r: r['editor']['observed']['failure_buffers']['app'].update(modified=False),
            lambda r: r['fixture_after'].update(**{'source.java': 'changed'}),
            lambda r: r.update(unchanged_executables=False),
            lambda r: r.update(unchanged_scripts=False),
        ):
            report = legacy_report(); mutate(report)
            self.assertTrue(MOD.validate_negative_control(report))
    def test_legacy_malformed_reports_fail_closed(self):
        for report in (None, [], {}, 1, {'scenario': 'excluded_buffers'}):
            self.assertEqual(MOD.validate_negative_control(report), ['EVIDENCE_SCHEMA'])
    def test_public_summary_contains_no_raw_data(self):
        hostile = {'scenario': 'private/path', 'failure_codes': ['secret'] * 10000, 'ok': 'secret', 'error': 'private/path'}
        result = json.dumps(MOD.public_summary(hostile)); self.assertLess(len(result), 200)
        self.assertNotIn('secret', result); self.assertNotIn('private', result)
        self.assertEqual(MOD.public_summary(None)['scenario'], 'unknown')


if __name__ == '__main__': unittest.main()
