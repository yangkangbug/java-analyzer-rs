"""Exact label-stage projection, historical isolation and complete-record mutation controls."""
import copy
import hashlib
import importlib.util
from pathlib import Path
import unittest
from unittest import mock
spec = importlib.util.spec_from_file_location('labels_contract', Path(__file__).with_name('labeled-statement-parser-contract.py'))
contract = importlib.util.module_from_spec(spec)
spec.loader.exec_module(contract)

class LabelsContractTests(unittest.TestCase):
    def test_exact_projection_retains_every_other_record_and_hash(self):
        for corpus, count in [('commons', 56), ('gson', 132)]:
            old, hashes = contract.switch.load(corpus)
            frozen = copy.deepcopy(old)
            new, new_hashes = contract.load(corpus)
            self.assertEqual(new_hashes, hashes)
            self.assertEqual(new['report_count'], count)
            changed = {path for path, _, _ in contract.CHANGES[corpus]}
            for a, b in zip(old['files'], new['files']):
                expected = copy.deepcopy(a)
                if a['path'] in changed: expected['reports'] = []
                self.assertEqual(b, expected)
            self.assertEqual(contract.switch.load(corpus)[0], frozen)
            self.assertEqual(old['files_with_reports'] - new['files_with_reports'], len(changed))

    def test_prior_complete_identity_and_diagnostics_are_required(self):
        for corpus in contract.CHANGES:
            old, hashes = contract.switch.load(corpus)
            path = contract.CHANGES[corpus][0][0]
            for operation in ['path', 'owner', 'message', 'classification', 'start_byte', 'end_byte', 'start_utf16', 'end_utf16', 'missing', 'extra', 'row_extra']:
                changed = copy.deepcopy(old)
                row = next(r for r in changed['files'] if r['path'] == path)
                if operation == 'path': row['path'] += '.wrong'
                elif operation == 'owner': row['compilation']['source_set'] = 'wrong'
                elif operation == 'missing': row['reports'].pop()
                elif operation == 'extra': row['reports'].append(copy.deepcopy(row['reports'][0]))
                elif operation == 'row_extra': row['extra'] = True
                elif operation in ['message', 'classification']: row['reports'][0][operation] += 'wrong'
                else: row['reports'][0]['range'][operation] += 1
                with self.subTest(corpus=corpus, operation=operation), mock.patch.object(contract.switch, 'load', return_value=(changed, hashes)), self.assertRaises(AssertionError):
                    contract.load(corpus)

    def test_complete_output_mutations_and_stage_swaps_fail(self):
        for corpus in contract.CHANGES:
            expected, _ = contract.load(corpus)
            for operation in ['old', 'baseline', 'message', 'range', 'file', 'count', 'affected', 'extra', 'semantic']:
                changed = copy.deepcopy(expected)
                row = next(r for r in changed['files'] if r['reports'])
                if operation == 'old': changed = contract.switch.load(corpus)[0]
                elif operation == 'baseline': changed = contract.switch.load(corpus, 'baseline')[0]
                elif operation == 'message': row['reports'][0]['message'] += 'wrong'
                elif operation == 'range': row['reports'][0]['range']['start_byte'] += 1
                elif operation == 'file': changed['files'].pop()
                elif operation == 'count': changed['report_count'] += 1
                elif operation == 'affected': changed['files_with_reports'] += 1
                elif operation == 'extra': changed['unexpected'] = True
                else: changed['java_semantic_diagnostics'] = True
                with self.subTest(corpus=corpus, operation=operation), self.assertRaisesRegex(AssertionError, 'complete labeled parser records differ'):
                    contract.check(corpus, changed, {})

    def test_complete_source_hash_inventory_is_checked(self):
        # Synthetic bytes exercise both successful check and hash mismatches;
        # real pinned bytes are independently checked by the underlying loader.
        expected, _ = contract.load('commons')
        sources = {'one.java': b'one', 'two.java': b'two'}
        hashes = {p: hashlib.sha256(b).hexdigest() for p, b in sources.items()}
        with mock.patch.object(contract, 'load', return_value=(expected, hashes)):
            contract.check('commons', expected, sources)
            for wrong in [{'one.java': b'one'}, dict(sources, extra=b'x'), dict(sources, **{'one.java': b'changed'})]:
                with self.assertRaisesRegex(AssertionError, 'complete labeled source inventory differs'):
                    contract.check('commons', expected, wrong)

    def test_historical_stage_names_are_not_reinterpreted(self):
        for stage in ['baseline', 'current', 'switch', 'candidate', 'auto']:
            with self.assertRaises(AssertionError): contract.load('commons', stage)
        with self.assertRaises(AssertionError): contract.load('other')

if __name__ == '__main__': unittest.main()
