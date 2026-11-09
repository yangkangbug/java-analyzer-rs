#!/usr/bin/env python3
"""Transparent local test proxy for already-computed LSP responses.

Never substitutes server results. This measures client stale-result safety,
not interruption of server computation. No network or external project code.
"""
import argparse
import json
import os
from pathlib import Path
import select
import subprocess
import sys
import threading
import time


def frame(stream):
    header = bytearray()
    while not header.endswith(b'\r\n\r\n'):
        char = stream.read(1)
        if not char:
            if header:
                raise ValueError('truncated protocol header')
            return None
        header.extend(char)
        if len(header) > 8192:
            raise ValueError('protocol header cap exceeded')
    lengths = [line.split(b':', 1)[1].strip() for line in bytes(header).split(b'\r\n')
               if line.lower().startswith(b'content-length:')]
    if len(lengths) != 1 or not lengths[0].isdigit():
        raise ValueError('invalid Content-Length')
    size = int(lengths[0])
    if size > 8388608:
        raise ValueError('protocol body cap exceeded')
    body = stream.read(size)
    if len(body) != size:
        raise ValueError('truncated protocol body')
    return bytes(header) + body, json.loads(body)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--native', required=True)
    parser.add_argument('--control', type=Path, required=True)
    args, remaining = parser.parse_known_args()
    native = Path(args.native).resolve(strict=True)
    child = subprocess.Popen([str(native), *remaining], stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             env={'PATH': '/nonexistent'})
    pid = os.getpid()
    lock = threading.Lock()
    held_ids = set()
    errors = []
    stop = threading.Event()
    logged_bytes = 0
    def log(event):
        nonlocal logged_bytes
        payload = (json.dumps({'proxy_pid': pid, **event}, ensure_ascii=False) + '\n').encode()
        with lock, (args.control / 'protocol.jsonl').open('ab', buffering=0) as out:
            logged_bytes += len(payload)
            if logged_bytes > 4194304:
                raise ValueError('test transcript cap exceeded')
            out.write(payload)
    log({'event': 'spawn', 'native_pid': child.pid, 'native': str(native),
         'environment': {'PATH': '/nonexistent'}})

    def incoming():
        try:
            data = bytearray()
            while not stop.is_set():
                if not select.select([sys.stdin.fileno()], [], [], 0.05)[0]:
                    continue
                block = os.read(sys.stdin.fileno(), 65536)
                if not block:
                    if data:
                        raise ValueError('truncated client protocol frame')
                    break
                data.extend(block)
                while True:
                    end = data.find(b'\r\n\r\n')
                    if end < 0:
                        if len(data) > 8192:
                            raise ValueError('client protocol header cap exceeded')
                        break
                    lengths = [line.split(b':', 1)[1].strip() for line in data[:end].split(b'\r\n')
                               if line.lower().startswith(b'content-length:')]
                    if len(lengths) != 1 or not lengths[0].isdigit():
                        raise ValueError('invalid client Content-Length')
                    size = int(lengths[0])
                    if end > 8192 or size > 8388608:
                        raise ValueError('client protocol frame cap exceeded')
                    total = end + 4 + size
                    if len(data) < total:
                        break
                    raw = bytes(data[:total])
                    message = json.loads(data[end + 4:total])
                    del data[:total]
                    log({'direction': 'client_to_server', 'message': message})
                    if message.get('method') == 'textDocument/definition':
                        try:
                            (args.control / 'hold-next').unlink()
                        except FileNotFoundError:
                            pass
                        else:
                            with lock:
                                held_ids.add(message['id'])
                    child.stdin.write(raw)
                    child.stdin.flush()
        except (OSError, ValueError) as error:
            errors.append(str(error))
        finally:
            child.stdin.close()

    def stderr():
        total = 0
        overflow = False
        while block := os.read(child.stderr.fileno(), 4096):
            total += len(block)
            if total <= 65536:
                os.write(sys.stderr.fileno(), block)
            elif not overflow:
                overflow = True
                errors.append('native stderr cap exceeded')
                child.terminate()

    reader = threading.Thread(target=incoming)
    drain = threading.Thread(target=stderr, daemon=True)
    reader.start()
    drain.start()
    try:
        while pair := frame(child.stdout):
            raw, message = pair
            log({'direction': 'server_to_client', 'message': message})
            with lock:
                hold = message.get('id') in held_ids
                held_ids.discard(message.get('id'))
            if hold:
                release = args.control / f'release-{pid}'
                release.unlink(missing_ok=True)
                stage = args.control / f'.held-{pid}.tmp'
                stage.write_text(json.dumps({'pid': pid, 'message': message}), encoding='utf-8')
                stage.replace(args.control / f'held-{pid}.json')
                deadline = time.monotonic() + 20
                while not release.exists():
                    if time.monotonic() >= deadline:
                        raise ValueError('test response release timed out')
                    time.sleep(0.005)
                release.unlink()
                log({'event': 'released', 'request_id': message.get('id')})
            sys.stdout.buffer.write(raw)
            sys.stdout.buffer.flush()
        code = child.wait(timeout=10)
        stop.set()
        reader.join(timeout=1)
        drain.join(timeout=1)
        if reader.is_alive() or drain.is_alive():
            raise ValueError('proxy I/O threads did not finish')
        log({'event': 'native_exit', 'code': code})
        if code != 0 or errors:
            raise ValueError(f'native exit {code}; proxy errors: {errors}')
    finally:
        stop.set()
        if child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=3)
        reader.join(timeout=1)
        drain.join(timeout=1)


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        print(f'TEST PROXY FAILED: {error}', file=sys.stderr)
        sys.exit(1)
