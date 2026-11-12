"""Exact historical null overlays; no native or compiler execution."""
import copy
import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(file))
    m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m); return m


o = load('null_contract_semantic', 'check-semantic-oracle.py')
c = load('null_prior_contract', 'null-oracle-contract.py')
g = load('null_prior_guard', 'receiver-guard-oracle-contract.py')
SUITES = ('reference-v1', 'string-v1', 'strict-reference-v1', 'qualified-primitive-v1', 'mixed-strict-v1')


class NullPriorContractTests(unittest.TestCase):
    def fixture(self, suite):
        root = o.ORACLE / suite
        files = (root / 'files.tsv').read_text().splitlines()
        ds, qs, _ = o.fixture_markers(root / 'source', files)
        expected = {q: (s, t, cs) for q, s, t, cs in o.rows(root / 'expected.tsv', 4)}
        if suite == 'reference-v1': expected = g.apply(o.ORACLE, suite, expected, qs, ds)
        return expected, qs, ds

    def test_only_one_frozen_selection_per_suite(self):
        for suite in SUITES:
            before, qs, ds = self.fixture(suite)
            after = c.apply(o.ORACLE, suite, before, qs, ds)
            changed = [q for q in before if before[q] != after[q]]
            self.assertEqual(len(changed), 1)
            q = changed[0]; self.assertEqual(after[q], ('Resolved', before[q][1], before[q][1]))
            with self.assertRaises(AssertionError): c.apply(o.ORACLE, suite, after, qs, ds)
            for key in before:
                wrong = dict(before); wrong[key] = ('Wrong', before[key][1], '-')
                with self.assertRaises(AssertionError): c.apply(o.ORACLE, suite, wrong, qs, ds)
            for key in qs:
                wrong = dict(qs); row = list(wrong[key]); row[1] += 1; wrong[key] = tuple(row)
                with self.assertRaises(AssertionError): c.apply(o.ORACLE, suite, before, wrong, ds)
            for key in ds:
                wrong = dict(ds); row = list(wrong[key]); row[2] += 1; wrong[key] = tuple(row)
                with self.assertRaises(AssertionError): c.apply(o.ORACLE, suite, before, qs, wrong)

    def test_sources_plan_goldens_and_prior_guard_are_pinned(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / 'oracle'; shutil.copytree(o.ORACLE, root)
            for suite in SUITES:
                expected, qs, ds = self.fixture(suite)
                paths = [root / suite / 'expected.tsv', root / suite / 'source' / next(iter(qs.values()))[0],
                         root / 'reference-return-v1/receiver-guard-updates.json', root / 'null-actual-v1/prior-null-updates.json']
                for path in paths:
                    before = path.read_bytes(); path.write_bytes(before + b'changed')
                    with self.assertRaises(AssertionError): c.apply(root, suite, expected, qs, ds)
                    path.write_bytes(before)

if __name__ == '__main__': unittest.main()
