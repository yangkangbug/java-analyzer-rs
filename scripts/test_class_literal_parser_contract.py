"""Offline exact-stage controls for the additive primitive class-literal parser contract."""
import copy
import importlib.util
import json
import tempfile
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location('class_literal_second', Path(__file__).with_name('check-second-project.py'))
second = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(second)


class ParserStageControls(unittest.TestCase):
    def setUp(self):
        self.fixture = SimpleNamespace(source_root=Path('/fixture'))
        self.old = {'files': [{'path': '/fixture/A.java', 'reports': [{'message': 'old'}] * 331}],
                    'report_count': 331, 'files_with_reports': 58}
        self.current = {'files': [{'path': '/fixture/A.java', 'reports': [{
            'message': 'expected expression',
            'range': {'start_byte': 1, 'end_byte': 2, 'start_utf16': 1, 'end_utf16': 2}}] * 166}],
                        'report_count': 166, 'files_with_reports': 44}
        self.historical = {'parser_after': {'A.java': 331}}
        self.old_digest = second.parser_result_digest(self.old, self.fixture)
        self.current_digest = second.parser_result_digest(self.current, self.fixture)

    def check(self, parser, stage='current'):
        # These synthetic records isolate the exact inventory/digest gate.
        # The production check_parser and contract loader validate full sources separately.
        with mock.patch.object(second, 'check_parser'), mock.patch.object(
                second, 'class_literal_parser_contract', create=True,
                return_value=({'A.java': 166}, self.current_digest)):
            return second.check_reference_parser(parser, self.fixture, self.historical, self.old_digest, stage=stage)

    def test_stages_are_explicit_and_not_old_or_new_alternatives(self):
        self.check(self.current)
        self.check(self.old, 'baseline')
        for parser, stage in [(self.old, 'current'), (self.current, 'baseline'), (self.current, 'expected')]:
            with self.subTest(stage=stage), self.assertRaises(second.AcceptanceError):
                self.check(parser, stage)

    def test_current_message_range_count_and_file_mutations_fail(self):
        for mutation in ['message', 'start_byte', 'end_byte', 'start_utf16', 'end_utf16',
                         'count', 'affected', 'missing_file', 'extra_file', 'extra_report']:
            parser = copy.deepcopy(self.current)
            if mutation == 'message':
                parser['files'][0]['reports'][0]['message'] = 'different'
            elif mutation in ['start_byte', 'end_byte', 'start_utf16', 'end_utf16']:
                parser['files'][0]['reports'][0]['range'][mutation] += 1
            elif mutation == 'count':
                parser['report_count'] += 1
            elif mutation == 'affected':
                parser['files_with_reports'] += 1
            elif mutation == 'missing_file':
                parser['files'].pop()
            elif mutation == 'extra_file':
                parser['files'].append({'path': '/fixture/extra.java', 'reports': []})
            else:
                parser['files'][0]['reports'].append({'message': 'extra'})
            with self.subTest(mutation=mutation), self.assertRaises(second.AcceptanceError):
                self.check(parser)


class ContractArtifactControls(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        for name in ['class-literal-parser-contract.json', 'class-literal-parser-comparison.tsv',
                     'reference-call-probes.json', 'local-type-comparison.json']:
            (self.root / name).write_bytes((second.CORPUS / name).read_bytes())
        self.spec = json.loads((self.root / 'class-literal-parser-contract.json').read_text())
        self.fixture = SimpleNamespace(metadata={'inputs': {'sources': {
            'sha256': self.spec['source_archive_sha256']}}})
        self.historical = json.loads((self.root / 'local-type-comparison.json').read_text())
        self.old_digest = self.spec['baseline']['parser_result_canonical_sha256']

    def check(self):
        with mock.patch.object(second, 'CORPUS', self.root):
            return second.class_literal_parser_contract(self.fixture, self.historical, self.old_digest)

    def test_committed_contract_has_exact_complete_current_inventory(self):
        inventory, digest = self.check()
        self.assertEqual(len(inventory), 211)
        self.assertEqual(sum(inventory.values()), 166)
        self.assertEqual(sum(bool(n) for n in inventory.values()), 44)
        self.assertNotEqual(digest, self.old_digest)

    def test_complete_comparison_rejects_missing_extra_duplicate_reordered_and_invalid_rows(self):
        path = self.root / 'class-literal-parser-comparison.tsv'
        original = path.read_text().splitlines()
        headers = [line for line in original if line.startswith('#')]
        original = [line for line in original if not line.startswith('#')]
        for mutation in ['missing', 'extra', 'duplicate', 'reordered', 'negative', 'fraction',
                         'leading_zero', 'wrong_before', 'old_as_current']:
            rows = original.copy()
            if mutation == 'missing':
                rows.pop()
            elif mutation in ['extra', 'duplicate']:
                rows.append('extra.java\t0\t0' if mutation == 'extra' else rows[0])
            elif mutation == 'reordered':
                rows[0], rows[1] = rows[1], rows[0]
            elif mutation == 'old_as_current':
                rows = ['\t'.join([name, old, old]) for name, old, _ in (row.split('\t') for row in rows)]
            else:
                columns = rows[0].split('\t')
                if mutation == 'wrong_before':
                    columns[1] = str(int(columns[1]) + 1)
                else:
                    columns[2] = {'negative': '-1', 'fraction': '0.5', 'leading_zero': '00'}[mutation]
                rows[0] = '\t'.join(columns)
            path.write_text('\n'.join(headers + rows) + '\n')
            spec = copy.deepcopy(self.spec)
            # Even an updated byte pin cannot relax inventory/schema constraints.
            spec['comparison_sha256'] = second.digest(path)
            (self.root / 'class-literal-parser-contract.json').write_text(json.dumps(spec))
            with self.subTest(mutation=mutation), self.assertRaises(second.AcceptanceError):
                self.check()

    def test_historical_bytes_archive_and_comparison_pin_are_mandatory(self):
        for name in ['reference-call-probes.json', 'local-type-comparison.json',
                     'class-literal-parser-comparison.tsv']:
            path = self.root / name
            original = path.read_bytes()
            path.write_bytes(original + b' ')
            with self.subTest(name=name), self.assertRaises(second.AcceptanceError):
                self.check()
            path.write_bytes(original)
        self.fixture.metadata['inputs']['sources']['sha256'] = '0' * 64
        with self.assertRaises(second.AcceptanceError):
            self.check()


if __name__ == '__main__':
    unittest.main()
