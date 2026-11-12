"""Offline exact stage, source, candidate and metadata mutation controls."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import unittest
from unittest import mock


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(file))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


gate = load('qualified_gate_test', 'check-null-actual-acceptance.py')
old_test = load('qualified_old_test', 'test_strict_reference_acceptance.py')
second = gate.second
contract = gate.strict.nulls


class NullControls(unittest.TestCase):
    response_for = old_test.StrictReferenceControls.response_for

    def setUp(self):
        old_test.StrictReferenceControls.setUp(self)
        self.historical = gate.strict.returns.load_plan(second)['queries']
        self.plan = contract.load_plan(second)
        self.queries = self.plan['queries']
        # Synthetic bytes preserve exact independently fixed token positions.
        for name in self.plan['source_sha256']:
            if name not in self.fixture.sources:
                self.fixture.sources[name] = b' ' * 12000
                self.fixture.owner[name] = 'gson/main'
        buffers = {p: bytearray(data) for p, data in self.fixture.sources.items()}
        for item in self.plan['source_spellings']:
            data = item['spelling'].encode(); at = item['byte']
            buffers[item['path']][at:at+len(data)] = data
        path = self.queries[4]['file']; data = b'get(0).getAsString()'
        buffers[path][2482:2482+len(data)] = data
        for q in self.queries:
            data = q['expression'].encode(); at = q['byte']
            buffers[q['file']][at:at+len(data)] = data
        self.fixture.sources = {p: bytes(data) for p, data in buffers.items()}
        for p, data in self.fixture.sources.items():
            dest = self.fixture.source_root / p; dest.parent.mkdir(parents=True, exist_ok=True); dest.write_bytes(data)
        self.response = self.response_for('expected')

    def check(self, response, baseline=False):
        gate.check_response(response, self.queries, self.fixture, baseline)

    def test_exact_return_baseline_not_older_primitive_baseline(self):
        self.check(self.response)
        before = self.response_for('baseline')
        self.check(before, True)
        for response, mode in [(before, False), (self.response, True)]:
            with self.assertRaises(second.AcceptanceError):
                self.check(response, mode)
        self.assertEqual('Resolved', self.queries[0]['baseline']['status'])
        self.assertEqual([2527], [q['byte'] for q in self.queries if q['baseline'] != q['expected']])
        old_primitive = copy.deepcopy(before)
        old_primitive['result']['queries'][1]['status'] = 'OverloadNotResolved'
        with self.assertRaises(second.AcceptanceError): self.check(old_primitive, True)
        old = copy.deepcopy(before)
        old['result']['queries'][0]['status'] = 'OverloadNotResolved'
        with self.assertRaises(second.AcceptanceError):
            self.check(old, True)

    def test_complete_rows_targets_and_refusals(self):
        for baseline in (False, True):
            original = self.response_for('baseline' if baseline else 'expected')
            for i in range(8):
                for change in ('omit', 'duplicate', 'status', 'target_omit', 'target_duplicate'):
                    row = copy.deepcopy(original)
                    queries = row['result']['queries']
                    if change == 'omit': queries.pop(i)
                    elif change == 'duplicate': queries.append(copy.deepcopy(queries[i]))
                    elif change == 'status': queries[i]['status'] = 'Wrong'
                    elif change == 'target_omit':
                        queries[i]['targets'].pop() if queries[i]['targets'] else queries[i]['targets'].append({})
                    else:
                        queries[i]['targets'].append(copy.deepcopy(queries[i]['targets'][0]) if queries[i]['targets'] else {})
                    with self.subTest(i=i, change=change, baseline=baseline), self.assertRaises(second.AcceptanceError):
                        self.check(row, baseline)
        before = self.response_for('baseline')
        before['result']['queries'][1]['targets'].reverse()
        self.check(before, True)

    def test_all_identity_and_position_fields(self):
        original = self.response
        for i, q in enumerate(original['result']['queries']):
            for key, value in [('query_index', 9), ('byte_offset', 0), ('file', 'wrong'),
                               ('compilation', {'project': 'x', 'source_set': 'main'}),
                               ('source_compilation', {'project': 'x', 'source_set': 'main'}),
                               ('position', {'encoding': 'utf16', 'offset': 0})]:
                changed = copy.deepcopy(original); changed['result']['queries'][i][key] = value
                with self.assertRaises(second.AcceptanceError): self.check(changed)
            for j in range(len(q['targets'])):
                for key, value in [('name', 'wrong'), ('owner', 'wrong'), ('path', 'wrong'), ('kind', 'Field'),
                                   ('origin', 'binary'), ('source_available', False), ('extra', True),
                                   ('compilation', {'project': 'x', 'source_set': 'test'})]:
                    changed = copy.deepcopy(original); changed['result']['queries'][i]['targets'][j][key] = value
                    with self.assertRaises(second.AcceptanceError): self.check(changed)
                for key in ('start_byte', 'end_byte', 'start_utf16', 'end_utf16'):
                    changed = copy.deepcopy(original); changed['result']['queries'][i]['targets'][j]['range'][key] += 1
                    with self.assertRaises(second.AcceptanceError): self.check(changed)

    def test_frozen_plan_cannot_change_any_expectation(self):
        path = self.fixture.workdir / 'changed-plan.json'
        for change in [lambda p: p.update(base_commit='wrong'), lambda p: p.update(baseline_native_sha256='wrong'),
                       lambda p: p['historical_artifact_sha256'].clear(),
                       lambda p: p['queries'][5].update(expected=p['queries'][5]['baseline']),
                       lambda p: p['queries'][1]['expected'].update(status='CallNotResolved')]:
            plan = copy.deepcopy(self.plan); change(plan); path.write_text(json.dumps(plan))
            with self.assertRaises(second.AcceptanceError): contract.load_plan(second, path)
        path.write_bytes((second.CORPUS / contract.PLAN_NAME).read_bytes())
        self.assertEqual(self.plan, contract.load_plan(second, path))

    def synthetic_plan(self):
        plan = copy.deepcopy(self.plan)
        plan['source_sha256'] = {p: hashlib.sha256(b).hexdigest() for p, b in self.fixture.sources.items()}
        return plan

    def test_overlay_only_allows_the_frozen_delta(self):
        plan = self.synthetic_plan()
        with mock.patch.object(contract, 'load_plan', return_value=plan):
            self.assertEqual(self.queries, contract.overlay(second, self.fixture, self.historical))
            for mutate in [lambda q: q.pop(), lambda q: q.reverse(),
                           lambda q: q[3]['expected'].update(status='Wrong'),
                           lambda q: q[0].update(byte=0)]:
                historical = copy.deepcopy(self.historical); mutate(historical)
                with self.assertRaises(second.AcceptanceError):
                    contract.overlay(second, self.fixture, historical)
            plan['queries'][1]['expected']['status'] = 'CallNotResolved'
            with self.assertRaises(second.AcceptanceError):
                contract.overlay(second, self.fixture, self.historical)

    def test_source_bytes_and_independent_formal_spelling(self):
        plan = self.synthetic_plan()
        with mock.patch.object(contract, 'load_plan', return_value=plan):
            path = self.fixture.source_root / self.queries[0]['file']
            old = path.read_bytes(); path.write_bytes(old + b'changed')
            with self.assertRaises(second.AcceptanceError):
                contract.overlay(second, self.fixture, self.historical)
            path.write_bytes(old)
        path = 'gson/src/main/java/com/google/gson/JsonArray.java'
        bad = self.fixture.sources[path].replace(b'remove(int index)', b'remove(long index)')
        self.fixture.sources[path] = bad
        (self.fixture.source_root / path).write_bytes(bad)
        with mock.patch.object(contract, 'load_plan', return_value=self.synthetic_plan()):
            with self.assertRaises(second.AcceptanceError):
                contract.overlay(second, self.fixture, self.historical)

    def test_invalid_modes_and_frozen_binary_gate(self):
        for mode in (0, 1, None, 'baseline'):
            with self.assertRaises(second.AcceptanceError): self.check(self.response, mode)
        with mock.patch.object(gate.strict, 'frozen_null_probes', return_value=self.queries), \
                mock.patch.object(second, 'digest', return_value='wrong'), \
                mock.patch.object(second, 'successful') as execute:
            with self.assertRaises(second.AcceptanceError):
                gate.run(Path('native'), self.fixture, True)
            execute.assert_not_called()




    def test_v2_only_defers_second_forecast_and_rejects_old_positive(self):
        old = json.loads((second.CORPUS / contract.INITIAL_PLAN_NAME).read_text())
        self.assertEqual(hashlib.sha256((second.CORPUS / contract.INITIAL_PLAN_NAME).read_bytes()).hexdigest(), contract.INITIAL_PLAN_SHA256)
        self.assertEqual([i for i, (a, b) in enumerate(zip(old['queries'], self.plan['queries'])) if a != b], [6])
        revised = copy.deepcopy(old['queries'][6]); revised['expected'] = copy.deepcopy(revised['baseline'])
        self.assertEqual(self.plan['queries'][6], revised)
        self.assertEqual(self.plan['scope_revision']['disposition'], 'deferred_refusal')
        old_positive = copy.deepcopy(self.response)
        old_positive['result']['queries'][6]['status'] = 'Resolved'
        with self.assertRaises(second.AcceptanceError): self.check(old_positive)
        # V1 cannot masquerade as the accepted latest contract.
        with self.assertRaises(second.AcceptanceError):
            contract.load_plan(second, second.CORPUS / contract.INITIAL_PLAN_NAME)

    def test_deferred_diagnosis_preserves_source_inputs_and_full_results(self):
        revision = self.plan['scope_revision']
        path = second.CORPUS / revision['diagnosis_file']
        self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), revision['diagnosis_sha256'])
        evidence = json.loads(path.read_text()); inputs = evidence['inputs']
        self.assertEqual(inputs['original_sha256'], self.plan['source_sha256'][inputs['source_file']])
        self.assertEqual((inputs['start_byte'], inputs['end_byte'], inputs['original_bytes']), (2143, 2155, 'propertyName'))
        cases = evidence['actual_cases']
        self.assertEqual([case['label'] for case in cases], ['original', 'diagnostic_literal_only', 'diagnostic_both_null'])
        self.assertEqual([[q['status'] for q in case['queries']] for case in cases],
                         [['CallNotResolved', 'CallNotResolved'], ['Resolved', 'CallNotResolved'], ['Resolved', 'CallNotResolved']])
        for item in inputs['mutations']:
            data = item['replacement_utf8'].encode()
            self.assertEqual(len(data), 12); self.assertEqual(data.hex(), item['replacement_hex'])
        for case in cases[1:]:
            self.assertEqual(case['queries'][1], cases[0]['queries'][1])
            wanted = copy.deepcopy(cases[0]['queries'][0]); wanted['status'] = 'Resolved'
            self.assertEqual(case['queries'][0], wanted)


if __name__ == '__main__':
    unittest.main()
