"""Offline strict-phase oracle contracts. No JVM or native subprocess dependencies."""
import copy
import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('strict_oracle', Path(__file__).with_name('check-strict-reference-oracle.py'))
c = importlib.util.module_from_spec(spec)
spec.loader.exec_module(c)


class StrictReferenceOracleTests(unittest.TestCase):
    def evidence(self):
        ds, qs, _, expected = c.fixture()
        compiler, native = {}, {}
        for q, (status, selected, candidates) in expected.items():
            marker, owner = selected.split('@')
            path, _, _, name = ds[marker]
            start, end = c.compiler_range(ds, marker)
            compiler[q] = [path, 'METHOD', owner, name, str(start), str(end)]
            targets = [c.o.reference_target(ds, item, Path('/unused')) for item in candidates.split(',')]
            native[q] = [status, str(len(targets)), *targets]
        return ds, qs, expected, compiler, native

    def test_fixed_inventory(self):
        ds, qs, expected, _, _ = self.evidence()
        self.assertEqual(len(qs), 15)
        self.assertEqual(len(ds), 30)
        self.assertEqual(sum(row[0] == 'Resolved' for row in expected.values()), 6)
        self.assertEqual(len(list(c.invalid_fixtures())), 4)
        self.assertTrue(all(v[0] != 'Resolved' for v in c.baseline_expectations(expected).values()))
        self.assertEqual(expected['strict_wrapper'][0], 'OverloadNotResolved')
        self.assertEqual(expected['strict_wrapper'][1], 'wrapper_object@strict.Target')
        self.assertEqual(expected['strict_loose'][0], 'CallNotResolved')

    def test_current_loose_projection_is_exact(self):
        ds, qs, expected, compiler, native = self.evidence()
        current = c.current_expectations(expected, qs, ds)
        self.assertEqual(current['strict_loose'], ('Resolved', 'loose_scalar@strict.Target', 'loose_scalar@strict.Target'))
        self.assertEqual(expected['strict_loose'][0], 'CallNotResolved')
        for changed in [('Resolved', 'loose_scalar@strict.Target', 'loose_scalar@strict.Target'),
                        ('CallNotResolved', 'wrapper_scalar@strict.Target', 'loose_scalar@strict.Target'),
                        ('CallNotResolved', 'loose_scalar@strict.Target', 'wrapper_scalar@strict.Target')]:
            bad = dict(expected); bad['strict_loose'] = changed
            with self.assertRaises(AssertionError):
                c.current_expectations(bad, qs, ds)

        for q, (status, selected, candidates) in current.items():
            targets = [c.o.reference_target(ds, item, Path('/unused')) for item in candidates.split(',')]
            native[q] = [status, str(len(targets)), *targets]
        c.compare(current, qs, ds, compiler, native, Path('/unused'))
        for mutation in ['status', 'kind', 'owner', 'range']:
            bad = copy.deepcopy(native)
            if mutation == 'status': bad['strict_loose'][0] = 'CallNotResolved'
            elif mutation == 'kind': bad['strict_loose'][2] = bad['strict_loose'][2].replace('|Method|', '|Field|')
            elif mutation == 'owner': bad['strict_loose'][2] = bad['strict_loose'][2].replace('.Target|', '.Wrong|')
            else: bad['strict_loose'][2] += '0'
            with self.assertRaises(AssertionError):
                c.compare(current, qs, ds, compiler, bad, Path('/unused'))

    def test_inventory_mutations(self):
        for change in ('extra_file', 'missing_query', 'duplicate_query', 'extra_golden', 'missing_invalid', 'duplicate_golden'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp) / 'oracle'; shutil.copytree(c.ROOT, root)
                with patch.object(c, 'ROOT', root):
                    if change == 'extra_file':
                        (root / 'source/Extra.java').write_text('class Extra {}')
                    elif change in ('missing_query', 'duplicate_query'):
                        path = root / 'source/Use.java'
                        path.write_text(path.read_text().replace('/*Q:strict_identity*/', '' if change == 'missing_query' else '/*Q:strict_child*/'))
                    elif change in ('extra_golden', 'duplicate_golden'):
                        with (root / 'expected.tsv').open('a') as out:
                            out.write(('extra' if change == 'extra_golden' else 'strict_child') + '\tResolved\tparent@strict.Target\tparent@strict.Target\n')
                    else:
                        (root / 'invalid/duplicate/Use.java').unlink()
                    with self.assertRaises(AssertionError):
                        c.fixture(); list(c.invalid_fixtures())

    def test_exact_evidence_mutations(self):
        ds, qs, expected, compiler, native = self.evidence()
        def compare(j=compiler, n=native):
            c.compare(expected, qs, ds, j, n, Path('/unused'))
        compare()
        for field in range(6):
            changed = copy.deepcopy(compiler)
            changed['strict_child'][field] = 'wrong' if field < 4 else '0'
            with self.subTest(compiler_field=field), self.assertRaises((AssertionError, ValueError)):
                compare(j=changed)
        for q in expected:
            changed = copy.deepcopy(native)
            changed[q][0] = 'CallNotResolved' if expected[q][0] == 'Resolved' else 'Resolved'
            with self.subTest(status=q), self.assertRaises(AssertionError):
                compare(n=changed)
        for target in ('scalar@strict.Target', 'parent@wrong.Owner'):
            changed = copy.deepcopy(native)
            changed['strict_child'][2] = c.o.reference_target(ds, target, Path('/unused'))
            with self.assertRaises(AssertionError):
                compare(n=changed)
        for altered in ({}, {**native, 'extra': native['strict_child']}):
            with self.assertRaises(AssertionError): compare(n=altered)
        for altered in ({}, {**compiler, 'extra': compiler['strict_child']}):
            with self.assertRaises(AssertionError): compare(j=altered)
        for action in ('duplicate', 'omit', 'count'):
            changed = copy.deepcopy(native)
            if action == 'duplicate': changed['strict_wrapper'][3] = changed['strict_wrapper'][2]
            elif action == 'omit': changed['strict_wrapper'].pop()
            else: changed['strict_wrapper'][1] = '1'
            with self.subTest(action=action), self.assertRaises(AssertionError): compare(n=changed)
        # A too-broad tree range containing only one matching name is still invalid.
        changed = copy.deepcopy(compiler); changed['strict_child'][4] = str(int(changed['strict_child'][4]) - 1)
        with self.assertRaises(AssertionError): compare(j=changed)

    def test_current_parent_projection_preserves_historical_stages(self):
        ds, qs, expected, _, _ = self.evidence()
        original = copy.deepcopy(expected)
        baseline = c.baseline_expectations(expected)
        omitted = c.omitted_expectations(expected)
        prior = c.o.current_inherited_expectations(
            'strict-reference-v1',
            c.o.current_null_expectations('strict-reference-v1', expected, qs, ds), qs, ds)
        current = c.current_expectations(expected, qs, ds)
        self.assertEqual(expected, original)
        self.assertEqual(c.baseline_expectations(expected), baseline)
        self.assertEqual(c.omitted_expectations(expected), omitted)
        self.assertEqual({q for q in prior if current[q] != prior[q]}, {'strict_wrapper', 'strict_loose', 'strict_array'})
        self.assertEqual(current['strict_wrapper'],
                         ('Resolved', 'wrapper_object@strict.Target', 'wrapper_object@strict.Target'))
        self.assertEqual(current['strict_loose'], ('Resolved', 'loose_scalar@strict.Target', 'loose_scalar@strict.Target'))
        for replacement in [
            ('Resolved', prior['strict_wrapper'][1], prior['strict_wrapper'][2]),
            ('OverloadNotResolved', 'wrapper_scalar@strict.Target', prior['strict_wrapper'][2]),
            ('OverloadNotResolved', prior['strict_wrapper'][1], 'wrapper_object@strict.Target'),
        ]:
            changed = dict(prior); changed['strict_wrapper'] = replacement
            with patch.object(c.o, 'current_inherited_expectations', return_value=changed):
                with self.assertRaises(AssertionError):
                    c.current_expectations(expected, qs, ds)

    def test_current_wrapper_exact_evidence_controls(self):
        ds, qs, historical, compiler, _ = self.evidence()
        expected = c.current_expectations(historical, qs, ds)
        native = {}
        for q, (status, _, candidates) in expected.items():
            targets = [c.o.reference_target(ds, item, Path('/unused'))
                       for item in candidates.split(',')]
            native[q] = [status, str(len(targets)), *targets]
        def compare(n):
            c.compare(expected, qs, ds, compiler, n, Path('/unused'))
        compare(native)
        for action in ('status', 'count', 'scalar', 'owner', 'duplicate'):
            bad = copy.deepcopy(native); row = bad['strict_wrapper']
            if action == 'status': row[0] = 'OverloadNotResolved'
            elif action == 'count': row[1] = '2'
            elif action == 'scalar':
                row[2] = c.o.reference_target(ds, 'wrapper_scalar@strict.Target', Path('/unused'))
            elif action == 'owner': row[2] = row[2].replace('strict.Target', 'wrong.Owner')
            else: row[1] = '2'; row.append(row[2])
            with self.subTest(action=action), self.assertRaises(AssertionError):
                compare(bad)

    def test_invalid_cases_never_become_success(self):
        for case, status, candidates, _, ds, _ in c.invalid_fixtures():
            q = 'invalid_' + case
            targets = [c.o.reference_target(ds, item, Path('/unused')) for item in candidates.split(',')]
            native = [status, str(len(targets)), *targets]
            c.o.check_reference_candidates(q, status, candidates, ds, native, Path('/unused'))
            native[0] = 'Resolved'
            with self.subTest(case=case), self.assertRaises(AssertionError):
                c.o.check_reference_candidates(q, status, candidates, ds, native, Path('/unused'))

    def test_baseline_is_red_and_omission_refuses(self):
        ds, qs, expected, compiler, _ = self.evidence()
        old = c.baseline_expectations(expected)
        native = {}
        for q, (status, _, candidates) in old.items():
            targets = [c.o.reference_target(ds, item, Path('/unused')) for item in candidates.split(',')]
            native[q] = [status, str(len(targets)), *targets]
        c.compare(old, qs, ds, compiler, native, Path('/unused'))
        with self.assertRaises(AssertionError):
            c.compare(expected, qs, ds, compiler, native, Path('/unused'))
        omitted = c.omitted_expectations(expected)
        self.assertTrue(all(row[0] != 'Resolved' for row in omitted.values()))
        self.assertTrue(all(omitted[q][2] == old[q][2] for q in old))

    def test_platform_requires_digest_gate(self):
        with tempfile.TemporaryDirectory() as tmp:
            with patch.object(c.o, 'pinned_object', side_effect=AssertionError('checksum differs')) as gate:
                with patch.object(c.zipfile, 'ZipFile') as archive:
                    with self.assertRaisesRegex(AssertionError, 'checksum differs'):
                        c.pinned_platform(Path(tmp), Path(tmp))
                    gate.assert_called_once()
                    archive.assert_not_called()

    def test_jdk_requires_archive_digest_before_reading_archive(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'jdk.tar.gz'; path.write_bytes(b'not the pinned archive')
            with patch.object(c.tarfile, 'open') as archive:
                with self.assertRaisesRegex(AssertionError, 'JDK archive checksum differs'):
                    c.verify_jdk(Path(tmp), path)
                archive.assert_not_called()


    def test_array_projection_exact_identity_and_history(self):
        ds, qs, expected, compiler, _ = self.evidence()
        old = copy.deepcopy(expected)
        current = c.current_expectations(expected, qs, ds)
        self.assertEqual(expected, old)
        self.assertEqual(current['strict_array'], ('Resolved', 'array_parent@strict.Target', 'array_parent@strict.Target'))
        for field in range(3):
            saved = expected['strict_array']
            row = list(saved); row[field] = 'wrong'; expected['strict_array'] = tuple(row)
            with self.assertRaises(AssertionError):
                c.current_expectations(expected, qs, ds)
            expected['strict_array'] = saved
        native = {q: [status, str(0 if candidates == '-' else len(candidates.split(','))),
                   *([] if candidates == '-' else [c.o.reference_target(ds, item, Path('/unused')) for item in candidates.split(',')])]
                  for q, (status, _, candidates) in current.items()}
        c.compare(current, qs, ds, compiler, native, Path('/unused'))
        for mutation in ('status', 'count', 'kind', 'owner', 'range'):
            bad = copy.deepcopy(native)
            if mutation == 'status': bad['strict_array'][0] = 'CallNotResolved'
            elif mutation == 'count': bad['strict_array'][1] = '2'
            elif mutation == 'kind': bad['strict_array'][2] = bad['strict_array'][2].replace('|Method|', '|Field|')
            elif mutation == 'owner': bad['strict_array'][2] = bad['strict_array'][2].replace('.Target|', '.Wrong|')
            else: bad['strict_array'][2] += '0'
            with self.assertRaises((AssertionError, ValueError)):
                c.compare(current, qs, ds, compiler, bad, Path('/unused'))

if __name__ == '__main__':
    unittest.main()
