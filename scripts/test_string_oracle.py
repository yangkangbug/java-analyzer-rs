"""Offline oracle mutation controls. Never invokes Java or native binaries."""
import copy
import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('string_oracle', Path(__file__).with_name('check-string-oracle.py'))
c = importlib.util.module_from_spec(spec)
spec.loader.exec_module(c)


class StringOracleTests(unittest.TestCase):
    def test_fixed_inventory(self):
        ds, qs, _, expected = c.fixture()
        self.assertEqual(len(qs), 19)
        self.assertEqual(len(ds), 10)
        self.assertEqual(sum(row[0] == 'Resolved' for row in expected.values()), 9)
        self.assertEqual(len(list(c.invalid_fixtures())), 4)
        self.assertEqual(len(c.PLATFORM), 7)
        self.assertTrue(all(v[0] != 'Resolved' for v in c.baseline_expectations(expected).values()))

    def test_inventory_mutations(self):
        changes = ('extra_file', 'missing_query', 'duplicate_query', 'extra_golden', 'missing_invalid')
        for change in changes:
            with self.subTest(change=change), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp) / 'oracle'; shutil.copytree(c.ROOT, root)
                with patch.object(c, 'ROOT', root):
                    if change == 'extra_file':
                        (root / 'source/Extra.java').write_text('class Extra {}')
                    elif change == 'missing_query':
                        path = root / 'source/Use.java'
                        path.write_text(path.read_text().replace('/*Q:string_plain*/', ''))
                    elif change == 'duplicate_query':
                        path = root / 'source/Use.java'
                        path.write_text(path.read_text().replace('/*Q:string_empty*/', '/*Q:string_plain*/'))
                    elif change == 'extra_golden':
                        with (root / 'expected.tsv').open('a') as out:
                            out.write('extra\tResolved\texact@literal.Target\texact@literal.Target\n')
                    else:
                        (root / 'invalid/escape/Use.java').unlink()
                    with self.assertRaises(AssertionError):
                        c.fixture(); list(c.invalid_fixtures())

    def test_exact_evidence_mutations(self):
        ds, qs, _, expected = c.fixture()
        compiler, native = {}, {}
        for q, (status, selected, candidates) in expected.items():
            marker, owner = selected.split('@')
            path, start, end, name = ds[marker]
            start, end = c.compiler_range(ds, marker)
            compiler[q] = [path, 'METHOD', owner, name, str(start), str(end)]
            targets = [c.o.reference_target(ds, item, Path('/unused')) for item in candidates.split(',')]
            native[q] = [status, str(len(targets)), *targets]
        def compare(j=compiler, n=native):
            c.compare(expected, qs, ds, j, n, Path('/unused'))
        compare()
        for field in range(6):
            changed = copy.deepcopy(compiler)
            changed['string_plain'][field] = 'wrong' if field < 4 else '0'
            with self.subTest(compiler_field=field), self.assertRaises((AssertionError, ValueError)):
                compare(j=changed)
        for q in expected:
            changed = copy.deepcopy(native)
            changed[q][0] = 'CallNotResolved' if expected[q][0] == 'Resolved' else 'Resolved'
            with self.subTest(status=q), self.assertRaises(AssertionError):
                compare(n=changed)
        for target in ('pair@literal.Target', 'exact@wrong.Owner'):
            changed = copy.deepcopy(native)
            changed['string_plain'][2] = c.o.reference_target(ds, target, Path('/unused'))
            with self.assertRaises(AssertionError):
                compare(n=changed)
        for altered in ({}, {**native, 'extra': native['string_plain']}):
            with self.assertRaises(AssertionError):
                compare(n=altered)
        for altered in ({}, {**compiler, 'extra': compiler['string_plain']}):
            with self.assertRaises(AssertionError):
                compare(j=altered)
        changed = copy.deepcopy(native); changed['string_plain'][1] = '2'
        with self.assertRaises(AssertionError):
            compare(n=changed)
        changed = copy.deepcopy(native)
        changed['string_other'][3] = changed['string_other'][2]
        with self.assertRaises(AssertionError):
            compare(n=changed)

    def test_platform_requires_digest_gate(self):
        with tempfile.TemporaryDirectory() as tmp:
            with patch.object(c.o, 'pinned_object', side_effect=AssertionError('checksum differs')) as gate:
                with patch.object(c.zipfile, 'ZipFile') as archive:
                    with self.assertRaisesRegex(AssertionError, 'checksum differs'):
                        c.pinned_string(Path(tmp), Path(tmp))
                    gate.assert_called_once()
                    archive.assert_not_called()


if __name__ == '__main__':
    unittest.main()
