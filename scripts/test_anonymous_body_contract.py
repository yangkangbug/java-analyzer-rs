"""Offline fail-closed controls for the explicitly documented capture regression."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('anonymous_second', Path(__file__).with_name('check-second-project.py'))
second = importlib.util.module_from_spec(spec)
spec.loader.exec_module(second)


class AnonymousBodyContract(unittest.TestCase):
    def setUp(self):
        self.old = json.loads((second.CORPUS / 'local-type-probes.json').read_text())['queries']
        self.current = second.current_local_type_probes(self.old)
        self.fixture = second.Fixture()
        self.fixture.source_root = Path('/fixture/sources')
        path = self.old[0]['file']
        self.fixture.sources = {path: b' ' * 6500}
        self.fixture.owner = {path: 'gson/test'}

    def response(self, probes):
        rows = []
        for i, q in enumerate(probes):
            rows.append({'query_index': i, 'compilation': q['compilation'],
                         'source_compilation': q['compilation'],
                         'file': str(self.fixture.source_root / q['file']),
                         'position': {'encoding': 'utf8', 'offset': q['byte']},
                         'byte_offset': q['byte'], 'status': q['expected']['status'],
                         'targets': [second.expected_target(t, self.fixture)
                                     for t in q['expected']['targets']]})
        return {'result': {'queries': rows}}

    def test_only_two_declared_regressions_and_no_mutation(self):
        original = copy.deepcopy(self.old)
        result = second.current_local_type_probes(self.old)
        self.assertEqual(original, self.old)
        self.assertEqual([q['id'] for q, r in zip(self.old, result) if q != r],
                         ['thread_adapter_capture', 'adapter_counter_capture'])
        self.assertEqual(self.old[2:], result[2:])

    def test_changed_old_identity_or_missing_record_is_rejected(self):
        bad = copy.deepcopy(self.old)
        bad[0]['expected']['targets'][0]['start'] += 1
        for rows in [bad, self.old[1:], self.old + [copy.deepcopy(self.old[0])], self.current]:
            with self.assertRaises(second.AcceptanceError):
                second.current_local_type_probes(rows)

    def test_historical_and_current_results_are_not_interchangeable(self):
        for expected in [self.old, self.current]:
            second.check_local_type_response(self.response(expected), expected, self.fixture)
        for actual, expected in [(self.old, self.current), (self.current, self.old)]:
            with self.assertRaises(second.AcceptanceError):
                second.check_local_type_response(self.response(actual), expected, self.fixture)

    def test_additional_outside_refusal_is_rejected(self):
        bad = self.response(self.current)
        bad['result']['queries'][2].update(status='Unresolved', targets=[])
        with self.assertRaises(second.AcceptanceError):
            second.check_local_type_response(bad, self.current, self.fixture)
