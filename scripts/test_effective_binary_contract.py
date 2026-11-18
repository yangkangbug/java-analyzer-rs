"""Exact versioned effective-binary stage; old expectations remain independently usable."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
import test_header_inventory_contract as previous
gate = previous.gate

class EffectiveBinaryContract(unittest.TestCase):
    def test_three_predicted_changes_preserve_all_other_records(self):
        old, new = gate.header_inventory_plan(), gate.effective_binary_plan()
        for i in range(10):
            if i not in (3, 4, 8):
                self.assertEqual(old['queries'][i], new['queries'][i])
            else:
                for key in ('expected', 'expected_details'):
                    self.assertEqual(old['queries'][i][key]['status'], 'Ambiguous')
                    self.assertEqual(new['queries'][i][key]['status'], 'Resolved')
                    self.assertEqual(len(new['queries'][i][key]['targets']), 1)
                    self.assertEqual(new['queries'][i][key]['targets'][0]['binary_name'], 'java/lang/String')
        self.assertEqual(old, gate.header_inventory_plan())

    def test_complete_rows_and_versions_do_not_accept_a_union(self):
        helper = previous.HeaderInventoryContract()
        old, new = gate.header_inventory_plan(), gate.effective_binary_plan()
        for details in (False, True):
            for plan, other in ((old, new), (new, old)):
                response = helper.response(plan, details)
                helper.check(response, plan, details)
                with self.assertRaises(gate.AcceptanceError):
                    helper.check(response, other, details)
                for i in range(10):
                    bad = copy.deepcopy(response)
                    bad['result']['queries'][i]['status'] = 'Wrong'
                    with self.assertRaises(gate.AcceptanceError):
                        helper.check(bad, plan, details)
                for key in ('binary_name', 'linking_context'):
                    bad = copy.deepcopy(response)
                    bad['result']['queries'][3]['targets'][0][key] = 'Wrong'
                    with self.assertRaises(gate.AcceptanceError):
                        helper.check(bad, plan, details)
                if details:
                    bad = copy.deepcopy(response)
                    bad['result']['queries'][3]['targets'][0]['descriptor'] = '()J'
                    with self.assertRaises(gate.AcceptanceError):
                        helper.check(bad, plan, details)

    def test_overlapping_legacy_q06_has_exact_separate_stages(self):
        queries = [["q06", "org/apache/commons/lang3/StringUtils.java", "15520", "length", "Resolved", "1"]]
        targets = [["q06", "!/classes/java/lang/String.class", "Method", "java.lang.String", "length", "0", "0"]]
        for current in (False, True):
            plan = gate.effective_binary_plan() if current else gate.header_inventory_plan()
            row = previous.HeaderInventoryContract().response(plan, False)['result']['queries'][8]
            row['query_index'] = 0
            response = {'result': {'index_builds': 1, 'queries': [row]}}
            gate.check_declarations(response, queries, targets, Path('/fixture'), Path('/java.base.jmod'), {}, effective_binary=current)
            with self.assertRaises(gate.AcceptanceError):
                gate.check_declarations(response, queries, targets, Path('/fixture'), Path('/java.base.jmod'), {}, effective_binary=not current)

    def test_q07_array_overload_exact_current_and_historical(self):
        queries = [["q07", "org/apache/commons/lang3/StringUtils.java", "44758", "isEmpty", "OverloadNotResolved", "9"]]
        origin = "org/apache/commons/lang3/ArrayUtils.java"
        starts = [128952, 129271, 129587, 129905, 130224, 130540, 130855, 131163, 131482]
        targets = [["q07", origin, "Method", "org.apache.commons.lang3.ArrayUtils", "isEmpty", str(x), str(x + 7)] for x in starts]
        def target(start):
            return dict(kind="Method", owner="org.apache.commons.lang3.ArrayUtils", name="isEmpty",
                        origin="source", source_available=True, path="/fixture/" + origin,
                        compilation=gate.COMPILATION,
                        range=dict(start_byte=start, end_byte=start+7,
                                   start_utf16=start, end_utf16=start+7))
        row = dict(query_index=0, compilation=gate.COMPILATION, source_compilation=gate.COMPILATION,
                   file="/fixture/" + queries[0][1], position=dict(encoding="utf8", offset=44758),
                   byte_offset=44758, status="Resolved", targets=[target(129587)])
        def check(value, current=True, expected=targets):
            gate.check_declarations({"result": {"index_builds": 1, "queries": [value]}},
                queries, expected, Path("/fixture"), Path("/java.base.jmod"),
                {origin: b" " * 132000}, effective_binary=current)
        check(row)
        old = copy.deepcopy(row)
        old.update(status="OverloadNotResolved", targets=[target(x) for x in starts])
        check(old, False)
        for value, current in ((row, False), (old, True)):
            with self.assertRaises(gate.AcceptanceError):
                check(value, current)
        # Other primitive/reference array overloads have the same name/owner,
        # but not the char[] declaration identity (including wrong rank stand-ins).
        for start in starts:
            if start == 129587:
                continue
            bad = copy.deepcopy(row)
            bad["targets"] = [target(start)]
            with self.assertRaises(gate.AcceptanceError):
                check(bad)
        for key, value in (("kind", "Field"), ("owner", "Wrong"), ("path", "/wrong"),
                           ("source_available", False), ("name", "other")):
            bad = copy.deepcopy(row)
            bad["targets"][0][key] = value
            with self.assertRaises(gate.AcceptanceError):
                check(bad)
        for key in ("start_byte", "end_byte", "start_utf16", "end_utf16"):
            bad = copy.deepcopy(row)
            bad["targets"][0]["range"][key] += 1
            with self.assertRaises(gate.AcceptanceError):
                check(bad)
        for i in range(9):
            bad = copy.deepcopy(targets)
            bad[i][5] = "1"
            with self.assertRaises(gate.AcceptanceError):
                check(row, expected=bad)

    def test_q26_own_static_exact_current_and_historical(self):
        queries = [["q26", "org/apache/commons/lang3/StringUtils.java", "259409", "repeat", "OverloadNotResolved", "3"]]
        origin = "org/apache/commons/lang3/StringUtils.java"
        starts = [256362, 257188, 259273]
        targets = [["q26", origin, "Method", "org.apache.commons.lang3.StringUtils", "repeat", str(x), str(x + 6)] for x in starts]
        def target(start):
            return dict(kind="Method", owner="org.apache.commons.lang3.StringUtils", name="repeat",
                        origin="source", source_available=True, path="/fixture/" + origin,
                        compilation=gate.COMPILATION,
                        range=dict(start_byte=start, end_byte=start+6,
                                   start_utf16=start, end_utf16=start+6))
        row = dict(query_index=0, compilation=gate.COMPILATION, source_compilation=gate.COMPILATION,
                   file="/fixture/" + queries[0][1], position=dict(encoding="utf8", offset=259409),
                   byte_offset=259409, status="Resolved", targets=[target(257188)])
        def check(value, current=True, expected=targets):
            gate.check_declarations({"result": {"index_builds": 1, "queries": [value]}},
                queries, expected, Path("/fixture"), Path("/java.base.jmod"),
                {origin: b" " * 260000}, effective_binary=current)
        check(row)
        old = copy.deepcopy(row)
        old.update(status="OverloadNotResolved", targets=[target(x) for x in starts])
        check(old, False)
        for value, current in ((row, False), (old, True)):
            with self.assertRaises(gate.AcceptanceError):
                check(value, current)
        # Other repeat overloads have the same owner/name but different declarations.
        for start in starts:
            if start == 257188:
                continue
            bad = copy.deepcopy(row)
            bad["targets"] = [target(start)]
            with self.assertRaises(gate.AcceptanceError):
                check(bad)
        for key, value in (("kind", "Field"), ("owner", "Wrong"), ("path", "/wrong"),
                           ("source_available", False), ("name", "other")):
            bad = copy.deepcopy(row)
            bad["targets"][0][key] = value
            with self.assertRaises(gate.AcceptanceError):
                check(bad)
        for key in ("start_byte", "end_byte", "start_utf16", "end_utf16"):
            bad = copy.deepcopy(row)
            bad["targets"][0]["range"][key] += 1
            with self.assertRaises(gate.AcceptanceError):
                check(bad)
        for i in range(3):
            bad = copy.deepcopy(targets)
            bad[i][5] = "1"
            with self.assertRaises(gate.AcceptanceError):
                check(row, expected=bad)

    def test_forecast_cannot_be_modified(self):
        prediction = gate.effective_binary_prediction()
        prediction['changes'][0]['expected']['targets'].clear()
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / 'changed.json'
            path.write_text(json.dumps(prediction))
            with self.assertRaises(gate.AcceptanceError):
                gate.effective_binary_prediction(path)

if __name__ == '__main__':
    unittest.main()

