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

