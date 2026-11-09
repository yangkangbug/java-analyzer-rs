"""Offline fail-closed controls for additive real Gson instance-call acceptance."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


second = load('reference_second', 'check-second-project.py')
target_tests = load('reference_target_tests', 'test_second_project.py')


class ReferenceResponseControls(unittest.TestCase):
    def setUp(self):
        helper = target_tests.TargetValidationTests()
        helper.setUp()
        self.fixture = helper.fixture
        self.query = {**helper.query, 'expected': helper.expected}
        alternate = copy.deepcopy(helper.expected['targets'][0])
        alternate['start'] += 1
        self.query['baseline'] = {'status': 'OverloadNotResolved',
                                  'targets': [helper.expected['targets'][0], alternate]}
        self.response = {
            'schema_version': 1, 'command': 'declaration',
            'snapshot': {'id': 'unit-test', 'identity': 'invocation_local',
                         'manifest': str(self.fixture.manifest)},
            'capabilities': {'runtime': 'native_rust_no_jvm',
                             'build_tool_import': False, 'semantic_diagnostics': False},
            'result': {'index_builds': 1, 'queries': [helper.row]},
        }

    def check(self, response=Ellipsis, stage='expected'):
        second.check_reference_call_response(
            self.response if response is Ellipsis else response, [self.query], self.fixture, stage)

    def baseline(self):
        response = copy.deepcopy(self.response)
        response['result']['queries'][0].update(
            status='OverloadNotResolved',
            targets=[second.expected_target(t, self.fixture)
                     for t in self.query['baseline']['targets']])
        return response

    def test_exact_target_and_strict_baseline_are_distinct(self):
        self.check()
        self.check(self.baseline(), 'baseline')
        with self.assertRaises(second.AcceptanceError):
            self.check(self.baseline())
        with self.assertRaises(second.AcceptanceError):
            self.check(stage='baseline')
        with self.assertRaises(second.AcceptanceError):
            self.check(stage='capture-any-failure')

    def test_capture_baseline_never_swallows_malformed_or_changed_results(self):
        self.query['spelling'] = 'Alpha'
        malformed = self.baseline()
        malformed['result']['queries'][0]['targets'].pop()
        with mock.patch.object(second, 'reference_call_probes',
                               return_value=([self.query], {}, 'unused')), \
                mock.patch.object(second, 'all_source_hashes', return_value={}), \
                mock.patch.object(second, 'document', return_value=Path('/fixture/requests.json')), \
                mock.patch.object(second, 'successful', return_value=malformed):
            with self.assertRaises(second.AcceptanceError):
                second.run_reference_call_acceptance(Path('/native'), self.fixture,
                                                     capture_baseline=True)

    def test_refusal_candidate_multiset_preserves_duplicates_not_order(self):
        response = self.baseline()
        response['result']['queries'][0]['targets'].reverse()
        self.check(response, 'baseline')
        response['result']['queries'][0]['targets'].append(
            copy.deepcopy(response['result']['queries'][0]['targets'][0]))
        with self.assertRaises(second.AcceptanceError):
            self.check(response, 'baseline')
        response = self.baseline()
        response['result']['queries'][0]['targets'].pop()
        with self.assertRaises(second.AcceptanceError):
            self.check(response, 'baseline')

    def test_every_query_and_target_field_is_required_without_extra_fields(self):
        row = self.response['result']['queries'][0]
        for key in row:
            response = copy.deepcopy(self.response)
            del response['result']['queries'][0][key]
            with self.subTest(missing=key), self.assertRaises(second.AcceptanceError):
                self.check(response)
        for key in row['targets'][0]:
            response = copy.deepcopy(self.response)
            del response['result']['queries'][0]['targets'][0][key]
            with self.subTest(missing_target=key), self.assertRaises(second.AcceptanceError):
                self.check(response)
        for where in ['row', 'target', 'range', 'result']:
            response = copy.deepcopy(self.response)
            row = response['result']['queries'][0]
            target = row['targets'][0]
            {'row': row, 'target': target, 'range': target['range'],
             'result': response['result']}[where]['unexpected'] = 'ignored?'  # Must fail.
            with self.subTest(extra=where), self.assertRaises(second.AcceptanceError):
                self.check(response)

    def test_missing_extra_reordered_and_duplicate_queries_fail(self):
        for rows in [[], [self.response['result']['queries'][0]] * 2, None, {}, 'query']:
            response = copy.deepcopy(self.response)
            response['result']['queries'] = rows
            with self.subTest(rows=rows), self.assertRaises(second.AcceptanceError):
                self.check(response)
        query2 = copy.deepcopy(self.query)
        query2['id'] = 'second'
        query2['byte'] += 1
        with self.assertRaises(second.AcceptanceError):
            second.check_reference_call_response(self.response, [self.query, query2], self.fixture)
        response = copy.deepcopy(self.response)
        row2 = copy.deepcopy(response['result']['queries'][0])
        row2.update(query_index=1, byte_offset=query2['byte'],
                    position={'encoding': 'utf8', 'offset': query2['byte']})
        response['result']['queries'].append(row2)
        second.check_reference_call_response(response, [self.query, query2], self.fixture)
        response['result']['queries'].reverse()
        with self.assertRaises(second.AcceptanceError):
            second.check_reference_call_response(response, [self.query, query2], self.fixture)
        response['result']['queries'] = [copy.deepcopy(row2), copy.deepcopy(row2)]
        with self.assertRaises(second.AcceptanceError):
            second.check_reference_call_response(response, [self.query, query2], self.fixture)

    def test_malformed_envelope_and_result_fail_closed(self):
        for response in [None, [], {}, {'result': {}}, {'result': {'queries': [None]}}]:
            with self.subTest(response=response), self.assertRaises(second.AcceptanceError):
                self.check(response)
        for key in ['schema_version', 'command', 'snapshot', 'capabilities', 'result']:
            response = copy.deepcopy(self.response)
            del response[key]
            with self.subTest(missing=key), self.assertRaises(second.AcceptanceError):
                self.check(response)
        for builds in [0, 2, True, '1', None]:
            response = copy.deepcopy(self.response)
            response['result']['index_builds'] = builds
            with self.subTest(builds=builds), self.assertRaises(second.AcceptanceError):
                self.check(response)

    def test_duplicate_json_keys_fail_before_comparison(self):
        for text in ['{"result": {}, "result": {}}',
                     '{"result": {"queries": [], "queries": []}}',
                     '{"range": {"start_byte": 1, "start_byte": 2}}']:
            with self.subTest(text=text), self.assertRaises(second.AcceptanceError):
                json.loads(text, object_pairs_hook=second.unique_json_object)

    def test_semantic_identity_mutations_cannot_pass(self):
        mutations = [
            lambda r: r.update(status='OverloadNotResolved'),
            lambda r: r.update(query_index=True),
            lambda r: r.update(byte_offset=1),
            lambda r: r.update(file='/wrong.java'),
            lambda r: r.update(source_compilation=second.compilation('gson/test')),
            lambda r: r['position'].update(encoding='utf16'),
            lambda r: r['targets'][0].update(path='/wrong.java'),
            lambda r: r['targets'][0].update(owner='WrongOwner'),
            lambda r: r['targets'][0].update(kind='Field'),
            lambda r: r['targets'][0].update(name='wrong'),
            lambda r: r['targets'][0].update(origin='binary'),
            lambda r: r['targets'][0].update(source_available=False),
            lambda r: r['targets'][0].update(compilation=second.compilation('gson/test')),
            lambda r: r.update(targets=None),
            lambda r: r.update(targets=['not an object']),
        ]
        for field in ['start_byte', 'end_byte', 'start_utf16', 'end_utf16']:
            mutations.append(lambda r, field=field: r['targets'][0]['range'].__setitem__(field, 0))
        for mutate in mutations:
            response = copy.deepcopy(self.response)
            mutate(response['result']['queries'][0])
            with self.subTest(mutation=mutate), self.assertRaises(second.AcceptanceError):
                self.check(response)


class FrozenPlanControls(unittest.TestCase):
    def test_preserves_historical_artifacts_and_exact_frozen_scope(self):
        spec = json.loads((second.CORPUS / 'reference-call-probes.json').read_text())
        self.assertTrue(spec['selected_before_implementation'])
        self.assertEqual('751c68b759848ec1d2fef620a323d54a67771817f0e727ef3f630f8da660e7df',
                         spec['parser_result_canonical_sha256'])
        self.assertEqual('e0a47eb85ad11d6aa274654b263703fbdd5786c7', spec['base_commit'])
        for filename, checksum in spec['historical_artifact_sha256'].items():
            self.assertEqual(checksum, second.digest(second.CORPUS / filename))
        self.assertEqual({'query-plan.json', 'expected.json', 'local-type-probes.json',
                          'local-type-comparison.json', 'parser-errors.tsv'},
                         set(spec['historical_artifact_sha256']))
        queries = spec['queries']
        self.assertEqual([1814, 1901, 1976], [q['byte'] for q in queries])
        self.assertEqual(3, len({q['id'] for q in queries}))
        for q in queries[:2]:
            self.assertEqual('OverloadNotResolved', q['baseline']['status'])
            self.assertEqual([2652, 2918, 3192, 3454, 3703],
                             [t['start'] for t in q['baseline']['targets']])
            self.assertEqual('Resolved', q['expected']['status'])
            self.assertEqual([(3703, 3706)],
                             [(t['start'], t['end']) for t in q['expected']['targets']])
        self.assertEqual(queries[2]['baseline'], queries[2]['expected'])
        self.assertEqual([4945, 5494], [t['start'] for t in queries[2]['expected']['targets']])
        historical = json.loads((second.CORPUS / 'local-type-comparison.json').read_text())
        self.assertEqual(226, len(historical['all_226_source_sha256']))
        self.assertEqual(211, len(historical['parser_after']))
        self.assertEqual(331, sum(historical['parser_after'].values()))
        self.assertEqual(58, sum(bool(n) for n in historical['parser_after'].values()))

    def test_ci_keeps_three_separate_no_java_gates(self):
        workflow = (second.ROOT / '.github/workflows/extended-acceptance.yml').read_text()
        cases = [('Query second real project through shipped CLI', None),
                 ('Additive Gson local-type containment probes', '--local-types'),
                 ('Additive Gson reference-call proof and preservation', '--reference-calls')]
        for name, flag in cases:
            body = workflow.split('name: ' + name, 1)[1].split('      - name:', 1)[0]
            self.assertIn('check-second-project.py', body)
            self.assertNotIn('--capture-baseline', body)
            if flag:
                self.assertIn(flag, body)
            else:
                self.assertNotIn('--local-types', body)
                self.assertNotIn('--reference-calls', body)
        self.assertIn("'scripts/test_reference_call_acceptance.py'", workflow)
        self.assertIn("'scripts/test_reference_oracle.py'", workflow)

    def test_source_inventory_detects_missing_extra_modified_and_symlinked_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            fixture = second.Fixture()
            fixture.source_root = Path(tmp)
            for i in range(226):
                (fixture.source_root / f'C{i}.java').write_text(f'class C{i} {{}}')
            expected = second.all_source_hashes(fixture)
            path = fixture.source_root / 'C0.java'
            path.write_text('class Changed {}')
            self.assertNotEqual(expected, second.all_source_hashes(fixture))
            path.unlink()
            with self.assertRaises(second.AcceptanceError):
                second.all_source_hashes(fixture)
            path.symlink_to(fixture.source_root / 'C1.java')
            with self.assertRaises(second.AcceptanceError):
                second.all_source_hashes(fixture)
            path.unlink()
            path.write_text('class C0 {}')
            (fixture.source_root / 'Extra.java').write_text('class Extra {}')
            with self.assertRaises(second.AcceptanceError):
                second.all_source_hashes(fixture)

    def test_parser_inventory_is_exact_not_only_total_count(self):
        fixture = second.Fixture()
        fixture.source_root = Path('/fixture')
        historical = {'parser_after': {'A.java': 330, 'B.java': 1}}
        parser = {'report_count': 331, 'files_with_reports': 58,
                  'files': [{'path': '/fixture/A.java', 'reports': [None] * 330},
                            {'path': '/fixture/B.java', 'reports': [None]}]}
        expected_digest = second.parser_result_digest(parser, fixture)
        with mock.patch.object(second, 'check_parser'):
            second.check_reference_parser(parser, fixture, historical, expected_digest)
            parser['files'][0]['reports'].pop()
            parser['files'][1]['reports'].append(None)
            with self.assertRaises(second.AcceptanceError):
                second.check_reference_parser(parser, fixture, historical, expected_digest)


    def test_same_count_parser_message_and_range_mutations_fail(self):
        fixture = second.Fixture()
        fixture.source_root = Path('/fixture')
        historical = {'parser_after': {'A.java': 1}}
        original = {'report_count': 331, 'files_with_reports': 58,
                    'files': [{'path': '/fixture/A.java', 'reports': [{
                        'message': 'expected expression',
                        'range': {'start_byte': 10, 'end_byte': 15,
                                  'start_utf16': 10, 'end_utf16': 15}}]}]}
        expected_digest = second.parser_result_digest(original, fixture)
        with mock.patch.object(second, 'check_parser'):
            second.check_reference_parser(original, fixture, historical, expected_digest)
            for field in ['message', 'start_byte', 'end_byte', 'start_utf16', 'end_utf16']:
                parser = copy.deepcopy(original)
                report = parser['files'][0]['reports'][0]
                if field == 'message':
                    report[field] = 'different message with same report count'
                else:
                    report['range'][field] += 1
                with self.subTest(field=field), self.assertRaises(second.AcceptanceError):
                    second.check_reference_parser(parser, fixture, historical, expected_digest)


if __name__ == '__main__':
    unittest.main()
