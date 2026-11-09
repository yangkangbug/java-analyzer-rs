#!/usr/bin/env python3
"""Hold real native replies to exercise editor lifecycle boundaries, never results.

All faults are test transport faults: delayed initialize/definition/shutdown
replies or explicit termination of our child. The actual native result and exit
are recorded. No network, Java, or arbitrary command is accepted.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import threading
import time

SPEC = importlib.util.spec_from_file_location('delay', Path(__file__).with_name('delay_proxy.py'))
DELAY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DELAY)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument('--control', type=Path, required=True)
    args, remaining = parser.parse_known_args()
    native = args.native.resolve(strict=True)
    pid = os.getpid()
    lock, stopped = threading.Lock(), threading.Event()
    methods, holds, errors = {}, {}, []
    logged = 0

    def log(event):
        nonlocal logged
        raw = (json.dumps({'proxy_pid': pid, **event}, ensure_ascii=False) + '\n').encode()
        with lock:
            logged += len(raw)
            if logged > 4194304:
                raise ValueError('test transcript cap exceeded')
            fd = os.open(args.control / 'protocol.jsonl', os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
            try:
                if os.write(fd, raw) != len(raw):
                    raise ValueError('short transcript write')
            finally:
                os.close(fd)

    child = subprocess.Popen([str(native), *remaining], stdin=subprocess.PIPE,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, env={'PATH': '/nonexistent'})
    log({'event': 'spawn', 'native_pid': child.pid, 'native': str(native),
         'environment': {'PATH': '/nonexistent'}})

    def incoming():
        data = bytearray()
        batch = None
        try:
            while not stopped.is_set():
                crash = args.control / f'terminate-native-{pid}'
                if crash.exists():
                    crash.unlink()
                    log({'event': 'test_terminate_native'})
                    child.terminate()
                if not select.select([sys.stdin.fileno()], [], [], 0.02)[0]:
                    continue
                block = os.read(sys.stdin.fileno(), 65536)
                if not block:
                    if data:
                        raise ValueError('truncated client frame')
                    break
                data.extend(block)
                while True:
                    end = data.find(b'\r\n\r\n')
                    if end < 0:
                        if len(data) > 8192:
                            raise ValueError('client header cap exceeded')
                        break
                    lengths = [s.split(b':', 1)[1].strip() for s in data[:end].split(b'\r\n')
                               if s.lower().startswith(b'content-length:')]
                    if len(lengths) != 1 or not lengths[0].isdigit():
                        raise ValueError('invalid client Content-Length')
                    size = int(lengths[0])
                    if end > 8192 or size > 8388608:
                        raise ValueError('client frame cap exceeded')
                    total = end + 4 + size
                    if len(data) < total:
                        break
                    raw, message = bytes(data[:total]), json.loads(data[end + 4:total])
                    del data[:total]
                    log({'direction': 'client_to_server', 'message': message})
                    method = message.get('method')
                    if 'id' in message and method:
                        methods[message['id']] = method
                        kind = {'initialize': 'initialize', 'shutdown': 'shutdown',
                                'textDocument/definition': 'definition'}.get(method)
                        if kind:
                            try:
                                (args.control / ('hold-next-' + kind)).unlink()
                            except FileNotFoundError:
                                pass
                            else:
                                holds[message['id']] = kind
                    if method == 'textDocument/definition' and (args.control / 'batch-cancel-next').exists():
                        (args.control / 'batch-cancel-next').unlink()
                        batch = (message['id'], raw)
                        continue
                    if batch is not None:
                        if method != '$/cancelRequest' or message.get('params', {}).get('id') != batch[0]:
                            raise ValueError('cancellation batch expected matching cancel')
                        log({'event': 'test_batch_cancel', 'request_id': batch[0]})
                        raw = batch[1] + raw
                        batch = None
                    child.stdin.write(raw)
                    child.stdin.flush()
        except (OSError, ValueError) as error:
            if not stopped.is_set():
                errors.append(str(error))
        finally:
            try:
                child.stdin.close()
            except OSError:
                pass

    def stderr():
        total = 0
        while block := os.read(child.stderr.fileno(), 4096):
            total += len(block)
            if total > 65536:
                errors.append('native stderr cap exceeded')
                child.terminate()
                break
            os.write(sys.stderr.fileno(), block)

    def terminated(signum, _frame):
        raise InterruptedError(f'proxy received signal {signum}')

    signal.signal(signal.SIGTERM, terminated)
    reader, drain = threading.Thread(target=incoming), threading.Thread(target=stderr)
    reader.start()
    drain.start()
    code, fault = None, None
    try:
        while pair := DELAY.frame(child.stdout):
            raw, message = pair
            method = methods.get(message.get('id'))
            log({'direction': 'server_to_client', 'method': method, 'message': message})
            kind = holds.pop(message.get('id'), None)
            if kind:
                held = {'pid': pid, 'kind': kind, 'message': message}
                stage = args.control / f'.held-{pid}-{kind}.tmp'
                stage.write_text(json.dumps(held))
                stage.replace(args.control / f'held-{pid}-{kind}.json')
                log({'event': 'held', 'kind': kind, 'request_id': message['id']})
                deadline, release = time.monotonic() + 12, args.control / f'release-{pid}-{kind}'
                while not release.exists():
                    if time.monotonic() > deadline:
                        raise ValueError('test response release timed out')
                    time.sleep(0.005)
                release.unlink()
                log({'event': 'released', 'kind': kind, 'request_id': message['id']})
            sys.stdout.buffer.write(raw)
            sys.stdout.buffer.flush()
            log({'event': 'delivered', 'method': method, 'request_id': message.get('id')})
        code = child.wait(timeout=3)
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        fault = str(error)
    finally:
        stopped.set()
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        if child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=2)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=2)
        code = child.returncode
        log({'event': 'native_exit', 'code': code, 'fault': fault})
        reader.join(timeout=1)
        drain.join(timeout=1)
        if reader.is_alive() or drain.is_alive():
            errors.append('proxy threads did not finish')
        log({'event': 'proxy_exit', 'code': code, 'fault': fault, 'errors': errors})
    if errors:
        raise ValueError('; '.join(errors))
    return code if code is not None and code >= 0 and not fault else 1


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f'REFRESH PROXY FAILED: {error}', file=sys.stderr)
        sys.exit(1)
