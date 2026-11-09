"""Offline regressions for the bounded headless-editor harness, no editor needed."""
import importlib.util
import errno
import os
from pathlib import Path
import select
import signal
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch
from types import SimpleNamespace


SPEC = importlib.util.spec_from_file_location(
    'neovim_editor', Path(__file__).with_name('check-neovim-editor.py'))
MOD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MOD)


def kqueue_facade(events, *, registration_error=None, kill_before_release=False):
    """Real kqueue on Darwin; Linux WNOWAIT emulates events for plumbing tests.

    The Linux branch is not evidence that Darwin's kernel API has passed.
    """
    real_waitid = getattr(os, 'waitid', None)
    real_kqueue = getattr(select, 'kqueue', None)
    values = {'KQ_FILTER_PROC': -5, 'KQ_EV_ADD': 1, 'KQ_EV_ENABLE': 4,
              'KQ_EV_ONESHOT': 16, 'KQ_EV_ERROR': 16384, 'KQ_NOTE_EXIT': 2147483648}
    values = {key: getattr(select, key, value) for key, value in values.items()}
    facade = SimpleNamespace(**values)
    facade.kevent = getattr(select, 'kevent',
                            lambda ident, **kwargs: SimpleNamespace(ident=ident, data=0, **kwargs))

    class Queue:
        def __init__(self):
            self.actual = real_kqueue() if real_kqueue else None
            self.pid = None

        def control(self, changes, count, timeout):
            if changes:
                self.pid = changes[0].ident
                events.append(('register', self.pid))
                if registration_error:
                    raise OSError(registration_error, 'injected registration failure')
            if self.actual:
                result = self.actual.control(changes, count, timeout)
            elif changes:
                result = []
            else:
                exited = real_waitid(os.P_PID, self.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
                result = [] if exited is None else [SimpleNamespace(
                    ident=self.pid, filter=facade.KQ_FILTER_PROC, flags=0,
                    fflags=facade.KQ_NOTE_EXIT, data=0)]
            if changes and kill_before_release:
                os.kill(self.pid, signal.SIGKILL)
            return result

        def close(self):
            events.append(('queue_close', self.pid))
            if self.actual:
                self.actual.close()

    facade.kqueue = Queue
    return facade


class KqueueSelectionTests(unittest.TestCase):
    def test_darwin_without_waitid_selects_nonreaping_kqueue(self):
        events = []
        queue = SimpleNamespace(control=lambda *args: events.append(args) or [],
                                close=lambda: events.append('closed'))
        fake = SimpleNamespace(kqueue=lambda: queue,
            kevent=lambda ident, **kwargs: SimpleNamespace(ident=ident, **kwargs),
            KQ_FILTER_PROC=-5, KQ_EV_ADD=1, KQ_EV_ENABLE=4,
            KQ_EV_ONESHOT=16, KQ_EV_ERROR=16384, KQ_NOTE_EXIT=2147483648)
        with patch.object(MOD.os, 'waitid', None, create=True), \
                patch.object(MOD.sys, 'platform', 'darwin'), patch.object(MOD, 'select', fake, create=True):
            observer = MOD.exit_observer()
            self.assertTrue(observer.requires_gate)
            observer.register(123)
            self.assertFalse(observer.exited())
            observer.close()
        self.assertEqual(events[0][0][0].ident, 123)
        self.assertEqual(events[0][0][0].fflags, fake.KQ_NOTE_EXIT)
        self.assertEqual(events[-1], 'closed')

    def test_kqueue_only_latches_matching_exit_and_rejects_errors(self):
        events = []
        pending = []
        queue = SimpleNamespace(control=lambda changes, count, timeout: pending[:], close=lambda: None)
        fake = kqueue_facade(events)
        fake.kqueue = lambda: queue
        with patch.object(MOD, 'select', fake):
            observer = MOD.KqueueObserver()
            observer.register(123)
            for ident, filter_, flags, fflags, data in (
                (999, fake.KQ_FILTER_PROC, 0, fake.KQ_NOTE_EXIT, 0),
                (123, 42, 0, fake.KQ_NOTE_EXIT, 0),
                (123, fake.KQ_FILTER_PROC, 0, 0, 0),
                (123, fake.KQ_FILTER_PROC, fake.KQ_EV_ERROR, 0, errno.ESRCH),
            ):
                pending[:] = [SimpleNamespace(ident=ident, filter=filter_, flags=flags,
                                              fflags=fflags, data=data)]
                with self.assertRaises((ValueError, OSError)):
                    observer.exited()
                self.assertFalse(observer.seen_exit)
            pending[:] = [SimpleNamespace(ident=123, filter=fake.KQ_FILTER_PROC,
                                          flags=0, fflags=fake.KQ_NOTE_EXIT, data=0)]
            self.assertTrue(observer.exited())
            pending.clear()
            self.assertTrue(observer.exited())
            observer.close()

    def test_waitid_uses_wnowait_and_echild_is_not_exit(self):
        calls = []
        fake = SimpleNamespace(P_PID=1, WEXITED=2, WNOHANG=4, WNOWAIT=8,
                               waitid=lambda *args: calls.append(args))
        with patch.object(MOD, 'os', fake):
            observer = MOD.WaitidObserver()
            observer.register(123)
            self.assertFalse(observer.exited())
            self.assertEqual(calls, [(1, 123, 2 | 4 | 8)])
            fake.waitid = lambda *args: (_ for _ in ()).throw(ChildProcessError(errno.ECHILD, 'lost'))
            with self.assertRaises(MOD.ChildOwnershipLost):
                observer.exited()

    def test_nondefault_sigchld_rejected_before_spawn(self):
        with patch.object(MOD.signal, 'getsignal', return_value=signal.SIG_IGN), \
                patch.object(MOD.subprocess, 'Popen') as spawn:
            with self.assertRaisesRegex(ValueError, 'SIGCHLD'):
                MOD.run_bounded(['unused'], env={}, cwd='.', timeout=1)
            spawn.assert_not_called()


class DarwinMembershipTests(unittest.TestCase):
    def test_scalar_abi_and_exact_single_pid_result(self):
        calls = []
        def query(kind, group, buffer, size):
            calls.append((kind, group, size))
            buffer[0] = group
            return MOD.ctypes.sizeof(MOD.ctypes.c_int)
        with patch.object(MOD.ctypes, 'CDLL', return_value=SimpleNamespace(proc_listpids=query)) as load:
            self.assertEqual(MOD.darwin_group_members(123), [123])
            load.assert_called_once_with('/usr/lib/libproc.dylib', use_errno=True)
        self.assertEqual(calls, [(2, 123, 2 * MOD.ctypes.sizeof(MOD.ctypes.c_int))])
        self.assertEqual(query.argtypes, [MOD.ctypes.c_uint32, MOD.ctypes.c_uint32,
                                         MOD.ctypes.c_void_p, MOD.ctypes.c_int])
        self.assertIs(query.restype, MOD.ctypes.c_int)

    def test_empty_full_malformed_oversized_and_errno_fail_closed(self):
        width = MOD.ctypes.sizeof(MOD.ctypes.c_int)
        for count, first, error in ((0, 0, 0), (2 * width, 123, 0),
                (3 * width, 123, 0), (width - 1, 123, 0), (-1, 123, 0),
                (width, 0, 0), (width, 123, errno.EPERM), (0, 0, errno.EACCES)):
            with self.subTest(count=count, first=first, error=error):
                def query(kind, group, buffer, size):
                    buffer[0] = first
                    MOD.ctypes.set_errno(error)
                    return count
                with patch.object(MOD.ctypes, 'CDLL', return_value=SimpleNamespace(proc_listpids=query)):
                    with self.assertRaises((ValueError, OSError)):
                        MOD.darwin_group_members(123)


@unittest.skipUnless(os.name == 'posix', 'headless acceptance targets POSIX only')
class CaptureTests(unittest.TestCase):
    def run_child(self, code, **kwargs):
        with tempfile.TemporaryDirectory() as td:
            return MOD.run_bounded([sys.executable, '-c', code], env={'PATH': '/nonexistent'},
                                   cwd=td, timeout=kwargs.pop('timeout', 3), **kwargs)

    def test_normal_stdout_stderr_and_exit(self):
        result = self.run_child('import sys; print("ok"); print("err", file=sys.stderr)')
        self.assertEqual(result['returncode'], 0)
        self.assertIsNone(result['failure'])
        self.assertEqual(result['stdout'], b'ok\n')
        self.assertEqual(result['stderr'], b'err\n')

    def test_stdout_and_stderr_overflow_are_bounded(self):
        for fd in (1, 2):
            result = self.run_child(f'import os;\nwhile True: os.write({fd}, b"x" * 4096)', max_output=1024)
            self.assertIn('output cap', result['failure'])
            self.assertLessEqual(len(result['stdout']) + len(result['stderr']), 1024)
            self.assertTrue(result['reaped'])

    def test_timeout_kills_and_reaps(self):
        result = self.run_child('import time; time.sleep(20)', timeout=0.05)
        self.assertIn('timeout', result['failure'])
        self.assertTrue(result['reaped'])

    def test_darwin_zombie_only_eperm_is_verified_then_reaped(self):
        real_popen, real_killpg = MOD.subprocess.Popen, os.killpg
        children, events = [], []
        def spawn(*args, **kwargs):
            process = real_popen(*args, **kwargs)
            children.append(process)
            wait = process.wait
            def checked_wait(*args, **kwargs):
                events.append('wait')
                self.assertEqual(events[:2], ['killpg', 'members'])
                return wait(*args, **kwargs)
            process.wait = checked_wait
            return process
        def killpg(pid, sig):
            self.assertNotIn('wait', events)
            events.append('killpg')
            raise PermissionError(errno.EPERM, 'Darwin zombie-only group')
        def members(pid):
            events.append('members')
            self.assertEqual(pid, children[0].pid)
            return [pid]
        try:
            with patch.object(MOD.sys, 'platform', 'darwin'), \
                    patch.object(MOD.subprocess, 'Popen', spawn), patch.object(MOD.os, 'killpg', killpg), \
                    patch.object(MOD, 'darwin_group_members', members, create=True):
                result = self.run_child('print("already exited")')
            self.assertEqual(result['returncode'], 0)
            self.assertIsNone(result['failure'])
            self.assertTrue(result['reaped'])
            self.assertEqual(result['group_cleanup'], 'darwin-confirmed-exited-only')
        finally:
            for process in children:
                if process.returncode is None:
                    try:
                        real_killpg(process.pid, signal.SIGKILL)
                    except (ProcessLookupError, PermissionError):
                        pass
                    # Bypass the ordering assertion only for the red baseline cleanup.
                    MOD.subprocess.Popen.wait(process, timeout=5)

    def test_eperm_without_exact_proof_fails_but_reaps(self):
        for mode in ('linux', 'other-pid', 'empty', 'lookup-error', 'wrong-errno', 'live-leader'):
            with self.subTest(mode=mode):
                events = []
                host_observer = MOD.exit_observer()
                def denied(pid, sig):
                    events.append('killpg')
                    raise PermissionError(errno.EACCES if mode == 'wrong-errno' else errno.EPERM,
                                          'injected group denial')
                def members(pid):
                    events.append('members')
                    if mode == 'lookup-error':
                        raise OSError(errno.EIO, 'injected lookup failure')
                    return [] if mode == 'empty' else [pid + 1]
                with patch.object(MOD.sys, 'platform', 'linux' if mode == 'linux' else 'darwin'), \
                        patch.object(MOD, 'exit_observer', return_value=host_observer), \
                        patch.object(MOD.os, 'killpg', denied), \
                        patch.object(MOD, 'darwin_group_members', members):
                    code = 'import time; time.sleep(20)' if mode == 'live-leader' else 'pass'
                    result = self.run_child(code, timeout=0.02 if mode == 'live-leader' else 3)
                self.assertIn('group cleanup failed', result['failure'])
                self.assertEqual(result['group_cleanup'], 'failed')
                self.assertTrue(result['reaped'])
                self.assertTrue(result['drainers_finished'])
                self.assertEqual(events.count('killpg'), 1)
                if mode in ('linux', 'wrong-errno', 'live-leader'):
                    self.assertNotIn('members', events)

    def test_signal_and_reap_failures_are_both_reported(self):
        real_popen = MOD.subprocess.Popen
        children = []
        host_observer = MOD.exit_observer()
        def spawn(*args, **kwargs):
            process = real_popen(*args, **kwargs)
            wait = process.wait
            children.append((process, wait))
            def failed_wait(*args, **kwargs):
                raise MOD.subprocess.TimeoutExpired('injected wait failure', 5)
            process.wait = failed_wait
            return process
        try:
            with patch.object(MOD.sys, 'platform', 'linux'), \
                    patch.object(MOD, 'exit_observer', return_value=host_observer), \
                    patch.object(MOD.subprocess, 'Popen', spawn), \
                    patch.object(MOD.os, 'killpg', side_effect=PermissionError(errno.EPERM, 'injected denial')):
                result = self.run_child('pass')
            self.assertIn('group cleanup failed', result['failure'])
            self.assertIn('direct child reap failed', result['failure'])
            self.assertFalse(result['reaped'])
            self.assertEqual(result['group_cleanup'], 'failed')
        finally:
            for process, wait in children:
                wait(timeout=5)  # No signal is issued after this reap.

    def test_live_grandchild_with_closed_stdio_cannot_excuse_eperm(self):
        real_popen, real_members = MOD.subprocess.Popen, MOD.darwin_group_members
        real_platform = sys.platform
        children, events = [], []
        control_read, control_write = os.pipe()
        try:
            with tempfile.TemporaryDirectory() as td:
                marker = Path(td) / 'grandchild-pid'
                code = ('import os\n'
                        'read_ready, write_ready = os.pipe()\n'
                        'child = os.fork()\n'
                        'if child == 0:\n'
                        '  os.close(read_ready); os.close(1); os.close(2)\n'
                        '  os.write(write_ready, b"R"); os.close(write_ready)\n'
                        f'  os.read({control_read}, 1); os._exit(0)\n'
                        'os.close(write_ready)\n'
                        'assert os.read(read_ready, 1) == b"R"\n'
                        'os.close(read_ready)\n'
                        f'open({str(marker)!r}, "w").write(str(child))\n'
                        f'os.close({control_read}); os._exit(0)\n')

                def spawn(*args, **kwargs):
                    kwargs['pass_fds'] = (*kwargs.get('pass_fds', ()), control_read)
                    process = real_popen(*args, **kwargs)
                    wait = process.wait
                    def checked_wait(*args, **kwargs):
                        self.assertIn('members', events)
                        events.append('wait')
                        return wait(*args, **kwargs)
                    process.wait = checked_wait
                    children.append(process)
                    return process

                def denied(pid, sig):
                    self.assertNotIn('wait', events)
                    events.append('killpg')
                    raise PermissionError(errno.EPERM, 'injected denial with live descendant')

                def members(pid):
                    self.assertNotIn('wait', events)
                    events.append('members')
                    os.kill(int(marker.read_text()), 0)
                    if real_platform == 'darwin':
                        return real_members(pid)  # Actual kernel list in hosted Mac CI.
                    return [pid, int(marker.read_text())]  # Linux proof-path plumbing only.

                with patch.object(MOD.sys, 'platform', 'darwin'), \
                        patch.object(MOD.subprocess, 'Popen', spawn), patch.object(MOD.os, 'killpg', denied), \
                        patch.object(MOD, 'darwin_group_members', members):
                    result = self.run_child(code)
                self.assertEqual(result['stdout'], b'')
                self.assertEqual(result['stderr'], b'')
                self.assertIn('group cleanup failed', result['failure'])
                self.assertEqual(result['group_cleanup'], 'failed')
                self.assertTrue(result['reaped'])
                self.assertTrue(result['drainers_finished'])
                self.assertEqual(events, ['killpg', 'members', 'wait'])
        finally:
            # The controlled descendant exits via its private lease, not a PID
            # or process-group signal after the leader has been reaped.
            os.write(control_write, b'X')
            os.close(control_write)
            os.close(control_read)
            for process in children:
                if process.returncode is None:
                    os.kill(process.pid, signal.SIGKILL)
                    MOD.subprocess.Popen.wait(process, timeout=5)

    def test_group_cleanup_precedes_reaping_on_host_backend(self):
        real_popen, real_killpg, real_factory = MOD.subprocess.Popen, os.killpg, MOD.exit_observer
        events, children = [], []

        def spawn(*args, **kwargs):
            process = real_popen(*args, **kwargs)
            original_wait = process.wait
            children.append((process, original_wait))
            def wait(*args, **kwargs):
                self.assertIn(('killpg', process.pid), events, 'reaping must follow group cleanup')
                events.append(('wait', process.pid))
                return original_wait(*args, **kwargs)
            def poll():
                self.fail('poll reaps the group leader too early')
            process.wait, process.poll = wait, poll
            return process

        def killpg(pid, sig):
            events.append(('killpg', pid))
            return real_killpg(pid, sig)

        def factory():
            observer = real_factory()
            observe = observer.exited
            def exited():
                events.append(('observe', observer.name))
                exited = observe()
                if exited and sys.platform == 'darwin':
                    # Actual OS API assertion on hosted Mac, not a ctypes mock.
                    self.assertEqual(MOD.darwin_group_members(children[0][0].pid),
                                     [children[0][0].pid])
                return exited
            observer.exited = exited
            return observer

        try:
            with patch.object(MOD.subprocess, 'Popen', spawn), patch.object(MOD.os, 'killpg', killpg), \
                    patch.object(MOD, 'exit_observer', factory):
                result = self.run_child('print("reserved")')
                self.assertEqual(result['returncode'], 0)
            self.assertTrue(any(event[0] == 'observe' for event in events))
        finally:
            # Keep the regression safe even against the old, failing implementation.
            for process, original_wait in children:
                if process.returncode is None:
                    try:
                        real_killpg(process.pid, 9)
                    except ProcessLookupError:
                        pass
                    original_wait(timeout=5)

    def test_gated_exec_registration_order_pid_group_fds_and_exact_exit(self):
        events, children = [], []
        facade = kqueue_facade(events)
        real_popen, real_write, real_killpg = MOD.subprocess.Popen, os.write, os.killpg
        real_pipe = os.pipe
        gate = os.pipe()
        gate_pending = True

        def pipe():
            nonlocal gate_pending
            if gate_pending:
                gate_pending = False
                return gate
            return real_pipe()

        def spawn(command, **kwargs):
            self.assertEqual(command[:4], [sys.executable, '-I', '-S', '-c'])
            self.assertEqual(kwargs['pass_fds'], (gate[0],))
            process = real_popen(command, **kwargs)
            wait = process.wait
            def checked_wait(*args, **kwargs):
                self.assertIn(('killpg', process.pid), events)
                return wait(*args, **kwargs)
            process.wait = checked_wait
            children.append(process)
            return process

        def release(fd, data):
            self.assertIn(('register', children[0].pid), events)
            events.append(('release', children[0].pid))
            return real_write(fd, data)

        def killpg(pid, sig):
            events.append(('killpg', pid))
            return real_killpg(pid, sig)

        code = ('import os,sys\n'
                f'fd = {gate[0]}\n'
                'try: os.fstat(fd)\n'
                'except OSError: pass\n'
                'else: raise SystemExit("gate fd leaked across exec")\n'
                'print(os.getpid(), os.getpgrp())\n'
                'sys.exit(7)\n')
        with patch.object(MOD.os, 'waitid', None, create=True), \
                patch.object(MOD.sys, 'platform', 'darwin'), patch.object(MOD, 'select', facade), \
                patch.object(MOD.os, 'pipe', pipe), \
                patch.object(MOD.subprocess, 'Popen', spawn), patch.object(MOD.os, 'write', release), \
                patch.object(MOD.os, 'killpg', killpg):
            result = self.run_child(code)
        self.assertEqual(result['returncode'], 7)
        self.assertIsNone(result['failure'])
        self.assertEqual(result['stdout'].split(), [str(children[0].pid).encode()] * 2)
        self.assertEqual(result['exit_observer'], 'kqueue-note-exit-gated-exec')
        self.assertIn(('queue_close', children[0].pid), events)
        for fd in gate:
            with self.assertRaises(OSError):
                os.fstat(fd)

    def test_gate_registration_failure_never_executes_and_reaps(self):
        for failure in (errno.ESRCH, errno.EACCES):
            events, children, descriptors = [], [], []
            facade = kqueue_facade(events, registration_error=failure)
            real_popen, real_pipe, real_killpg = MOD.subprocess.Popen, os.pipe, os.killpg

            def pipe():
                pair = real_pipe()
                descriptors.extend(pair)
                return pair

            def spawn(*args, **kwargs):
                process = real_popen(*args, **kwargs)
                wait = process.wait
                def checked_wait(*args, **kwargs):
                    self.assertIn(('killpg', process.pid), events)
                    return wait(*args, **kwargs)
                process.wait = checked_wait
                children.append(process)
                return process

            def killpg(pid, sig):
                events.append(('killpg', pid))
                return real_killpg(pid, sig)

            with tempfile.TemporaryDirectory() as td:
                marker = Path(td) / 'must-not-run'
                with patch.object(MOD.os, 'waitid', None, create=True), \
                        patch.object(MOD.sys, 'platform', 'darwin'), patch.object(MOD, 'select', facade), \
                        patch.object(MOD.os, 'pipe', pipe), patch.object(MOD.subprocess, 'Popen', spawn), \
                        patch.object(MOD.os, 'killpg', killpg), patch.object(MOD.os, 'write') as release:
                    with self.assertRaises(OSError):
                        self.run_child(f'open({str(marker)!r}, "w").close()')
                    release.assert_not_called()
                self.assertFalse(marker.exists())
            self.assertIsNotNone(children[0].returncode)
            self.assertIn(('queue_close', children[0].pid), events)
            for fd in descriptors:
                with self.assertRaises(OSError):
                    os.fstat(fd)

    def test_gate_close_error_is_not_retried_and_still_cleans_up(self):
        for failing_end in (0, 1):
            events, children, gate, attempts = [], [], [], []
            facade = kqueue_facade(events)
            real_popen, real_pipe, real_close = MOD.subprocess.Popen, os.pipe, os.close

            def pipe():
                pair = real_pipe()
                if not gate:
                    gate.extend(pair)
                return pair

            def spawn(*args, **kwargs):
                process = real_popen(*args, **kwargs)
                children.append(process)
                return process

            def close(fd):
                if children and fd == gate[failing_end]:
                    attempts.append(fd)
                    real_close(fd)
                    raise InterruptedError(errno.EINTR, 'injected error after FD closed')
                return real_close(fd)

            with patch.object(MOD.os, 'waitid', None, create=True), \
                    patch.object(MOD.sys, 'platform', 'darwin'), patch.object(MOD, 'select', facade), \
                    patch.object(MOD.os, 'pipe', pipe), patch.object(MOD.os, 'close', close), \
                    patch.object(MOD.subprocess, 'Popen', spawn):
                with self.assertRaises(InterruptedError):
                    MOD.run_bounded([sys.executable, '-c', 'pass'], env={'PATH': '/nonexistent'},
                                    cwd='.', timeout=3)
            self.assertEqual(attempts, [gate[failing_end]])
            self.assertIsNotNone(children[0].returncode)
            self.assertIn(('queue_close', None if failing_end == 0 else children[0].pid), events)
            for fd in gate:
                with self.assertRaises(OSError):
                    os.fstat(fd)

    def test_gate_terminated_before_release_never_executes(self):
        events = []
        facade = kqueue_facade(events, kill_before_release=True)
        with tempfile.TemporaryDirectory() as td:
            marker = Path(td) / 'must-not-run'
            with patch.object(MOD.os, 'waitid', None, create=True), \
                    patch.object(MOD.sys, 'platform', 'darwin'), patch.object(MOD, 'select', facade):
                try:
                    result = self.run_child(f'open({str(marker)!r}, "w").close()')
                except BrokenPipeError:
                    pass  # The dead gate may close its read end before release.
                else:
                    self.assertNotEqual(result['returncode'], 0)
                    self.assertTrue(result['reaped'])
            self.assertFalse(marker.exists())
        self.assertTrue(any(event[0] == 'queue_close' for event in events))

    def test_gate_eof_never_executes_target(self):
        with tempfile.TemporaryDirectory() as td:
            marker = Path(td) / 'must-not-run'
            code = ('import os, sys\n'
                    'read_fd, write_fd = os.pipe()\n'
                    'os.close(write_fd)\n'
                    f'sys.argv = ["gate", str(read_fd), sys.executable, "-c", '
                    f'{f"open({str(marker)!r}, chr(119)).close()"!r}]\n'
                    f'exec({MOD.EXEC_GATE!r})\n')
            result = self.run_child(code)
            self.assertEqual(result['returncode'], 125)
            self.assertIsNone(result['failure'])
            self.assertFalse(marker.exists())
            self.assertTrue(result['reaped'])

    def test_echild_does_not_signal_reused_numeric_group(self):
        real_factory, real_popen = MOD.exit_observer, MOD.subprocess.Popen
        children = []
        def spawn(*args, **kwargs):
            process = real_popen(*args, **kwargs)
            children.append(process)
            return process
        def factory():
            observer = real_factory()
            observe = observer.exited
            def exited():
                if not observe():
                    return False
                children[0].wait(timeout=5)  # Deliberately emulate an external reaper.
                raise MOD.ChildOwnershipLost('injected ECHILD')
            observer.exited = exited
            return observer
        with patch.object(MOD, 'exit_observer', factory), patch.object(MOD.subprocess, 'Popen', spawn), \
                patch.object(MOD.os, 'killpg') as killpg:
            result = self.run_child('print("already exited")')
            killpg.assert_not_called()
        self.assertIn('ECHILD', result['failure'])
        self.assertFalse(result['reaped'])
        self.assertTrue(result['drainers_finished'])

    def test_ownership_loss_returns_with_daemon_drainers_without_tracebacks(self):
        real_factory, real_popen = MOD.exit_observer, MOD.subprocess.Popen
        real_thread, real_read = threading.Thread, os.read
        children, drainers, exceptions = [], [], []
        release = threading.Event()

        def spawn(*args, **kwargs):
            process = real_popen(*args, **kwargs)
            children.append(process)
            return process

        def factory():
            observer = real_factory()
            observe = observer.exited
            def exited():
                if not observe():
                    return False
                children[0].wait(timeout=5)  # Emulate an external reaper.
                raise MOD.ChildOwnershipLost('injected ECHILD with held pipe')
            observer.exited = exited
            return observer

        class Drainer(real_thread):
            def __init__(self, *args, **kwargs):
                super().__init__(*args, **kwargs)
                drainers.append(self)

            def join(self, timeout=None):
                # Bound this synthetic held-pipe test without a ten-second wait.
                return super().join(timeout=min(timeout, 0.02) if timeout else timeout)

        def held_read(fd, size):
            if threading.current_thread() in drainers:
                release.wait()
                return b'late'  # Next fileno() sees the closed stream.
            return real_read(fd, size)

        try:
            with patch.object(MOD, 'exit_observer', factory), \
                    patch.object(MOD.subprocess, 'Popen', spawn), \
                    patch.object(MOD.os, 'killpg') as killpg, patch.object(MOD.os, 'read', held_read), \
                    patch.object(MOD.threading, 'Thread', Drainer), \
                    patch.object(threading, 'excepthook', exceptions.append):
                start = time.monotonic()
                result = self.run_child('pass')
                self.assertLess(time.monotonic() - start, 2)
                killpg.assert_not_called()
                self.assertIn('ECHILD', result['failure'])
                self.assertIn('drainer failed', result['failure'])
                self.assertFalse(result['drainers_finished'])
                self.assertFalse(result['reaped'])
                self.assertTrue(all(t.daemon for t in drainers))
                release.set()
                for thread in drainers:
                    real_thread.join(thread, timeout=2)
                self.assertFalse(any(t.is_alive() for t in drainers))
                self.assertEqual(exceptions, [])
        finally:
            release.set()
            for thread in drainers:
                real_thread.join(thread, timeout=2)


if __name__ == '__main__':
    unittest.main()
