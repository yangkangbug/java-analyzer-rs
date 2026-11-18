"""Current parser contract: exact delta, immutable history and fail-closed checks."""
import copy
import hashlib
import importlib.util
from pathlib import Path
import unittest
from unittest import mock
spec = importlib.util.spec_from_file_location('current_contract', Path(__file__).with_name('current-parser-contract.py'))
contract = importlib.util.module_from_spec(spec)
spec.loader.exec_module(contract)

class CurrentParserContractTests(unittest.TestCase):
    def test_exact_delta_and_history_unchanged(self):
        for corpus, count in [('commons', 0), ('gson', 30)]:
            before, hashes = contract.previous.load(corpus)
            frozen = copy.deepcopy(before)
            actual, actual_hashes = contract.load(corpus)
            self.assertEqual(actual_hashes, hashes)
            self.assertEqual(actual['report_count'], count)
            changes = {path for path, _ in contract.CHANGES[corpus]}
            for old, new in zip(before['files'], actual['files']):
                expected = copy.deepcopy(old)
                if old['path'] in changes: expected['reports'] = []
                invocation = next((c for c in contract.INVOCATION_CHANGES[corpus] if c['path'] == old['path']), None)
                if invocation:
                    self.assertEqual(expected['reports'], invocation['before'])
                    expected['reports'] = invocation['after']
                annotation = next((c for c in contract.ANNOTATION_CHANGES[corpus] if c['path'] == old['path']), None)
                if annotation:
                    self.assertEqual(expected['reports'], annotation['before'])
                    expected['reports'] = annotation['after']
                var = next((c for c in contract.VAR_CHANGES[corpus] if c['path'] == old['path']), None)
                if var:
                    self.assertEqual(expected['reports'], var['before'])
                    expected['reports'] = var['after']
                decimal = next((c for c in contract.DECIMAL_CHANGES[corpus] if c['path'] == old['path']), None)
                if decimal:
                    self.assertEqual(expected['reports'], decimal['before'])
                    expected['reports'] = decimal['after']
                qualified = next((c for c in contract.QUALIFIED_CREATION_CHANGES[corpus] if c['path'] == old['path']), None)
                if qualified:
                    self.assertEqual(expected['reports'], qualified['before'])
                    expected['reports'] = qualified['after']
                self.assertEqual(new, expected)
            self.assertEqual(contract.previous.load(corpus)[0], frozen)

    def test_prior_diagnostic_identity_cannot_drift(self):
        for corpus in ['commons', 'gson']:
            original, hashes = contract.previous.load(corpus)
            path = contract.CHANGES[corpus][0][0]
            for operation in ['path', 'owner', 'missing', 'extra', 'message', 'classification',
                              'start_byte', 'end_byte', 'start_utf16', 'end_utf16']:
                changed = copy.deepcopy(original)
                row = next(r for r in changed['files'] if r['path'] == path)
                if operation == 'path': row['path'] += '.wrong'
                elif operation == 'owner': row['compilation']['project'] = 'wrong'
                elif operation == 'missing': row['reports'].pop()
                elif operation == 'extra': row['unexpected'] = True
                elif operation in ['message', 'classification']: row['reports'][0][operation] += 'wrong'
                else: row['reports'][0]['range'][operation] += 1
                with self.subTest(corpus=corpus, operation=operation), mock.patch.object(contract.previous, 'load', return_value=(changed, hashes)), self.assertRaises(AssertionError):
                    contract.load(corpus)

    def test_invocation_prior_records_cannot_drift(self):
        for corpus in ['commons', 'gson']:
            original, hashes = contract.previous.load(corpus)
            path = contract.INVOCATION_CHANGES[corpus][0]['path']
            for operation in ['missing', 'message', 'position', 'owner']:
                changed = copy.deepcopy(original)
                row = next(r for r in changed['files'] if r['path'] == path)
                if operation == 'missing': row['reports'].pop()
                elif operation == 'message': row['reports'][0]['message'] += 'wrong'
                elif operation == 'position': row['reports'][0]['range']['start_byte'] += 1
                else: row['compilation']['project'] += 'wrong'
                with self.subTest(corpus=corpus, operation=operation), mock.patch.object(contract.previous, 'load', return_value=(changed, hashes)), self.assertRaises(AssertionError):
                    contract.load(corpus)

    def test_annotation_delta_is_exact_and_prior_records_cannot_drift(self):
        for corpus, files, reports in [('gson', 7, 32), ('commons', 5, 5)]:
            changes = contract.ANNOTATION_CHANGES[corpus]
            self.assertEqual(len(changes), files)
            self.assertEqual(sum(len(c['before']) for c in changes), reports)
            self.assertTrue(all(not c['after'] for c in changes))
            for change in changes:
                for operation in ['path', 'owner', 'missing', 'extra', 'message',
                                  'classification', 'start_byte', 'end_byte', 'start_utf16', 'end_utf16']:
                    original, hashes = contract.previous.load(corpus)
                    changed = copy.deepcopy(original)
                    row = next(r for r in changed['files'] if r['path'] == change['path'])
                    if operation == 'path': row['path'] += '.wrong'
                    elif operation == 'owner': row['compilation']['source_set'] += '.wrong'
                    elif operation == 'missing': row['reports'].pop()
                    elif operation == 'extra': row['reports'].append(copy.deepcopy(row['reports'][0]))
                    elif operation in ['message', 'classification']: row['reports'][0][operation] += '.wrong'
                    else: row['reports'][0]['range'][operation] += 1
                    with self.subTest(path=change['path'], operation=operation), mock.patch.object(contract.previous, 'load', return_value=(changed, hashes)), self.assertRaises(AssertionError):
                        contract.load(corpus)

    def test_contextual_var_delta_is_exact_and_cannot_drift(self):
        for corpus, suffix, reports in [('gson', 'reflect/TypeToken.java', 3),
                                       ('commons', 'reflect/TypeUtils.java', 29)]:
            changes = contract.VAR_CHANGES[corpus]
            self.assertEqual(len(changes), 1)
            self.assertTrue(changes[0]['path'].endswith(suffix))
            self.assertEqual(len(changes[0]['before']), reports)
            self.assertEqual(changes[0]['after'], [])
            for operation in ['path', 'owner', 'missing', 'extra', 'message',
                              'classification', 'start_byte', 'end_byte', 'start_utf16', 'end_utf16']:
                changed = copy.deepcopy(contract.VAR_CHANGES)
                row = changed[corpus][0]
                if operation == 'path': row['path'] += '.wrong'
                elif operation == 'owner': row['compilation']['source_set'] += '.wrong'
                elif operation == 'missing': row['before'].pop()
                elif operation == 'extra': row['before'].append(copy.deepcopy(row['before'][0]))
                elif operation in ['message', 'classification']: row['before'][0][operation] += '.wrong'
                else: row['before'][0]['range'][operation] += 1
                with self.subTest(corpus=corpus, operation=operation), mock.patch.object(contract, 'VAR_CHANGES', changed), self.assertRaises(AssertionError):
                    contract.load(corpus)

    def test_decimal_tail_delta_is_exact_and_cannot_drift(self):
        self.assertEqual(contract.DECIMAL_CHANGES['gson'], [])
        changes = contract.DECIMAL_CHANGES['commons']
        self.assertEqual(len(changes), 1)
        self.assertEqual(changes[0]['path'], 'org/apache/commons/lang3/JavaVersion.java')
        self.assertEqual(len(changes[0]['before']), 2)
        self.assertEqual(changes[0]['after'], [])
        for operation in ['path', 'owner', 'missing', 'extra', 'message',
                          'classification', 'start_byte', 'end_byte', 'start_utf16', 'end_utf16']:
            changed = copy.deepcopy(contract.DECIMAL_CHANGES)
            row = changed['commons'][0]
            if operation == 'path': row['path'] += '.wrong'
            elif operation == 'owner': row['compilation']['source_set'] += '.wrong'
            elif operation == 'missing': row['before'].pop()
            elif operation == 'extra': row['before'].append(copy.deepcopy(row['before'][0]))
            elif operation in ['message', 'classification']: row['before'][0][operation] += '.wrong'
            else: row['before'][0]['range'][operation] += 1
            with self.subTest(operation=operation), mock.patch.object(contract, 'DECIMAL_CHANGES', changed), self.assertRaises(AssertionError):
                contract.load('commons')

    def test_complete_current_output_and_source_hashes_checked(self):
        expected, _ = contract.load('gson')
        for operation in ['old', 'missing', 'count', 'range', 'message', 'extra']:
            changed = copy.deepcopy(expected)
            if operation == 'old': changed = contract.previous.load('gson')[0]
            elif operation == 'missing': changed['files'].pop()
            elif operation == 'count': changed['report_count'] += 1
            elif operation == 'extra': changed['unexpected'] = True
            else:
                row = next(r for r in changed['files'] if r['reports'])
                if operation == 'message': row['reports'][0]['message'] += 'wrong'
                else: row['reports'][0]['range']['start_byte'] += 1
            with self.subTest(operation=operation), self.assertRaisesRegex(AssertionError, 'complete current parser records differ'):
                contract.check('gson', changed, {})
        sources = {'A.java': b'one'}
        hashes = {path: hashlib.sha256(data).hexdigest() for path, data in sources.items()}
        with mock.patch.object(contract, 'load', return_value=(expected, hashes)):
            contract.check('gson', expected, sources)
            for wrong in [{}, {'A.java': b'changed'}, dict(sources, extra=b'two')]:
                with self.assertRaisesRegex(AssertionError, 'source inventory differs'):
                    contract.check('gson', expected, wrong)

    def test_current_entry_never_reinterprets_historical_names(self):
        for stage in ['baseline', 'current', 'switch', 'labels', 'auto']:
            with self.assertRaises(AssertionError): contract.load('gson', stage)


    def test_qualified_creation_delta_is_exact_and_cannot_drift(self):
        self.assertEqual(contract.QUALIFIED_CREATION_CHANGES['commons'], [])
        changes = contract.QUALIFIED_CREATION_CHANGES['gson']
        self.assertEqual(len(changes), 2)
        self.assertEqual(sorted(len(c['before']) for c in changes), [4, 10])
        self.assertTrue(all(not c['after'] for c in changes))
        for index in range(2):
            for operation in ['path', 'owner', 'missing', 'extra', 'message',
                              'classification', 'start_byte', 'end_byte', 'start_utf16', 'end_utf16']:
                changed = copy.deepcopy(contract.QUALIFIED_CREATION_CHANGES)
                row = changed['gson'][index]
                if operation == 'path': row['path'] += '.wrong'
                elif operation == 'owner': row['compilation']['source_set'] += '.wrong'
                elif operation == 'missing': row['before'].pop()
                elif operation == 'extra': row['before'].append(copy.deepcopy(row['before'][0]))
                elif operation in ['message', 'classification']: row['before'][0][operation] += '.wrong'
                else: row['before'][0]['range'][operation] += 1
                with self.subTest(index=index, operation=operation), mock.patch.object(contract, 'QUALIFIED_CREATION_CHANGES', changed), self.assertRaises(AssertionError):
                    contract.load('gson')
