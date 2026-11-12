"""Frozen real-source old/new inventory stages; never a union of expectations."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('header_cli_gate', Path(__file__).with_name('check-cli-acceptance.py'))
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)

class HeaderInventoryContract(unittest.TestCase):
    def test_complete_history_and_original_predictions_are_retained(self):
        old = gate.binary_method_plan()
        new = gate.header_inventory_plan()
        original = json.loads((gate.CORPUS / 'binary-method-probes.json').read_text())
        self.assertEqual(new['queries'][3:], old['queries'][3:])
        for i in range(3):
            self.assertEqual(new['queries'][i]['expected'], original['queries'][i]['expected'])
            self.assertEqual(new['queries'][i]['expected_details'], original['queries'][i]['expected_details'])
            self.assertNotEqual(new['queries'][i]['expected'], old['queries'][i]['expected'])
        self.assertEqual(old, gate.binary_method_plan())

    def response(self, plan, details):
        key = 'expected_details' if details else 'expected'
        rows = copy.deepcopy([q[key] for q in plan['queries']])
        for row in rows:
            row['file'] = '/fixture/' + row['file']
            for target in row['targets']:
                target['path'] = target['path'].replace('@PLATFORM@', '/java.base.jmod')
        result = {'index_builds': 1, 'queries': rows}
        if details: result['target_details_version'] = 1
        return {'result': result}

    def check(self, response, plan, details):
        gate.check_binary_methods(response, plan, Path('/fixture'), Path('/java.base.jmod'), details=details)

    def test_exact_modes_stages_and_all_rows(self):
        old, new = gate.binary_method_plan(), gate.header_inventory_plan()
        for details in (False, True):
            for plan, wrong in ((old, new), (new, old)):
                response = self.response(plan, details)
                self.check(response, plan, details)
                with self.assertRaises(gate.AcceptanceError): self.check(response, wrong, details)
                for i in range(10):
                    mutated = copy.deepcopy(response); mutated['result']['queries'][i]['status'] = 'Wrong'
                    with self.assertRaises(gate.AcceptanceError): self.check(mutated, plan, details)
                if details:
                    for key in ('descriptor', 'linking_context', 'binary_name'):
                        mutated = copy.deepcopy(response)
                        mutated['result']['queries'][0]['targets'][0][key] = 'Wrong'
                        with self.assertRaises(gate.AcceptanceError): self.check(mutated, plan, details)

    def test_modified_prediction_is_rejected(self):
        prediction = gate.header_inventory_prediction()
        prediction['changes'][0]['expected']['status'] = 'OverloadNotResolved'
        with tempfile.TemporaryDirectory() as t:
            path = Path(t) / 'changed.json'; path.write_text(json.dumps(prediction))
            with self.assertRaises(gate.AcceptanceError): gate.header_inventory_prediction(path)

if __name__ == '__main__': unittest.main()
