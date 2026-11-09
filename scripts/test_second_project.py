#!/usr/bin/env python3
"""Offline negative tests of the repository-authored second-project checker."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

SPEC = importlib.util.spec_from_file_location('second_project',
                                             Path(__file__).with_name('check-second-project.py'))
checker = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(checker)


class TargetValidationTests(unittest.TestCase):
    def setUp(self):
        self.fixture = checker.Fixture()
        self.fixture.source_root = Path('/fixture/sources')
        self.fixture.manifest = Path('/fixture/project.json')
        self.path = 'gson/src/main/java/com/example/Alpha.java'
        self.data = '// 😀\r\nclass Alpha {}\n'.encode()
        self.start = self.data.index(b'Alpha')
        self.fixture.sources = {self.path: self.data}
        self.fixture.owner = {self.path: 'gson/main'}
        self.fixture.inputs = {'junit': Path('/input/junit.jar')}
        self.query = {'id': 'exact-source', 'file': self.path, 'byte': self.start,
                      'compilation': checker.compilation('gson/main')}
        self.expected = {'status': 'Resolved', 'targets': [{'origin': 'source',
                         'path': self.path, 'kind': 'Class', 'owner': 'example.Alpha',
                         'name': 'Alpha', 'start': self.start, 'end': self.start + 5}]}
        self.row = {'query_index': 0, 'compilation': self.query['compilation'],
                    'source_compilation': self.query['compilation'],
                    'file': str(self.fixture.source_root / self.path),
                    'position': {'encoding': 'utf8', 'offset': self.start},
                    'byte_offset': self.start, 'status': 'Resolved',
                    'targets': [checker.expected_target(self.expected['targets'][0], self.fixture)]}

    def check(self, row):
        checker.check_query_row(row, self.query, self.expected, self.fixture, 0)

    def test_source_range_includes_utf16_and_crlf(self):
        self.check(self.row)
        extent = self.row['targets'][0]['range']
        self.assertEqual(extent['start_byte'] - 2, extent['start_utf16'])

    def test_mutated_target_fields_rejected(self):
        mutations = [lambda r: r['targets'][0]['range'].__setitem__('start_byte', self.start + 1),
                     lambda r: r['targets'][0]['range'].__setitem__('end_utf16', 0),
                     lambda r: r['targets'][0].__setitem__('path', '/wrong/source.java'),
                     lambda r: r['targets'][0].__setitem__('owner', 'wrong.Owner'),
                     lambda r: r['targets'][0].__setitem__('kind', 'Field'),
                     lambda r: r['targets'][0].__setitem__('compilation', checker.compilation('gson/test')),
                     lambda r: r.__setitem__('status', 'Unresolved'),
                     lambda r: r.__setitem__('source_compilation', checker.compilation('extras/main')),
                     lambda r: r.__setitem__('file', '/stale/file.java'),
                     lambda r: r.__setitem__('query_index', 1),
                     lambda r: r['targets'].append(copy.deepcopy(r['targets'][0]))]
        for mutate in mutations:
            with self.subTest(mutation=mutate):
                row = copy.deepcopy(self.row)
                mutate(row)
                with self.assertRaises(checker.AcceptanceError):
                    self.check(row)

    def test_binary_has_no_fabricated_range_and_exact_context(self):
        target = {'origin': 'binary', 'artifact': 'junit', 'binary_name': 'org/junit/Assert',
                  'name': 'Assert', 'owner': 'org.junit.Assert', 'kind': 'Class'}
        actual = checker.expected_target(target, self.fixture)
        self.assertNotIn('range', actual)
        self.assertFalse(actual['source_available'])
        self.assertEqual([checker.compilation('extras/test'), checker.compilation('gson/test')],
                         actual['loaded_contexts'])
        self.assertEqual('/input/junit.jar!/org/junit/Assert.class', actual['path'])

    def test_candidate_order_and_completeness_are_checked(self):
        second = copy.deepcopy(self.expected['targets'][0])
        second['kind'] = 'Method'
        self.expected['status'] = 'OverloadNotResolved'
        self.expected['targets'].append(second)
        row = copy.deepcopy(self.row)
        row['status'] = 'OverloadNotResolved'
        row['targets'].append(checker.expected_target(second, self.fixture))
        self.check(row)
        row['targets'].reverse()
        with self.assertRaises(checker.AcceptanceError):
            self.check(row)

    def test_plan_and_goldens_cover_all_predeclared_sites(self):
        plan = json.loads((checker.CORPUS / 'query-plan.json').read_text())
        goldens = json.loads((checker.CORPUS / 'expected.json').read_text())
        self.assertTrue(plan['selected_before_first_cli_analysis'])
        self.assertEqual(16, len(plan['queries']))
        self.assertEqual(16, len(goldens))
        self.assertEqual(16, len({q['id'] for q in plan['queries']}))
        self.assertEqual(11, sum(g['status'] == 'Resolved' for g in goldens))
        self.assertEqual([11, 5], [len(goldens[i]['targets']) for i in [11, 12]])


class SourceArchiveValidationTests(unittest.TestCase):
    def test_archive_validation_happens_before_source_writes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            archive = root / 'input.zip'
            with zipfile.ZipFile(archive, 'w') as z:
                z.writestr('p/LICENSE', 'fixture license')
                z.writestr('p/Good.java', 'class Good {}')
                z.writestr('p/../bad.java', 'class Bad {}')
            spec = {'kind': 'zip', 'root': 'p', 'required_notices': ['p/LICENSE'],
                    'max_entries': 10, 'max_file': 1000, 'max_expanded': 10000}
            destination = root / 'sources'
            destination.mkdir()
            with self.assertRaises(ValueError):
                checker.extract_sources(archive, spec, destination)
            self.assertEqual([], list(destination.iterdir()))
            self.assertFalse((root / 'notices').exists())

    def test_symlink_input_and_ancestor_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'real').mkdir()
            (root / 'alias').symlink_to(root / 'real', target_is_directory=True)
            with self.assertRaises(checker.AcceptanceError):
                checker.reject_symlinks(root / 'alias' / 'leaf')

    def test_noncanonical_archive_names_rejected(self):
        for name in ['../x', '/x', 'p//x', 'p/./x', 'p/../x', 'p\\x', 'p:x']:
            with self.subTest(name=name), self.assertRaises(checker.AcceptanceError):
                checker.safe_name(name)


if __name__ == '__main__':
    unittest.main()
