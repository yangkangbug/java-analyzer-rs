"""Offline complete-history mutation controls for current refusal-only overlays."""
import copy
import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module


o = load('guard_semantic', 'check-semantic-oracle.py')
c = load('guard_contract', 'receiver-guard-oracle-contract.py')


class GuardOverlayTests(unittest.TestCase):
    def fixture(self, suite):
        files = ('PrimitiveCalls.java', 'Boundaries.java', 'SingletonCalls.java') if suite == 'primitive-v1' else o.REFERENCE_FILES
        ds, qs, _ = o.fixture_markers(o.ORACLE / suite / 'source', files)
        expected = {q: (s, t, cs) for q, s, t, cs in o.rows(o.ORACLE / suite / 'expected.tsv', 4)}
        if suite == 'primitive-v1':
            for q, before, after, target, candidates in o.rows(o.ORACLE / 'return-v1/primitive-updates.tsv', 5):
                self.assertEqual(expected[q][0], before)
                expected[q] = after, target, candidates
        return expected, qs, ds

    def test_only_two_exact_refusal_states_change(self):
        for suite in ('primitive-v1', 'reference-v1'):
            before, qs, ds = self.fixture(suite)
            after = c.apply(o.ORACLE, suite, before, qs, ds)
            changed = [q for q in before if before[q] != after[q]]
            self.assertEqual(len(changed), 1)
            q = changed[0]
            self.assertEqual(before[q], ('Unresolved', after[q][1], '-'))
            self.assertEqual(after[q], ('ReceiverNotBound', before[q][1], '-'))
            with self.assertRaises(AssertionError): c.apply(o.ORACLE, suite, after, qs, ds)
            for q in before:
                wrong = dict(before); wrong[q] = ('Resolved', before[q][1], before[q][1])
                if wrong == before: wrong[q] = ('Wrong', before[q][1], '-')
                with self.assertRaises(AssertionError): c.apply(o.ORACLE, suite, wrong, qs, ds)
            for changed in ({}, {**before, 'extra': next(iter(before.values()))}):
                with self.assertRaises((AssertionError, KeyError)): c.apply(o.ORACLE, suite, changed, qs, ds)

    def test_metadata_and_source_hashes_are_exact(self):
        for suite in ('primitive-v1', 'reference-v1'):
            expected, qs, ds = self.fixture(suite)
            q = 'receiver_spine_outer_refusal' if suite == 'primitive-v1' else 'reference_receiver_return_refusal'
            for i in range(4):
                wrong = dict(qs); row = list(wrong[q]); row[i] = 'wrong' if i in (0, 3) else 0; wrong[q] = tuple(row)
                with self.assertRaises(AssertionError): c.apply(o.ORACLE, suite, expected, wrong, ds)
            for key in qs:
                wrong = dict(qs); row = list(wrong[key]); row[1] += 1; wrong[key] = tuple(row)
                with self.assertRaises(AssertionError): c.apply(o.ORACLE, suite, expected, wrong, ds)
            with tempfile.TemporaryDirectory() as temp:
                root = Path(temp) / 'oracle'; shutil.copytree(o.ORACLE, root)
                files = [root / suite / 'expected.tsv', root / suite / 'source' / qs[q][0],
                         root / 'return-v1/primitive-updates.tsv', root / 'reference-return-v1/receiver-guard-updates.json']
                for path in files:
                    saved = path.read_bytes(); path.write_bytes(saved + b'changed')
                    with self.assertRaises(AssertionError): c.apply(root, suite, expected, qs, ds)
                    path.write_bytes(saved)

if __name__ == '__main__': unittest.main()
