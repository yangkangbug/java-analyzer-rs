"""Offline corpus forecast tests; never run Java, native executables, or downloads."""
import copy
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('binary_return_acceptance', Path(__file__).with_name('check-binary-return-acceptance.py'))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class BinaryReturnAcceptanceTests(unittest.TestCase):
    def setUp(self):
        self.plan = m.load_plan()

    def test_complete_frozen_inventory_without_execution(self):
        with patch.object(m.a, 'run_cli', side_effect=AssertionError('offline')):
            plan = m.load_plan()
        self.assertEqual(plan['source_count'], 249)
        self.assertEqual(plan['source_bytes'], 3676819)
        self.assertEqual(len(plan['source_sha256']), 249)
        self.assertEqual(len(plan['queries']), 8)
        self.assertEqual(sum(q['newly_supported'] for q in plan['queries']), 5)
        self.assertEqual(m.controls(plan['candidate']['details']), 7)

    def test_only_five_predicted_rows_change(self):
        for mode, detailed in [('default', False), ('details', True)]:
            before = copy.deepcopy(self.plan['baseline'][mode])
            after = m.forecast(before, self.plan['queries'], detailed)
            self.assertEqual(before, self.plan['baseline'][mode])
            changed = []
            for i, (metadata, old, new) in enumerate(zip(self.plan['queries'], before['queries'], after['queries'])):
                if metadata['newly_supported']:
                    changed.append(i)
                    self.assertNotEqual(old['status'], 'Resolved')
                    expected = dict(old, status='Resolved', targets=[m.target(metadata['name'], metadata['descriptor'], detailed)])
                    self.assertEqual(new, expected)
                else:
                    self.assertEqual(new, old)
            self.assertEqual(changed, [0, 1, 2, 3, 4])
            self.assertEqual(after, self.plan['candidate'][mode])

    def test_refusal_and_preservation_cannot_be_reclassified(self):
        before = copy.deepcopy(self.plan['baseline']['details'])
        before['queries'][5]['status'] = 'Resolved'
        with self.assertRaises(m.a.AcceptanceError):
            m.forecast(before, self.plan['queries'], True)
        before = copy.deepcopy(self.plan['baseline']['details'])
        before['queries'][7]['targets'][0]['descriptor'] = '(I)Ljava/lang/String;'
        with self.assertRaises(m.a.AcceptanceError):
            m.forecast(before, self.plan['queries'], True)

    def test_target_default_projection_is_exact(self):
        for q in self.plan['queries']:
            if q['descriptor'] is None: continue
            rich = m.target(q['name'], q['descriptor'], True)
            del rich['descriptor']
            rich['linking_context'] = None
            rich['identity_precision'] = 'origin_kind_owner_name_only'
            self.assertEqual(rich, m.target(q['name'], q['descriptor'], False))

    def test_spelling_and_unique_expression_are_required(self):
        sources = {}
        for _, path, expression, *_ in m.PROBES:
            sources.setdefault(path, [])
            if expression not in sources[path]: sources[path].append(expression)
        sources = {path: '\n'.join(expressions).encode() for path, expressions in sources.items()}
        self.assertEqual(len(m.query_plan(sources)), 8)
        sources[m.S] += b'\n' + m.PROBES[0][2].encode()
        with self.assertRaises(m.a.AcceptanceError): m.query_plan(sources)

    def test_additive_projection_preserves_all_other_records(self):
        original = copy.deepcopy(self.plan)
        result = m.current_projection(self.plan)
        self.assertEqual(self.plan, original)
        for mode, detailed in [('default', False), ('details', True)]:
            before = self.plan['candidate'][mode]
            expected = copy.deepcopy(before)
            expected['queries'][6].update(status='Resolved', targets=[m.target('trim', '()Ljava/lang/String;', detailed)])
            self.assertEqual(result[mode], expected)
            self.assertEqual(m.controls(result['details']), 7)
        for change in [lambda p: p['queries'][6].update(id='wrong'),
                       lambda p: p['candidate']['details']['queries'][6].update(status='Resolved'),
                       lambda p: p['candidate']['default']['queries'][6].update(targets=[{}])]:
            bad = copy.deepcopy(self.plan)
            change(bad)
            with self.assertRaises(m.a.AcceptanceError):
                m.current_projection(bad)

    def test_additive_projection_target_mutations_are_rejected(self):
        wanted = m.current_projection(self.plan)['details']
        for change in [lambda r: r.update(status='ReceiverNotBound'),
                       lambda r: r['targets'][0].update(descriptor='()I'),
                       lambda r: r['targets'][0].update(owner='java.lang.Object'),
                       lambda r: r['targets'][0].update(binary_name='java/lang/Object'),
                       lambda r: r['targets'][0]['linking_context'].update(project='wrong')]:
            bad = copy.deepcopy(wanted)
            change(bad['queries'][6])
            with self.assertRaises(m.a.AcceptanceError):
                m.exact(bad, wanted, 'additive target mutation')

    def test_frozen_hash_detects_mutation(self):
        with patch.object(m.a, 'sha256', return_value='0'*64):
            with self.assertRaises(m.a.AcceptanceError): m.load_plan()



class CurrentDetailsV2EnvelopeTests(unittest.TestCase):
    def test_only_protocol_envelope_changes_and_history_is_immutable(self):
        contract = m.load_plan()
        frozen = copy.deepcopy(contract)
        old = m.current_projection(contract)
        current = m.current_details_v2(contract)
        rich = current['details']
        self.assertEqual(rich['target_details_version'], 2)
        rich['target_details_version'] = 1
        self.assertEqual(current, old)
        self.assertEqual(contract, frozen)

    def test_unexpected_old_protocol_or_field_is_rejected(self):
        contract = m.load_plan()
        # V2 is an explicit migration from a verified v1 stage, never a union.
        for kind in ['version', 'field']:
            wrong = copy.deepcopy(contract)
            rich = wrong['candidate']['details']
            if kind == 'version': rich['target_details_version'] = 2
            else:
                rich['queries'][0]['targets'][0].update(origin='binary', kind='Field')
            with self.assertRaises(AssertionError): m.current_details_v2(wrong)

if __name__ == '__main__':
    unittest.main()
