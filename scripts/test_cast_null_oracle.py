"""Offline exact cast-null contract mutations; no Java/native processes."""
import copy
import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('cast_null', Path(__file__).with_name('check-cast-null-oracle.py'))
c = importlib.util.module_from_spec(spec); spec.loader.exec_module(c)

class CastNullOracleTests(unittest.TestCase):
    def setUp(self):
        self.ds, self.qs, self.req, self.expected = c.fixture()
        self.platform = Path('/unused-platform')
        self.compiler = {}
        for q, (_, selected, _) in self.expected.items():
            marker, owner = selected.split('@'); path, _, _, name = self.ds[marker]
            start, end = c.compiler_range(self.ds, marker)
            self.compiler[q] = [path, 'METHOD', owner, name, str(start), str(end)]

    def native(self, expected):
        return {q: [status, str(len(names.split(','))), *[c.o.reference_target(self.ds, name, self.platform) for name in names.split(',')]]
                for q, (status, _, names) in expected.items()}

    def compare(self, rows, expected=None, compiler=None):
        c.compare(expected or self.expected, self.qs, self.ds, compiler or self.compiler, rows, self.platform)

    def test_exact_current_and_baseline_not_union(self):
        current = self.native(self.expected); old_expected = c.baseline_expectations(self.expected); old = self.native(old_expected)
        self.compare(current); self.compare(old, old_expected)
        with self.assertRaises(AssertionError): self.compare(old)
        with self.assertRaises(AssertionError): self.compare(current, old_expected)
        self.assertEqual(old['plain_null'], current['plain_null'])
        self.assertEqual(len([q for q in current if current[q] != old[q]]), 3)

    def test_typed_parent_cannot_select_child_or_plain_null(self):
        rows = self.native(self.expected)
        for q in ('typed_parent', 'parenthesized'):
            bad = copy.deepcopy(rows); bad[q] = copy.deepcopy(rows['plain_null'])
            with self.assertRaises(AssertionError): self.compare(bad)

    def test_all_status_and_full_candidate_metadata_mutations(self):
        rows = self.native(self.expected)
        for q, row in rows.items():
            bad = copy.deepcopy(rows); bad[q][0] = 'Resolved' if row[0] != 'Resolved' else 'CallNotResolved'
            with self.assertRaises(AssertionError): self.compare(bad)
            bad = copy.deepcopy(rows); bad[q][1] = '99'
            with self.assertRaises(AssertionError): self.compare(bad)
            for i in range(2, len(row)):
                for field in range(6):
                    bad = copy.deepcopy(rows); cells = bad[q][i].split('|'); cells[field] += 'wrong'; bad[q][i] = '|'.join(cells)
                    with self.assertRaises(AssertionError): self.compare(bad)
                bad = copy.deepcopy(rows); bad[q].append(row[i]); bad[q][1] = str(int(row[1])+1)
                with self.assertRaises(AssertionError): self.compare(bad)
        for operation in ('missing', 'extra'):
            bad = copy.deepcopy(rows)
            if operation == 'missing': bad.pop('typed_parent')
            else: bad['extra'] = rows['typed_parent']
            with self.assertRaises(AssertionError): self.compare(bad)

    def test_compiler_exact_full_ranges_owner_and_target(self):
        rows = self.native(self.expected)
        for q in self.compiler:
            for field in range(6):
                bad = copy.deepcopy(self.compiler)
                bad[q][field] = str(int(bad[q][field])+1) if field >= 4 else 'wrong'
                with self.assertRaises(AssertionError): self.compare(rows, compiler=bad)

    def test_all_frozen_inputs_and_manifest_reject_mutation(self):
        for filename in ('source/Use.java', 'source/Target.java', 'expected.tsv', 'files.tsv', 'invalid/primitive/Use.java', 'invalid/unknown/Use.java', 'fixture-manifest.json'):
            with tempfile.TemporaryDirectory() as temp:
                root = Path(temp)/'fixture'; shutil.copytree(c.ROOT, root)
                p = root/filename; p.write_bytes(p.read_bytes()+b'changed')
                with patch.object(c, 'ROOT', root), self.assertRaises(AssertionError): c.fixture()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)/'fixture'; shutil.copytree(c.ROOT, root); (root/'extra.java').write_text('class Extra {}')
            with patch.object(c, 'ROOT', root), self.assertRaises(AssertionError): c.fixture()

    def test_compiler_invalid_inventory_and_exact_diagnostics(self):
        rows = list(c.invalid_fixtures())
        self.assertEqual({row[0] for row in rows}, {'primitive', 'unknown'})
        for case, status, names, diagnostic, ds, req in rows:
            self.assertEqual(status, 'CallNotResolved'); self.assertEqual(names, 'only@casts.Target')
            self.assertEqual(len(ds), 1); self.assertIn('invalid_'+case, req)
            self.assertTrue(diagnostic.startswith('compiler.err.'))

if __name__ == '__main__': unittest.main()
