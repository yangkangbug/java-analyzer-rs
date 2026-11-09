"""Offline measurement-harness regressions; fixtures execute Python, never Java."""
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("measure_cli", Path(__file__).with_name("measure-cli-preview.py"))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


@unittest.skipUnless(sys.platform == "linux", "Linux resource protocol")
class Measurements(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def run_fixture(self, code, **options):
        return m.measured_run([sys.executable, "-c", code], self.root, **options)

    def test_process_stat_with_parentheses_and_spaces(self):
        fields = ["S", "1", "321"] + ["0"] * 16 + ["1234", "9999", "42"]
        stat = m.parse_proc_stat("10 (some (name) here) " + " ".join(fields))
        self.assertEqual(stat, {"state": "S", "pgrp": 321, "start_ticks": 1234, "rss_pages": 42})

    def test_normal_exit_and_both_streams(self):
        facts, out, err = self.run_fixture("import os; os.write(1,b'hello'); os.write(2,b'world')")
        self.assertEqual((out, err), (b"hello", b"world"))
        self.assertEqual(facts["exit_code"], 0)
        self.assertIsNone(facts["failure"])
        self.assertTrue(facts["supervisor_reaped"])
        self.assertGreater(facts["linux_wait4_maxrss_kib"], 0)
        self.assertGreater(facts["wall_ms"], 0)

    def test_concurrent_stream_draining_and_combined_limit(self):
        code = "import os\nfor _ in range(200):\n os.write(1,b'a'*4096); os.write(2,b'b'*4096)"
        facts, out, err = self.run_fixture(code, max_output=2_000_000)
        self.assertIsNone(facts["failure"])
        self.assertEqual((len(out), len(err)), (819200, 819200))
        facts, out, err = self.run_fixture(code, max_output=8192)
        self.assertEqual(facts["failure"], "combined_output_limit")
        self.assertLessEqual(len(out) + len(err), 8192)
        self.assertTrue(facts["supervisor_reaped"])

    def test_timeout_kills_group_including_pipe_holder(self):
        # The direct child exits; its child keeps stdout open. EOF is part of the
        # outer deadline and the orphan must not remain running afterward.
        code = "import os,time\npid=os.fork()\nif pid:\n print(pid,flush=True); os._exit(0)\ntime.sleep(30)"
        started = time.monotonic()
        facts, out, _ = self.run_fixture(code, timeout=0.2)
        self.assertEqual(facts["failure"], "outer_timeout")
        self.assertTrue(facts["supervisor_reaped"])
        self.assertLess(time.monotonic() - started, 3)
        child = int(out.strip())
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            try:
                state = m.parse_proc_stat(Path(f"/proc/{child}/stat").read_text())["state"]
            except FileNotFoundError:
                break
            if state == "Z":  # Orphan reaping belongs to the container/OS.
                break
            time.sleep(0.01)
        else:
            self.fail("descendant remained running after group cleanup")

    def test_worker_memory_not_only_low_supervisor_rss(self):
        code = ("import os,time\npid=os.fork()\nif not pid:\n"
                " data=bytearray(64*1024*1024)\n"
                " for i in range(0,len(data),4096): data[i]=1\n"
                " time.sleep(0.12); os._exit(0)\n"
                "os.waitpid(pid,0)\n")
        facts, _, _ = self.run_fixture(code)
        self.assertIsNone(facts["failure"])
        self.assertGreater(facts["linux_wait4_maxrss_kib"], 55 * 1024)
        sample = facts["proc_sampling"]
        self.assertGreaterEqual(sample["distinct_processes_observed"], 2)
        self.assertGreaterEqual(sample["peak_simultaneously_observed_processes"], 2)
        self.assertGreater(sample["peak_sum_rss_kib"], 65 * 1024)

    def test_proc_unavailable_reported_not_assumed_zero(self):
        sampler = m.ProcessGroupSampler(1, 0.005)
        with patch.object(Path, "iterdir", side_effect=PermissionError("denied")):
            sampler.sample(time.perf_counter())
        facts = sampler.result()
        self.assertFalse(facts["available"])
        self.assertIsNone(facts["peak_sum_rss_kib"])
        self.assertEqual(facts["samples"], 0)

    def test_invalid_options_are_rejected_before_spawn(self):
        for options in ({"timeout": float("nan")}, {"max_output": 0}, {"sample_interval": 0}):
            with self.subTest(options=options), self.assertRaises(m.MeasurementError):
                self.run_fixture("pass", **options)

    def test_edit_offsets_only_shift_the_edited_file(self):
        queries = [["q1", "A.java", "10", "a", "Resolved", "1"],
                   ["q2", "B.java", "20", "a", "Resolved", "1"]]
        targets = [["q1", "A.java", "Class", "A", "a", "3", "4"],
                   ["q2", "!/classes/B.class", "Class", "B", "b", "0", "0"]]
        q, t = m.shifted_goldens(queries, targets, "A.java", b"// edit\n")
        self.assertEqual((q[0][2], t[0][5:]), ("18", ["11", "12"]))
        self.assertEqual(q[1], queries[1])
        self.assertEqual(t[1], targets[1])
        self.assertEqual(queries[0][2], "10")
        with self.assertRaises(UnicodeDecodeError):
            m.shifted_goldens(queries, targets, "A.java", "😀".encode())

    def test_statistics_exclude_warmup_and_do_not_invent_percentiles(self):
        samples = [{"workload": "x", "measured": measured, "wall_ms": value,
                    "linux_wait4_maxrss_kib": 20,
                    "proc_sampling": {"available": True, "peak_sum_rss_kib": 30}}
                   for measured, value in ((False, 999), (True, 1), (True, 3), (True, 2))]
        summary = m.summarize(samples)["x"]
        self.assertEqual(summary["n"], 3)
        self.assertEqual(summary["wall_ms"], {"min": 1, "median": 2, "max": 3})
        self.assertNotIn("p95", json.dumps(summary))

    def test_envelope_rejects_false_success_or_wrong_snapshot(self):
        manifest = self.root / "project.json"
        facts = {"failure": None, "exit_code": 0}
        response = {"schema_version": 1, "command": "inspect",
                    "snapshot": {"identity": "invocation_local", "id": "x", "manifest": str(manifest)}}
        encoded = (json.dumps(response) + "\n").encode()
        self.assertEqual(m.validate_envelope(facts, encoded, b"", "inspect", manifest), response)
        for bad_facts, out, err in (({"failure": "outer_timeout", "exit_code": 0}, encoded, b""),
                                    (facts, encoded, b"noise"), (facts, encoded.rstrip(), b""),
                                    ({"failure": None, "exit_code": 2}, encoded, b"")):
            with self.assertRaises(m.MeasurementError):
                m.validate_envelope(bad_facts, out, err, "inspect", manifest)
        response["snapshot"]["identity"] = "persistent"
        with self.assertRaises(m.MeasurementError):
            m.validate_envelope(facts, (json.dumps(response) + "\n").encode(), b"", "inspect", manifest)

    def test_failed_edit_restores_source(self):
        path = self.root / "source.java"
        path.write_bytes(b"original")
        case = {"name": "edit", "command": "declaration", "argv": [], "cwd": self.root,
                "before": lambda: path.write_bytes(b"edited"),
                "after": lambda: path.write_bytes(b"original")}
        with patch.object(m, "measured_run", side_effect=OSError("fixture failure")):
            with self.assertRaises(OSError):
                m.measure_case(case, {}, True, 1)
        self.assertEqual(path.read_bytes(), b"original")

    def test_group_cleanup_precedes_reap_for_both_runners(self):
        acceptance = m.load_acceptance()
        for runner, wait_name in ((m.measured_run, "wait4"), (acceptance.run_bounded, "waitpid")):
            calls = []
            real_kill = os.killpg
            real_wait = getattr(os, wait_name)
            real_observe = os.waitid
            def kill(*args):
                calls.append("kill")
                return real_kill(*args)
            def wait(*args):
                calls.append("reap")
                return real_wait(*args)
            def observe(*args):
                self.assertTrue(args[2] & os.WNOWAIT)
                calls.append("observe")
                return real_observe(*args)
            with self.subTest(runner=runner.__name__), patch.object(os, "killpg", side_effect=kill), \
                    patch.object(os, wait_name, side_effect=wait), patch.object(os, "waitid", side_effect=observe):
                runner([sys.executable, "-c", "print('done')"], self.root, 2, 1024)
            self.assertIn("observe", calls)
            self.assertLess(calls.index("kill"), calls.index("reap"))

    def test_report_first_creation_is_exclusive_and_symlinks_rejected(self):
        target = self.root / "existing.json"
        target.write_text("original")
        with self.assertRaises(FileExistsError):
            m.write_json(target, {"changed": True})
        self.assertEqual(target.read_text(), "original")
        link = self.root / "dangling.json"
        link.symlink_to(self.root / "missing.json")
        with self.assertRaises(m.MeasurementError):
            m.write_json(link, {})
        alias = self.root / "alias"
        alias.symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(m.MeasurementError):
            m.write_json(alias / "new.json", {})
        self.assertFalse((self.root / "missing.json").exists())

    def test_failed_atomic_report_update_preserves_previous_json(self):
        path = self.root / "report.json"
        m.write_json(path, {"round": 1})
        with patch.object(os, "replace", side_effect=OSError("disk fixture")):
            with self.assertRaises(OSError):
                m.write_json(path, {"round": 2})
        self.assertEqual(json.loads(path.read_text()), {"round": 1})
        self.assertEqual([p.name for p in self.root.iterdir()], ["report.json"])
        m.write_json(path, {"round": 3})
        self.assertEqual(json.loads(path.read_text()), {"round": 3})

    def test_report_replaced_by_another_writer_is_not_overwritten(self):
        path = self.root / "report.json"
        m.write_json(path, {})
        other = self.root / "other"
        other.write_text("external replacement")
        os.replace(other, path)
        with self.assertRaises(m.MeasurementError):
            m.write_json(path, {"round": 2})
        self.assertEqual(path.read_text(), "external replacement")

    def test_provenance_does_not_copy_arbitrary_paths_or_unknown_fields(self):
        identity = {"git_revision": "a" * 40, "rust_build_inputs_sha256": "b" * 64}
        receipt = dict(identity, binary_sha256="c" * 64, source_dirty=True,
                       build_command="cargo build --locked --release --features cli --bin java-analyzer --target-dir /private/example --config SECRET",
                       rustc_version="release: 1.96.0\ncommit-hash: " + "a" * 40 +
                                     "\ncommit-date: 2026-05-25\nhost: x86_64-unknown-linux-gnu\nLLVM version: 22.1.2\nextra: /private/SECRET")
        result = m.portable_build_provenance(receipt, identity, "c" * 64, "d" * 64)
        self.assertNotIn("/private", json.dumps(result))
        self.assertNotIn("SECRET", json.dumps(result))
        self.assertTrue(result["source_dirty"])
        receipt["rustc_version"] = receipt["rustc_version"].replace("release: 1.96.0", "release: /private/SECRET")
        with self.assertRaises(m.MeasurementError):
            m.portable_build_provenance(receipt, identity, "c" * 64, "d" * 64)

    def test_failed_check_keeps_raw_measurement_facts(self):
        facts = {"failure": "outer_timeout", "exit_code": -9, "wall_ms": 30}
        case = {"name": "failed", "argv": [], "cwd": self.root,
                "command": "inspect", "manifest": self.root / "project.json"}
        with patch.object(m, "measured_run", return_value=(facts, b"", b"")):
            with self.assertRaises(m.CaseFailure) as caught:
                m.measure_case(case, {}, True, 1)
        self.assertFalse(caught.exception.facts["correctness_passed"])
        self.assertEqual(caught.exception.facts["failure"], "outer_timeout")
        self.assertEqual(caught.exception.facts["wall_ms"], 30)

    def test_protocol_is_explicit_about_unavailable_metrics(self):
        protocol = m.protocol(7, 60, 1024, 0.005, 30000)
        self.assertEqual(protocol["repetitions"], 7)
        self.assertIn("warm prepared-query latency", protocol["not_measured"])
        self.assertIn("shared pages double counted", protocol["memory_semantics"]["proc_peak_sum_rss_kib"])
        self.assertNotIn("hostname", m.host_identity())


if __name__ == "__main__":
    unittest.main()
