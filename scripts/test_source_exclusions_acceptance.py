#!/usr/bin/env python3
"""Offline adversarial mutations of the independent source-selection oracle."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location('selection_gate', Path(__file__).with_name('check-source-exclusions-acceptance.py'))
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)


def model():
    return {'schema_version': 1, 'compilations': [
        {'id': {'project': 'gson', 'source_set': 'main'}, 'source_roots': ['sources/gson/src/main/java'],
         'source_exclusions': ['module-info.java', 'excluded'],
         'dependencies': [{'project': 'dependency', 'source_set': 'main'}, {'project': 'other', 'source_set': 'main'}],
         'classpath': [{'kind': 'jar', 'path': 'a.jar'}, {'kind': 'jar', 'path': 'b.jar'}],
         'platform': [{'kind': 'jmod', 'path': 'base.jmod'}, {'kind': 'jmod', 'path': 'sql.jmod'}],
         'target_release': 21},
        {'id': {'project': 'extras', 'source_set': 'test'}, 'source_roots': ['sources/extras/src/test/java'],
         'dependencies': [], 'classpath': [], 'platform': [], 'target_release': 21}]}


def pom():
    return b'''<project xmlns="http://maven.apache.org/POM/4.0.0"><build><plugins><plugin>
      <artifactId>maven-compiler-plugin</artifactId><executions><execution><id>default-compile</id>
      <configuration><excludes><exclude>module-info.java</exclude></excludes></configuration>
      </execution></executions></plugin></plugins></build></project>'''


class RuleControls(unittest.TestCase):
    def test_literal_paths_match_exact_files_or_directory_component_subtrees(self):
        for path in ['module-info.java', 'module-info.java/A.java']:
            self.assertTrue(gate.matches(path, ['module-info.java']))
        for path in ['nested/module-info.java', 'module-info.java.other']:
            self.assertFalse(gate.matches(path, ['module-info.java']))
        for path in ['gen', 'gen/A.java', 'gen/nested/A.java']:
            self.assertTrue(gate.matches(path, ['gen']))
        for path in ['generation/A.java', 'parent/gen/A.java', 'Gen/A.java']:
            self.assertFalse(gate.matches(path, ['gen']))
        # Each root uses its own relative path: an explicitly nested root can
        # reinclude B.java even when the parent-root walk prunes nested/B.java.
        self.assertTrue(gate.matches('nested/B.java', ['nested']))
        self.assertFalse(gate.matches('B.java', ['nested']))

    def test_empty_dot_parent_and_separators_rejected(self):
        for rule in ['', '/', '//', './A.java', '../A.java', 'a/../A.java', 'a/./A.java',
                     'a//b', 'a//', 'a/', '/A.java', 'C:/A.java', 'a\\b', '\\A.java']:
            with self.subTest(rule=rule), self.assertRaises(ValueError):
                gate.rules([rule])

    def test_globs_and_braces_rejected(self):
        for rule in ['*.java', '**/A.java', '?.java', '[ab].java', 'a].java', '{a,b}/', 'x}.java']:
            with self.subTest(rule=rule), self.assertRaises(ValueError):
                gate.rules([rule])

    def test_exactly_cc_controls_rejected_valid_unicode_not_broadened(self):
        for code in [*range(32), *range(127, 160)]:
            with self.subTest(code=code), self.assertRaises(ValueError):
                gate.rules(['a' + chr(code) + '.java'])
        for rule in ['!literal.java', '--literal.java', '中文/😀.java', 'a\u200d.java',
                     'a\u00a0.java', 'a\u2028.java', 'a\u2029.java', 'a\u0301.java']:
            self.assertEqual([rule], gate.rules([rule]))

    def test_exact_rule_count_and_utf8_limits(self):
        gate.rules(['x' * 4096])
        gate.rules(['中' * 1365 + 'a'])
        gate.rules([str(i) for i in range(1024)])
        for rules in [['x' * 4097], ['中' * 1366], [str(i) for i in range(1025)], ['a', 'a']]:
            with self.assertRaises(ValueError):
                gate.rules(rules)
        for value in [None, 'a', {}, True, [1], [False]]:
            with self.assertRaises(ValueError):
                gate.rules(value)


class ManifestControls(unittest.TestCase):
    def setUp(self):
        self.base = Path('/fixture')
        self.manual = model()
        self.generated = gate.effective_manifest(self.manual, self.base)
        self.generated['compilations'][1].pop('source_exclusions')

    def check(self, value):
        return gate.compare_manifests(value, self.manual, self.base)

    def test_optional_empty_rules_are_only_additive_extension(self):
        self.check(self.generated)
        self.assertNotIn('source_exclusions', self.generated['compilations'][1])
        empty = copy.deepcopy(self.generated)
        empty['compilations'][1]['source_exclusions'] = []
        with self.assertRaises(ValueError): self.check(empty)
        with self.assertRaises(ValueError):
            gate.configure_gate.effective_manifest(self.manual, self.base)
        changed = copy.deepcopy(self.generated)
        changed['compilations'][0]['source_exclusions'].append('extra.java')
        with self.assertRaises(ValueError):
            self.check(changed)

    def test_nonempty_exclusions_require_roots_without_banning_nested_roots(self):
        manual = copy.deepcopy(self.manual)
        manual['compilations'][0]['source_roots'] = []
        with self.assertRaises(ValueError): gate.effective_manifest(manual, self.base)
        manual['compilations'][0]['source_exclusions'] = []
        gate.effective_manifest(manual, self.base)
        manual['compilations'][0].update(source_roots=['src', 'src/nested'],
                                         source_exclusions=['nested'])
        gate.effective_manifest(manual, self.base)

    def test_rule_order_multiplicity_missing_or_moved_scope_rejected(self):
        for operation in ['reverse', 'duplicate', 'remove', 'missing', 'scope']:
            value = copy.deepcopy(self.generated)
            rules = value['compilations'][0]['source_exclusions']
            if operation == 'reverse': rules.reverse()
            elif operation == 'duplicate': rules.append(rules[0])
            elif operation == 'remove': rules.pop()
            elif operation == 'missing': value['compilations'][0].pop('source_exclusions')
            else: value['compilations'][1]['source_exclusions'] = value['compilations'][0].pop('source_exclusions')
            with self.subTest(operation=operation), self.assertRaises(ValueError):
                self.check(value)

    def test_every_ordered_input_and_edge_preserved(self):
        for field in ['dependencies', 'classpath', 'platform', 'source_roots']:
            for operation in ['reverse', 'remove', 'duplicate']:
                value = copy.deepcopy(self.generated)
                rows = value['compilations'][0][field]
                if operation == 'reverse' and len(rows) < 2:
                    continue
                if operation == 'reverse': rows.reverse()
                elif operation == 'remove': rows.pop()
                else: rows.append(copy.deepcopy(rows[0]))
                with self.subTest(field=field, operation=operation), self.assertRaises(ValueError):
                    self.check(value)

    def test_all_default_budgets_preserved(self):
        fields = [(self.generated['limits'], ('limits',))]
        for field in ['limits', 'jar_limits', 'jmod_limits']:
            fields.append((self.generated['compilations'][0][field], ('compilations', 0, field)))
        for limits, path in fields:
            for key in limits:
                value = copy.deepcopy(self.generated)
                target = value
                for component in path: target = target[component]
                target[key] += 1
                with self.subTest(path=path, key=key), self.assertRaises(ValueError): self.check(value)

    def test_types_unknown_fields_paths_and_compilations_strict(self):
        mutations = [lambda x: x.update(schema_version=True),
                     lambda x: x['compilations'][0].update(target_release=True),
                     lambda x: x['compilations'][0].update(source_exclusions='module-info.java'),
                     lambda x: x['compilations'][0].update(source_excludes=[]),
                     lambda x: x['compilations'][0].update(source_roots=['relative']),
                     lambda x: x['compilations'][0]['id'].update(source_set='test'),
                     lambda x: x['compilations'].reverse(),
                     lambda x: x['compilations'].append(copy.deepcopy(x['compilations'][0]))]
        for mutate in mutations:
            value = copy.deepcopy(self.generated); mutate(value)
            with self.assertRaises(ValueError): self.check(value)

    def test_strict_json_duplicate_nonfinite_and_extra_documents(self):
        for raw in ['{"a":1,"a":2}', '{"a":{"b":1,"b":2}}', '{"a":NaN}',
                    '{"a":Infinity}', '{}{}']:
            with self.assertRaises(ValueError): gate.strict_json(raw)

    def test_type_sensitive_equality_every_depth(self):
        for actual, expected in [(True, 1), (1.0, 1), ({'n': True}, {'n': 1}),
                                 ([{'n': False}], [{'n': 0}])]:
            with self.assertRaises(ValueError): gate.same(actual, expected, 'mutation')


class UpstreamControls(unittest.TestCase):
    def test_only_direct_default_compile_exclusion_accepted(self):
        self.assertEqual(['module-info.java'], gate.pom_semantics(pom())['excludes'])
        mutations = [pom().replace(b'default-compile', b'default-testCompile'),
                     pom().replace(b'module-info.java', b'**/module-info.java'),
                     pom().replace(b'maven-compiler-plugin', b'another-plugin'),
                     pom().replace(b'<build>', b'<profiles><profile><build>').replace(b'</build>', b'</build></profile></profiles>'),
                     pom().replace(b'<plugins>', b'<pluginManagement><plugins>').replace(b'</plugins>', b'</plugins></pluginManagement>'),
                     pom().replace(b'<exclude>module-info.java</exclude>', b'<exclude>module-info.java</exclude><exclude>other.java</exclude>'),
                     pom().replace(b'<id>default-compile</id>', b'<id>default-compile</id><id>other</id>'),
                     pom().replace(b'<excludes>', b'<testExcludes>').replace(b'</excludes>', b'</testExcludes>')]
        # Duplicate execution IDs are rejected even when an attacker repeats the same exclude.
        execution = pom().split(b'<executions>')[1].split(b'</executions>')[0]
        mutations.append(pom().replace(execution, execution + execution))
        for data in mutations:
            with self.subTest(data=data), self.assertRaises(ValueError): gate.pom_semantics(data)

    def test_entities_and_non_maven_documents_rejected(self):
        for data in [b'<!DOCTYPE x><project/>', b'<!ENTITY x "x">' + pom(), b'<project/>']:
            with self.assertRaises(ValueError): gate.pom_semantics(data)

    def test_member_digest_cannot_be_fabricated(self):
        data = b'original pinned member'
        checksum = hashlib.sha256(data).hexdigest()
        self.assertEqual(checksum, gate.verify_member(data, checksum, 'member'))
        for altered in [data + b' ', data[:-1], b'']:
            with self.assertRaises(ValueError): gate.verify_member(altered, checksum, 'member')

    def test_exact_archive_and_descriptor_identity_frozen(self):
        pins = gate.strict_json(gate.second.PINS.read_bytes())
        self.assertEqual(pins['artifacts']['sources']['sha256'], gate.SOURCE_ARCHIVE_SHA256)
        old = gate.strict_json((gate.second.CORPUS / 'local-type-comparison.json').read_bytes())
        self.assertEqual(gate.DESCRIPTOR_SHA256, old['all_226_source_sha256'][gate.DESCRIPTOR])

    def test_all_inventory_paths_and_hashes_are_independently_compared(self):
        f = gate.second.Fixture()
        f.source_root = Path('/fixture/sources')
        historical = gate.strict_json((gate.second.CORPUS / 'local-type-comparison.json').read_bytes())['all_226_source_sha256']
        for operation in ['omit', 'hash', 'rename', 'extra']:
            actual = copy.deepcopy(historical)
            key = next(iter(actual))
            if operation == 'omit': actual.pop(key)
            elif operation == 'hash': actual[key] = '0' * 64
            elif operation == 'rename': actual['renamed.java'] = actual.pop(key)
            else: actual['generated/GsonBuildConfig.java'] = '0' * 64
            with mock.patch.object(gate.second, 'all_source_hashes', return_value=actual), self.assertRaises(ValueError):
                gate.check_inventory(f)


class InvocationControls(unittest.TestCase):
    def fixture(self, root):
        f = gate.second.Fixture()
        f.source_root = root / 'sources'; f.source_root.mkdir()
        (f.source_root / 'Original.java').write_text('class Original {}')
        f.workdir = root
        return f

    def test_flags_belong_to_gson_main_by_id_not_final_group(self):
        f = gate.second.Fixture()
        f.source_root = Path('/fixture/sources')
        f.inputs = {key: Path('/inputs') / key for key in ['java_base', 'java_sql', 'jsr250', 'junit', 'hamcrest']}
        f.manifest_data = {'deliberately': 'not used'}
        args = gate.arguments(f, Path('/fixture'))
        groups, current = {}, None
        i = 0
        while i < len(args):
            if args[i] == '--compilation':
                current = '/'.join(args[i+1:i+3]); groups[current] = []; i += 3
            else: groups[current].append(args[i]); i += 1
        self.assertEqual(4, len(groups))
        self.assertIn('--exclude-source', groups['gson/main'])
        self.assertEqual(1, args.count('--exclude-source'))
        for key in ['extras/main', 'extras/test', 'gson/test']:
            self.assertNotIn('--exclude-source', groups[key])
        self.assertIn('sources/gson/src/main/java', groups['gson/main'])
        self.assertNotIn('sources/gson/src/main/java/com', args)
        self.assertNotIn('--exclude-source', gate.arguments(f, Path('/fixture'), False))

    def test_control_manifests_keep_the_fixture_relative_path_base(self):
        with tempfile.TemporaryDirectory() as tmp:
            stage = Path(tmp)
            root = stage / 'fixture'; root.mkdir()
            f = self.fixture(root)
            f.manifest = root / 'project.json'
            f.manifest_data = model()
            gate.gson_main(f.manifest_data)['source_roots'] = ['sources/' + gate.WHOLE_ROOT + '/com']
            descriptor = f.source_root / gate.DESCRIPTOR
            descriptor.parent.mkdir(parents=True)
            descriptor.write_text('module gson {}')
            selected = copy.copy(f)
            selected.manifest_data = gate.selected_model(f)
            selected.manifest = root / 'selected.json'
            gate.write_new(selected.manifest, selected.manifest_data)
            runner = mock.Mock()
            def check_base(label, args, cwd, *unused):
                if args[0] == 'inspect':
                    path = Path(args[args.index('--manifest') + 1])
                    actual = json.loads(path.read_text())
                    source = gate.gson_main(actual)['source_roots'][0]
                    self.assertEqual(descriptor.parent, (path.parent / source).resolve())
                return {'refusal': label}
            runner.failure.side_effect = check_base
            with mock.patch.object(gate, 'arguments', return_value=[]):
                gate.capture_baseline(runner, f, stage)
                controls = gate.failure_controls(runner, f, selected, stage)
                self.assertEqual({'no-configure-rule', 'removed-rule', 'wrong-rule',
                                  'excluded-query', 'excluded-overlay'}, set(controls))
                calls = {call.args[0]: call.args for call in runner.failure.call_args_list}
                self.assertEqual('configure', calls['no-configure-rule'][1][0])

    def test_failed_invocations_preserve_raw_receipts(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); f = self.fixture(root)
            runner = gate.Runner(Path('/native'), root / 'receipts', f)
            error = b'{"schema_version":1,"error":{"code":"load_failed","message":"module-info.java"}}\n'
            with mock.patch.object(gate.configure_gate.commons, 'run_bounded', return_value=(2, b'', error)):
                runner.failure('refused', ['inspect'], root, 'load_failed', 'module-info.java')
            self.assertEqual(error, (root / 'receipts/001-refused.stderr').read_bytes())
            record = json.loads((root / 'receipts/001-refused.json').read_text())
            self.assertEqual(2, record['exit_code'])
            self.assertEqual(record['source_tree_before'], record['source_tree_after'])

    def test_source_mutation_fails_after_receipts_are_saved(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); f = self.fixture(root)
            runner = gate.Runner(Path('/native'), root / 'receipts', f)
            def mutate(*unused):
                (f.source_root / 'Original.java').write_text('class Modified {}')
                return 0, b'{}', b''
            with mock.patch.object(gate.configure_gate.commons, 'run_bounded', side_effect=mutate), self.assertRaises(ValueError):
                runner.invoke('mutation', ['inspect'], root)
            self.assertTrue((root / 'receipts/001-mutation.stdout').is_file())
            record = json.loads((root / 'receipts/001-mutation.json').read_text())
            self.assertNotEqual(record['source_tree_before'], record['source_tree_after'])

    def test_runner_exceptions_leave_exclusive_receipt(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); f = self.fixture(root)
            runner = gate.Runner(Path('/native'), root / 'receipts', f)
            with mock.patch.object(gate.configure_gate.commons, 'run_bounded', side_effect=TimeoutError('timed out')), self.assertRaises(TimeoutError):
                runner.invoke('timeout', ['inspect'], root)
            self.assertIn('timed out', json.loads((root / 'receipts/001-timeout.json').read_text())['exception'])
            with self.assertRaises(ValueError): gate.Runner(Path('/native'), root / 'receipts', f)

    def test_wrong_refusal_or_partial_output_cannot_pass(self):
        error = b'{"schema_version":1,"error":{"code":"invisible_source","message":"module-info.java"}}\n'
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); f = self.fixture(root)
            for i, result in enumerate([(0, b'', error), (2, b'partial', error),
                    (2, b'', error.replace(b'invisible_source', b'invalid_overlay')),
                    (2, b'', error.replace(b'module-info.java', b'wrong input'))]):
                runner = gate.Runner(Path('/native'), root / str(i), f)
                with mock.patch.object(gate.configure_gate.commons, 'run_bounded', return_value=result), self.assertRaises(ValueError):
                    runner.failure('failure', ['declaration'], root, 'invisible_source', 'module-info.java')

    def test_configure_bound_manifest_validation_and_no_writes(self):
        with tempfile.TemporaryDirectory() as tmp:
            outer = Path(tmp); root = outer / 'stage'; root.mkdir(); f = self.fixture(root)
            f.manifest = root / 'project.json'; f.inputs = {}
            manual = model(); generated = gate.effective_manifest(manual, root)
            generated['compilations'][1].pop('source_exclusions')
            raw = (json.dumps(generated) + '\n').encode()
            for index, result in enumerate([(0, raw, b''), (2, raw, b''), (0, raw, b'warning'),
                                            (0, raw[:-1], b''), (0, b'x' * (gate.MAX_MANIFEST+1), b''),
                                            (0, b'{}{}\n', b'')]):
                runner = gate.Runner(Path('/native'), outer / str(index), f)
                with mock.patch.object(gate, 'arguments', return_value=[]), mock.patch.object(gate.configure_gate.commons, 'run_bounded', return_value=result) as call:
                    if index == 0:
                        self.assertEqual(raw, gate.run_configure(runner, f, manual, root, root)[0])
                        self.assertEqual(gate.MAX_MANIFEST, call.call_args.args[3])
                    else:
                        with self.assertRaises(ValueError): gate.run_configure(runner, f, manual, root, root)
            runner = gate.Runner(Path('/native'), outer / 'writes', f)
            def writes(*unused):
                (root / 'unexpected-config.json').write_bytes(b'{}')
                return 0, raw, b''
            with mock.patch.object(gate, 'arguments', return_value=[]), mock.patch.object(gate.configure_gate.commons, 'run_bounded', side_effect=writes), self.assertRaises(ValueError):
                gate.run_configure(runner, f, manual, root, root)

    def test_reports_and_manifest_bytes_never_overwrite(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'evidence.json'
            gate.write_new(path, {'failed': True})
            with self.assertRaises(FileExistsError): gate.write_new(path, {'passed': True})
            self.assertEqual({'failed': True}, json.loads(path.read_text()))


class ResultControls(unittest.TestCase):
    def test_exact_record_comparison_rejects_target_refusal_scope_type_and_parser_mutations(self):
        original = {'queries': [{'query_index': 0, 'compilation': {'project': 'gson', 'source_set': 'test'},
                    'status': 'Resolved', 'targets': [{'origin': 'source', 'name': 'remove',
                    'range': {'start_byte': 4945, 'end_byte': 4951}}]},
                    {'query_index': 1, 'status': 'OverloadNotResolved', 'targets': []}],
                    'parser': {'report_count': 331, 'files_with_reports': 58, 'files': [
                    {'path': 'Some.java', 'reports': [{'message': 'unsupported', 'range': {'start_byte': 5, 'end_byte': 8}}]}]}}
        mutations = [lambda x: x['queries'][0]['targets'][0]['range'].update(start_byte=4946),
                     lambda x: x['queries'][0]['compilation'].update(source_set='main'),
                     lambda x: x['queries'][1].update(status='Resolved'),
                     lambda x: x['queries'][0].update(query_index=False),
                     lambda x: x['queries'].pop(),
                     lambda x: x['parser'].update(report_count=330),
                     lambda x: x['parser']['files'][0]['reports'][0].update(message='different'),
                     lambda x: x['parser']['files'][0]['reports'][0]['range'].update(end_byte=9)]
        for mutate in mutations:
            changed = copy.deepcopy(original); mutate(changed)
            with self.assertRaises(ValueError): gate.same(changed, original, 'complete preservation')

    def test_all_three_actual_lsp_manifest_variants_are_exercised_and_compared(self):
        with tempfile.TemporaryDirectory() as tmp:
            stage = Path(tmp); sources = stage / 'sources'; sources.mkdir()
            fixtures = []
            for label in ['historical', 'whole_root', 'generated']:
                f = gate.second.Fixture()
                f.source_root, f.manifest = sources, stage / (label + '.json')
                fixtures.append(f)
            runner = mock.Mock(native=Path('/native'), receipts=stage / 'receipts')
            runner.receipts.mkdir()
            rows = [{'stage': str(i), 'result': None} for i in range(9)]
            with mock.patch.object(gate.lsp, 'Session') as sessions, \
                    mock.patch.object(gate.lsp, 'reviewed_case'), \
                    mock.patch.object(gate.lsp, 'exercise', return_value=rows):
                result = gate.lsp_sessions(runner, *fixtures, stage)
            self.assertEqual({'historical', 'whole_root', 'generated'}, set(result))
            self.assertEqual([f.manifest for f in fixtures],
                             [call.args[0][-1] for call in sessions.call_args_list])
            changed = copy.deepcopy(rows); changed[-1]['result'] = {'uri': 'file:///invented'}
            runner.receipts = stage / 'mutated-receipts'; runner.receipts.mkdir()
            with mock.patch.object(gate.lsp, 'Session'), \
                    mock.patch.object(gate.lsp, 'reviewed_case'), \
                    mock.patch.object(gate.lsp, 'exercise', side_effect=[rows, rows, changed]), \
                    self.assertRaises(ValueError):
                gate.lsp_sessions(runner, *fixtures, stage)
            self.assertTrue((runner.receipts / 'lsp-generated.json').is_file())

    def test_lsp_exact_stage_identity_and_count_preserved(self):
        stages = [{'stage': str(i), 'position': {'line': i, 'character': 1}, 'result': None} for i in range(9)]
        for mutate in [lambda x: x.pop(), lambda x: x.reverse(),
                       lambda x: x[4].update(result={'uri': 'file:///invented.java'}),
                       lambda x: x[0]['position'].update(line=False)]:
            changed = copy.deepcopy(stages); mutate(changed)
            with self.assertRaises(ValueError): gate.same(changed, stages, 'LSP preservation')


    def test_current_probe_inventory_explicitly_adds_binary_query(self):
        # The imported contracts own exact targets; this test checks the gate's
        # aggregate wiring without an external corpus or native process.
        f = gate.second.Fixture()
        f.plan = [{'id': str(i)} for i in range(16)]
        groups = {'original': [{} for _ in range(16)], 'reference': [{} for _ in range(3)],
                  'local': [{} for _ in range(5)], 'string': [{} for _ in range(8)],
                  'current': [{'byte': b} for b in [1843, 1976, 2412, 1986, 2489, 2527, 2139, 2489, 1653]]}
        def run():
            with mock.patch.object(gate.second, 'current_original_expectations', return_value=groups['original']), \
                 mock.patch.object(gate.second, 'reference_call_probes', return_value=(groups['reference'], {}, 'digest')), \
                 mock.patch.object(gate.second, 'qualified_reference_probes', side_effect=lambda fixture, rows: rows), \
                 mock.patch.object(gate.second, 'local_type_probes', return_value=groups['local']), \
                 mock.patch.object(gate.strict.string, 'probes', return_value=groups['string']), \
                 mock.patch.object(gate.strict, 'current_probes', return_value=groups['current']):
                return gate.probe_groups(f)[0]
        result = run()
        self.assertEqual(tuple((name, len(rows)) for name, rows in result), gate.CURRENT_PROBE_GROUPS)
        self.assertEqual(sum(len(rows) for name, rows in result), 41)
        self.assertEqual(gate.CURRENT_QUERY_COUNT, 41)
        self.assertEqual(result[-1][0], 'binary_argument_9')
        self.assertEqual(result[-1][1][-1]['byte'], 1653)
        original = copy.deepcopy(groups)
        for key in groups:
            for change in ('omit', 'duplicate'):
                groups = copy.deepcopy(original)
                if change == 'omit': groups[key].pop()
                else: groups[key].append(copy.deepcopy(groups[key][-1]))
                with self.subTest(group=key, change=change), self.assertRaises(ValueError): run()
        # Equal total is insufficient: an extra string row cannot replace the
        # new independent return query or conceal a lost original row.
        groups = copy.deepcopy(original); groups['current'].pop(); groups['string'].append({})
        with self.assertRaises(ValueError): run()


if __name__ == '__main__':
    unittest.main()
