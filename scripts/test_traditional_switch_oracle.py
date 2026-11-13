"""Offline traditional-switch contract and comparator tests. No Java execution."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import unittest

spec = importlib.util.spec_from_file_location("switch_oracle", Path(__file__).with_name("check-traditional-switch-oracle.py"))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

class TraditionalSwitchOracleTests(unittest.TestCase):
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
                key = "later_shared"
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

    def test_shared_scope_targets_same_declaration(self):
        candidate = self.contract["candidate"]
        self.assertEqual(candidate["first_shared"][2], candidate["later_shared"][2])
        self.assertNotEqual(candidate["later_shared"][2], candidate["after_use"][2])
        for query in ("constant_label", "enum_label", "hidden_use"):
            self.assertEqual(candidate[query], ["Unresolved", "0"])

    def test_native_wrong_owner_range_and_duplicate_are_rejected(self):
        for field in range(6):
            changed = copy.deepcopy(self.contract["candidate"])
            target = changed["later_shared"][2].split("|")
            target[field] += "wrong"
            changed["later_shared"][2] = "|".join(target)
            with self.assertRaises(AssertionError):
                m.compare_native(changed, self.contract["candidate"])

    def test_optimized_python_cannot_bypass_assertion_validation(self):
        result = subprocess.run([sys.executable, "-O", str(Path(m.__file__)), "--help"],
                                capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires Python without -O", result.stderr)

if __name__ == "__main__":
    unittest.main()
