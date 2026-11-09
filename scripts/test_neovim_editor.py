"""Offline regressions for the bounded headless-editor harness, no editor needed."""
import importlib.util
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    'neovim_editor', Path(__file__).with_name('check-neovim-editor.py'))
MOD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MOD)


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

    def test_group_cleanup_precedes_reaping_and_uses_wnowait(self):
        real_popen, real_killpg, real_waitid = MOD.subprocess.Popen, os.killpg, os.waitid
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

        def waitid(kind, pid, flags):
            self.assertTrue(flags & os.WNOWAIT)
            events.append(('waitid', pid))
            return real_waitid(kind, pid, flags)

        try:
            with patch.object(MOD.subprocess, 'Popen', spawn), patch.object(MOD.os, 'killpg', killpg), \
                    patch.object(MOD.os, 'waitid', waitid):
                result = self.run_child('print("reserved")')
                self.assertEqual(result['returncode'], 0)
            self.assertTrue(any(event[0] == 'waitid' for event in events))
        finally:
            # Keep the regression safe even against the old, failing implementation.
            for process, original_wait in children:
                if process.returncode is None:
                    try:
                        real_killpg(process.pid, 9)
                    except ProcessLookupError:
                        pass
                    original_wait(timeout=5)


if __name__ == '__main__':
    unittest.main()
