"""Synthetic full parser records test real cross-layer stage propagation.

Only the frozen contract loader is replaced for synthetic messages/digests.
check_parser and check_reference_parser are real, not mocked. This is not a
claim that an ancient feature-baseline executable was run.
"""
import copy
from contextlib import ExitStack
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest import mock
from test_reference_return_acceptance import gate, second


class ReturnStageControls(unittest.TestCase):
    def setUp(self):
        rows = [s.split('\t') for s in (second.CORPUS / 'class-literal-parser-comparison.tsv').read_text().splitlines() if not s.startswith('#')]
        self.old = {p: int(a) for p, a, b in rows}
        self.new = {p: int(b) for p, a, b in rows}
        self.f = SimpleNamespace(source_root=Path('/synthetic/sources'), sources={p: b'x' for p in self.old},
                                 owned={k: [] for k in second.ROOTS}, metadata={}, plan=[], expected=[])
        for p in self.old:
            key = 'gson/main' if '/src/main/' in p else 'gson/test'
            self.f.owned[key].append(str(self.f.source_root / p))
        for paths in self.f.owned.values(): paths.sort()
        self.parsers = {stage: self.parser(inv) for stage, inv in [('baseline', self.old), ('current', self.new)]}

        self.parsers['switch'] = copy.deepcopy(self.parsers['current'])
        for row in self.parsers['switch']['files']:
            for report in row['reports']:
                report['message'] = 'synthetic switch stage'

    def parser(self, inventory):
        report = {'classification': 'parser_limit_or_invalid_syntax', 'message': 'synthetic',
                  'range': dict(start_byte=0, end_byte=1, start_utf16=0, end_utf16=1)}
        return dict(classification='parser_limit_or_invalid_syntax', java_semantic_diagnostics=False,
                    scope='owned_sources_only', source_count=211,
                    report_count=sum(inventory.values()), files_with_reports=sum(bool(n) for n in inventory.values()),
                    files=[{'compilation': second.compilation(k), 'path': p,
                            'reports': [copy.deepcopy(report) for _ in range(inventory[Path(p).relative_to(self.f.source_root).as_posix()])]}
                           for k in sorted(self.f.owned) for p in self.f.owned[k]])

    def run_chain(self, caller, baseline, actual_stage):
        old_digest = second.parser_result_digest(self.parsers['baseline'], self.f)
        current_digest = second.parser_result_digest(self.parsers['current'], self.f)
        with ExitStack() as stack:
            patch = lambda obj, name, **kw: stack.enter_context(mock.patch.object(obj, name, **kw))
            patch(second, 'class_literal_parser_contract', return_value=(self.new, current_digest))
            patch(second, 'switch_parser_contract',
                  return_value=(self.new, second.parser_result_digest(self.parsers['switch'], self.f),
                                sum(self.new.values()), sum(bool(n) for n in self.new.values())))
            patch(second, 'reference_call_probes', return_value=([], {'parser_after': self.old}, old_digest))
            patch(second, 'constructor_expectations', return_value=[])
            original = patch(second, 'current_original_expectations', return_value=[])
            patch(second, 'local_type_probes', return_value=[])
            overlay = patch(second, 'qualified_reference_probes', side_effect=lambda f, p: p)
            for name in ['check_reference_call_response', 'check_local_type_response', 'check_response']:
                patch(second, name)
            patch(second, 'successful', side_effect=lambda n, f, cmd, **kw: {'result': self.parsers[actual_stage]} if cmd == 'parser-report' else {'result': {'index_builds': 1}})
            patch(second, 'document', return_value=Path('request'))
            patch(second, 'all_source_hashes', return_value={})
            patch(second, 'portable_queries', return_value=[])
            patch(second, 'digest', return_value=gate.strict.returns.load_plan(second)['baseline_native_sha256'])
            for module in [gate, gate.strict, gate.strict.string]:
                patch(module, 'check_response')
            patch(gate.strict, 'probes', return_value=[])
            patch(gate.strict, 'current_probes', return_value=[])
            patch(gate.strict, 'frozen_qualified_probes', return_value=[])
            patch(gate.strict, 'frozen_mixed_probes', return_value=[])
            patch(gate.strict, 'frozen_return_probes', return_value=[])
            patch(gate.strict, 'frozen_null_probes', return_value=[])
            patch(gate.strict.string, 'probes', return_value=[])
            caller(Path('native'), self.f, baseline)
            self.assertEqual(not baseline or caller is gate.run, overlay.called)
            if original.called:
                stage = 'mixed' if baseline and caller is gate.run else ('pre_primitive' if baseline else 'source_invocation')
                original.assert_called_once_with(self.f, stage)

    def test_every_layer_rejects_parser_stage_swap(self):
        for caller in [second.run_reference_call_acceptance, second.run_constructor_call_acceptance,
                       gate.strict.string.run, gate.strict.run, gate.run]:
            for baseline in [False, True]:
                expected = ('baseline' if caller is not gate.run else 'current') if baseline else 'switch'
                with self.subTest(caller=caller.__module__, baseline=baseline):
                    self.run_chain(caller, baseline, expected)
                    for wrong_stage in {'baseline', 'current', 'switch'} - {expected}:
                        with self.assertRaises(second.AcceptanceError):
                            self.run_chain(caller, baseline, wrong_stage)

    def test_invalid_stage_parameters_fail_closed(self):
        for args in [(False, 1, 'current'), (False, 'mixed', 'both'), (1, 'pre_primitive', 'baseline'), (False, 'auto', 'current')]:
            with self.assertRaises(second.AcceptanceError): second.proof_stages(*args)
        self.assertEqual(('mixed', 'current'), second.proof_stages(False, 'mixed', 'current'))

if __name__ == '__main__': unittest.main()
