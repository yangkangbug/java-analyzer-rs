"""Offline fail-closed tests of the sole original-Gson constructor correction."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location('constructor_second', Path(__file__).with_name('check-second-project.py'))
second = importlib.util.module_from_spec(spec)
spec.loader.exec_module(second)


class ConstructorCorrectionControls(unittest.TestCase):
    def setUp(self):
        self.fixture = second.Fixture()
        self.fixture.plan = json.loads((second.CORPUS / 'query-plan.json').read_text())['queries']
        self.fixture.expected = json.loads((second.CORPUS / 'expected.json').read_text())
        self.spec = json.loads((second.CORPUS / 'constructor-call-probe.json').read_text())
        self.fixture.metadata = {'inputs': {'sources': {'sha256': self.spec['source_archive_sha256']}}}
        self.historical = json.loads((second.CORPUS / 'local-type-comparison.json').read_text())
        target = self.spec['expected']['targets'][0]
        self.fixture.sources = {
            target['path']: b' ' * 3703 + b'add(JsonElement element)',
            self.spec['query']['file']: b' ' * 1237 + b'add(new JsonObject())',
        }
        self.hashes = mock.patch.object(second, 'all_source_hashes',
                                       return_value=self.historical['all_226_source_sha256'])
        self.hashes.start()
        self.addCleanup(self.hashes.stop)

    def test_exact_one_correction_without_mutating_any_historical_row(self):
        original = copy.deepcopy(self.fixture.expected)
        actual = second.constructor_expectations(self.fixture)
        self.assertEqual(original, self.fixture.expected)
        self.assertEqual([12], [i for i in range(16) if actual[i] != original[i]])
        self.assertEqual(self.spec['expected'], actual[12])
        self.assertEqual('OverloadNotResolved', original[12]['status'])
        self.assertEqual(5, len(original[12]['targets']))

    def test_old_row_changes_cannot_be_hidden_by_correction(self):
        for mutation in [lambda e: e.update(status='Resolved'),
                         lambda e: e['targets'].pop(),
                         lambda e: e['targets'].append(copy.deepcopy(e['targets'][0])),
                         lambda e: e['targets'][0].update(start=3703),
                         lambda e: e['targets'][4].update(owner='Wrong')]:
            original = copy.deepcopy(self.fixture.expected)
            mutation(self.fixture.expected[12])
            with self.assertRaises(second.AcceptanceError):
                second.constructor_expectations(self.fixture)
            self.fixture.expected = original

    def test_query_and_source_pin_changes_fail(self):
        for field, value in [('byte', 1238), ('id', 'different'), ('file', 'other.java')]:
            old = self.fixture.plan[12][field]
            self.fixture.plan[12][field] = value
            with self.assertRaises(second.AcceptanceError):
                second.constructor_expectations(self.fixture)
            self.fixture.plan[12][field] = old
        self.fixture.metadata['inputs']['sources']['sha256'] = 'changed'
        with self.assertRaises(second.AcceptanceError):
            second.constructor_expectations(self.fixture)

    def test_source_content_and_full_inventory_are_required(self):
        for path in self.fixture.sources:
            original = self.fixture.sources[path]
            self.fixture.sources[path] = original.replace(b'Json', b'Fake')
            with self.assertRaises(second.AcceptanceError):
                second.constructor_expectations(self.fixture)
            self.fixture.sources[path] = original
        for change in ['remove', 'add', 'modify']:
            inventory = copy.deepcopy(self.historical['all_226_source_sha256'])
            first = next(iter(inventory))
            if change == 'remove':
                del inventory[first]
            elif change == 'add':
                inventory['Extra.java'] = 'hash'
            else:
                inventory[first] = 'changed'
            with mock.patch.object(second, 'all_source_hashes', return_value=inventory):
                with self.assertRaises(second.AcceptanceError):
                    second.constructor_expectations(self.fixture)

    def test_independent_target_cannot_be_changed_with_old_candidate_membership(self):
        altered = copy.deepcopy(self.spec)
        altered['expected']['targets'][0] = copy.deepcopy(altered['baseline']['targets'][0])
        real_loads = json.loads
        def load(text, **kwargs):
            result = real_loads(text, **kwargs)
            return altered if result == self.spec else result
        with mock.patch.object(second.json, 'loads', side_effect=load):
            with self.assertRaises(second.AcceptanceError):
                second.constructor_expectations(self.fixture)

    def test_all_16_actual_rows_still_checked_and_old_binary_fails(self):
        expected = second.constructor_expectations(self.fixture)
        response = {'result': {'index_builds': 1, 'queries': copy.deepcopy(expected)}}
        def exact(row, query, wanted, fixture, index):
            second.same(row, wanted, 'actual complete row ' + str(index))
        with mock.patch.object(second, 'check_envelope'), \
                mock.patch.object(second, 'check_query_row', side_effect=exact):
            second.check_response('declaration', response, self.fixture)
            for i in range(16):
                changed = copy.deepcopy(response)
                changed['result']['queries'][i]['status'] = 'Unjustified'
                with self.subTest(row=i), self.assertRaises(second.AcceptanceError):
                    second.check_response('declaration', changed, self.fixture)
            old = {'result': {'index_builds': 1, 'queries': self.fixture.expected}}
            with self.assertRaises(second.AcceptanceError):
                second.check_response('declaration', old, self.fixture)
            second.check_response('declaration', old, self.fixture, historical_constructor=True)

    def test_frozen_inventory_provenance_and_historical_artifacts(self):
        self.assertEqual(226, len(self.historical['all_226_source_sha256']))
        self.assertEqual(211, len(self.historical['parser_after']))
        self.assertEqual(15, len(set(self.historical['all_226_source_sha256']) -
                                 set(self.historical['parser_after'])))
        for filename, checksum in self.spec['historical_artifact_sha256'].items():
            self.assertEqual(checksum, second.digest(second.CORPUS / filename))


if __name__ == '__main__':
    unittest.main()
