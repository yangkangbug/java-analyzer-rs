"""Offline oracle evidence checks. No JDK or native process is started."""
import copy
import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('qualified_oracle_test', Path(__file__).with_name('check-qualified-primitive-oracle.py'))
c = importlib.util.module_from_spec(spec)
spec.loader.exec_module(c)


class QualifiedOracleTests(unittest.TestCase):
    def evidence(self):
        ds, qs, _, expected = c.fixture()
        compiler, native = {}, {}
        for q, (status, selected, candidates) in expected.items():
            marker, owner = selected.split('@'); path, _, _, name = ds[marker]
            start, end = c.compiler_range(ds, marker)
            compiler[q] = [path, 'METHOD', owner, name, str(start), str(end)]
            targets = [c.o.reference_target(ds, item, Path('/unused')) for item in candidates.split(',')]
            native[q] = [status, str(len(targets)), *targets]
        return ds, qs, expected, compiler, native

    def test_current_boxing_projection_is_exact(self):
        ds, qs, expected, compiler, native = self.evidence()
        current = c.current_expectations(expected, qs, ds)
        self.assertEqual(current['primitive_boxed'], ('Resolved', 'boxed@qualified.Target', 'boxed@qualified.Target'))
        self.assertEqual(expected['primitive_boxed'][0], 'CallNotResolved')
        for changed in [('Resolved', 'boxed@qualified.Target', 'boxed@qualified.Target'),
                        ('CallNotResolved', 'set@qualified.Target', 'boxed@qualified.Target'),
                        ('CallNotResolved', 'boxed@qualified.Target', 'set@qualified.Target')]:
            bad = dict(expected); bad['primitive_boxed'] = changed
            with self.assertRaises(AssertionError):
                c.current_expectations(bad, qs, ds)

        for q, (status, selected, candidates) in current.items():
            targets = [c.o.reference_target(ds, item, Path('/unused')) for item in candidates.split(',')]
            native[q] = [status, str(len(targets)), *targets]
        c.compare(current, qs, ds, compiler, native, Path('/unused'))
        for mutation in ['status', 'kind', 'owner', 'range']:
            bad = copy.deepcopy(native)
            if mutation == 'status': bad['primitive_boxed'][0] = 'CallNotResolved'
            elif mutation == 'kind': bad['primitive_boxed'][2] = bad['primitive_boxed'][2].replace('|Method|', '|Field|')
            elif mutation == 'owner': bad['primitive_boxed'][2] = bad['primitive_boxed'][2].replace('.Target|', '.Wrong|')
            else: bad['primitive_boxed'][2] += '0'
            with self.assertRaises(AssertionError):
                c.compare(current, qs, ds, compiler, bad, Path('/unused'))

    def test_complete_fixed_inventory(self):
        ds, qs, expected, compiler, native = self.evidence()
        self.assertEqual((len(qs), len(ds)), (14, 18))
        self.assertEqual(sum(v[0] == 'Resolved' for v in expected.values()), 7)
        self.assertEqual(len(list(c.invalid_fixtures())), 2)
        c.compare(expected, qs, ds, compiler, native, Path('/unused'))

    def test_missing_extra_duplicate_inputs_are_rejected(self):
        for change in ('extra_file', 'missing_query', 'duplicate_query', 'extra_golden', 'duplicate_golden', 'missing_invalid'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp) / 'oracle'; shutil.copytree(c.ROOT, root)
                with patch.object(c, 'ROOT', root):
                    if change == 'extra_file': (root / 'source/Extra.java').write_text('class Extra {}')
                    elif change in ('missing_query', 'duplicate_query'):
                        p = root / 'source/Use.java'; p.write_text(p.read_text().replace('/*Q:primitive_int*/', '' if change == 'missing_query' else '/*Q:primitive_byte*/'))
                    elif change in ('extra_golden', 'duplicate_golden'):
                        with (root / 'expected.tsv').open('a') as out:
                            out.write(('extra' if change == 'extra_golden' else 'primitive_literal') + '\tResolved\tpick_int@qualified.Target\tpick_int@qualified.Target\n')
                    else: (root / 'invalid/narrow/Use.java').unlink()
                    with self.assertRaises(AssertionError): c.fixture(); list(c.invalid_fixtures())

    def test_exact_compiler_and_native_records(self):
        ds, qs, expected, compiler, native = self.evidence()
        def check(j=compiler, n=native): c.compare(expected, qs, ds, j, n, Path('/unused'))
        for field in range(6):
            changed = copy.deepcopy(compiler); changed['primitive_literal'][field] = 'wrong' if field < 4 else '0'
            with self.assertRaises((AssertionError, ValueError)): check(j=changed)
        for q in expected:
            changed = copy.deepcopy(native); changed[q][0] = 'Wrong'
            with self.assertRaises(AssertionError): check(n=changed)
        for altered in ({}, {**native, 'extra': native['primitive_int']}):
            with self.assertRaises(AssertionError): check(n=altered)
        for altered in ({}, {**compiler, 'extra': compiler['primitive_int']}):
            with self.assertRaises(AssertionError): check(j=altered)
        for change in ('duplicate', 'omit', 'count', 'wrong_target'):
            n = copy.deepcopy(native)
            if change == 'duplicate': n['primitive_unknown'][3] = n['primitive_unknown'][2]
            elif change == 'omit': n['primitive_unknown'].pop()
            elif change == 'count': n['primitive_unknown'][1] = '1'
            else: n['primitive_literal'][2] = c.o.reference_target(ds, 'pick_long@qualified.Target', Path('/unused'))
            with self.assertRaises(AssertionError): check(n=n)

    def test_baseline_is_not_current_success(self):
        ds, qs, expected, compiler, _ = self.evidence(); old = c.baseline_expectations(expected)
        native = {}
        for q, (status, _, candidates) in old.items():
            targets = [c.o.reference_target(ds, item, Path('/unused')) for item in candidates.split(',')]
            native[q] = [status, str(len(targets)), *targets]
        c.compare(old, qs, ds, compiler, native, Path('/unused'))
        self.assertTrue(all(v[0] != 'Resolved' for v in old.values()))
        with self.assertRaises(AssertionError): c.compare(expected, qs, ds, compiler, native, Path('/unused'))

    def test_invalid_programs_keep_exact_native_refusals(self):
        for case, status, candidates, _, ds, _ in c.invalid_fixtures():
            q = 'invalid_' + case
            targets = [c.o.reference_target(ds, item, Path('/unused')) for item in candidates.split(',')]
            result = [status, str(len(targets)), *targets]
            c.o.check_reference_candidates(q, status, candidates, ds, result, Path('/unused'))
            result[0] = 'Resolved'
            with self.assertRaises(AssertionError): c.o.check_reference_candidates(q, status, candidates, ds, result, Path('/unused'))


    def test_own_instance_current_projection_is_exact_and_baseline_unchanged(self):
        ds, qs, _, original = c.fixture()
        frozen = copy.deepcopy(original)
        baseline = c.baseline_expectations(original)
        prior = c.o.current_inherited_expectations('qualified-primitive-v1', c.o.current_null_expectations('qualified-primitive-v1', original, qs, ds), qs, ds)
        current = c.current_expectations(original, qs, ds)
        self.assertEqual(original, frozen)
        self.assertEqual(c.baseline_expectations(original), baseline)
        self.assertEqual({q for q in prior if prior[q] != current[q]}, {'primitive_unknown', 'primitive_boxed'})
        self.assertEqual(current['primitive_unknown'], ('Resolved', 'pick_int@qualified.Target', 'pick_int@qualified.Target'))
        for field in range(3):
            bad = dict(original)
            row = list(bad['primitive_unknown']); row[field] = 'wrong'; bad['primitive_unknown'] = tuple(row)
            with self.assertRaises(AssertionError): c.current_expectations(bad, qs, ds)
        compiler, native = {}, {}
        for name, (status, target, candidates) in current.items():
            marker, owner = target.split('@'); path, _, _, method = ds[marker]
            start, end = c.compiler_range(ds, marker)
            compiler[name] = [path, 'METHOD', owner, method, str(start), str(end)]
            targets = [c.o.reference_target(ds, x, Path('/unused')) for x in candidates.split(',')]
            native[name] = [status, str(len(targets)), *targets]
        c.compare(current, qs, ds, compiler, native, Path('/unused'))
        for mutation in ['status', 'count', 'target', 'owner', 'range']:
            bad = copy.deepcopy(native)
            if mutation == 'status': bad['primitive_unknown'][0] = 'OverloadNotResolved'
            elif mutation == 'count': bad['primitive_unknown'][1] = '2'
            elif mutation == 'target': bad['primitive_unknown'][2] = c.o.reference_target(ds, 'pick_long@qualified.Target', Path('/unused'))
            elif mutation == 'owner': bad['primitive_unknown'][2] = bad['primitive_unknown'][2].replace('qualified.Target', 'wrong.Owner')
            else: bad['primitive_unknown'][2] += '0'
            with self.subTest(mutation=mutation), self.assertRaises(AssertionError):
                c.compare(current, qs, ds, compiler, bad, Path('/unused'))


if __name__ == '__main__':
    unittest.main()
