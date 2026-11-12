"""Offline mutation controls for the frozen strict-reference acceptance contract."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

_spec = importlib.util.spec_from_file_location('strict_gate', Path(__file__).with_name('check-strict-reference-acceptance.py'))
gate = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gate)
second = gate.second


class StrictReferenceControls(unittest.TestCase):
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
            for t in q['baseline']['targets']:
                buffers[t['path']][t['start']:t['end']] = t['name'].encode()
                f.owner[t['path']] = 'gson/main'
        for item in self.plan['source_spellings']:
            spelling = item['spelling'].encode()
            offset = item['byte']
            buffers[item['path']][offset:offset + len(spelling)] = spelling
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
        self.assertEqual(1, sum(q['expected']['status'] == 'Resolved' for q in self.queries))
        self.assertEqual(0, len(self.queries[-1]['expected']['targets']))

    def test_every_row_and_candidate_is_required(self):
        for baseline in (False, True):
            original = self.response_for('baseline' if baseline else 'expected')
            for index in range(4):
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
                        rows[index]['targets'].pop() if rows[index]['targets'] else rows[index]['targets'].append({})
                    else:
                        rows[index]['targets'].append(copy.deepcopy(rows[index]['targets'][0]) if rows[index]['targets'] else {})
                    with self.subTest(baseline=baseline, index=index, mutation=mutation), self.assertRaises(second.AcceptanceError):
                        self.check(changed, baseline)

    def test_wrong_identity_ranges_and_query_metadata_fail(self):
        for i in range(3):
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
                               ('compilation', {'project': 'extras', 'source_set': 'test'}),
                               ('source_compilation', {'project': 'gson', 'source_set': 'main'}),
                               ('position', {'encoding': 'utf16', 'offset': 1843})]:
                changed = copy.deepcopy(self.response)
                changed['result']['queries'][i][key] = value
                with self.subTest(index=i, field=key), self.assertRaises(second.AcceptanceError):
                    self.check(changed)

    def test_refusal_candidates_are_multiset_not_order(self):
        changed = copy.deepcopy(self.response)
        changed['result']['queries'][1]['targets'].reverse()
        self.check(changed)
        for index in range(2):
            changed = copy.deepcopy(self.response)
            changed['result']['queries'][1]['targets'].pop(index)
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
        p = 'gson/src/main/java/com/google/gson/JsonArray.java'
        altered = self.fixture.sources[p].replace(b'remove(int index)', b'remove(long index)')
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
                    mock.patch.object(gate, 'current_probes', return_value=self.queries), \
                    mock.patch.object(second, 'all_source_hashes', return_value=inventory), \
                    mock.patch.object(second, 'successful', return_value=response), \
                    mock.patch.object(second, 'document', return_value=Path('requests')), \
                    mock.patch.object(gate.string, 'run', return_value=preservation) as keep, \
                    mock.patch.object(second, 'portable_queries', return_value=[]), \
                    mock.patch.object(second, 'digest', return_value='binary'):
                report = gate.run(Path('native'), self.fixture, baseline)
                keep.assert_called_once_with(Path('native'), self.fixture, proof_stage='pre_primitive' if baseline else 'source_invocation',
                                             parser_stage='baseline' if baseline else 'current')
                self.assertEqual('baseline_verified_not_acceptance' if baseline else 'passed', report['validation'])
                with mock.patch.object(second, 'all_source_hashes', side_effect=[inventory, {'extra.java': 'changed'}]):
                    with self.assertRaises(second.AcceptanceError):
                        gate.run(Path('native'), self.fixture, baseline)

    def test_invalid_modes_and_order_fail(self):
        for mode in ('baseline', 'expected', None, 0, 1):
            with self.subTest(mode=mode), self.assertRaises(second.AcceptanceError):
                self.check(self.response, mode)
        changed = copy.deepcopy(self.response)
        changed['result']['queries'].reverse()
        with self.assertRaises(second.AcceptanceError):
            self.check(changed)

    def test_every_target_compilation_and_extra_field_fail(self):
        for stage in ('baseline', 'expected'):
            original = self.response_for(stage)
            for i, row in enumerate(original['result']['queries']):
                for j in range(len(row['targets'])):
                    for field, value in [('compilation', {'project': 'gson', 'source_set': 'test'}),
                                         ('extra', True)]:
                        changed = copy.deepcopy(original)
                        changed['result']['queries'][i]['targets'][j][field] = value
                        with self.subTest(stage=stage, row=i, target=j, field=field), self.assertRaises(second.AcceptanceError):
                            self.check(changed, stage == 'baseline')

    def test_historical_metadata_has_exact_pins(self):
        self.assertEqual('360d9878196825e5891280ae9c5b5c934e1875e3', self.plan['base_commit'])
        for name, checksum in self.plan['historical_artifact_sha256'].items():
            self.assertEqual(checksum, second.digest(second.CORPUS / name))


if __name__ == '__main__':
    unittest.main()
