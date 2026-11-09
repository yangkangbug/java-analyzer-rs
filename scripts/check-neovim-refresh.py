#!/usr/bin/env python3
"""Real pinned Neovim explicit-refresh acceptance, with immutable evidence.

Uses the existing trusted bounded process runner and separately verified input.
No download, build, JVM, user configuration, or user-machine access occurs here.
Fixture disk mutations are deliberate harness actions, never product saves.
"""
import argparse
import importlib.util
import json
import hashlib
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location('editor', ROOT / 'scripts/check-neovim-editor.py')
EDITOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EDITOR)
SCENARIOS = ('add', 'delete', 'rename', 'configuration', 'owner', 'edge', 'add_new_buffer',
    'initialize_refusal', 'readiness_reentrant', 'held_restarts', 'observer_stability',
    'initializing_edits', 'initializing_stop', 'initializing_restart', 'initialize_timeout',
    'shutdown_timeout', 'abnormal_exit', 'reentrant_restart', 'reentrant_stop',
    'attach_load_edit', 'attach_restart', 'attach_stop', 'request_restart', 'request_stop', 'explicit_cancel',
    'exit_restart', 'exit_stop')
FAULT_SCENARIOS = ('initialize_timeout', 'shutdown_timeout', 'abnormal_exit')


def _validate(report):
    """Fail closed on independently observed process, protocol and editor facts."""
    failures = []
    process, editor = report.get('process', {}), report.get('editor', {})
    if (process.get('returncode') != 0 or process.get('failure')
            or process.get('reaped') is not True or process.get('drainers_finished') is not True
            or process.get('group_cleanup') not in ('signalled', 'darwin-confirmed-exited-only')):
        failures.append('PROCESS_CLEANUP')
    if editor.get('ok') is not True or editor.get('observed', {}).get('completed') is not True:
        failures.append('EDITOR_ASSERTION')
    if (not isinstance(report.get('fixture_before'), dict) or not report['fixture_before']
            or report.get('fixture_before') != report.get('fixture_after')):
        failures.append('FIXTURE_RESTORATION')
    if report.get('intentional_disk_verified') is not True:
        failures.append('UNINTENTIONAL_DISK_CHANGE')
    if report.get('unchanged_executables') is not True:
        failures.append('EXECUTABLE_CHANGED')
    events = report.get('events', [])
    live, spawned, exited, peak = set(), set(), set(), 0
    methods, opened, flushed, states, proxy_exits = {}, {}, {}, {}, set()
    initial_documents, initial_flushed, native_exits = {}, set(), {}
    for event in events:
        pid, kind = event.get('proxy_pid'), event.get('event')
        if kind == 'spawn':
            if pid in spawned or event.get('environment') != {'PATH': '/nonexistent'}:
                failures.append('NATIVE_ENVIRONMENT_OR_IDENTITY')
            spawned.add(pid)
            live.add(pid)
            peak = max(peak, len(live))
            opened[pid], flushed[pid], states[pid] = set(), False, 'new'
            initial_documents[pid] = {}
        elif kind == 'native_exit':
            if pid not in live or pid in exited:
                failures.append('NATIVE_EXIT_IDENTITY')
            live.discard(pid)
            exited.add(pid)
            native_exits[pid] = event
            allowed = (0, 1) if states.get(pid) == 'refused' else (0,)
            code = event.get('code')
            if report.get('scenario') in FAULT_SCENARIOS:
                if type(code) is not int or code not in (-15, -9, 1):
                    failures.append('NATIVE_EXIT_STATUS')
            elif code not in allowed or type(code) is not int or event.get('fault') is not None:
                failures.append('NATIVE_EXIT_STATUS')
        elif kind == 'proxy_exit':
            if pid not in exited or pid in proxy_exits or event.get('errors') != []:
                failures.append('PROXY_CLEANUP')
            native_exit = native_exits.get(pid, {})
            if (event.get('code') != native_exit.get('code') or type(event.get('code')) is not int
                    or event.get('fault') != native_exit.get('fault')):
                failures.append('PROXY_CLEANUP')
            proxy_exits.add(pid)
        if (event.get('direction') or kind in ('held', 'released', 'delivered')) and pid not in live:
            failures.append('PROTOCOL_SESSION_IDENTITY')
        if event.get('direction') == 'server_to_client' and event.get('method') == 'initialize':
            message = event.get('message', {})
            if message.get('error', {}).get('code') == -32803:
                states[pid] = 'refused'
            elif isinstance(message.get('result'), dict):
                states[pid] = 'initialize_replied'
        if event.get('direction') == 'client_to_server':
            message = event.get('message', {})
            method = message.get('method')
            if 'id' in message:
                methods[(pid, message['id'])] = method
            if method == 'initialized':
                if states.get(pid) != 'initialize_replied':
                    failures.append('INITIALIZATION_ORDER')
                states[pid] = 'initialized'
            if method == 'textDocument/didOpen':
                if states.get(pid) != 'initialized':
                    failures.append('INITIALIZATION_ORDER')
                opened.setdefault(pid, set()).add(message['params']['textDocument']['uri'])
                flushed[pid] = False
                if pid not in initial_flushed:
                    doc = message['params']['textDocument']
                    if doc['uri'] in initial_documents[pid]: failures.append('REPLAY_DUPLICATE_OPEN')
                    initial_documents[pid][doc['uri']] = doc['text']
            elif method == 'textDocument/didChange':
                flushed[pid] = False
                if pid not in initial_flushed:
                    doc = message['params']
                    changes = doc['contentChanges']
                    if len(changes) != 1 or 'range' in changes[0]: failures.append('REPLAY_NOT_FULL_TEXT')
                    else: initial_documents[pid][doc['textDocument']['uri']] = changes[0]['text']
            elif method == '$/setTrace':
                initial_flushed.add(pid)
                flushed[pid] = True
            elif method == 'textDocument/definition' and not flushed.get(pid):
                failures.append('DEFINITION_BEFORE_REPLAY_FLUSH')
    if proxy_exits != spawned:
        failures.append('PROXY_CLEANUP')
    if not spawned or live or spawned != exited:
        failures.append('NATIVE_EXIT_MISSING')
    if peak != 1:
        failures.append('PROCESS_OVERLAP')
    expected = editor.get('observed', {}).get('expected_spawns')
    if type(expected) is not int or len(spawned) != expected:
        failures.append('UNEXPECTED_SESSION_COUNT')
    if type(editor.get('observed', {}).get('peak_clients')) is not int or editor['observed']['peak_clients'] != 1:
        failures.append('CLIENT_OVERLAP')
    replays = editor.get('observed', {}).get('replays')
    scenario = report.get('scenario')
    expected_ordinals = ({'initializing_stop': [], 'initialize_timeout': [],
        'initializing_restart': [2], 'initialize_refusal': [1, 3],
        'attach_restart': [1, 3], 'attach_stop': [1]}.get(scenario,
        list(range(1, expected + 1)) if type(expected) is int else []))
    if not isinstance(replays, list) or [r.get('ordinal') for r in replays] != expected_ordinals:
        failures.append('REPLAY_EVIDENCE_MISSING')
    for expected_open in replays or []:
        order = [e['proxy_pid'] for e in events if e.get('event') == 'spawn']
        ordinal = expected_open['ordinal']
        if type(ordinal) is not int or ordinal < 1 or ordinal > len(order):
            failures.append('REPLAY_SESSION_MISSING')
            continue
        pid, wanted = order[ordinal - 1], expected_open.get('documents')
        if not isinstance(wanted, dict) or not wanted:
            failures.append('REPLAY_EVIDENCE_MISSING')
            continue
        actual = initial_documents.get(pid, {})
        if set(actual) != set(wanted): failures.append('REPLAY_DOCUMENT_SET')
        if any(actual[uri] != wanted[uri] for uri in set(actual) & set(wanted)):
            failures.append('REPLAY_TEXT')
        if pid not in initial_flushed: failures.append('REPLAY_FLUSH_MISSING')
    observed = editor.get('observed', {})
    if scenario in ('held_restarts', 'reentrant_restart', 'reentrant_stop'):
        settled = observed.get('settled_before_release', {})
        if (settled != {'status': 'stale', 'count': 1} or type(settled.get('count')) is not int
                or observed.get('callback_final_count') != 1 or type(observed.get('callback_final_count')) is not int):
            failures.append('CALLBACK_SETTLEMENT')
        held = [e for e in events if e.get('event') == 'held' and e.get('kind') == 'definition']
        if len(held) != 1:
            failures.append('HELD_RESULT_EVIDENCE')
        else:
            h = held[0]
            matching = [e for e in events if e.get('proxy_pid') == h['proxy_pid']
                and e.get('direction') == 'server_to_client' and e.get('message', {}).get('id') == h['request_id']]
            if len(matching) != 1 or not isinstance(matching[0]['message'].get('result'), dict):
                failures.append('HELD_RESULT_EVIDENCE')
            elif matching[0]['message']['result'] != observed.get('held_result'):
                failures.append('HELD_RESULT_EVIDENCE')
            positions = [events.index(matching[0]) if len(matching) == 1 else -1, events.index(h)]
            for kind in ('released', 'delivered'):
                found = [i for i, e in enumerate(events) if e.get('event') == kind
                    and e.get('proxy_pid') == h['proxy_pid'] and e.get('request_id') == h['request_id']]
                if len(found) != 1: failures.append('HELD_RESULT_DELIVERY')
                positions.extend(found)
            if len(positions) != 4 or positions != sorted(set(positions)):
                failures.append('HELD_RESULT_DELIVERY')
    terminal = observed.get('terminal_status', {})
    expected_phase = 'blocked' if scenario in FAULT_SCENARIOS else 'idle'
    if (observed.get('terminal_phase') != expected_phase or terminal.get('phase') != expected_phase
            or terminal.get('active') is not False or terminal.get('ready') is not False
            or terminal.get('pending_start') is not False
            or (scenario not in FAULT_SCENARIOS and terminal.get('client_id') is not None)
            or terminal.get('restart_blocked') is not (scenario in FAULT_SCENARIOS)
            or terminal.get('cleanup') != ('uncertain' if scenario in FAULT_SCENARIOS else 'safe')):
        failures.append('TERMINAL_STATE')
    if scenario == 'initialize_refusal':
        refused = observed.get('refused_status', {})
        if (refused.get('phase') != 'failed' or refused.get('cleanup') != 'safe'
                or refused.get('active') is not False or refused.get('ready') is not False
                or refused.get('pending_start') is not False or refused.get('restart_blocked') is not False
                or refused.get('client_id') is not None or not isinstance(refused.get('error'), str)):
            failures.append('REFUSAL_STATE')
    navigation, results = observed.get('navigation', []), observed.get('results', [])
    if len(navigation) != len(results) or (not navigation and scenario not in ('initialize_timeout', 'initializing_stop')):
        failures.append('NAVIGATION_EVIDENCE')
    for nav, result in zip(navigation, results):
        if (result.get('status') != nav.get('expected_status') or nav.get('actual_buf') != nav.get('expected_buf')
                or nav.get('actual_cursor') != nav.get('expected_cursor')):
            failures.append('NAVIGATION_TARGET')
        if nav.get('expected_status') == 'jumped':
            line, byte_col = nav['expected_cursor']
            utf16 = len(nav['target_line'].encode()[:byte_col].decode().encode('utf-16-le')) // 2
            location = result.get('result', {})
            expected_range = {'start': {'line': line - 1, 'character': utf16},
                'end': {'line': line - 1, 'character': utf16 + len(nav['word'].encode('utf-16-le')) // 2}}
            if location.get('uri') != nav['target_uri'] or location.get('range') != expected_range:
                failures.append('NAVIGATION_RANGE')
        elif nav.get('expected_status') == 'empty' and result.get('result') not in (None, []):
            failures.append('NAVIGATION_EMPTY')
        elif nav.get('expected_status') == 'error' and not isinstance(result.get('error'), dict):
            failures.append('NAVIGATION_ERROR')
    if scenario == 'observer_stability':
        observers = observed.get('observers', [])
        if len(observers) != 6 or any(type(n) is not int or n != observers[0] for n in observers):
            failures.append('OBSERVER_ACCUMULATION')
        attachments = observed.get('observer_attachments', [])
        if (len(attachments) != 6 or not attachments[0]
                or any(a != attachments[0] for a in attachments)
                or any(value != 1 or type(value) is not int for value in attachments[0].values())
                or observed.get('observer_mutation_callbacks') != 1
                or type(observed.get('observer_mutation_callbacks')) is not int):
            failures.append('OBSERVER_ACCUMULATION')
    if scenario == 'readiness_reentrant':
        if (observed.get('attach_count') != 4 or observed.get('attach_ready') != [False] * 4
                or observed.get('attach_request') != [False] * 4):
            failures.append('PARTIAL_REPLAY_READINESS')
    if scenario == 'explicit_cancel':
        callback = observed.get('cancel_callback', {})
        if callback != {'status': 'cancelled', 'count': 1} or type(callback.get('count')) is not int:
            failures.append('CALLBACK_SETTLEMENT')
        if not any(e.get('direction') == 'server_to_client' and e.get('method') == 'textDocument/definition'
                and e.get('message', {}).get('error', {}).get('code') == -32800 for e in events):
            failures.append('NATIVE_CANCELLATION_MISSING')
    if scenario in ('request_restart', 'request_stop'):
        callback = observed.get('request_callback', {})
        if callback != {'status': 'stale', 'count': 1} or type(callback.get('count')) is not int:
            failures.append('CALLBACK_SETTLEMENT')
    return sorted(set(failures))


def validate(report):
    try:
        return _validate(report)
    except (AttributeError, KeyError, TypeError, ValueError, IndexError):
        return ['EVIDENCE_SCHEMA']


def fixture(output):
    base = output / 'fixture 中 # %'
    base.mkdir()
    app_text = ('package app;\npublic class App extends lib.Library {\n'
                '  void run() { /* 中😀 */ valueA++; deepA++; }\n}')
    target_text = 'package lib;\npublic class Library extends Base {\n  /* 😀中 */ public int valueA;\n}'
    extra_text = 'package lib;\npublic class Base {\n  public int deepA;\n}'
    paths = {'app': base / 'app/app/App.java', 'target': base / 'lib/lib/Library.java',
             'extra': base / 'lib/lib/Base.java', 'inventory': base / 'lib/lib/Inventory.java',
             'second': base / 'other/lib/Library.java'}
    for key, path in paths.items():
        data = {'app': app_text, 'target': target_text, 'extra': extra_text,
                'inventory': 'package lib; class Inventory {}',
                'second': target_text.replace(' extends Base', '')}[key]
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes((data + '\n').replace('\n', '\r\n').encode())
    binary = base / 'classes/lib/Lib.class'
    binary.parent.mkdir(parents=True)
    binary.write_bytes((ROOT / 'tests/fixtures/classes/Lib.class').read_bytes())
    initial = {'schema_version': 1, 'compilations': [
        {'id': {'project': 'app', 'source_set': 'main'}, 'source_roots': ['app'],
         'dependencies': [{'project': 'lib', 'source_set': 'main'}],
         'classpath': [{'kind': 'directory', 'path': 'classes'}]},
        {'id': {'project': 'lib', 'source_set': 'main'}, 'source_roots': ['lib']}]}
    manifest = base / 'project.json'
    manifest.write_text(json.dumps(initial))
    cfg = {key: str(value) for key, value in paths.items()}
    cfg.update({'app_text': app_text, 'target_text': target_text, 'extra_text': extra_text,
        'manifest': str(manifest), 'manifest_initial': initial,
        'added': str(base / 'lib/lib/Added.java'), 'renamed': str(base / 'lib/lib/MovedLibrary.java')})
    return base, cfg


def run(args):
    nvim, native = args.nvim.resolve(strict=True), args.native.resolve(strict=True)
    adapter = args.adapter.resolve(strict=True) if args.adapter else ROOT / 'editors/neovim'
    if args.output.is_symlink():
        raise ValueError('output may not be a symlink')
    output = args.output.parent.resolve() / args.output.name
    output.mkdir(parents=True, exist_ok=False)
    provenance = json.loads((nvim.parent.parent / 'INPUT.json').read_text())
    if provenance['binary_sha256'] != EDITOR.digest(nvim):
        raise ValueError('Neovim binary changed after verified setup')
    if provenance['manifest'] != json.loads((ROOT / 'scripts/neovim-inputs.json').read_text()):
        raise ValueError('Neovim manifest differs from fixed input')
    version = EDITOR.run_bounded([str(nvim), '--version'], env={'PATH': '/nonexistent'}, cwd=output, timeout=10)
    if version['failure'] or version['returncode'] or version['stdout'].splitlines()[:1] != [b'NVIM v0.11.5']:
        raise ValueError('Neovim 0.11.5 required')
    git = subprocess.run(['git', '-C', str(ROOT), 'rev-parse', 'HEAD', 'HEAD^{tree}'],
                         capture_output=True, text=True, check=True).stdout.splitlines()
    scripts = (Path(__file__).resolve(), ROOT / 'scripts/check-neovim-editor.py',
        ROOT / 'editors/neovim/tests/refresh.lua', ROOT / 'editors/neovim/tests/refresh_proxy.py',
        ROOT / 'editors/neovim/tests/delay_proxy.py')
    identity = {'native_sha256': EDITOR.digest(native), 'neovim_sha256': EDITOR.digest(nvim),
        'adapter_sha256': EDITOR.digest(adapter / 'lua/java_analyzer.lua'), 'adapter_override': bool(args.adapter),
        'source_revision': git[0], 'source_tree': git[1], 'client_input': provenance,
        'source_dirty': bool(subprocess.run(['git', '-C', str(ROOT), 'status', '--porcelain'],
            capture_output=True, check=True).stdout),
        'script_sha256': {str(p.relative_to(ROOT)): EDITOR.digest(p) for p in scripts}}
    results = []
    for scenario in args.scenario or SCENARIOS:
        folder = output / scenario
        folder.mkdir()
        base, cfg = fixture(folder)
        before_bytes = {str(p.relative_to(base)): p.read_bytes() for p in base.rglob('*') if p.is_file()}
        before = EDITOR.inventory(base)
        home, control = folder / 'home', folder / 'control'
        home.mkdir(); control.mkdir()
        proxy, proxy_script = folder / 'proxy', ROOT / 'editors/neovim/tests/refresh_proxy.py'
        proxy.write_text(f'#!{sys.executable}\nimport runpy,sys\n'
            f'sys.argv = {[str(proxy_script), "--native", str(native), "--control", str(control)]!r} + sys.argv[1:]\n'
            f'runpy.run_path({str(proxy_script)!r}, run_name="__main__")\n')
        proxy.chmod(0o755)
        cfg.update({'scenario': scenario, 'adapter': str(adapter), 'control': str(control),
            'proxy': str(proxy), 'result': str(folder / 'editor.json'), 'fixture': str(base),
            'original_disk': {str(base / p): data.decode() for p, data in before_bytes.items()
                              if not p.endswith('.class')}})
        config = folder / 'config.json'
        config.write_text(json.dumps(cfg))
        env = {'PATH': '/nonexistent', 'HOME': str(home), 'XDG_CONFIG_HOME': str(home / 'config'),
            'XDG_DATA_HOME': str(home / 'data'), 'XDG_STATE_HOME': str(home / 'state'),
            'XDG_CACHE_HOME': str(home / 'cache'), 'NVIM_LOG_FILE': str(folder / 'nvim.log'), 'LANG': 'C.UTF-8'}
        capture = EDITOR.run_bounded([str(nvim), '--headless', '-u', 'NONE', '-i', 'NONE', '-n', '-l',
            str(ROOT / 'editors/neovim/tests/refresh.lua'), str(config)], env=env, cwd=folder, timeout=55)
        (folder / 'stdout.log').write_bytes(capture.pop('stdout'))
        (folder / 'stderr.log').write_bytes(capture.pop('stderr'))
        editor = json.loads((folder / 'editor.json').read_text()) if (folder / 'editor.json').exists() else {}
        transcript = control / 'protocol.jsonl'
        events = [json.loads(s) for s in transcript.read_text().splitlines()] if transcript.exists() else []
        report = {'scenario': scenario, 'identity': identity, 'editor': editor, 'process': capture,
            'fixture_before': before, 'fixture_after': EDITOR.inventory(base), 'events': events,
            'unchanged_executables': identity['native_sha256'] == EDITOR.digest(native)
                and identity['neovim_sha256'] == EDITOR.digest(nvim),
            'intentional_disk_changes': editor.get('observed', {}).get('disk_actions', []),
            'limitations': ['Headless editor only, no user-machine or GUI acceptance.',
                'Held replies are genuine native results, not proof of server interruption.',
                'Source/native build provenance remains the separate CI build step.']}
        intended = {str(base / p): hashlib.sha256(data).hexdigest() for p, data in before_bytes.items() if not p.endswith('.class')}
        for action in report['intentional_disk_changes']:
            if action['kind'] == 'harness_write': intended[action['path']] = action['sha256']
            elif action['kind'] == 'harness_delete': intended[action['path']] = False
            elif action['kind'] == 'harness_rename':
                intended[action['to']], intended[action['from']] = intended[action['from']], False
        actual_disk = {path: hashlib.sha256(data.encode()).hexdigest() if data is not False else False
            for path, data in editor.get('observed', {}).get('before_restore', {}).items()}
        report['intentional_disk_verified'] = bool(actual_disk) and actual_disk == intended
        report['failure_codes'] = validate(report)
        report['ok'] = not report['failure_codes']
        (folder / 'result.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
        results.append({'scenario': scenario, 'ok': report['ok'], 'failure_codes': report['failure_codes'],
            'session_count': sum(e.get('event') == 'spawn' for e in events),
            'cleanup': capture.get('group_cleanup'), 'terminal_phase': editor.get('observed', {}).get('terminal_phase')})
        print(scenario + ': ' + ('PASS' if report['ok'] else ','.join(report['failure_codes'])), flush=True)
    summary = {'kind': 'real_neovim_explicit_refresh', 'scenarios': results,
        'passed': sum(r['ok'] for r in results), 'total': len(results), 'ok': all(r['ok'] for r in results)}
    (output / 'result.json').write_text(json.dumps({'identity': identity, 'summary': summary}, indent=2) + '\n')
    if not summary['ok']:
        raise ValueError(f'refresh acceptance failed; inspect {output / "result.json"}')
    print(f'PASS: {len(results)} real Neovim refresh scenarios; {output / "result.json"}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('nvim', 'native', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--adapter', type=Path)
    parser.add_argument('--scenario', action='append', choices=SCENARIOS)
    args = parser.parse_args()
    try:
        run(args)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        parser.exit(1, f'NEOVIM REFRESH FAILED: {error}\n')


if __name__ == '__main__':
    main()
