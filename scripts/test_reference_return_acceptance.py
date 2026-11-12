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


gate = load('qualified_gate_test', 'check-reference-return-acceptance.py')
old_test = load('qualified_old_test', 'test_strict_reference_acceptance.py')
second = gate.second
contract = gate.strict.returns


class ReturnControls(unittest.TestCase):
    response_for = old_test.StrictReferenceControls.response_for

    def setUp(self):
        old_test.StrictReferenceControls.setUp(self)
        self.historical = gate.strict.mixed.load_plan(second)['queries']
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
        self.assertEqual([1986, 2489], [q['byte'] for q in self.queries if q['baseline'] != q['expected']])
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
            for i in range(5):
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
                       lambda p: p['queries'][3].update(expected=p['queries'][3]['baseline']),
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
                           lambda q: q[3]['expected'].update(status='Resolved'),
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
        with mock.patch.object(gate.strict, 'current_probes', return_value=self.queries), \
                mock.patch.object(second, 'digest', return_value='wrong'), \
                mock.patch.object(second, 'successful') as execute:
            with self.assertRaises(second.AcceptanceError):
                gate.run(Path('native'), self.fixture, True)
            execute.assert_not_called()


    def test_original_sixteen_full_inventory_overlay(self):
        self.fixture.plan = json.loads((second.CORPUS / 'query-plan.json').read_text())['queries']
        expected = json.loads((second.CORPUS / 'expected.json').read_text())
        constructor = json.loads((second.CORPUS / 'constructor-call-probe.json').read_text())
        expected[constructor['query_index']] = constructor['expected']
        with mock.patch.object(contract, 'load_plan', return_value=self.synthetic_plan()):
            actual = contract.original_overlay(second, self.fixture, expected)
            changes = [i for i, pair in enumerate(zip(actual, expected)) if pair[0] != pair[1]]
            self.assertEqual(changes, [13])
            self.assertEqual(actual[13], self.queries[3]['expected'])
            for index in range(len(expected)):
                altered = copy.deepcopy(expected); altered[index]['status'] = 'Wrong'
                with self.assertRaises(second.AcceptanceError):
                    contract.original_overlay(second, self.fixture, altered)
            for field, value in [('byte', 0), ('compilation', {}), ('spelling', 'other'), ('goal', 'different')]:
                original = copy.deepcopy(self.fixture.plan)
                self.fixture.plan[13][field] = value
                with self.assertRaises(second.AcceptanceError):
                    contract.original_overlay(second, self.fixture, expected)
                self.fixture.plan = original


if __name__ == '__main__':
    unittest.main()
