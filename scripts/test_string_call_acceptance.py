"""Offline mutation controls for the frozen string-call acceptance contract."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

_spec = importlib.util.spec_from_file_location('string_gate', Path(__file__).with_name('check-string-call-acceptance.py'))
gate = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gate)
second = gate.second


class StringControls(unittest.TestCase):
    def setUp(self):
        self.plan = gate.load_plan()
        self.queries = self.plan['queries']
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        f = self.fixture = second.Fixture()
        f.workdir = Path(self.temp.name)
        f.source_root = f.workdir / 'sources'
        f.manifest = f.workdir / 'project.json'
        f.metadata = {'inputs': {'sources': {'sha256': self.plan['source_archive_sha256']}}}
        f.owner = {}
        buffers = {path: bytearray(b' ' * 12000) for path in self.plan['source_sha256']}
        for q in self.queries:
            value = q['expression'].encode()
            buffers[q['file']][q['byte']:q['byte'] + len(value)] = value
            f.owner[q['file']] = 'gson/test'
            for t in q['expected']['targets']:
                buffers[t['path']][t['start']:t['end']] = t['name'].encode()
                f.owner[t['path']] = 'gson/main'
        for offset, spelling in [(5914, b'get(String memberName)'),
                                 (2758, b'remove(String property)'),
                                 (5601, b'has(String memberName)'),
                                 (2345, b'add(String property, JsonElement value)')]:
            buffers['gson/src/main/java/com/google/gson/JsonObject.java'][offset:offset + len(spelling)] = spelling
        f.sources = {p: bytes(b) for p, b in buffers.items()}
        for p, b in f.sources.items():
            dest = f.source_root / p
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(b)
        self.response = self.response_for('expected')

    def response_for(self, stage):
        f = self.fixture
        return {'schema_version': 1, 'command': 'declaration',
                'snapshot': {'id': 'unit', 'identity': 'invocation_local', 'manifest': str(f.manifest)},
                'capabilities': {'runtime': 'native_rust_no_jvm', 'build_tool_import': False,
                                 'semantic_diagnostics': False},
                'result': {'index_builds': 1, 'queries': [
                    {'query_index': i, 'compilation': q['compilation'],
                     'source_compilation': second.compilation(f.owner[q['file']]),
                     'file': str(f.source_root / q['file']),
                     'position': {'encoding': 'utf8', 'offset': q['byte']}, 'byte_offset': q['byte'],
                     'status': q[stage]['status'],
                     'targets': [second.expected_target(t, f) for t in q[stage]['targets']]}
                    for i, q in enumerate(self.queries)]}}

    def check(self, response, baseline=False):
        gate.check_response(response, self.queries, self.fixture, baseline)

    def test_baseline_is_exact_and_never_candidate_success(self):
        self.check(self.response)
        baseline = self.response_for('baseline')
        self.check(baseline, True)
        with self.assertRaises(second.AcceptanceError):
            self.check(baseline)
        with self.assertRaises(second.AcceptanceError):
            self.check(self.response, True)
        self.assertEqual(7, sum(q['expected']['status'] == 'Resolved' for q in self.queries))
        self.assertEqual(5, len(self.queries[-1]['expected']['targets']))

    def test_every_row_and_candidate_is_required(self):
        for baseline in (False, True):
            original = self.response_for('baseline' if baseline else 'expected')
            for index in range(8):
                for mutation in ('omit', 'duplicate', 'status', 'target_omit', 'target_duplicate'):
                    changed = copy.deepcopy(original)
                    rows = changed['result']['queries']
                    if mutation == 'omit':
                        rows.pop(index)
                    elif mutation == 'duplicate':
                        rows.append(copy.deepcopy(rows[index]))
                    elif mutation == 'status':
                        rows[index]['status'] = 'Resolved' if rows[index]['status'] != 'Resolved' else 'CallNotResolved'
                    elif mutation == 'target_omit':
                        rows[index]['targets'].pop()
                    else:
                        rows[index]['targets'].append(copy.deepcopy(rows[index]['targets'][0]))
                    with self.subTest(baseline=baseline, index=index, mutation=mutation), self.assertRaises(second.AcceptanceError):
                        self.check(changed, baseline)

    def test_wrong_identity_ranges_and_query_metadata_fail(self):
        for i in range(8):
            for key, value in [('owner', 'wrong.Owner'), ('name', 'wrong'), ('path', 'wrong.java'),
                               ('kind', 'Field'), ('source_available', False), ('origin', 'binary')]:
                changed = copy.deepcopy(self.response)
                changed['result']['queries'][i]['targets'][0][key] = value
                with self.subTest(index=i, field=key), self.assertRaises(second.AcceptanceError):
                    self.check(changed)
            for key in ('start_byte', 'end_byte', 'start_utf16', 'end_utf16'):
                changed = copy.deepcopy(self.response)
                changed['result']['queries'][i]['targets'][0]['range'][key] += 1
                with self.subTest(index=i, field=key), self.assertRaises(second.AcceptanceError):
                    self.check(changed)
            for key, value in [('query_index', 99), ('byte_offset', 0), ('file', 'wrong.java'),
                               ('compilation', {'project': 'extras', 'source_set': 'test'})]:
                changed = copy.deepcopy(self.response)
                changed['result']['queries'][i][key] = value
                with self.subTest(index=i, field=key), self.assertRaises(second.AcceptanceError):
                    self.check(changed)

    def test_refusal_candidates_are_multiset_not_order(self):
        changed = copy.deepcopy(self.response)
        changed['result']['queries'][-1]['targets'].reverse()
        self.check(changed)
        for index in range(5):
            changed = copy.deepcopy(self.response)
            changed['result']['queries'][-1]['targets'].pop(index)
            with self.assertRaises(second.AcceptanceError):
                self.check(changed)

    def test_frozen_plan_rejects_pinned_metadata_and_target_mutations(self):
        changes = [lambda p: p.update(base_commit='other'),
                   lambda p: p.update(source_archive_sha256='other'),
                   lambda p: p.update(selected_before_implementation=False),
                   lambda p: p['source_sha256'].clear(),
                   lambda p: p['historical_artifact_sha256'].clear(),
                   lambda p: p['queries'][0].update(byte=4827),
                   lambda p: p['queries'][0]['expected']['targets'][0].update(start=5915),
                   lambda p: p['queries'][-1]['expected'].update(status='Resolved')]
        path = self.fixture.workdir / 'plan.json'
        for change in changes:
            plan = copy.deepcopy(self.plan)
            change(plan)
            path.write_text(json.dumps(plan))
            with self.assertRaises(second.AcceptanceError):
                gate.load_plan(path)
        path.write_bytes(gate.PLAN.read_bytes())
        self.assertEqual(self.plan, gate.load_plan(path))
        with self.assertRaises(second.AcceptanceError):
            json.loads('{"a":1,"a":2}', object_pairs_hook=second.unique_json_object)

    def synthetic_plan(self):
        plan = copy.deepcopy(self.plan)
        plan['source_sha256'] = {p: hashlib.sha256(b).hexdigest() for p, b in self.fixture.sources.items()}
        return plan

    def test_source_bytes_query_spellings_and_signature_are_checked(self):
        plan = self.synthetic_plan()
        with mock.patch.object(gate, 'load_plan', return_value=plan):
            self.assertEqual(self.queries, gate.probes(self.fixture))
            self.fixture.metadata['inputs']['sources']['sha256'] = 'wrong'
            with self.assertRaises(second.AcceptanceError):
                gate.probes(self.fixture)
            self.fixture.metadata['inputs']['sources']['sha256'] = plan['source_archive_sha256']
            path = self.fixture.source_root / self.queries[0]['file']
            data = path.read_bytes()
            path.write_bytes(data + b'changed')
            with self.assertRaises(second.AcceptanceError):
                gate.probes(self.fixture)
            path.write_bytes(data)
            for q in plan['queries']:
                q['expression'] += 'wrong'
                with self.assertRaises(second.AcceptanceError):
                    gate.probes(self.fixture)
                q['expression'] = q['expression'][:-5]
        # A same-name declaration at the fixed byte is insufficient: check its
        # independently specified formal spelling even if source digests agree.
        p = 'gson/src/main/java/com/google/gson/JsonObject.java'
        altered = self.fixture.sources[p].replace(b'get(String memberName)', b'get(Object memberName)')
        self.fixture.sources[p] = altered
        (self.fixture.source_root / p).write_bytes(altered)
        with mock.patch.object(gate, 'load_plan', return_value=self.synthetic_plan()):
            with self.assertRaises(second.AcceptanceError):
                gate.probes(self.fixture)

    def test_both_modes_use_current_constructor_preservation_and_check_hashes(self):
        inventory = {'source.java': 'fixed'}
        for baseline in (False, True):
            response = self.response_for('baseline' if baseline else 'expected')
            preservation = {'source_sha256_after': inventory, 'validation': 'passed'}
            with mock.patch.object(gate, 'probes', return_value=self.queries), \
                    mock.patch.object(second, 'all_source_hashes', return_value=inventory), \
                    mock.patch.object(second, 'successful', return_value=response), \
                    mock.patch.object(second, 'document', return_value=Path('requests')), \
                    mock.patch.object(second, 'run_constructor_call_acceptance', return_value=preservation) as keep, \
                    mock.patch.object(second, 'portable_queries', return_value=[]), \
                    mock.patch.object(second, 'digest', return_value='binary'):
                report = gate.run(Path('native'), self.fixture, baseline)
                keep.assert_called_once_with(Path('native'), self.fixture, primitive_current=not baseline,
                                             parser_stage='baseline' if baseline else 'current')
                self.assertEqual('baseline_verified_not_acceptance' if baseline else 'passed', report['validation'])
                with mock.patch.object(second, 'all_source_hashes', side_effect=[inventory, {'extra.java': 'changed'}]):
                    with self.assertRaises(second.AcceptanceError):
                        gate.run(Path('native'), self.fixture, baseline)

    def test_historical_metadata_has_exact_pins(self):
        self.assertEqual('aa3f2a12c3a24094667b7c225668c1b17585b161', self.plan['base_commit'])
        for name, checksum in self.plan['historical_artifact_sha256'].items():
            self.assertEqual(checksum, second.digest(second.CORPUS / name))


if __name__ == '__main__':
    unittest.main()
