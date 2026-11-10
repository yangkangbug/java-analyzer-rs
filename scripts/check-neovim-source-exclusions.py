#!/usr/bin/env python3
"""Independent real pinned-Neovim source selection gate; no fetch/build/JVM.

Only the established bounded process runner and transparent real-native proxy
are reused. This gate has its own fixtures, expectations and evidence validator.
"""
import argparse
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location('exclusion_editor_runner', ROOT / 'scripts/check-neovim-editor.py')
EDITOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EDITOR)
SCENARIOS = ('excluded_buffers', 'restart', 'nested', 'validation')
LEGACY_COMMIT = 'd30ff6ba4ff67dae4a9862c67507bda4c0bb40ee'
LEGACY_SHA256 = 'a8be5c2bd9f85c2428ccc65492bccc31592f2131125412892736639c48e78ab6'
CODES = frozenset(('EVIDENCE_SCHEMA', 'PROCESS_CLEANUP', 'EDITOR_ASSERTION', 'FIXTURE_CHANGED',
    'EXECUTABLE_CHANGED', 'SESSION_COUNT', 'PROCESS_OVERLAP', 'PROXY_CLEANUP', 'PROTOCOL_ORDER',
    'NATIVE_ENVIRONMENT', 'NATIVE_EXIT', 'EXCLUDED_SYNC', 'REPLAY_TEXT', 'REPLAY_DOCUMENTS',
    'REPLAY_FLUSH', 'NAVIGATION', 'BUFFER_PRESERVATION', 'ATTACHMENT', 'REFUSAL', 'VALIDATION', 'TERMINAL_STATE', 'LEGACY_IDENTITY', 'NEGATIVE_CONTROL'))


def clean(process):
    return (type(process) is dict and type(process.get('returncode')) is int
        and process['returncode'] == 0 and process.get('failure') is None
        and process.get('reaped') is True and process.get('drainers_finished') is True
        and process.get('group_cleanup') in ('signalled', 'darwin-confirmed-exited-only'))


def _validate(report):
    failures = []
    def require(value, code):
        if not value: failures.append(code)
    scenario = report['scenario']
    require(scenario in SCENARIOS, 'EVIDENCE_SCHEMA')
    require(clean(report['process']), 'PROCESS_CLEANUP')
    observed = report['editor']['observed']
    oracle = report['oracle']
    require(report['editor']['ok'] is True and observed.get('completed') is True, 'EDITOR_ASSERTION')
    require(type(report['fixture_before']) is dict and bool(report['fixture_before'])
        and report['fixture_before'] == report['fixture_after'], 'FIXTURE_CHANGED')
    require(report['unchanged_executables'] is True and report['unchanged_scripts'] is True, 'EXECUTABLE_CHANGED')
    terminal = observed['terminal_status']
    require(terminal['phase'] in ('idle', 'failed') and terminal['cleanup'] == 'safe'
        and all(terminal[k] is False for k in ('active', 'ready', 'pending_start', 'restart_blocked')), 'TERMINAL_STATE')
    events = report['events']
    require(type(events) is list and len(events) <= 8192, 'EVIDENCE_SCHEMA')
    sessions, live, ended, proxies = [], set(), set(), set()
    states, opened, initial, flushed, requests, definitions, session_definitions = {}, {}, {}, set(), {}, [], {}
    for event in events:
        pid = event['proxy_pid']
        require(type(pid) is int and pid > 0, 'EVIDENCE_SCHEMA')
        kind = event.get('event')
        if kind == 'spawn':
            require(pid not in states, 'PROTOCOL_ORDER')
            require(not live, 'PROCESS_OVERLAP')
            sessions.append(pid); live.add(pid)
            states[pid], opened[pid], initial[pid], requests[pid], session_definitions[pid] = 'new', set(), {}, {}, []
            require(event['environment'] == {'PATH': '/nonexistent'}, 'NATIVE_ENVIRONMENT')
        elif kind == 'native_exit':
            require(pid in live and pid not in ended, 'NATIVE_EXIT')
            require(type(event['code']) is int and event['code'] == (1 if states[pid] == 'refused_exit' else 0)
                and event.get('fault') is None and states[pid] in ('exit', 'refused_exit'), 'NATIVE_EXIT')
            live.discard(pid); ended.add(pid)
        elif kind == 'proxy_exit':
            require(pid in ended and pid not in proxies and event['errors'] == []
                and event.get('fault') is None and type(event['code']) is int
                and event['code'] == (1 if states[pid] == 'refused_exit' else 0), 'PROXY_CLEANUP')
            proxies.add(pid)
        elif event.get('direction') == 'client_to_server':
            require(pid in live, 'PROTOCOL_ORDER')
            msg = event['message']; method = msg.get('method'); params = msg.get('params', {})
            if 'id' in msg:
                require(msg['id'] not in requests[pid], 'PROTOCOL_ORDER')
                requests[pid][msg['id']] = method
            if method == 'initialize':
                require(states[pid] == 'new', 'PROTOCOL_ORDER'); states[pid] = 'initializing'
            elif method == 'initialized':
                require(states[pid] == 'initialized_response', 'PROTOCOL_ORDER'); states[pid] = 'ready'
            elif method == 'textDocument/didOpen':
                require(states[pid] == 'ready', 'PROTOCOL_ORDER')
                doc = params['textDocument']; uri = doc['uri']
                expected = oracle['sessions'][sessions.index(pid)]['documents']
                require(uri in expected, 'EXCLUDED_SYNC')
                require(uri not in opened[pid], 'REPLAY_DOCUMENTS'); opened[pid].add(uri)
                require(doc.get('languageId') == 'java' and type(doc.get('version')) is int, 'REPLAY_TEXT')
                if pid not in flushed: initial[pid][uri] = doc['text']
                else: require(False, 'REPLAY_DOCUMENTS')
            elif method == 'textDocument/didChange':
                uri = params['textDocument']['uri']
                expected = oracle['sessions'][sessions.index(pid)]['documents']
                require(states[pid] == 'ready' and uri in opened[pid] and uri in expected, 'EXCLUDED_SYNC')
                require(type(params['contentChanges']) is list and len(params['contentChanges']) == 1
                    and set(params['contentChanges'][0]) == {'text'}
                    and params['contentChanges'][0]['text'] == expected.get(uri), 'REPLAY_TEXT')
            elif method == '$/setTrace':
                require(states[pid] == 'ready', 'PROTOCOL_ORDER'); flushed.add(pid)
            elif method == 'textDocument/definition':
                require(pid in flushed and states[pid] == 'ready', 'REPLAY_FLUSH')
                require(params['textDocument']['uri'] in opened[pid], 'EXCLUDED_SYNC')
            elif method == 'textDocument/didClose':
                require(params['textDocument']['uri'] in opened[pid], 'EXCLUDED_SYNC')
            elif method == 'shutdown':
                require(states[pid] == 'ready', 'PROTOCOL_ORDER'); states[pid] = 'shutdown'
            elif method == 'exit':
                require(states[pid] in ('shutdown_response', 'refused'), 'PROTOCOL_ORDER')
                states[pid] = 'refused_exit' if states[pid] == 'refused' else 'exit'
        elif event.get('direction') == 'server_to_client':
            msg = event['message']; method = event.get('method')
            if 'id' not in msg: continue
            require(requests[pid].pop(msg['id'], None) == method, 'PROTOCOL_ORDER')
            if method == 'initialize':
                require(states[pid] == 'initializing', 'PROTOCOL_ORDER')
                refused = oracle['sessions'][sessions.index(pid)]['refused']
                if refused:
                    require(msg.get('error', {}).get('code') == -32803
                        and 'source' in msg.get('error', {}).get('message', '').lower(), 'REFUSAL')
                    states[pid] = 'refused'
                else:
                    require(type(msg.get('result', {}).get('capabilities')) is dict and 'error' not in msg, 'REFUSAL')
                    states[pid] = 'initialized_response'
            elif method == 'shutdown':
                require(states[pid] == 'shutdown' and msg.get('result', False) is None, 'PROTOCOL_ORDER')
                states[pid] = 'shutdown_response'
            elif method == 'textDocument/definition':
                require(states[pid] == 'ready' and 'error' not in msg, 'NAVIGATION')
                definitions.append(msg.get('result'))
                session_definitions[pid].append(msg.get('result'))
    require(len(sessions) == len(oracle['sessions']), 'SESSION_COUNT')
    require(not live and ended == set(sessions) and proxies == ended, 'PROXY_CLEANUP')
    for pid, expected in zip(sessions, oracle['sessions']):
        require(set(initial[pid]) == set(expected['documents']), 'REPLAY_DOCUMENTS')
        require(initial[pid] == expected['documents'], 'REPLAY_TEXT')
        require((pid in flushed) is (not expected['refused']), 'REPLAY_FLUSH')
        require(session_definitions[pid] == expected['navigation'], 'NAVIGATION')
    require(type(observed['peak_clients']) is int and observed['peak_clients'] == (0 if scenario == 'validation' else 1), 'PROCESS_OVERLAP')
    require(definitions == oracle['navigation'], 'NAVIGATION')
    navigation = observed['navigation']
    require(len(navigation) == len(oracle['navigation']), 'NAVIGATION')
    for actual, expected in zip(navigation, oracle['navigation']):
        require(actual['result']['status'] == 'jumped' and actual['result']['result'] == expected
            and actual['count'] == 1 and type(actual['count']) is int and actual['uri'] == expected['uri']
            and actual['cursor'] == oracle['cursors'][expected['uri']], 'NAVIGATION')
    require(observed['attachments'] == oracle['attachments'], 'ATTACHMENT')
    if scenario != 'validation':
        before, after = observed['before'], observed['after']
        require(type(before) is dict and set(before) == set(oracle['buffers']) and before == after, 'BUFFER_PRESERVATION')
        for key, expected in oracle['buffers'].items():
            actual = before[key]
            require(actual['name'] == expected['name'] and actual['lines'] == expected['lines']
                and actual['modified'] is True and type(actual['bufnr']) is int and actual['bufnr'] > 0
                and type(actual['tick']) is int and actual['tick'] > 0
                and actual['endofline'] is expected['endofline'] and actual['fileformat'] == expected['fileformat']
                and actual['fixendofline'] is False, 'BUFFER_PRESERVATION')
        if scenario == 'nested':
            require(observed['after_refusal'] == before, 'BUFFER_PRESERVATION')
            status = observed['refused_status']
            require(status['phase'] == 'failed' and status['cleanup'] == 'safe'
                and all(status[k] is False for k in ('active', 'ready', 'pending_start', 'restart_blocked'))
                and type(status['error']) is str and bool(status['error']), 'REFUSAL')
    else:
        require(observed['validations'] == oracle['validations'], 'VALIDATION')
        native = report['native_validation']
        require(len(native) == len(oracle['validations']), 'VALIDATION')
        for actual, expected in zip(native, oracle['validations']):
            process = dict(actual['process']); code = process['returncode']; process['returncode'] = 0
            require(clean(process) and type(code) is int and code in (0, 2)
                and actual['name'] == expected['name'] and actual['accepted'] is expected['accepted']
                and (code == 0) is expected['accepted'] and actual['json_valid'] is True
                and (actual['error_code'] is None if expected['accepted']
                     else actual['error_code'] in ('invalid_json', 'invalid_configuration', 'project_load')), 'VALIDATION')
    return sorted(set(failures))


def validate(report):
    try: return _validate(report)
    except (KeyError, TypeError, ValueError, IndexError, AttributeError): return ['EVIDENCE_SCHEMA']



def _negative_control(report):
    """Accept only the pinned legacy adapter's excluded-file attach/sync defect.

    A timeout, crash, failed initialize, unrelated assertion or uncertain cleanup
    is not RED evidence. No product or protocol result is rewritten.
    """
    failures = []
    def require(value, code):
        if not value: failures.append(code)
    require(report['scenario'] == 'excluded_buffers', 'NEGATIVE_CONTROL')
    require(report['identity']['adapter_sha256'] == LEGACY_SHA256
        and report['identity']['adapter_override'] is True, 'LEGACY_IDENTITY')
    process = dict(report['process'])
    require(type(process['returncode']) is int and process['returncode'] == 1, 'NEGATIVE_CONTROL')
    process['returncode'] = 0
    require(clean(process), 'PROCESS_CLEANUP')
    require(report['fixture_before'] and report['fixture_before'] == report['fixture_after'], 'FIXTURE_CHANGED')
    require(report['unchanged_executables'] is True and report['unchanged_scripts'] is True, 'EXECUTABLE_CHANGED')
    observed, oracle = report['editor']['observed'], report['oracle']
    require(report['editor']['ok'] is False and observed.get('completed') is not True
        and observed['failure_code'] == 'EXCLUDED_BUFFER_ATTACHED'
        and observed['attachment_failure'] == {'key': 'one', 'expected': False, 'actual': True},
        'NEGATIVE_CONTROL')
    require(observed['navigation'] == [] and len(observed['attachments']) == 1
        and observed['attachments'][0]['one'] is True and type(observed['peak_clients']) is int
        and observed['peak_clients'] == 1, 'NEGATIVE_CONTROL')
    terminal = observed['failure_cleanup_status']
    require(terminal['phase'] == 'idle' and terminal['cleanup'] == 'safe'
        and all(terminal[k] is False for k in ('active', 'ready', 'pending_start', 'restart_blocked')), 'TERMINAL_STATE')
    before = observed['initial_buffers']
    require(type(before) is dict and before == observed['failure_buffers']
        and set(before) == set(oracle['initial_buffers']), 'BUFFER_PRESERVATION')
    for key, expected in oracle['initial_buffers'].items():
        actual = before[key]
        require(all(actual[k] == v for k, v in expected.items())
            and actual['modified'] is True and actual['fixendofline'] is False
            and type(actual['bufnr']) is int and actual['bufnr'] > 0
            and type(actual['tick']) is int and actual['tick'] > 0, 'BUFFER_PRESERVATION')
    events = report['events']
    require(type(events) is list and len(events) <= 8192, 'EVIDENCE_SCHEMA')
    pid, state, requests, witnessed, flushed = None, 'new', {}, False, False
    for event in events:
        kind = event.get('event')
        if kind == 'spawn':
            require(pid is None and type(event['proxy_pid']) is int and event['proxy_pid'] > 0, 'PROCESS_OVERLAP')
            pid = event['proxy_pid']
            require(event['environment'] == {'PATH': '/nonexistent'}, 'NATIVE_ENVIRONMENT')
        else:
            require(pid is not None and event['proxy_pid'] == pid, 'PROTOCOL_ORDER')
        if event.get('direction') == 'client_to_server':
            msg = event['message']; method = msg.get('method')
            if 'id' in msg:
                require(msg['id'] not in requests, 'PROTOCOL_ORDER'); requests[msg['id']] = method
            if method == 'initialize':
                require(state == 'new', 'PROTOCOL_ORDER'); state = 'initializing'
            elif method == 'initialized':
                require(state == 'initialized_response', 'PROTOCOL_ORDER'); state = 'ready'
            elif method == 'textDocument/didOpen':
                require(state == 'ready', 'PROTOCOL_ORDER')
                doc = msg['params']['textDocument']
                if doc['uri'] == oracle['excluded_witness_uri']:
                    expected = oracle['initial_buffers']['one']['lines']
                    require(doc.get('languageId') == 'java' and type(doc.get('version')) is int
                        and doc['text'] == '\n'.join(expected) + '\n' and not witnessed, 'NEGATIVE_CONTROL')
                    witnessed = True
            elif method == '$/setTrace':
                require(state == 'ready', 'PROTOCOL_ORDER'); flushed = True
            elif method == 'textDocument/didClose':
                require(state in ('ready', 'shutdown', 'shutdown_response', 'exit'), 'PROTOCOL_ORDER')
            elif method == 'shutdown':
                require(state == 'ready' and witnessed and flushed, 'PROTOCOL_ORDER'); state = 'shutdown'
            elif method == 'exit':
                require(state == 'shutdown_response', 'PROTOCOL_ORDER'); state = 'exit'
            else:
                require(False, 'NEGATIVE_CONTROL')
        elif event.get('direction') == 'server_to_client':
            msg = event['message']; method = event.get('method')
            if 'id' not in msg: continue
            require(requests.pop(msg['id'], None) == method and 'error' not in msg, 'PROTOCOL_ORDER')
            if method == 'initialize':
                require(state == 'initializing' and type(msg.get('result', {}).get('capabilities')) is dict, 'REFUSAL')
                state = 'initialized_response'
            elif method == 'shutdown':
                require(state == 'shutdown' and msg.get('result', False) is None, 'PROTOCOL_ORDER')
                state = 'shutdown_response'
            else: require(False, 'NEGATIVE_CONTROL')
        elif kind == 'native_exit':
            require(state == 'exit' and type(event['code']) is int and event['code'] == 0
                and event.get('fault') is None, 'NATIVE_EXIT'); state = 'native_exited'
        elif kind == 'proxy_exit':
            require(state == 'native_exited' and type(event['code']) is int and event['code'] == 0
                and event.get('fault') is None and event['errors'] == [], 'PROXY_CLEANUP'); state = 'done'
    require(state == 'done' and witnessed and flushed and not requests, 'NEGATIVE_CONTROL')
    return sorted(set(failures))


def validate_negative_control(report):
    try: return _negative_control(report)
    except (KeyError, TypeError, ValueError, IndexError, AttributeError): return ['EVIDENCE_SCHEMA']

def public_summary(report):
    """Bounded enum-only summary, never diagnostics, text, paths or process IDs."""
    if type(report) is not dict: report = {}
    codes = report.get('failure_codes')
    if type(codes) is not list: codes = ['EVIDENCE_SCHEMA']
    safe = {c if type(c) is str and c in CODES else 'EVIDENCE_SCHEMA' for c in codes[:128]}
    if len(codes) > 128: safe.add('EVIDENCE_SCHEMA')
    scenario = report.get('scenario')
    return {'scenario': scenario if type(scenario) is str and scenario in SCENARIOS else 'unknown',
        'ok': report.get('ok') is True, 'failure_codes': sorted(safe)}


def validation_cases(base):
    template = {'schema_version': 1, 'compilations': [{'id': {'project': 'validation', 'source_set': 'main'},
        'source_roots': ['empty']}]}
    (base / 'empty').mkdir()
    cases = []
    def add(name, accepted, value): cases.append((name, accepted, value))
    for name, rules in [('absent', None), ('empty', []), ('literal_bang_dash', ['!skip', '--skip']),
        ('ordinary_unicode', ['é', '中', '😀', 'Ω', 'a\u0301', 'space here']), ('case_sensitive', ['Foo', 'foo']),
        ('component_sensitive', ['foo', 'foobar', 'foo/bar']), ('max_bytes', ['é' * 2048]),
        ('max_rules', ['rule' + str(i) for i in range(1024)])]:
        value = copy.deepcopy(template)
        if rules is not None: value['compilations'][0]['source_exclusions'] = rules
        add(name, True, value)
    invalid = [('empty_rule', ['']), ('duplicate', ['skip', 'skip']), ('too_many', ['r' + str(i) for i in range(1025)]),
        ('too_long', ['x' * 4097]), ('unicode_too_long', ['é' * 2048 + 'x']), ('null', None),
        ('string', 'skip'), ('object', {}), ('bool', True), ('number', 1), ('element_null', [None]),
        ('element_object', [{}]), ('element_number', [1]), ('element_bool', [False]), ('element_array', [[]])]
    invalid += [('path_' + str(i), [rule]) for i, rule in enumerate(
        ('/a', 'a/', 'a//b', '.', '..', 'a/./b', 'a/../b', 'a\\b', 'a:b', '*', '?', '[x]', 'a]b', '{x}', 'a}b',
         '\x00', '\x01', '\x1f', '\x7f', '\x80', '\x9f'))]
    for name, rules in invalid:
        value = copy.deepcopy(template); value['compilations'][0]['source_exclusions'] = rules; add(name, False, value)
    for name, field, value in [('roots_object', 'source_roots', {}), ('roots_string', 'source_roots', 'empty'),
        ('roots_null', 'source_roots', None), ('roots_number', 'source_roots', [1]),
        ('roots_element_array', 'source_roots', [[]]), ('roots_element_object', 'source_roots', [{}])]:
        doc = copy.deepcopy(template); doc['compilations'][0][field] = value; add(name, False, doc)
    for name, value in [('top_null', None), ('top_array', []), ('top_number', 1), ('top_string', 'x'),
        ('compilations_object', {'schema_version': 1, 'compilations': {}}),
        ('compilations_null', {'schema_version': 1, 'compilations': None}),
        ('compilation_scalar', {'schema_version': 1, 'compilations': [1]}),
        ('compilation_array', {'schema_version': 1, 'compilations': [[]]})]: add(name, False, value)
    for name, roots in [('no_roots', []), ('absent_roots', None)]:
        value = copy.deepcopy(template); value['compilations'][0]['source_exclusions'] = ['skip']
        if roots is None: del value['compilations'][0]['source_roots']
        else: value['compilations'][0]['source_roots'] = roots
        add(name, False, value)
    result = []
    for i, (name, accepted, value) in enumerate(cases):
        path = base / ('validation-' + str(i) + '.json'); path.write_text(json.dumps(value, ensure_ascii=False))
        result.append({'name': name, 'accepted': accepted, 'path': str(path)})
    for name, raw in [('truncated_json', '{'), ('trailing_json', '{}{}')]:
        path = base / (name + '.json'); path.write_text(raw)
        result.append({'name': name, 'accepted': False, 'path': str(path)})
    return result


def fixture(folder, scenario):
    base = folder / 'fixture 中 # %'; base.mkdir()
    sources = {'app': ('src/App.java', 'class App extends Library {\n  void run() { /* 中😀 */ valueA++; }\n  nested.Nested nested;\n}'),
        'target': ('src/Library.java', 'class Library {\n  /* 😀中 */ public int valueA;\n}'),
        'one': ('src/One.java', 'class One {}'), 'neighbor': ('src/Ones.java', 'class Ones {}'),
        'skip': ('src/skip/Skip.java', 'class Skip {}'), 'peer_skip': ('peer/skip/PeerSkip.java', 'class PeerSkip {}'),
        'peer': ('peer/Peer.java', 'class Peer {}'), 'prefix': ('src/skipOther/Keep.java', 'class Keep {}'),
        'bang': ('src/!Literal.java', 'class Bang {}'), 'dash': ('src/--Literal.java', 'class Dash {}'),
        'unicode': ('src/中/Hidden.java', 'class Hidden {}'), 'accent': ('src/Ω/Accent.java', 'class Accent {}'),
        'emoji': ('src/😀/Emoji.java', 'class Emoji {}'), 'case': ('src/CaseOnly/Case.java', 'class Case {}'),
        'nested': ('src/nested/Nested.java', 'package nested; public class Nested {}'),
        'nested_hidden': ('src/nested/Hidden.java', 'package nested; class Hidden {}'),
        'late': ('src/skip/Late.java', 'class Late {}')}
    paths, dirty = {}, {}
    for key, (relative, text) in sources.items():
        path = base / relative; path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes((text + '\n').encode())
        paths[key] = str(path); dirty[key] = '// unsaved ' + key + '\n' + text.replace('valueA', 'valueB')
    directory = base / 'src/Directory.java'; directory.mkdir(); paths['directory'] = str(directory)
    dirty['directory'] = '// a directory buffer, never a source'
    excluded = ['one', 'skip', 'peer_skip', 'bang', 'dash', 'unicode', 'accent', 'emoji', 'late', 'directory']
    rules = ['One.java', 'skip', '!Literal.java', '--Literal.java', '中', 'Ω', '😀', 'caseonly']
    manifest = {'schema_version': 1, 'compilations': [{'id': {'project': 'main', 'source_set': 'main'},
        'source_roots': ['src', 'peer'], 'source_exclusions': rules}]}
    if scenario == 'nested':
        manifest['compilations'][0]['source_exclusions'].append('nested')
        manifest['compilations'][0]['dependencies'] = [{'project': 'nested', 'source_set': 'main'}]
        manifest['compilations'].append({'id': {'project': 'nested', 'source_set': 'main'},
            'source_roots': ['src/nested'], 'source_exclusions': ['Hidden.java']})
        excluded.append('nested_hidden')
    initial = [key for key in paths if key not in excluded]
    changed = copy.deepcopy(manifest)
    changed['compilations'][0]['source_exclusions'].remove('One.java')
    changed['compilations'][0]['source_exclusions'].append('Ones.java')
    changed_keys = [key for key in initial if key != 'neighbor'] + ['one']
    duplicate = copy.deepcopy(manifest)
    if scenario == 'nested': duplicate['compilations'][0]['source_exclusions'].remove('nested')
    manifest_path = base / 'project.json'; original = json.dumps(manifest); manifest_path.write_text(original)
    cfg = {'scenario': scenario, 'paths': paths, 'keys': list(paths), 'dirty': dirty,
        'manifest': str(manifest_path), 'original_manifest': original, 'initial_keys': initial,
        'changed_manifest': changed, 'changed_keys': changed_keys, 'duplicate_manifest': duplicate,
        'excluded_keys': excluded, 'original_sources': {str(base / rel): (text + '\n') for rel, text in sources.values()},
        'validation_cases': validation_cases(base) if scenario == 'validation' else []}
    def document(key, edited=False):
        text = dirty[key]
        if edited and key in excluded: text = '// additional unsaved excluded edit\n' + text
        ending = '\r\n' if key == 'app' else '\n'
        return text.replace('\n', ending) + ('' if key == 'target' else ending)
    def session(keys, edited=False):
        return {'refused': False, 'documents': {Path(paths[k]).as_uri(): document(k, edited) for k in keys}}
    def attached(keys, late=True): return {k: k in keys for k in paths if late or k != 'late'}
    sessions = [session(initial)]
    attachment = [attached(initial, False), attached(initial)]
    if scenario == 'restart':
        sessions += [session(changed_keys, True), session(initial, True)]
        attachment += [attached(initial), attached(changed_keys), attached(initial)]
    if scenario == 'nested':
        sessions += [{'refused': True, 'documents': {}}, session(initial, True)]
        attachment += [attached(initial)]
    cursors, locations = {}, {}
    for key, word in [('target', 'valueB'), ('nested', 'Nested')]:
        lines = dirty[key].splitlines(); row = next(i for i, line in enumerate(lines) if word in line)
        col = lines[row].index(word); uri = Path(paths[key]).as_uri()
        start = len(lines[row][:col].encode('utf-16-le')) // 2
        locations[key] = {'uri': uri, 'range': {'start': {'line': row, 'character': start},
            'end': {'line': row, 'character': start + len(word)}}}
        cursors[uri] = [row + 1, len(lines[row][:col].encode())]
    navigation = [locations['target']] * (5 if scenario == 'restart' else 2)
    if scenario == 'nested': navigation += [locations['nested'], locations['target'], locations['nested']]
    buffers = {key: {'name': path, 'lines': document(key, True).replace('\r\n', '\n').removesuffix('\n').split('\n'),
        'endofline': key != 'target', 'fileformat': 'dos' if key == 'app' else 'unix'} for key, path in paths.items()}
    counts = {'excluded_buffers': [2], 'restart': [3, 1, 1], 'nested': [3, 0, 2], 'validation': [2]}[scenario]
    offset = 0
    for entry, count in zip(sessions, counts):
        entry['navigation'] = navigation[offset:offset + count]; offset += count
    initial_buffers = {key: {**value, 'lines': document(key).replace('\r\n', '\n').removesuffix('\n').split('\n')}
        for key, value in buffers.items() if key != 'late'}
    oracle = {'sessions': sessions if scenario != 'validation' else [], 'navigation': navigation if scenario != 'validation' else [],
        'cursors': cursors, 'attachments': attachment if scenario != 'validation' else [], 'buffers': buffers,
        'initial_buffers': initial_buffers, 'excluded_witness_uri': Path(paths['one']).as_uri(),
        'validations': [{'name': c['name'], 'accepted': c['accepted']} for c in cfg['validation_cases']]}
    return base, cfg, oracle


def run(args):
    nvim, native = args.nvim.resolve(strict=True), args.native.resolve(strict=True)
    adapter = args.adapter.resolve(strict=True) if args.adapter else ROOT / 'editors/neovim'
    negative = getattr(args, 'expect_legacy_exclusion_failure', False)
    if negative and (not args.adapter or args.scenario != ['excluded_buffers']
            or EDITOR.digest(adapter / 'lua/java_analyzer.lua') != LEGACY_SHA256):
        raise ValueError('negative control requires the pinned legacy adapter and excluded_buffers only')
    if args.output.is_symlink(): raise ValueError('output may not be a symlink')
    output = args.output.parent.resolve() / args.output.name; output.mkdir(parents=True, exist_ok=False)
    provenance = json.loads((nvim.parent.parent / 'INPUT.json').read_text())
    if provenance['binary_sha256'] != EDITOR.digest(nvim): raise ValueError('Neovim binary changed')
    if provenance['manifest'] != json.loads((ROOT / 'scripts/neovim-inputs.json').read_text()): raise ValueError('wrong client provenance')
    version = EDITOR.run_bounded([str(nvim), '--version'], env={'PATH': '/nonexistent'}, cwd=output, timeout=10)
    if not clean(version) or version['stdout'].splitlines()[:1] != [b'NVIM v0.11.5']: raise ValueError('pinned client required')
    scripts = [Path(__file__).resolve(), ROOT / 'scripts/check-neovim-editor.py', ROOT / 'editors/neovim/tests/source_exclusions.lua',
        ROOT / 'editors/neovim/tests/refresh_proxy.py', ROOT / 'editors/neovim/tests/delay_proxy.py', adapter / 'lua/java_analyzer.lua']
    script_hashes = {str(path): EDITOR.digest(path) for path in scripts}
    git = subprocess.run(['git', '-C', str(ROOT), 'rev-parse', 'HEAD', 'HEAD^{tree}'], capture_output=True, text=True, check=True).stdout.splitlines()
    identity = {'native_sha256': EDITOR.digest(native), 'neovim_sha256': EDITOR.digest(nvim),
        'adapter_sha256': EDITOR.digest(adapter / 'lua/java_analyzer.lua'), 'adapter_override': bool(args.adapter),
        'source_revision': git[0], 'source_tree': git[1], 'client_input': provenance,
        'script_sha256': script_hashes, 'source_dirty': bool(subprocess.run(['git', '-C', str(ROOT), 'status', '--porcelain'], capture_output=True, check=True).stdout)}
    results = []
    for scenario in args.scenario or SCENARIOS:
        folder = output / scenario; folder.mkdir()
        base, cfg, oracle = fixture(folder, scenario); before = EDITOR.inventory(base)
        home, control = folder / 'home', folder / 'control'; home.mkdir(); control.mkdir()
        proxy_script = ROOT / 'editors/neovim/tests/refresh_proxy.py'; proxy = folder / 'proxy'
        argv = [str(proxy_script), '--native', str(native), '--control', str(control)]
        proxy.write_text(f'#!{sys.executable}\nimport runpy,sys\nsys.argv = {argv!r} + sys.argv[1:]\nrunpy.run_path({str(proxy_script)!r}, run_name="__main__")\n'); proxy.chmod(0o755)
        cfg.update({'adapter': str(adapter), 'proxy': str(proxy), 'result': str(folder / 'editor.json')})
        config = folder / 'config.json'; config.write_text(json.dumps(cfg, ensure_ascii=False))
        env = {'PATH': '/nonexistent', 'HOME': str(home), 'XDG_CONFIG_HOME': str(home / 'config'), 'XDG_DATA_HOME': str(home / 'data'),
            'XDG_STATE_HOME': str(home / 'state'), 'XDG_CACHE_HOME': str(home / 'cache'), 'NVIM_LOG_FILE': str(folder / 'nvim.log'), 'LANG': 'C.UTF-8'}
        native_validation = []
        for case in cfg['validation_cases']:
            capture = EDITOR.run_bounded([str(native), 'inspect', '--manifest', case['path']], env={'PATH': '/nonexistent'}, cwd=folder, timeout=10)
            stdout, stderr = capture.pop('stdout'), capture.pop('stderr')
            (folder / (case['name'] + '-native.json')).write_bytes(stdout)
            (folder / (case['name'] + '-stderr.log')).write_bytes(stderr)
            try:
                response = json.loads(stdout if capture['returncode'] == 0 else stderr)
                json_valid = type(response) is dict and response.get('schema_version') == 1
                error_code = response.get('error', {}).get('code')
            except (ValueError, UnicodeDecodeError, AttributeError): json_valid, error_code = False, None
            native_validation.append({'name': case['name'], 'accepted': capture['returncode'] == 0,
                'json_valid': json_valid, 'error_code': error_code, 'process': capture})
        capture = EDITOR.run_bounded([str(nvim), '--headless', '-u', 'NONE', '-i', 'NONE', '-n', '-l',
            str(ROOT / 'editors/neovim/tests/source_exclusions.lua'), str(config)], env=env, cwd=folder, timeout=55)
        (folder / 'stdout.log').write_bytes(capture.pop('stdout')); (folder / 'stderr.log').write_bytes(capture.pop('stderr'))
        editor = json.loads((folder / 'editor.json').read_text()) if (folder / 'editor.json').exists() else {}
        protocol = control / 'protocol.jsonl'; events = [json.loads(line) for line in protocol.read_text().splitlines()] if protocol.exists() else []
        report = {'scenario': scenario, 'identity': identity, 'process': capture, 'editor': editor, 'events': events,
            'oracle': oracle, 'native_validation': native_validation, 'fixture_before': before, 'fixture_after': EDITOR.inventory(base),
            'unchanged_executables': EDITOR.digest(native) == identity['native_sha256'] and EDITOR.digest(nvim) == identity['neovim_sha256'],
            'unchanged_scripts': all(EDITOR.digest(Path(path)) == digest for path, digest in script_hashes.items())}
        report['failure_codes'] = (validate_negative_control if negative else validate)(report)
        report['ok'] = not report['failure_codes']
        report['negative_control'] = negative
        (folder / 'result.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
        summary = public_summary(report); results.append(summary); print(json.dumps(summary), flush=True)
    summary = {'kind': 'real_neovim_source_exclusions', 'negative_control': negative,
        'expected_failure': 'EXCLUDED_BUFFER_ATTACHED' if negative else None,
        'scenarios': results, 'passed': sum(r['ok'] for r in results),
        'total': len(results), 'ok': all(r['ok'] for r in results)}
    (output / 'result.json').write_text(json.dumps({'identity': identity, 'summary': summary}, indent=2) + '\n')
    if not summary['ok']: raise ValueError('source selection acceptance failed')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('nvim', 'native', 'output'): parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--adapter', type=Path); parser.add_argument('--scenario', action='append', choices=SCENARIOS)
    parser.add_argument('--expect-legacy-exclusion-failure', action='store_true',
        help='accept only pinned legacy excluded-file attachment plus wire evidence and safe cleanup')
    try: run(parser.parse_args())
    except (OSError, ValueError, KeyError, TypeError, AttributeError, IndexError, subprocess.SubprocessError):
        parser.exit(1, 'NEOVIM SOURCE SELECTION FAILED: inspect local evidence\n')


if __name__ == '__main__': main()
