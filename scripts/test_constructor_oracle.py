"""Offline constructor proof checker controls: never starts JVM or native tools."""
import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('constructor_oracle', Path(__file__).with_name('check-constructor-oracle.py'))
c = importlib.util.module_from_spec(spec)
spec.loader.exec_module(c)


class ConstructorOracleTests(unittest.TestCase):
    def test_fixed_inventory(self):
        ds, qs, _, expected = c.fixture()
        self.assertEqual(len(qs), 19)
        self.assertEqual(sum(row[0] == 'Resolved' for row in expected.values()), 6)
        self.assertEqual(len(list(c.invalid_fixtures())), 5)
        self.assertEqual(len(ds), 11)

    def test_inventory_changes_rejected(self):
        import shutil
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / 'oracle'
            shutil.copytree(c.ROOT, root)
            with patch.object(c, 'ROOT', root):
                (root / 'source/ExtraFile.java').write_text('class ExtraFile {}')
                with self.assertRaises(AssertionError):
                    c.fixture()

    def test_exact_compiler_and_native_targets(self):
        ds, qs, _, expected = c.fixture()
        compiler, native = {}, {}
        for q, (status, selected, candidates) in expected.items():
            marker, owner = selected.split('@')
            path, start, end, name = ds[marker]
            compiler[q] = [path, 'METHOD', owner, name, str(start), str(end)]
            targets = [c.o.reference_target(ds, item, Path('/unused')) for item in candidates.split(',')]
            native[q] = [status, str(len(targets)), *targets]
        def compare(j=compiler, n=native):
            c.o.compare_reference(expected, qs, ds, j, n, Path('/unused'))
        compare()
        for field in range(6):
            changed = copy.deepcopy(compiler)
            changed['constructor_specificity'][field] = 'wrong' if field < 4 else '0'
            with self.subTest(field=field), self.assertRaises((AssertionError, ValueError)):
                compare(j=changed)
        changed = copy.deepcopy(native)
        changed['constructor_specificity'][2] = c.o.reference_target(ds, 'specific_root@ctor.Target', Path('/unused'))
        with self.assertRaises(AssertionError):
            compare(n=changed)
        for q, (status, _, _) in expected.items():
            if status == 'Resolved':
                continue
            changed = copy.deepcopy(native)
            changed[q] = ['Resolved', '1', changed[q][2]]
            with self.subTest(false_success=q), self.assertRaises(AssertionError):
                compare(n=changed)
        for altered in ({}, {**native, 'extra': native['constructor_identity']}):
            with self.assertRaises(AssertionError):
                compare(n=altered)
        changed = copy.deepcopy(native)
        changed['constructor_identity'][1] = '2'
        with self.assertRaises(AssertionError):
            compare(n=changed)


if __name__ == '__main__':
    unittest.main()
