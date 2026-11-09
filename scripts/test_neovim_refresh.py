"""Offline adversarial regressions for refresh evidence; no Neovim/JVM needed."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    'neovim_refresh', Path(__file__).with_name('check-neovim-refresh.py'))
MOD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MOD)
URI = 'file:///fixture%20%E4%B8%AD/App.java'
TEXT = 'class App { /* unsaved 中😀 */ int changed; }\n'


def wire(pid, method, params=None, request_id=None):
    message = {'jsonrpc': '2.0', 'method': method}
    if params is not None:
        message['params'] = params
    if request_id is not None:
        message['id'] = request_id
    return {'proxy_pid': pid, 'direction': 'client_to_server', 'message': message}


def response(pid, method, request_id, result):
    return [
        {'proxy_pid': pid, 'direction': 'server_to_client', 'method': method,
         'message': {'jsonrpc': '2.0', 'id': request_id, 'result': result}},
        {'proxy_pid': pid, 'event': 'delivered', 'method': method, 'request_id': request_id},
    ]


def session(pid):
    return [
        {'event': 'spawn', 'proxy_pid': pid, 'native_pid': pid + 1000,
         'native': '/native/java-analyzer', 'environment': {'PATH': '/nonexistent'}},
        wire(pid, 'initialize', {}, 1),
        *response(pid, 'initialize', 1, {'capabilities': {}}),
        wire(pid, 'initialized', {}),
        wire(pid, 'textDocument/didOpen', {'textDocument': {
            'uri': URI, 'languageId': 'java', 'version': 0, 'text': TEXT}}),
        wire(pid, '$/setTrace', {'value': 'off'}),
        wire(pid, 'textDocument/definition', {
            'textDocument': {'uri': URI}, 'position': {'line': 0, 'character': 40}}, 2),
        *response(pid, 'textDocument/definition', 2, []),
        wire(pid, 'shutdown', request_id=3),
        *response(pid, 'shutdown', 3, None),
        wire(pid, 'exit'),
        {'event': 'native_exit', 'proxy_pid': pid, 'code': 0, 'fault': None},
        {'event': 'proxy_exit', 'proxy_pid': pid, 'code': 0, 'fault': None, 'errors': []},
    ]


def valid_report():
    inventory = {'App.java': 'a' * 64, 'project.json': 'b' * 64}
    return {
        'scenario': 'add',
        'process': {'returncode': 0, 'failure': None, 'reaped': True,
                    'drainers_finished': True, 'group_cleanup': 'signalled'},
        'editor': {'ok': True, 'observed': {
            'completed': True, 'expected_spawns': 2, 'peak_clients': 1,
            'terminal_phase': 'idle',
            'terminal_status': {'phase': 'idle', 'cleanup': 'safe', 'active': False,
                                'ready': False, 'pending_start': False, 'restart_blocked': False},
            'navigation': [{'word': 'changed', 'expected_status': 'empty',
                            'actual_buf': 1, 'expected_buf': 1,
                            'actual_cursor': [1, 0], 'expected_cursor': [1, 0],
                            'target_uri': False, 'target_line': ''}],
            'results': [{'status': 'empty', 'result': None}], 'replays': [
                {'ordinal': 1, 'documents': {URI: TEXT}},
                {'ordinal': 2, 'documents': {URI: TEXT}},
            ]}},
        'fixture_before': inventory, 'fixture_after': dict(inventory),
        'unchanged_executables': True, 'intentional_disk_verified': True,
        'events': session(101) + session(202),
    }


def refusal_report():
    report = valid_report()
    report['scenario'] = 'initialize_refusal'
    report['editor']['observed']['refused_status'] = {
        'phase': 'failed', 'cleanup': 'safe', 'active': False, 'ready': False,
        'pending_start': False, 'restart_blocked': False, 'error': 'actual refusal'}
    rejected = session(202)
    error = {'proxy_pid': 202, 'direction': 'server_to_client', 'method': 'initialize',
             'message': {'jsonrpc': '2.0', 'id': 1,
                         'error': {'code': -32803, 'message': 'project refused'}}}
    report['events'] = session(101) + [rejected[0], rejected[1], error,
        {'proxy_pid': 202, 'event': 'delivered', 'method': 'initialize', 'request_id': 1},
        wire(202, 'exit'),
        {'proxy_pid': 202, 'event': 'native_exit', 'code': 1, 'fault': None},
        {'proxy_pid': 202, 'event': 'proxy_exit', 'code': 1, 'fault': None, 'errors': []},
    ] + session(303)
    report['editor']['observed']['expected_spawns'] = 3
    report['editor']['observed']['replays'][1]['ordinal'] = 3
    return report


def fault_report(scenario):
    report = valid_report()
    report['scenario'] = scenario
    report['editor']['observed']['terminal_phase'] = 'blocked'
    report['editor']['observed']['terminal_status'] = {
        'phase': 'blocked', 'cleanup': 'uncertain', 'active': False, 'ready': False,
        'pending_start': False, 'restart_blocked': True, 'client_id': 1}
    events = session(101)
    if scenario == 'initialize_timeout':
        events = events[:3] + [{'proxy_pid': 101, 'event': 'held',
                               'kind': 'initialize', 'request_id': 1}]
        report['editor']['observed']['replays'] = []
        report['editor']['observed']['navigation'] = []
        report['editor']['observed']['results'] = []
    elif scenario == 'shutdown_timeout':
        events = events[:12] + [{'proxy_pid': 101, 'event': 'held',
                                'kind': 'shutdown', 'request_id': 3}]
    else:
        events = events[:10] + [{'proxy_pid': 101, 'event': 'test_terminate_native'}]
    events += [
        {'proxy_pid': 101, 'event': 'native_exit', 'code': -15, 'fault': None},
        {'proxy_pid': 101, 'event': 'proxy_exit', 'code': -15, 'fault': None, 'errors': []},
    ]
    report['events'] = events
    report['editor']['observed']['expected_spawns'] = 1
    if scenario != 'initialize_timeout':
        report['editor']['observed']['replays'] = report['editor']['observed']['replays'][:1]
    return report


def held_report(scenario='held_restarts'):
    report = valid_report()
    report['scenario'] = scenario
    result = {'uri': URI, 'range': {'start': {'line': 0, 'character': 1},
                                  'end': {'line': 0, 'character': 4}}}
    if scenario == 'reentrant_stop':
        report['events'] = session(101)
        report['editor']['observed']['expected_spawns'] = 1
        report['editor']['observed']['replays'] = report['editor']['observed']['replays'][:1]
    events = report['events']
    reply = next(e for e in events if e.get('direction') == 'server_to_client'
                 and e.get('method') == 'textDocument/definition')
    reply['message']['result'] = result
    events[events.index(reply) + 1:events.index(reply) + 1] = [
        {'proxy_pid': 101, 'event': 'held', 'kind': 'definition', 'request_id': 2},
        {'proxy_pid': 101, 'event': 'released', 'kind': 'definition', 'request_id': 2},
    ]
    report['editor']['observed'].update({
        'settled_before_release': {'status': 'stale', 'count': 1},
        'callback_final_count': 1, 'held_result': copy.deepcopy(result),
    })
    return report


def jumped_report():
    report = valid_report()
    prefix, word = '  /* 中😀 */ int ', 'changed'
    byte_col = len(prefix.encode())
    utf16_col = len(prefix.encode('utf-16-le')) // 2
    observed = report['editor']['observed']
    observed['navigation'] = [{
        'word': word, 'expected_status': 'jumped', 'actual_buf': 2, 'expected_buf': 2,
        'actual_cursor': [3, byte_col], 'expected_cursor': [3, byte_col],
        'target_uri': URI, 'target_line': prefix + word + ';',
    }]
    result = {'uri': URI, 'range': {
        'start': {'line': 2, 'character': utf16_col},
        'end': {'line': 2, 'character': utf16_col + len(word)},
    }}
    observed['results'] = [{'status': 'jumped', 'result': result}]
    reply = next(e for e in report['events'] if e.get('direction') == 'server_to_client'
                 and e.get('method') == 'textDocument/definition')
    reply['message']['result'] = copy.deepcopy(result)
    return report


def observer_report():
    report = valid_report()
    report['scenario'] = 'observer_stability'
    report['events'] = [e for pid in range(101, 107) for e in session(pid)]
    observed = report['editor']['observed']
    observed['expected_spawns'] = 6
    observed['replays'] = [{'ordinal': i, 'documents': {URI: TEXT}} for i in range(1, 7)]
    observed['observers'] = [2] * 6
    observed['observer_attachments'] = [{'1': 1, '2': 1} for _ in range(6)]
    observed['observer_mutation_callbacks'] = 1
    return report


class RefreshEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.report = valid_report()

    def reject(self, code=None):
        failures = MOD.validate(self.report)
        self.assertTrue(failures, 'corrupted evidence must not pass')
        self.assertEqual(failures, sorted(set(failures)))
        if code:
            self.assertIn(code, failures)

    def events(self, event=None, method=None):
        return [e for e in self.report['events']
                if (event is None or e.get('event') == event)
                and (method is None or e.get('message', {}).get('method') == method)]

    def test_complete_serialized_evidence_passes_without_mutation(self):
        before = copy.deepcopy(self.report)
        self.assertEqual(MOD.validate(self.report), [])
        self.assertEqual(self.report, before)

    def test_darwin_verified_cleanup_passes(self):
        self.report['process']['group_cleanup'] = 'darwin-confirmed-exited-only'
        self.assertEqual(MOD.validate(self.report), [])

    def test_process_cleanup_requires_each_fact(self):
        for key, value in [('returncode', 1), ('failure', 'timeout'), ('reaped', False),
                           ('drainers_finished', False), ('group_cleanup', 'failed')]:
            with self.subTest(key=key):
                self.report = valid_report()
                self.report['process'][key] = value
                self.reject('PROCESS_CLEANUP')
                del self.report['process'][key]
                # Missing failure alone is equivalent to no reported failure.
                if key != 'failure':
                    self.reject('PROCESS_CLEANUP')

    def test_editor_must_complete_successfully(self):
        for key in ('ok', 'completed'):
            for value in (False, None, 'yes', 1):
                with self.subTest(key=key, value=value):
                    self.report = valid_report()
                    target = self.report['editor'] if key == 'ok' else self.report['editor']['observed']
                    target[key] = value
                    self.reject('EDITOR_ASSERTION')

    def test_fixture_changed_added_or_deleted_fails(self):
        for mutation in ('changed', 'added', 'deleted'):
            with self.subTest(mutation=mutation):
                self.report = valid_report()
                after = self.report['fixture_after']
                if mutation == 'changed':
                    after['App.java'] = 'c' * 64
                elif mutation == 'added':
                    after['leak.java'] = 'd' * 64
                else:
                    del after['App.java']
                self.reject('FIXTURE_RESTORATION')

    def test_missing_both_fixture_inventories_fails(self):
        del self.report['fixture_before'], self.report['fixture_after']
        self.reject()

    def test_executable_integrity_requires_explicit_true(self):
        for value in (False, None, 1, 'true'):
            with self.subTest(value=value):
                self.report['unchanged_executables'] = value
                self.reject('EXECUTABLE_CHANGED')

    def test_environment_must_be_exactly_native_only(self):
        for env in (None, {}, {'PATH': '/usr/bin'},
                    {'PATH': '/nonexistent', 'JAVA_HOME': '/jdk'}):
            with self.subTest(env=env):
                self.events(event='spawn')[0]['environment'] = env
                self.reject('NATIVE_ENVIRONMENT_OR_IDENTITY')

    def test_duplicate_spawn_is_not_a_new_session(self):
        self.report['events'].insert(1, copy.deepcopy(self.events(event='spawn')[0]))
        self.reject('NATIVE_ENVIRONMENT_OR_IDENTITY')

    def test_missing_native_exit_fails(self):
        self.report['events'].remove(self.events(event='native_exit')[0])
        self.reject('NATIVE_EXIT_MISSING')

    def test_duplicate_and_unknown_native_exit_fail(self):
        for pid in (101, 999):
            with self.subTest(pid=pid):
                self.report = valid_report()
                self.report['events'].append({'event': 'native_exit', 'proxy_pid': pid, 'code': 0})
                self.reject('NATIVE_EXIT_IDENTITY')

    def test_unclean_or_missing_native_exit_status_fails(self):
        for code in (None, -9, 1, 2):
            with self.subTest(code=code):
                self.events(event='native_exit')[0]['code'] = code
                self.reject('NATIVE_EXIT_STATUS')

    def test_initialization_refusal_allows_exit_one_only_for_refused_pid(self):
        self.report = refusal_report()
        self.assertEqual(MOD.validate(self.report), [])
        self.events(event='native_exit')[1]['code'] = 2
        self.reject('NATIVE_EXIT_STATUS')
        self.report = refusal_report()
        self.events(event='native_exit')[0]['code'] = 1
        self.reject('NATIVE_EXIT_STATUS')

    def test_fault_scenarios_still_require_a_recorded_exit_code(self):
        for scenario in MOD.FAULT_SCENARIOS:
            with self.subTest(scenario=scenario):
                self.report = fault_report(scenario)
                self.assertEqual(MOD.validate(self.report), [])
                del self.events(event='native_exit')[0]['code']
                self.reject('NATIVE_EXIT_STATUS')

    def test_native_sessions_must_not_overlap(self):
        spawn = self.events(event='spawn')[1]
        self.report['events'].remove(spawn)
        self.report['events'].insert(1, spawn)
        self.reject('PROCESS_OVERLAP')

    def test_no_native_session_fails(self):
        self.report['events'] = []
        self.reject('NATIVE_EXIT_MISSING')

    def test_expected_session_count_required_and_exact(self):
        for count in (None, '2', 0, 1, 3):
            with self.subTest(count=count):
                self.report['editor']['observed']['expected_spawns'] = count
                self.reject('UNEXPECTED_SESSION_COUNT')

    def test_editor_peak_clients_required_and_exact(self):
        for count in (None, 0, 2, '1'):
            with self.subTest(count=count):
                self.report['editor']['observed']['peak_clients'] = count
                self.reject('CLIENT_OVERLAP')

    def test_missing_or_empty_replay_evidence_fails(self):
        for missing in (False, True):
            with self.subTest(missing=missing):
                self.report = valid_report()
                if missing:
                    del self.report['editor']['observed']['replays']
                else:
                    self.report['editor']['observed']['replays'] = []
                self.reject()

    def test_replay_ordinal_must_identify_a_session(self):
        for ordinal in (0, -1, 3):
            with self.subTest(ordinal=ordinal):
                self.report['editor']['observed']['replays'][0]['ordinal'] = ordinal
                self.reject('REPLAY_SESSION_MISSING')

    def test_replay_requires_exact_unsaved_unicode_text(self):
        self.events(method='textDocument/didOpen')[1]['message']['params']['textDocument']['text'] = 'class App {}\n'
        self.reject('REPLAY_TEXT')

    def test_replay_requires_exact_document_set_and_cardinality(self):
        for mutation in ('missing', 'extra', 'duplicate', 'wrong_uri'):
            with self.subTest(mutation=mutation):
                self.report = valid_report()
                opened = self.events(method='textDocument/didOpen')[1]
                if mutation == 'missing':
                    self.report['events'].remove(opened)
                elif mutation == 'wrong_uri':
                    opened['message']['params']['textDocument']['uri'] = URI + '.other'
                else:
                    extra = copy.deepcopy(opened)
                    if mutation == 'extra':
                        extra['message']['params']['textDocument']['uri'] += '.extra'
                    self.report['events'].insert(self.report['events'].index(opened), extra)
                self.reject('REPLAY_DUPLICATE_OPEN' if mutation == 'duplicate' else 'REPLAY_DOCUMENT_SET')

    def test_definition_requires_flush_after_all_replays(self):
        for mutation in ('missing', 'before_open', 'after_definition'):
            with self.subTest(mutation=mutation):
                self.report = valid_report()
                flush = self.events(method='$/setTrace')[1]
                self.report['events'].remove(flush)
                if mutation != 'missing':
                    target = self.events(method='textDocument/didOpen' if mutation == 'before_open'
                                         else 'textDocument/definition')[1]
                    index = self.report['events'].index(target) + (mutation == 'after_definition')
                    self.report['events'].insert(index, flush)
                self.reject('DEFINITION_BEFORE_REPLAY_FLUSH')

    def test_protocol_from_unknown_or_exited_session_fails(self):
        for pid in (101, 999):
            with self.subTest(pid=pid):
                self.report = valid_report()
                self.report['events'].append(wire(pid, '$/setTrace', {'value': 'off'}))
                self.reject()

    def test_proxy_cleanup_errors_cannot_pass(self):
        self.events(event='proxy_exit')[0]['errors'] = ['proxy threads did not finish']
        self.reject()

    def test_boolean_counters_are_not_integer_evidence(self):
        for key in ('expected_spawns', 'peak_clients'):
            with self.subTest(key=key):
                self.report = valid_report()
                self.report['events'] = session(101)
                self.report['editor']['observed']['expected_spawns'] = 1
                self.report['editor']['observed']['replays'] = self.report['editor']['observed']['replays'][:1]
                self.assertEqual(MOD.validate(self.report), [])
                self.report['editor']['observed'][key] = True
                self.reject('UNEXPECTED_SESSION_COUNT' if key == 'expected_spawns' else 'CLIENT_OVERLAP')

    def test_every_expected_replay_ordinal_is_required_once(self):
        for mutation in ('omit', 'duplicate', 'boolean', 'empty_documents'):
            with self.subTest(mutation=mutation):
                self.report = valid_report()
                replays = self.report['editor']['observed']['replays']
                if mutation == 'omit':
                    replays.pop()
                elif mutation == 'duplicate':
                    replays[1]['ordinal'] = 1
                elif mutation == 'boolean':
                    replays[0]['ordinal'] = True
                else:
                    replays[0]['documents'] = {}
                self.reject()

    def test_intentional_disk_verification_required(self):
        for value in (None, False, 1, 'yes'):
            with self.subTest(value=value):
                self.report['intentional_disk_verified'] = value
                self.reject('UNINTENTIONAL_DISK_CHANGE')

    def test_initialization_must_precede_open(self):
        for mutation in ('missing_reply', 'missing_initialized', 'open_before_initialized'):
            with self.subTest(mutation=mutation):
                self.report = valid_report()
                if mutation == 'missing_reply':
                    event = next(e for e in self.report['events'] if e.get('direction') == 'server_to_client'
                                 and e.get('method') == 'initialize')
                elif mutation == 'missing_initialized':
                    event = self.events(method='initialized')[0]
                else:
                    event = self.events(method='textDocument/didOpen')[0]
                self.report['events'].remove(event)
                if mutation == 'open_before_initialized':
                    self.report['events'].insert(2, event)
                self.reject('INITIALIZATION_ORDER')

    def test_preflush_changes_require_full_text_and_exact_replay(self):
        opened = self.events(method='textDocument/didOpen')[0]
        opened['message']['params']['textDocument']['text'] = 'old text'
        change = wire(101, 'textDocument/didChange', {
            'textDocument': {'uri': URI, 'version': 1}, 'contentChanges': [{'text': TEXT}]})
        self.report['events'].insert(self.report['events'].index(opened) + 1, change)
        self.assertEqual(MOD.validate(self.report), [])
        change['message']['params']['contentChanges'][0]['range'] = {
            'start': {'line': 0, 'character': 0}, 'end': {'line': 0, 'character': 1}}
        self.reject('REPLAY_NOT_FULL_TEXT')

    def test_missing_duplicate_and_unclean_proxy_exit_fail(self):
        for mutation in ('missing', 'duplicate', 'errors', 'fault', 'code'):
            with self.subTest(mutation=mutation):
                self.report = valid_report()
                event = self.events(event='proxy_exit')[0]
                if mutation == 'missing':
                    self.report['events'].remove(event)
                elif mutation == 'duplicate':
                    self.report['events'].append(copy.deepcopy(event))
                elif mutation == 'errors':
                    event['errors'] = ['unfinished thread']
                elif mutation == 'fault':
                    event['fault'] = 'transport failure'
                else:
                    event['code'] = 99
                self.reject()

    def test_native_fault_cannot_pass_normal_scenario(self):
        self.events(event='native_exit')[0]['fault'] = 'transport failure'
        self.reject('NATIVE_EXIT_STATUS')

    def test_held_response_is_genuine_and_delivered(self):
        for scenario in ('held_restarts', 'reentrant_restart', 'reentrant_stop'):
            with self.subTest(scenario=scenario):
                self.report = held_report(scenario)
                self.assertEqual(MOD.validate(self.report), [])

    def test_held_result_cannot_be_null_missing_or_changed(self):
        for mutation in ('null', 'missing', 'changed', 'missing_held'):
            with self.subTest(mutation=mutation):
                self.report = held_report()
                reply = next(e for e in self.report['events'] if e.get('direction') == 'server_to_client'
                             and e.get('method') == 'textDocument/definition')
                if mutation == 'null':
                    reply['message']['result'] = None
                elif mutation == 'missing':
                    del reply['message']['result']
                elif mutation == 'changed':
                    reply['message']['result']['uri'] += '.wrong'
                else:
                    self.report['events'].remove(self.events(event='held')[0])
                self.reject('HELD_RESULT_EVIDENCE')

    def test_held_result_requires_matching_release_and_delivery(self):
        for kind in ('released', 'delivered'):
            for mutation in ('missing', 'wrong_id', 'wrong_pid'):
                with self.subTest(kind=kind, mutation=mutation):
                    self.report = held_report()
                    event = next(e for e in self.events(event=kind) if e.get('request_id') == 2)
                    if mutation == 'missing':
                        self.report['events'].remove(event)
                    else:
                        event['request_id' if mutation == 'wrong_id' else 'proxy_pid'] = 999
                    self.reject('HELD_RESULT_DELIVERY')

    def test_callbacks_settle_once_as_stale_before_release(self):
        for mutation in ('wrong_status', 'zero_count', 'two_count', 'missing_settlement',
                         'final_zero', 'final_two', 'missing_final', 'boolean_count', 'boolean_final'):
            with self.subTest(mutation=mutation):
                self.report = held_report()
                observed = self.report['editor']['observed']
                if mutation == 'wrong_status':
                    observed['settled_before_release']['status'] = 'resolved'
                elif mutation in ('zero_count', 'two_count', 'boolean_count'):
                    observed['settled_before_release']['count'] = {
                        'zero_count': 0, 'two_count': 2, 'boolean_count': True}[mutation]
                elif mutation == 'missing_settlement':
                    del observed['settled_before_release']
                elif mutation == 'missing_final':
                    del observed['callback_final_count']
                else:
                    observed['callback_final_count'] = {
                        'final_zero': 0, 'final_two': 2, 'boolean_final': True}[mutation]
                self.reject('CALLBACK_SETTLEMENT')

    def test_terminal_state_requires_clean_idle_or_blocked_uncertain(self):
        for scenario in ('add', *MOD.FAULT_SCENARIOS):
            for mutation in ('ready', 'active', 'pending_start', 'cleanup', 'phase', 'restart_blocked'):
                with self.subTest(scenario=scenario, mutation=mutation):
                    self.report = valid_report() if scenario == 'add' else fault_report(scenario)
                    self.assertEqual(MOD.validate(self.report), [])
                    terminal = self.report['editor']['observed']['terminal_status']
                    if mutation in ('ready', 'active', 'pending_start', 'restart_blocked'):
                        terminal[mutation] = not terminal[mutation]
                    else:
                        terminal[mutation] = 'incorrect'
                    self.reject('TERMINAL_STATE')

    def test_terminal_phase_and_no_clean_client_are_independently_checked(self):
        self.report['editor']['observed']['terminal_phase'] = 'ready'
        self.reject('TERMINAL_STATE')
        self.report = valid_report()
        self.report['editor']['observed']['terminal_status']['client_id'] = 7
        self.reject('TERMINAL_STATE')

    def test_refusal_state_is_failed_and_never_ready(self):
        for key in ('ready', 'active', 'pending_start', 'restart_blocked', 'error', 'cleanup', 'phase'):
            with self.subTest(key=key):
                self.report = refusal_report()
                refused = self.report['editor']['observed']['refused_status']
                refused[key] = True if isinstance(refused[key], bool) else None
                self.reject('REFUSAL_STATE')

    def test_exact_unicode_navigation_accepts_byte_to_utf16_conversion(self):
        self.assertEqual(MOD.validate(jumped_report()), [])

    def test_navigation_target_and_range_must_match_exactly(self):
        for mutation in ('uri', 'start_line', 'start_character', 'end_line', 'end_character',
                         'missing_range', 'missing_result'):
            with self.subTest(mutation=mutation):
                self.report = jumped_report()
                result = self.report['editor']['observed']['results'][0]
                if mutation == 'uri':
                    result['result']['uri'] += '.wrong'
                elif mutation == 'missing_result':
                    del result['result']
                elif mutation == 'missing_range':
                    del result['result']['range']
                else:
                    endpoint, coordinate = mutation.split('_')
                    result['result']['range'][endpoint][coordinate] += 1
                self.reject('NAVIGATION_RANGE')

    def test_actual_editor_buffer_cursor_and_status_must_match(self):
        for mutation in ('actual_buf', 'actual_cursor', 'status'):
            with self.subTest(mutation=mutation):
                self.report = jumped_report()
                observed = self.report['editor']['observed']
                if mutation == 'status':
                    observed['results'][0]['status'] = 'empty'
                else:
                    observed['navigation'][0][mutation] = 9 if mutation == 'actual_buf' else [9, 9]
                self.reject('NAVIGATION_TARGET')

    def test_navigation_evidence_cannot_be_missing_or_unpaired(self):
        for mutation in ('missing_navigation', 'missing_results', 'both_empty'):
            with self.subTest(mutation=mutation):
                self.report = valid_report()
                observed = self.report['editor']['observed']
                if mutation != 'missing_results':
                    observed['navigation'] = []
                if mutation != 'missing_navigation':
                    observed['results'] = []
                self.reject('NAVIGATION_EVIDENCE')

    def test_empty_navigation_cannot_carry_a_target(self):
        self.report['editor']['observed']['results'][0]['result'] = {'uri': URI}
        self.reject('NAVIGATION_EMPTY')

    def test_observer_status_and_real_attachment_counts_are_stable(self):
        self.assertEqual(MOD.validate(observer_report()), [])
        for mutation in ('status_changed', 'status_boolean', 'status_missing',
                         'attach_changed', 'attach_boolean', 'attach_missing', 'attach_empty',
                         'duplicate_callback', 'missing_callback', 'boolean_callback'):
            with self.subTest(mutation=mutation):
                self.report = observer_report()
                observed = self.report['editor']['observed']
                if mutation == 'status_changed':
                    observed['observers'][3] += 1
                elif mutation == 'status_boolean':
                    observed['observers'] = [True] * 6
                elif mutation == 'status_missing':
                    observed['observers'].pop()
                elif mutation == 'attach_changed':
                    observed['observer_attachments'][3]['1'] = 2
                elif mutation == 'attach_boolean':
                    for attachment in observed['observer_attachments']:
                        attachment['1'] = True
                elif mutation == 'attach_missing':
                    observed['observer_attachments'].pop()
                elif mutation == 'attach_empty':
                    observed['observer_attachments'] = [{} for _ in range(6)]
                else:
                    observed['observer_mutation_callbacks'] = {
                        'duplicate_callback': 2, 'missing_callback': None, 'boolean_callback': True}[mutation]
                self.reject('OBSERVER_ACCUMULATION')

    def test_held_delivery_order_and_uniqueness_are_required(self):
        for mutation in ('release_before_held', 'delivery_before_release', 'held_before_result',
                         'duplicate_release', 'duplicate_delivery', 'duplicate_held', 'duplicate_response'):
            with self.subTest(mutation=mutation):
                self.report = held_report()
                events = self.report['events']
                held = self.events(event='held')[0]
                released = self.events(event='released')[0]
                delivered = next(e for e in self.events(event='delivered') if e.get('request_id') == 2)
                reply = next(e for e in events if e.get('direction') == 'server_to_client'
                             and e.get('method') == 'textDocument/definition')
                if mutation.startswith('duplicate'):
                    event = {'duplicate_release': released, 'duplicate_delivery': delivered,
                             'duplicate_held': held, 'duplicate_response': reply}[mutation]
                    events.insert(events.index(event), copy.deepcopy(event))
                else:
                    event, target = {'release_before_held': (released, held),
                                     'delivery_before_release': (delivered, released),
                                     'held_before_result': (held, reply)}[mutation]
                    events.remove(event)
                    events.insert(events.index(target), event)
                self.reject()

    def test_malformed_evidence_returns_schema_failure(self):
        for malformed in (None, [], {'process': []}, {'process': {}, 'editor': []}):
            with self.subTest(malformed=malformed):
                self.assertEqual(MOD.validate(malformed), ['EVIDENCE_SCHEMA'])
        for key, value in (('events', [None]), ('events', 'not events')):
            self.report = valid_report()
            self.report[key] = value
            self.assertEqual(MOD.validate(self.report), ['EVIDENCE_SCHEMA'])


class RefreshInputGuardTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.nvim = self.root / 'input/bin/nvim'
        self.nvim.parent.mkdir(parents=True)
        self.nvim.write_bytes(b'offline dummy, never executed')
        self.native = self.root / 'java-analyzer'
        self.native.write_bytes(b'offline native dummy, never executed')
        self.args = SimpleNamespace(nvim=self.nvim, native=self.native,
                                    output=self.root / 'out', adapter=None, scenario=['add'])
        self.provenance = {'binary_sha256': MOD.EDITOR.digest(self.nvim),
                           'manifest': json.loads((MOD.ROOT / 'scripts/neovim-inputs.json').read_text())}
        self.write_provenance()

    def write_provenance(self):
        (self.nvim.parent.parent / 'INPUT.json').write_text(json.dumps(self.provenance))

    def test_existing_output_is_not_replaced(self):
        self.args.output.mkdir()
        keep = self.args.output / 'keep'
        keep.write_text('original')
        with patch.object(MOD.EDITOR, 'run_bounded') as run:
            with self.assertRaises(FileExistsError):
                MOD.run(self.args)
            run.assert_not_called()
        self.assertEqual(keep.read_text(), 'original')

    def test_output_symlink_is_rejected_without_touching_target(self):
        target = self.root / 'target'
        target.mkdir()
        self.args.output.symlink_to(target, target_is_directory=True)
        with patch.object(MOD.EDITOR, 'run_bounded') as run:
            with self.assertRaisesRegex(ValueError, 'symlink'):
                MOD.run(self.args)
            run.assert_not_called()
        self.assertEqual(list(target.iterdir()), [])

    def test_changed_binary_or_manifest_rejected_before_execution(self):
        for field in ('binary_sha256', 'manifest'):
            with self.subTest(field=field):
                self.args.output = self.root / ('out-' + field)
                original = self.provenance[field]
                self.provenance[field] = 'corrupt'
                self.write_provenance()
                with patch.object(MOD.EDITOR, 'run_bounded') as run:
                    with self.assertRaises(ValueError):
                        MOD.run(self.args)
                    run.assert_not_called()
                self.provenance[field] = original

    def test_wrong_or_failed_version_is_rejected(self):
        for index, result in enumerate((
                {'failure': None, 'returncode': 0, 'stdout': b'NVIM v0.11.4\n'},
                {'failure': 'timeout', 'returncode': 0, 'stdout': b'NVIM v0.11.5\n'},
                {'failure': None, 'returncode': 1, 'stdout': b'NVIM v0.11.5\n'})):
            with self.subTest(result=result):
                self.args.output = self.root / f'out-{index}'
                with patch.object(MOD.EDITOR, 'run_bounded', return_value=result) as run:
                    with self.assertRaisesRegex(ValueError, '0.11.5 required'):
                        MOD.run(self.args)
                    self.assertEqual(run.call_args.kwargs['env'], {'PATH': '/nonexistent'})

    def test_cli_missing_arguments_and_unknown_scenario_fail(self):
        for args in ([], ['--nvim', str(self.nvim), '--native', str(self.native),
                          '--output', str(self.args.output), '--scenario', 'invented']):
            result = subprocess.run([sys.executable, str(Path(MOD.__file__)), *args],
                                    capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 2)
            self.assertIn('error:', result.stderr)
            self.assertFalse(self.args.output.exists())


if __name__ == '__main__':
    unittest.main()
