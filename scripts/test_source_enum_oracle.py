"""Offline source-enum contract and comparator tests. No Java execution."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import unittest

spec = importlib.util.spec_from_file_location("enum_oracle", Path(__file__).with_name("check-source-enum-oracle.py"))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

class SourceEnumOracleTests(unittest.TestCase):
    def setUp(self):
        self.predictions, self.declarations, self.queries, _ = m.read_inputs()
        self.contract = json.loads((m.FIXTURE / "contract.json").read_text())

    def test_frozen_inputs_and_complete_handwritten_candidate(self):
        expected = m.expected_native(self.predictions, self.declarations)
        self.assertEqual(expected, self.contract["candidate"])
        m.compare_native(expected, self.contract["candidate"])
        m.check_compiler(self.predictions, self.declarations, self.queries, self.contract["compiler"])

    def test_exact_native_mutation_controls(self):
        self.assertEqual(m.mutation_controls(self.contract["candidate"]), 6)

    def test_baseline_cannot_pass_as_candidate(self):
        self.assertNotEqual(self.contract["baseline"], self.contract["candidate"])
        with self.assertRaises(AssertionError):
            m.compare_native(self.contract["baseline"], self.contract["candidate"])

    def test_compiler_missing_extra_identity_and_range_rejected(self):
        for change in ("missing", "extra", "path", "kind", "owner", "name", "range"):
            with self.subTest(change=change):
                records = copy.deepcopy(self.contract["compiler"])
                key = "qualified"
                if change == "missing":
                    del records[key]
                elif change == "extra":
                    records["extra"] = records[key]
                elif change == "range":
                    records[key][4:] = ["0", "1"]
                else:
                    records[key][{"path": 0, "kind": 1, "owner": 2, "name": 3}[change]] += "wrong"
                with self.assertRaises(AssertionError):
                    m.check_compiler(self.predictions, self.declarations, self.queries, records)

    def test_body_and_case_queries_remain_refused(self):
        for query in ("body_outer", "body_hidden", "case_label"):
            self.assertEqual(self.contract["candidate"][query], ["Unresolved", "0"])

    def test_native_wrong_owner_range_and_duplicate_are_rejected(self):
        for field in range(6):
            changed = copy.deepcopy(self.contract["candidate"])
            target = changed["qualified"][2].split("|")
            target[field] += "wrong"
            changed["qualified"][2] = "|".join(target)
            with self.assertRaises(AssertionError):
                m.compare_native(changed, self.contract["candidate"])

    def test_optimized_python_cannot_bypass_assertion_validation(self):
        result = subprocess.run([sys.executable, "-O", str(Path(m.__file__)), "--help"],
                                capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires Python without -O", result.stderr)

if __name__ == "__main__":
    unittest.main()
