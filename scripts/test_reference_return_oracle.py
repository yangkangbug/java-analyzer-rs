"""Offline oracle evidence checks. No JDK or native process is started."""
import copy
import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('mixed_oracle_test', Path(__file__).with_name('check-reference-return-oracle.py'))
c = importlib.util.module_from_spec(spec)
spec.loader.exec_module(c)


class ReturnOracleTests(unittest.TestCase):
    def evidence(self):
        ds, qs, _, expected = c.fixture()
        compiler, native = {}, {}
        for q, (status, selected, candidates) in expected.items():
            marker, owner = selected.split('@'); path, _, _, name = ds[marker]
            start, end = c.compiler_range(ds, marker)
            compiler[q] = [path, 'METHOD', owner, name, str(start), str(end)]
            targets = [] if candidates == '-' else [c.o.reference_target(ds, item, Path('/unused')) for item in candidates.split(',')]
            native[q] = [status, str(len(targets)), *targets]
        return ds, qs, expected, compiler, native

    def test_complete_fixed_inventory(self):
        ds, qs, expected, compiler, native = self.evidence()
        self.assertEqual((len(qs), len(ds)), (7, 8))
        self.assertEqual(sum(v[0] == 'Resolved' for v in expected.values()), 2)
        self.assertEqual(len(list(c.invalid_fixtures())), 1)
        self.assertEqual(sum(line.startswith('Q\t') for line in c.valid_refusal_fixture().splitlines()), 3)
        c.compare(expected, qs, ds, compiler, native, Path('/unused'))

    def test_missing_extra_duplicate_inputs_are_rejected(self):
        for change in ('extra_file', 'missing_query', 'duplicate_query', 'extra_golden', 'duplicate_golden', 'missing_invalid', 'extra_valid'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp) / 'oracle'; shutil.copytree(c.ROOT, root)
                with patch.object(c, 'ROOT', root):
                    if change == 'extra_file': (root / 'source/Extra.java').write_text('class Extra {}')
                    elif change in ('missing_query', 'duplicate_query'):
                        p = root / 'source/Use.java'; p.write_text(p.read_text().replace('/*Q:return_direct*/', '' if change == 'missing_query' else '/*Q:return_parens*/'))
                    elif change in ('extra_golden', 'duplicate_golden'):
                        with (root / 'expected.tsv').open('a') as out:
                            out.write(('extra' if change == 'extra_golden' else 'return_direct') + '\tResolved\tproduct_value@model.Product\tproduct_value@model.Product\n')
                    elif change == 'extra_valid': (root / 'valid/concat/Extra.java').write_text('class Extra {}')
                    else: (root / 'invalid/unknown/Use.java').unlink()
                    with self.assertRaises(AssertionError): c.fixture(); list(c.invalid_fixtures()); c.valid_refusal_fixture()

    def test_exact_compiler_and_native_records(self):
        ds, qs, expected, compiler, native = self.evidence()
        def check(j=compiler, n=native): c.compare(expected, qs, ds, j, n, Path('/unused'))
        for field in range(6):
            changed = copy.deepcopy(compiler); changed['return_direct'][field] = 'wrong' if field < 4 else '0'
            with self.assertRaises((AssertionError, ValueError)): check(j=changed)
        for q in expected:
            changed = copy.deepcopy(native); changed[q][0] = 'Wrong'
            with self.assertRaises(AssertionError): check(n=changed)
        for altered in ({}, {**native, 'extra': native['return_direct']}):
            with self.assertRaises(AssertionError): check(n=altered)
        for altered in ({}, {**compiler, 'extra': compiler['return_direct']}):
            with self.assertRaises(AssertionError): check(j=altered)
        for change in ('duplicate', 'omit', 'count', 'wrong_target'):
            n = copy.deepcopy(native)
            if change == 'duplicate': n['return_direct'].append(n['return_direct'][2])
            elif change == 'omit': n['return_direct'].pop()
            elif change == 'count': n['return_unknown_inner'][1] = '1'
            else: n['return_direct'][2] = c.o.reference_target(ds, 'shadow_value@shadow.Product', Path('/unused'))
            with self.assertRaises(AssertionError): check(n=n)

    def test_baseline_is_not_current_success(self):
        ds, qs, expected, compiler, _ = self.evidence(); old = c.baseline_expectations(expected)
        native = {}
        for q, (status, _, candidates) in old.items():
            targets = [] if candidates == '-' else [c.o.reference_target(ds, item, Path('/unused')) for item in candidates.split(',')]
            native[q] = [status, str(len(targets)), *targets]
        c.compare(old, qs, ds, compiler, native, Path('/unused'))
        self.assertTrue(all(v[0] != 'Resolved' for v in old.values()))
        with self.assertRaises(AssertionError): c.compare(expected, qs, ds, compiler, native, Path('/unused'))

    def test_invalid_programs_keep_exact_native_refusals(self):
        for case, status, candidates, _, ds, _ in c.invalid_fixtures():
            q = 'invalid_' + case
            targets = [] if candidates == '-' else [c.o.reference_target(ds, item, Path('/unused')) for item in candidates.split(',')]
            result = [status, str(len(targets)), *targets]
            c.o.check_reference_candidates(q, status, candidates, ds, result, Path('/unused'))
            result[0] = 'Resolved'
            with self.assertRaises(AssertionError): c.o.check_reference_candidates(q, status, candidates, ds, result, Path('/unused'))


if __name__ == '__main__':
    unittest.main()
