#!/usr/bin/env python3
"""Offline adversarial controls for native configure's real-project checker."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location('configure_gate',
                                             Path(__file__).with_name('check-configure-acceptance.py'))
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)


def model():
    return {'schema_version': 1, 'compilations': [
        {'id': {'project': 'opaque/project', 'source_set': 'main:custom'},
         'source_roots': ['../sources/a', '../sources/b'],
         'dependencies': [{'project': 'deps', 'source_set': 'main'},
                          {'project': 'deps', 'source_set': 'other'}],
         'classpath': [{'kind': 'jar', 'path': '../inputs/a.jar'},
                       {'kind': 'directory', 'path': '../inputs/classes'},
                       {'kind': 'jar', 'path': '../inputs/b.jar'}],
         'platform': [{'kind': 'jmod', 'path': '../inputs/base.jmod'},
                      {'kind': 'jar', 'path': '../inputs/platform.jar'}],
         'target_release': 21},
        {'id': {'project': 'deps', 'source_set': 'main'}, 'source_roots': ['../deps'],
         'dependencies': [], 'classpath': [], 'platform': [], 'target_release': 21}]}


class ManifestControls(unittest.TestCase):
    def setUp(self):
        self.base = Path('/fixture/config')
        self.manual = model()
        self.generated = gate.effective_manifest(self.manual, self.base)

    def check(self, changed):
        return gate.compare_manifests(changed, self.manual, self.base)

    def test_explicit_defaults_and_only_path_spelling_normalized(self):
        self.check(self.generated)
        equivalent = copy.deepcopy(self.generated)
        equivalent['compilations'][0]['source_roots'][0] = '/fixture/config/../sources/a'
        self.check(equivalent)
        self.assertEqual('opaque/project', self.generated['compilations'][0]['id']['project'])
        self.assertEqual('main:custom', self.generated['compilations'][0]['id']['source_set'])
        self.assertEqual(self.generated['limits'], gate.WORKSPACE_LIMITS)
        self.assertNotIn('limits', self.manual)
        self.assertNotIn('limits', self.manual['compilations'][0])

    def test_all_sequence_order_multiplicity_and_scope_changes_rejected(self):
        for field in ['source_roots', 'dependencies', 'classpath', 'platform']:
            for operation in ['reverse', 'remove', 'duplicate']:
                changed = copy.deepcopy(self.generated)
                sequence = changed['compilations'][0][field]
                if operation == 'reverse':
                    sequence.reverse()
                elif operation == 'remove':
                    sequence.pop()
                else:
                    sequence.append(copy.deepcopy(sequence[0]))
                with self.subTest(field=field, operation=operation), self.assertRaises(gate.AcceptanceError):
                    self.check(changed)
        for operation in ['reverse', 'remove', 'duplicate']:
            changed = copy.deepcopy(self.generated)
            if operation == 'reverse':
                changed['compilations'].reverse()
            elif operation == 'remove':
                changed['compilations'].pop()
            else:
                changed['compilations'].append(copy.deepcopy(changed['compilations'][0]))
            with self.subTest(operation=operation), self.assertRaises(gate.AcceptanceError):
                self.check(changed)

    def test_every_path_must_be_absolute_and_same_location(self):
        for field in ['source_roots', 'classpath', 'platform']:
            for wrong in ['relative/path', '/other/scope/path', '/fixture/sources/a/../../wrong', '']:
                changed = copy.deepcopy(self.generated)
                if field == 'source_roots':
                    changed['compilations'][0][field][0] = wrong
                else:
                    changed['compilations'][0][field][0]['path'] = wrong
                with self.subTest(field=field, wrong=wrong), self.assertRaises(gate.AcceptanceError):
                    self.check(changed)

    def test_opaque_identifiers_are_not_split_or_coalesced(self):
        changed = copy.deepcopy(self.generated)
        changed['compilations'][0]['id'] = {'project': 'opaque', 'source_set': 'project/main:custom'}
        with self.assertRaises(gate.AcceptanceError):
            self.check(changed)
        changed = copy.deepcopy(self.generated)
        changed['compilations'][0]['dependencies'][0]['source_set'] = 'test'
        with self.assertRaises(gate.AcceptanceError):
            self.check(changed)

    def test_all_default_budgets_must_be_explicit_and_exact(self):
        for field in ['limits', 'jar_limits', 'jmod_limits']:
            for key in self.generated['compilations'][0][field]:
                changed = copy.deepcopy(self.generated)
                changed['compilations'][0][field][key] += 1
                with self.subTest(field=field, key=key), self.assertRaises(gate.AcceptanceError):
                    self.check(changed)
            changed = copy.deepcopy(self.generated)
            del changed['compilations'][0][field]
            with self.assertRaises(gate.AcceptanceError):
                self.check(changed)
        for key in self.generated['limits']:
            changed = copy.deepcopy(self.generated)
            changed['limits'][key] += 1
            with self.subTest(key=key), self.assertRaises(gate.AcceptanceError):
                self.check(changed)
        changed = copy.deepcopy(self.generated)
        del changed['limits']
        with self.assertRaises(gate.AcceptanceError):
            self.check(changed)

    def test_schema_unknown_fields_wrong_types_and_kinds_rejected(self):
        mutations = [lambda x: x.update(schema_version=True), lambda x: x.update(command='configure'),
                     lambda x: x['compilations'][0].update(target_release=True),
                     lambda x: x['compilations'][0].update(target_release=17),
                     lambda x: x['compilations'][0].update(unrecognized=True),
                     lambda x: x['compilations'][0]['id'].update(project=['opaque/project']),
                     lambda x: x['compilations'][0]['classpath'][1].update(kind='class_directory'),
                     lambda x: x['compilations'][0]['platform'][0].update(kind='directory'),
                     lambda x: x['compilations'][0]['classpath'][0].update(unrecognized=True),
                     lambda x: x['limits'].update(unrecognized=1)]
        for mutate in mutations:
            changed = copy.deepcopy(self.generated)
            mutate(changed)
            with self.subTest(mutate=mutate), self.assertRaises(gate.AcceptanceError):
                self.check(changed)

    def test_strict_json_rejects_duplicate_keys_nonfinite_and_trailing_documents(self):
        for raw in ['{"a":1,"a":2}', '{"a":{"x":1,"x":2}}', '{"a":NaN}',
                    '{"a":Infinity}', '{"a":-Infinity}', '{}{}']:
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                gate.strict_json(raw)


class InspectionControls(unittest.TestCase):
    setUp = ManifestControls.setUp

    def response(self):
        record = {'id': self.manual['compilations'][0]['id'],
                  'configuration': copy.deepcopy(self.generated['compilations'][0]),
                  'source_paths': ['/fixture/sources/a/A.java', '/fixture/sources/b/B.java'],
                  'visible_source_paths': ['/fixture/deps/D.java', '/fixture/sources/a/A.java',
                                           '/fixture/sources/b/B.java'],
                  'class_origins': ['/fixture/inputs/a.jar!/A.class', '/fixture/inputs/b.jar!/B.class'],
                  'class_count': 2, 'shadowed_classes': []}
        return {'capabilities': {'build_tool_import': False},
                'result': {'detail_mode': 'full', 'workspace_limits': gate.WORKSPACE_LIMITS,
                           'compilations': [record]}}

    def compare(self, generated, manual):
        gate.compare_inspection(generated, manual, Path('/other/generated.json'), self.base / 'manual.json')

    def test_every_inventory_order_duplicate_edge_and_summary_is_compared(self):
        original = self.response()
        self.compare(original, original)
        for field in ['source_paths', 'visible_source_paths', 'class_origins']:
            for operation in ['reverse', 'remove', 'duplicate']:
                changed = copy.deepcopy(original)
                values = changed['result']['compilations'][0][field]
                if operation == 'reverse':
                    values.reverse()
                elif operation == 'remove':
                    values.pop()
                else:
                    values.append(values[0])
                with self.subTest(field=field, operation=operation), self.assertRaises(gate.AcceptanceError):
                    self.compare(changed, original)
        mutations = [lambda r: r.update(class_count=1), lambda r: r.update(shadowed_classes=[{}]),
                     lambda r: r['configuration']['dependencies'].reverse(),
                     lambda r: r['configuration']['platform'].reverse(),
                     lambda r: r['configuration']['classpath'].reverse()]
        for mutate in mutations:
            changed = copy.deepcopy(original)
            mutate(changed['result']['compilations'][0])
            with self.assertRaises(gate.AcceptanceError):
                self.compare(changed, original)
        changed = copy.deepcopy(original)
        changed['result']['workspace_limits']['max_entries'] += 1
        with self.assertRaises(gate.AcceptanceError):
            self.compare(changed, original)


class InvocationControls(unittest.TestCase):
    def test_binary_projection_changes_only_explicit_source_graph_fields(self):
        original = model()
        projected = gate.binary_projection(original, 0)
        self.assertEqual(1, len(projected['compilations']))
        actual = projected['compilations'][0]
        self.assertEqual([], actual['source_roots'])
        self.assertEqual([], actual['dependencies'])
        for key in ['id', 'classpath', 'platform', 'target_release']:
            self.assertEqual(original['compilations'][0][key], actual[key])
        self.assertEqual(model(), original)

    def test_full_graph_detail_limit_must_not_be_raised_or_hidden(self):
        error = b'{"schema_version":1,"error":{"code":"output_limit","message":"bounded"}}\n'
        with mock.patch.object(gate.commons, 'run_bounded', return_value=(2, b'', error)):
            gate.detail_output_limit(Path('/native'), Path('/cwd'), Path('/manifest'))
        for result in [(0, b'{}\n', b''), (2, b'', error.replace(b'output_limit', b'io'))]:
            with mock.patch.object(gate.commons, 'run_bounded', return_value=result), self.assertRaises(ValueError):
                gate.detail_output_limit(Path('/native'), Path('/cwd'), Path('/manifest'))

    def test_inputs_independent_of_manual_manifest_contents(self):
        fixture = gate.second.Fixture()
        fixture.source_root = Path('/fixture/sources')
        fixture.inputs = {key: Path('/inputs') / (key + '.artifact')
                          for key in ['java_base', 'java_sql', 'junit', 'hamcrest', 'jsr250']}
        fixture.manifest_data = {'deliberately': 'unusable'}
        args = gate.gson_arguments(fixture, Path('/fixture/cwd'))
        self.assertEqual(4, args.count('--compilation'))
        self.assertEqual(3, args.count('--depends-on'))
        self.assertEqual(8, args.count('--platform-jmod'))
        self.assertEqual(6, args.count('--classpath-jar'))
        first = args.index('--compilation', 1)
        self.assertEqual(['extras', 'test'], args[first+1:first+3])
        self.assertIn('../sources/gson/src/main/java/com', args)
        self.assertNotIn('unusable', args)
        self.assertEqual(['--compilation', 'commons-lang', 'main', '--source-root', '../sources',
                          '--platform-jmod', '../../inputs/base.jmod', '--target-release', '21'],
                         gate.commons_arguments(Path('/fixture/sources'), Path('/inputs/base.jmod'),
                                                Path('/fixture/cwd')))

    def test_configure_requires_zero_exit_empty_stderr_strict_output_and_no_writes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            manual = model()
            generated = gate.effective_manifest(manual, root)
            raw = (json.dumps(generated) + '\n').encode()
            def invoke():
                return gate.configure(Path('/native'), [], root, root, manual, root)
            with mock.patch.object(gate.commons, 'run_bounded', return_value=(0, raw, b'')) as run:
                self.assertEqual(raw, invoke()[0])
                self.assertEqual(1024*1024, run.call_args.args[3])
            for result in [(2, raw, b''), (0, raw, b'warning'), (0, raw.rstrip(), b''), (0, b'{"schema_version":true}', b''),
                           (0, b'{}{}', b'')]:
                with self.subTest(result=result), mock.patch.object(gate.commons, 'run_bounded', return_value=result), self.assertRaises(ValueError):
                    invoke()
            def writes(*unused):
                (root / 'unexpected-output').write_text('must be detected')
                return 0, raw, b''
            with mock.patch.object(gate.commons, 'run_bounded', side_effect=writes), self.assertRaises(gate.AcceptanceError):
                invoke()

    def test_failure_is_json_exit_two_without_partial_stdout(self):
        error = b'{"schema_version":1,"error":{"code":"invalid_configuration","message":"bad"}}\n'
        gate.check_failure(2, b'', error)
        for code, stdout, stderr in [(0, b'', error), (1, b'', error), (2, b'{}', error), (2, b'', error.rstrip()),
                                     (2, b'', b'not JSON'), (2, b'', b'{"schema_version":true,"error":{}}'),
                                     (2, b'', b'{"schema_version":1,"error":{"code":"bad","message":""}}')]:
            with self.subTest(code=code, stdout=stdout), self.assertRaises(ValueError):
                gate.check_failure(code, stdout, stderr)

    def test_generated_source_gaps_cannot_be_fabricated(self):
        with tempfile.TemporaryDirectory() as tmp:
            f = gate.second.Fixture()
            f.source_root = Path(tmp)
            f.sources = {}
            f.plan = [{'id': 'missing_generated_type'}, {'id': 'missing_generated_field'}]
            rows = [{'status': 'Unresolved', 'targets': []}, {'status': 'ReceiverNotBound', 'targets': []}]
            gate.missing_generated(rows, f)
            for index in [0, 1]:
                changed = copy.deepcopy(rows)
                changed[index]['status'] = 'Resolved'
                with self.assertRaises(gate.AcceptanceError):
                    gate.missing_generated(changed, f)
                changed = copy.deepcopy(rows)
                changed[index]['targets'] = [{}]
                with self.assertRaises(gate.AcceptanceError):
                    gate.missing_generated(changed, f)
            template = f.source_root / gate.GSON_TEMPLATE
            template.parent.mkdir(parents=True)
            template.write_text('raw excluded template')
            with self.assertRaises(gate.AcceptanceError):
                gate.missing_generated(rows, f)
            with mock.patch.object(gate.second, 'digest', return_value=gate.GSON_TEMPLATE_SHA256):
                gate.missing_generated(rows, f)
                f.sources[gate.GSON_TEMPLATE] = b'raw excluded template'
                with self.assertRaises(gate.AcceptanceError):
                    gate.missing_generated(rows, f)
                f.sources.clear()
            template.unlink()
            (f.source_root / 'GsonBuildConfig.java').write_text('')
            with self.assertRaises(gate.AcceptanceError):
                gate.missing_generated(rows, f)

    def test_commons_historical_parser_gaps_cannot_be_erased_or_invented(self):
        root = Path('/sources')
        expected = {p: int(n) for p, n in gate.commons.rows(gate.commons.CORPUS / 'parser-errors.tsv', 2)}
        parser = {'files': [{'path': str(root / p), 'reports': [{}] * n} for p, n in expected.items()],
                  'report_count': sum(expected.values()), 'files_with_reports': len(expected)}
        gate.preserve_commons_parser(parser, root)
        for operation in ['omit', 'add', 'summary']:
            changed = copy.deepcopy(parser)
            if operation == 'omit':
                changed['files'].pop()
            elif operation == 'add':
                changed['files'][0]['reports'].append({})
            else:
                changed['report_count'] += 1
            with self.subTest(operation=operation), self.assertRaises(gate.AcceptanceError):
                gate.preserve_commons_parser(changed, root)

    def test_reports_never_overwrite_old_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'report.json'
            gate.write_new(path, {'original': True})
            with self.assertRaises(FileExistsError):
                gate.write_new(path, {'replaced': True})
            self.assertEqual({'original': True}, json.loads(path.read_text()))


if __name__ == '__main__':
    unittest.main()
