#!/usr/bin/env python3
"""Run real Neovim headless acceptance against an already-built native LSP.

Requires the separately pinned dev-only Neovim input. No download, build,
Java, plugin, system install, or user-machine access occurs here.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import threading
import time


ROOT = Path(__file__).resolve().parent.parent


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def inventory(root):
    return {str(p.relative_to(root)): digest(p) for p in sorted(root.rglob('*')) if p.is_file()}


def run_bounded(command, *, env, cwd, timeout, max_output=1048576):
    """Bound output and wall time; reap the direct child and stop its owned group.

    This is a trusted test-process cleanup boundary, not a general OS sandbox.
    Output cap is shared by stdout/stderr; drainers never accumulate excess data.
    WNOWAIT reserves the leader's PID/PGID until the final group signal. Never
    poll/reap first and then signal a potentially reused numeric process group.
    """
    if os.name != 'posix' or not hasattr(os, 'waitid') or not hasattr(os, 'WNOWAIT'):
        raise ValueError('headless acceptance requires POSIX waitid(WNOWAIT) cleanup')
    process = subprocess.Popen(command, env=env, cwd=cwd, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, start_new_session=True)
    chunks = [bytearray(), bytearray()]
    lock = threading.Lock()
    problem = threading.Event()
    errors = []
    remaining = max_output

    def drain(index, stream):
        nonlocal remaining
        try:
            while block := os.read(stream.fileno(), 65536):
                with lock:
                    keep = min(len(block), remaining)
                    chunks[index].extend(block[:keep])
                    remaining -= keep
                    if keep < len(block) and not problem.is_set():
                        errors.append('output cap exceeded')
                        problem.set()
        except OSError as error:
            with lock:
                if not problem.is_set():
                    errors.append('output read failed: ' + str(error))
                    problem.set()

    def kill_group():
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass

    threads = [threading.Thread(target=drain, args=(0, process.stdout)),
               threading.Thread(target=drain, args=(1, process.stderr))]
    started = []
    try:
        for thread in threads:
            thread.start()
            started.append(thread)
        deadline = time.monotonic() + timeout
        while os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is None \
                and not problem.wait(0.01):
            if time.monotonic() >= deadline:
                errors.append('timeout exceeded')
                problem.set()
    finally:
        # Also covers every exception after Popen. The leader is still running
        # or retained as a zombie here, so its process-group ID cannot be reused.
        # Killing an already exited leader does not change its exit status.
        kill_group()
        process.wait(timeout=5)
        for thread in started:
            thread.join(timeout=5)
        if any(thread.is_alive() for thread in started):
            errors.append('output drainer failed to finish after group cleanup')
        else:
            process.stdout.close()
            process.stderr.close()
    return {'returncode': process.returncode, 'stdout': bytes(chunks[0]), 'stderr': bytes(chunks[1]),
            'failure': '; '.join(errors) if errors else None, 'reaped': process.returncode is not None,
            'drainers_finished': not any(t.is_alive() for t in threads)}


def run(args):
    nvim, native = args.nvim.resolve(strict=True), args.native.resolve(strict=True)
    if args.output.is_symlink():
        raise ValueError('output may not be a symlink')
    output = args.output.parent.resolve() / args.output.name
    output.mkdir(parents=True, exist_ok=False)
    home = output / 'home'
    home.mkdir()
    env = {'PATH': '/nonexistent', 'HOME': str(home), 'XDG_CONFIG_HOME': str(home / 'config'),
           'XDG_DATA_HOME': str(home / 'data'), 'XDG_STATE_HOME': str(home / 'state'),
           'XDG_CACHE_HOME': str(home / 'cache'), 'NVIM_LOG_FILE': str(output / 'nvim.log'),
           'LANG': 'C.UTF-8'}
    version = run_bounded([str(nvim), '--version'], env=env, cwd=output, timeout=10)
    if (version['failure'] or version['returncode'] != 0
            or version['stdout'].splitlines()[:1] != [b'NVIM v0.11.5']):
        raise ValueError('this adapter acceptance is pinned to Neovim 0.11.5')
    provenance = json.loads((nvim.parent.parent / 'INPUT.json').read_text())
    if provenance['binary_sha256'] != digest(nvim):
        raise ValueError('Neovim input binary changed after verified setup')
    if provenance['manifest'] != json.loads((ROOT / 'scripts/neovim-inputs.json').read_text()):
        raise ValueError('Neovim input manifest differs from the fixed manifest')
    fixture = output / 'fixture 中 # % space'
    fixture.mkdir()
    app = fixture / 'app 中 # %/app/App.java'
    target = fixture / 'lib 中 # %/lib/Library.java'
    app_text = ('package app;\npublic class App extends lib.Library {\n'
                '  void run() { /* 中😀 */ valueA++; }\n}')
    target_text = ('package lib;\npublic class Library {\n'
                   '  /* 😀中 */ public int valueA;\n}')
    binary_text = 'package app;\nclass App { lib.Lib field; }'
    for path, data in [(app, app_text), (target, target_text)]:
        path.parent.mkdir(parents=True)
        path.write_bytes((data + '\n').replace('\n', '\r\n').encode())
    binary_class = fixture / 'classes/lib/Lib.class'
    binary_class.parent.mkdir(parents=True)
    binary_class.write_bytes((ROOT / 'tests/fixtures/classes/Lib.class').read_bytes())
    manifest = fixture / 'project.json'
    manifest.write_text(json.dumps({'schema_version': 1, 'compilations': [
        {'id': {'project': 'app', 'source_set': 'main'},
         'source_roots': [str(app.parent.parent.relative_to(fixture))],
         'dependencies': [{'project': 'lib', 'source_set': 'main'}],
         'classpath': [{'kind': 'directory', 'path': 'classes'}]},
        {'id': {'project': 'lib', 'source_set': 'main'},
         'source_roots': [str(target.parent.parent.relative_to(fixture))]}]}, indent=2), encoding='utf-8')
    control = output / 'control'
    control.mkdir()
    proxy = output / 'proxy'
    proxy_script = ROOT / 'editors/neovim/tests/delay_proxy.py'
    proxy.write_text(f'#!{sys.executable}\nimport runpy, sys\n'
                     f'sys.argv = {[str(proxy_script), "--native", str(native), "--control", str(control)]!r} + sys.argv[1:]\n'
                     f'runpy.run_path({str(proxy_script)!r}, run_name="__main__")\n', encoding='utf-8')
    proxy.chmod(0o755)
    adapter = args.adapter.resolve(strict=True) if args.adapter else ROOT / 'editors/neovim'
    config = {'adapter': str(adapter), 'app': str(app), 'target': str(target),
              'app_text': app_text, 'target_text': target_text, 'binary_text': binary_text,
              'manifest': str(manifest), 'control': str(control), 'proxy': str(proxy),
              'result': str(output / 'headless-result.json')}
    config_path = output / 'config.json'
    config_path.write_text(json.dumps(config), encoding='utf-8')
    before = inventory(fixture)
    identity = {'native_sha256': digest(native), 'neovim_sha256': digest(nvim),
                'client_input': provenance, 'fixture_before': before,
                'script_sha256': {str(p.relative_to(ROOT)): digest(p) for p in (
                    Path(__file__).resolve(),
                    ROOT / 'editors/neovim/tests/headless.lua', proxy_script)}}
    identity['adapter_sha256'] = digest(adapter / 'lua/java_analyzer.lua')
    identity['adapter_override'] = bool(args.adapter)
    git = subprocess.run(['git', '-C', str(ROOT), 'rev-parse', 'HEAD', 'HEAD^{tree}'],
                         capture_output=True, text=True, check=True)
    identity['source_revision'], identity['source_tree'] = git.stdout.splitlines()
    identity['source_dirty'] = bool(subprocess.run(
        ['git', '-C', str(ROOT), 'status', '--porcelain'], capture_output=True, check=True).stdout)
    capture = run_bounded([str(nvim), '--headless', '-u', 'NONE', '-i', 'NONE', '-n', '-l',
                          str(ROOT / 'editors/neovim/tests/headless.lua'), str(config_path)],
                         env=env, cwd=output, timeout=150)
    (output / 'stdout.log').write_bytes(capture['stdout'])
    (output / 'stderr.log').write_bytes(capture['stderr'])
    result_path = output / 'headless-result.json'
    result = json.loads(result_path.read_text()) if result_path.exists() else {'failed': 'no editor report'}
    after = inventory(fixture)
    report = {'identity': identity, 'editor': result,
              'process': {k: v for k, v in capture.items() if k not in ('stdout', 'stderr')},
              'returncode': capture['returncode'],
              'unchanged_inputs': before == after, 'fixture_after': after,
              'unchanged_executables': identity['native_sha256'] == digest(native)
                 and identity['neovim_sha256'] == digest(nvim),
              'limits': ['Headless editor only; no GUI or user-device validation.',
                         'Delayed responses test client filtering, not server interruption.',
                         'Source revision is recorded; build provenance comes from the CI build step.']}
    transcript = control / 'protocol.jsonl'
    events = [json.loads(line) for line in transcript.read_text().splitlines()] if transcript.exists() else []
    (output / 'protocol.jsonl').write_text('\n'.join(json.dumps(e, ensure_ascii=False) for e in events) + '\n')
    spawns = [e for e in events if e.get('event') == 'spawn']
    exits = {e['proxy_pid'] for e in events if e.get('event') == 'native_exit' and e['code'] == 0}
    report['native_sessions_cleanly_exited'] = bool(spawns) and all(e['proxy_pid'] in exits for e in spawns)
    report['native_no_java_environment'] = bool(spawns) and all(
        e['environment'] == {'PATH': '/nonexistent'} for e in spawns)
    (output / 'result.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
    if (capture['returncode'] != 0 or capture['failure'] or not capture['drainers_finished']
            or result.get('failed') or not report['unchanged_inputs']
            or not report['unchanged_executables'] or not report['native_sessions_cleanly_exited']
            or not report['native_no_java_environment']):
        raise ValueError(f'editor acceptance failed; inspect {output / "result.json"} and stderr.log')
    print(f'PASS: {len(result["passed"])} real Neovim headless checks; {output / "result.json"}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--nvim', type=Path, required=True)
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--adapter', type=Path, help='explicit alternate adapter root for negative regression checks')
    args = parser.parse_args()
    try:
        run(args)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(1, f'NEOVIM ACCEPTANCE FAILED: {error}\n')


if __name__ == '__main__':
    main()
