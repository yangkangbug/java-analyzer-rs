"""Frozen complete-output and negative controls for the switch parser stage."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("switch_contract",
    Path(__file__).with_name("traditional-switch-parser-contract.py"))
contract = importlib.util.module_from_spec(spec)
spec.loader.exec_module(contract)

class SwitchParserContractTests(unittest.TestCase):
    def test_frozen_sources_predictions_and_complete_difference(self):
        for corpus, old_count, new_count in (("commons", 85, 60), ("gson", 166, 138)):
            old, hashes = contract.load(corpus, "baseline")
            current, current_hashes = contract.load(corpus)
            self.assertEqual(old["report_count"], old_count)
            self.assertEqual(current["report_count"], new_count)
            self.assertEqual(hashes, current_hashes)
            self.assertTrue({row["path"] for row in current["files"]}.issubset(hashes))
            self.assertNotEqual(contract.canonical(old), contract.canonical(current))

    def test_complete_message_range_inventory_and_stage_mutations_fail(self):
        for corpus in ("commons", "gson"):
            expected, _ = contract.load(corpus)
            row_index = next(i for i, row in enumerate(expected["files"]) if row["reports"])
            for mutation in ("message", "start_byte", "end_byte", "start_utf16", "end_utf16",
                             "missing_file", "extra_file", "missing_report", "extra_report",
                             "count", "semantic", "baseline", "extra_field"):
                actual = copy.deepcopy(expected)
                report = actual["files"][row_index]["reports"][0]
                if mutation == "message": report["message"] += "unexpected"
                elif mutation in ("start_byte", "end_byte", "start_utf16", "end_utf16"):
                    report["range"][mutation] += 1
                elif mutation == "missing_file": actual["files"].pop()
                elif mutation == "extra_file": actual["files"].append(actual["files"][0])
                elif mutation == "missing_report": actual["files"][row_index]["reports"].pop()
                elif mutation == "extra_report": actual["files"][row_index]["reports"].append(report)
                elif mutation == "count": actual["report_count"] += 1
                elif mutation == "semantic": actual["java_semantic_diagnostics"] = True
                elif mutation == "extra_field": actual["extra"] = True
                elif mutation == "baseline": actual = contract.load(corpus, "baseline")[0]
                with self.subTest(corpus=corpus, mutation=mutation), self.assertRaisesRegex(AssertionError, "complete switch parser records differ"):
                    contract.check(corpus, actual, {})

    def test_complete_calls_are_partitioned_by_old_opaque_ranges(self):
        old, _ = contract.load("commons", "baseline")
        ranges = {row["path"]: [gap["range"] for gap in row["reports"]
                  if gap["message"] == "switch is not parsed in this subset"] for row in old["files"]}
        rows = [line.split("\t") for line in (contract.CORPUS / "commons-calls.tsv").read_text().splitlines()
                if not line.startswith("#")]
        self.assertEqual(len(rows), 3077)
        self.assertEqual(len({(path, offset) for path, offset, name, stage in rows}), 3077)
        self.assertEqual(sum(stage == "historical" for _, _, _, stage in rows), 3030)
        self.assertEqual(sum(stage == "switch" for _, _, _, stage in rows), 47)
        for path, offset, name, stage in rows:
            opaque = any(gap["start_byte"] <= int(offset) < gap["end_byte"] for gap in ranges[path])
            self.assertEqual(opaque, stage == "switch")
            self.assertTrue(name)

    def test_unknown_stage_and_corpus_rejected(self):
        for corpus, stage in (("other", "candidate"), ("gson", "auto"), ("gson", "current")):
            with self.assertRaises(AssertionError): contract.load(corpus, stage)

if __name__ == "__main__": unittest.main()
