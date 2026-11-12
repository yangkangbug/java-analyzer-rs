"""Offline reference-oracle checker controls. Never invoke Java or native tools."""
import copy
import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("reference_oracle", Path(__file__).with_name("check-semantic-oracle.py"))
oracle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(oracle)


class ReferenceComparatorTests(unittest.TestCase):
    def setUp(self):
        self.declarations = {"root": ("Target.java", 10, 14, "pick"),
                             "leaf": ("Target.java", 40, 44, "pick")}
        self.queries = {"specific": ("Use.java", 100, 104, "pick")}
        self.expected = {"specific": ("Resolved", "leaf@model.Target", "leaf@model.Target")}
        self.javac = {"specific": ["Target.java", "METHOD", "model.Target", "pick", "30", "55"]}
        self.native = {"specific": ["Resolved", "1", "Target.java|Method|model.Target|pick|40|44"]}
        self.binaries = Path("/explicit/classes")

    def compare(self, native=None, javac=None, expected=None):
        oracle.compare_reference(self.expected if expected is None else expected,
                                 self.queries, self.declarations,
                                 self.javac if javac is None else javac,
                                 self.native if native is None else native, self.binaries)

    def test_exact_selected_source(self):
        self.compare()

    def test_same_named_wrong_overload(self):
        wrong = {"specific": ("Resolved", "root@model.Target", "root@model.Target")}
        with self.assertRaises(AssertionError):
            self.compare(expected=wrong)

    def test_compiler_identity_and_ranges(self):
        for index in range(6):
            changed = copy.deepcopy(self.javac)
            changed["specific"][index] = "wrong" if index < 4 else ("1000" if index == 4 else "1")
            with self.subTest(index=index), self.assertRaises(AssertionError):
                self.compare(javac=changed)
        changed = copy.deepcopy(self.javac)
        changed["specific"][-2:] = ["0", "1000"]
        with self.assertRaises(AssertionError):
            self.compare(javac=changed)

    def test_native_identity_and_ranges(self):
        for index in range(6):
            changed = copy.deepcopy(self.native)
            parts = changed["specific"][2].split("|")
            parts[index] = "wrong" if index < 4 else "1000"
            changed["specific"][2] = "|".join(parts)
            with self.subTest(index=index), self.assertRaises(AssertionError):
                self.compare(native=changed)

    def test_missing_extra_records(self):
        for which in ("native", "javac"):
            original = getattr(self, which)
            for changed in ({}, {**original, "extra": original["specific"]}):
                with self.subTest(which=which), self.assertRaises(AssertionError):
                    self.compare(**{which: changed})

    def test_duplicate_or_incomplete_candidates(self):
        target = self.native["specific"][2]
        for row in (["Resolved", "2", target], ["Resolved", "2", target, target]):
            with self.subTest(row=row), self.assertRaises(AssertionError):
                self.compare(native={"specific": row})

    def test_exact_binary_refusal_candidate(self):
        candidates = "leaf@model.Target,class:dep/BinaryParent.class@dep.BinaryParent#pick"
        expected = {"specific": ("OverloadNotResolved", "leaf@model.Target", candidates)}
        binary = "/explicit/classes/dep/BinaryParent.class|Method|dep.BinaryParent|pick|0|0"
        native = {"specific": ["OverloadNotResolved", "2", self.native["specific"][2], binary]}
        self.compare(native=native, expected=expected)
        for corrupted in (binary.replace("Parent", "Wrong"), binary.replace("|0|0", "|1|2")):
            changed = copy.deepcopy(native)
            changed["specific"][3] = corrupted
            with self.subTest(corrupted=corrupted), self.assertRaises(AssertionError):
                self.compare(native=changed, expected=expected)
        with self.assertRaises(AssertionError):
            self.compare(native={"specific": native["specific"][:3]}, expected=expected)

    def test_reviewed_overlay_preserves_original_golden_and_exact_target(self):
        ds = {"read_method": ("lib/Value.java", 40, 44, "read")}
        qs = {"receiver_read_call": ("app/Calls.java", 100, 104, "read")}
        expected = {"receiver_read_call": ("Resolved", "read_method")}
        javac = {"receiver_read_call": ["lib/Value.java", "METHOD", "lib.Value", "read", "30", "55"]}
        native = {"receiver_read_call": ["CallNotResolved", "1", "lib/Value.java|Method|lib.Value|read|40|44"]}
        oracle.compare_with_reference_updates(expected, qs, ds, javac, native)
        for changed in ({"receiver_read_call": ("CallNotResolved", "read_method")},
                        {"receiver_read_call": ("Resolved", "wrong")}):
            with self.assertRaises(AssertionError):
                oracle.compare_with_reference_updates(changed, qs, ds, javac, native)
        wrong = copy.deepcopy(native)
        wrong["receiver_read_call"][0] = "Resolved"
        with self.assertRaises(AssertionError):
            oracle.compare_with_reference_updates(expected, qs, ds, javac, wrong)

    def test_invalid_call_cannot_be_resolved(self):
        for status, candidates, actual in (
            ("CallNotResolved", "leaf@model.Target", self.native["specific"]),
            ("Cycle", "-", ["Resolved", "0"]),
        ):
            with self.subTest(status=status), self.assertRaises(AssertionError):
                oracle.check_reference_candidates("invalid", status, candidates,
                                                  self.declarations, actual, self.binaries)

    def test_no_target_on_unsupported_receiver(self):
        expected = {"specific": ("Unresolved", "leaf@model.Target", "-")}
        self.compare(native={"specific": ["Unresolved", "0"]}, expected=expected)
        with self.assertRaises(AssertionError):
            self.compare(native={"specific": ["Unresolved", *self.native["specific"][1:]]},
                         expected=expected)


class ReferenceFixtureTests(unittest.TestCase):
    def test_manual_inventory(self):
        ds, qs, _, expected = oracle.reference_fixture()
        self.assertEqual(len(ds), 22)
        self.assertEqual(len(qs), 24)
        self.assertEqual(sum(row[0] == "Resolved" for row in expected.values()), 15)
        self.assertEqual(len(oracle.reference_invalid_fixtures()), 11)

    def test_extra_query_declaration_file_and_missing_golden(self):
        original = oracle.ORACLE
        for mutation in ("query", "declaration", "file", "golden"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                fixture = root / "reference-v1"
                shutil.copytree(original / "reference-v1", fixture)
                source = fixture / "source/app/Use.java"
                if mutation in {"query", "declaration"}:
                    kind = "Q" if mutation == "query" else "D"
                    source.write_text(source.read_text() + f"\n/*{kind}:extra*/unexpected\n")
                elif mutation == "file":
                    (fixture / "source/Extra.java").write_text("class Extra {}\n")
                else:
                    path = fixture / "expected.tsv"
                    path.write_text("\n".join(path.read_text().splitlines()[:-1]) + "\n")
                with patch.object(oracle, "ORACLE", root), self.assertRaises(AssertionError):
                    oracle.reference_fixture()

    def test_invalid_extra_marker_or_case_rejected(self):
        original = oracle.ORACLE
        for mutation in ("declaration", "query", "case"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                fixture = root / "reference-v1"
                shutil.copytree(original / "reference-v1", fixture)
                source = fixture / "invalid/wrong_arity/C.java"
                if mutation != "case":
                    kind = "D" if mutation == "declaration" else "Q"
                    source.write_text(source.read_text() + f"\n/*{kind}:extra*/unexpected\n")
                else:
                    extra = fixture / "invalid/extra/C.java"
                    extra.parent.mkdir()
                    extra.write_text("class C {}\n")
                with patch.object(oracle, "ORACLE", root), self.assertRaises(AssertionError):
                    oracle.reference_invalid_fixtures()



class InheritedSourceContractTests(unittest.TestCase):
    def setUp(self):
        self.root, self.ds, self.qs, self.req, self.current, self.old = oracle.inherited_source_fixture()
        self.platform = Path('/unused')
        self.javac = {}
        for q, (_, selected, _) in self.current.items():
            marker, owner = selected.split('@'); path, start, end, name = self.ds[marker]
            self.javac[q] = [path, 'METHOD', owner, name, str(start), str(end)]

    def native(self, contract):
        return {q: [status, str(len(names.split(','))), *[oracle.reference_target(self.ds, n, self.platform) for n in names.split(',')]]
                for q, (status, _, names) in contract.items()}

    def compare(self, contract, native):
        oracle.compare_reference(contract, self.qs, self.ds, self.javac, native, self.platform)

    def test_exact_stages_and_declaring_import_owner(self):
        current, old = self.native(self.current), self.native(self.old)
        self.compare(self.current, current); self.compare(self.old, old)
        with self.assertRaises(AssertionError): self.compare(self.current, old)
        with self.assertRaises(AssertionError): self.compare(self.old, current)
        self.assertEqual(sum(current[q] != old[q] for q in current), 6)
        bad = copy.deepcopy(current); bad['declaring_import'] = current['child_shadow']
        with self.assertRaises(AssertionError): self.compare(self.current, bad)

    def test_complete_candidate_metadata_and_inventory(self):
        rows = self.native(self.current)
        for q, row in rows.items():
            bad = copy.deepcopy(rows); bad[q][0] = 'Resolved' if row[0] != 'Resolved' else 'CallNotResolved'
            with self.assertRaises(AssertionError): self.compare(self.current, bad)
            for i in range(2, len(row)):
                for field in range(6):
                    bad = copy.deepcopy(rows); cells = bad[q][i].split('|'); cells[field] += 'wrong'; bad[q][i] = '|'.join(cells)
                    with self.assertRaises(AssertionError): self.compare(self.current, bad)
                bad = copy.deepcopy(rows); bad[q].append(row[i]); bad[q][1] = str(int(row[1])+1)
                with self.assertRaises(AssertionError): self.compare(self.current, bad)
        bad = copy.deepcopy(rows); bad.pop('unique_parent')
        with self.assertRaises(AssertionError): self.compare(self.current, bad)
        bad = copy.deepcopy(rows); bad['extra'] = rows['unique_parent']
        with self.assertRaises(AssertionError): self.compare(self.current, bad)

    def test_current_overlays_pin_entire_previous_stage(self):
        import json
        plan = json.loads((self.root/'prior-inherited-updates.json').read_text())
        for suite in ('qualified-primitive-v1', 'reference-v1', 'strict-reference-v1'):
            update = plan['updates'][suite]
            expected = {k: tuple(v) for k, v in update['previous_current'].items()}
            queries = {k: tuple(v) for k, v in update['queries'].items()}
            declarations = {k: tuple(v) for k, v in update['declarations'].items()}
            result = oracle.current_inherited_expectations(suite, expected, queries, declarations)
            self.assertEqual([k for k in result if result[k] != expected[k]], [update['query']])
            self.assertEqual(result[update['query']], tuple(update['after']))
            with self.assertRaises(AssertionError): oracle.current_inherited_expectations(suite, result, queries, declarations)
            for kind in ('expected', 'queries', 'declarations'):
                args = [copy.deepcopy(expected), copy.deepcopy(queries), copy.deepcopy(declarations)]
                position = ('expected', 'queries', 'declarations').index(kind)
                key = next(iter(args[position])); args[position][key] = ('wrong',)
                with self.assertRaises(AssertionError): oracle.current_inherited_expectations(suite, *args)
        with self.assertRaises(AssertionError): oracle.current_inherited_expectations('primitive-v1', {}, {}, {})

    def test_pinned_source_and_stage_forecast_reject_mutations(self):
        for name in ('source/core/Use.java', 'source/base/Parent.java', 'source/client/Value.java',
                     'expected.tsv', 'baseline.tsv', 'invalid/missing/Use.java', 'invalid/unknown/Use.java',
                     'prior-inherited-updates.json', 'prior-inherited-revision.json', 'history/forecast-first-failure.log', 'fixture-manifest.json'):
            with tempfile.TemporaryDirectory() as temp:
                base = Path(temp); root = base/'inherited-source-v1'; shutil.copytree(self.root, root)
                p = root/name; p.write_bytes(p.read_bytes()+b'changed')
                with patch.object(oracle, 'ORACLE', base), self.assertRaises(AssertionError): oracle.inherited_source_fixture()


class SourceOverrideContractTests(unittest.TestCase):
    def setUp(self):
        self.root, self.ds, self.qs, self.req, self.current, self.old = oracle.source_override_fixture()
        self.platform = Path('/unused')
        self.javac = {}
        for q, (_, selected, _) in self.current.items():
            marker, owner = selected.split('@'); path, start, end, name = self.ds[marker]
            self.javac[q] = [path, 'METHOD', owner, name, str(start), str(end)]

    def native(self, contract):
        return {q: [status, str(len(names.split(','))), *[oracle.reference_target(self.ds, n, self.platform) for n in names.split(',')]]
                for q, (status, _, names) in contract.items()}

    def compare(self, contract, native):
        oracle.compare_reference(contract, self.qs, self.ds, self.javac, native, self.platform)

    def test_exact_stages_static_receiver_and_new_selection_count(self):
        current, old = self.native(self.current), self.native(self.old)
        self.compare(self.current, current); self.compare(self.old, old)
        with self.assertRaises(AssertionError): self.compare(self.current, old)
        with self.assertRaises(AssertionError): self.compare(self.old, current)
        self.assertEqual(sum(current[q] != old[q] for q in current), 6)
        self.assertEqual(current['parent_static_receiver'], old['parent_static_receiver'])
        bad = copy.deepcopy(current); bad['parent_static_receiver'] = current['child_override']
        with self.assertRaises(AssertionError): self.compare(self.current, bad)

    def test_complete_targets_refusals_and_inventory_mutations(self):
        rows = self.native(self.current)
        for q, row in rows.items():
            bad = copy.deepcopy(rows); bad[q][0] = 'Resolved' if row[0] != 'Resolved' else 'CallNotResolved'
            with self.assertRaises(AssertionError): self.compare(self.current, bad)
            for i in range(2, len(row)):
                for field in range(6):
                    bad = copy.deepcopy(rows); cells = bad[q][i].split('|'); cells[field] += 'wrong'; bad[q][i] = '|'.join(cells)
                    with self.assertRaises(AssertionError): self.compare(self.current, bad)
                bad = copy.deepcopy(rows); bad[q].append(row[i]); bad[q][1] = str(int(row[1])+1)
                with self.assertRaises(AssertionError): self.compare(self.current, bad)
        bad = copy.deepcopy(rows); bad.pop('covariant_return')
        with self.assertRaises(AssertionError): self.compare(self.current, bad)
        bad = copy.deepcopy(rows); bad['extra'] = rows['covariant_return']
        with self.assertRaises(AssertionError): self.compare(self.current, bad)

    def test_pinned_inputs_original_records_and_forecast_mutations(self):
        for name in ('source/Use.java', 'source/Refusals.java', 'expected.tsv', 'baseline.tsv', 'baseline-records.json',
                     'invalid/final/Use.java', 'invalid/return/Use.java', 'invalid/static/Use.java',
                     'invalid/visibility/Use.java', 'invalid/unknown/Use.java', 'invalid/missing/Use.java',
                     'prior-override-update.json', 'fixture-manifest.json'):
            with tempfile.TemporaryDirectory() as temp:
                base = Path(temp); root = base/'source-override-v1'; shutil.copytree(self.root, root)
                p = root/name; p.write_bytes(p.read_bytes()+b'changed')
                with patch.object(oracle, 'ORACLE', base), self.assertRaises(AssertionError): oracle.source_override_fixture()

    def test_prior_overlay_only_exact_override_not_other_refusals(self):
        _, ds, qs, _, current, baseline = oracle.inherited_source_fixture()
        updated = oracle.current_source_override_expectations(current, qs, ds)
        self.assertEqual([q for q in current if updated[q] != current[q]], ['override_refusal'])
        self.assertEqual(updated['override_refusal'], ('Resolved', 'child_override@core.Override', 'child_override@core.Override'))
        with self.assertRaises(AssertionError): oracle.current_source_override_expectations(updated, qs, ds)
        with self.assertRaises(AssertionError): oracle.current_source_override_expectations(baseline, qs, ds)
        for field in ('status', 'query', 'declaration'):
            args = [copy.deepcopy(current), copy.deepcopy(qs), copy.deepcopy(ds)]
            if field == 'status': args[0]['interface_refusal'] = ('Resolved', 'face@core.Face', 'face@core.Face')
            elif field == 'query': args[1]['override_refusal'] = ('wrong', 0, 1, 'over')
            else: args[2]['child_override'] = ('wrong', 0, 1, 'over')
            with self.assertRaises(AssertionError): oracle.current_source_override_expectations(*args)


class SourceInvocationContractTests(unittest.TestCase):
    def setUp(self):
        self.root, self.ds, self.qs, self.req, self.current, self.old = oracle.source_invocation_fixture()
        self.platform = Path('/unused')
        self.javac = {}
        for q, (_, selected, _) in self.current.items():
            marker, owner = selected.split('@'); path, start, end, name = self.ds[marker]
            self.javac[q] = [path, 'METHOD', owner, name, str(start), str(end)]

    def native(self, contract):
        result = {}
        for q, (status, _, names) in contract.items():
            targets = [] if names == '-' else [oracle.reference_target(self.ds, n, self.platform) for n in names.split(',')]
            result[q] = [status, str(len(targets)), *targets]
        return result

    def compare(self, contract, native):
        oracle.compare_reference(contract, self.qs, self.ds, self.javac, native, self.platform)

    def test_exact_stages_static_return_owner_and_eight_changes(self):
        current, old = self.native(self.current), self.native(self.old)
        self.compare(self.current, current); self.compare(self.old, old)
        with self.assertRaises(AssertionError): self.compare(self.current, old)
        with self.assertRaises(AssertionError): self.compare(self.old, current)
        self.assertEqual(sum(current[q] != old[q] for q in current), 8)
        self.assertEqual(current['static_declared_control'], old['static_declared_control'])
        for marker, owner in [('child_value', 'model.Child'), ('decoy_value', 'shadow.Product')]:
            bad = copy.deepcopy(current)
            bad['multi_receiver'] = ['Resolved', '1', oracle.reference_target(self.ds, marker+'@'+owner, self.platform)]
            with self.assertRaises(AssertionError): self.compare(self.current, bad)

    def test_complete_targets_empty_refusals_and_inventory_mutations(self):
        rows = self.native(self.current)
        for q, row in rows.items():
            bad = copy.deepcopy(rows); bad[q][0] = 'Resolved' if row[0] != 'Resolved' else 'CallNotResolved'
            with self.assertRaises(AssertionError): self.compare(self.current, bad)
            for i in range(2, len(row)):
                for field in range(6):
                    bad = copy.deepcopy(rows); cells = bad[q][i].split('|'); cells[field] += 'wrong'; bad[q][i] = '|'.join(cells)
                    with self.assertRaises(AssertionError): self.compare(self.current, bad)
                bad = copy.deepcopy(rows); bad[q].append(row[i]); bad[q][1] = str(int(row[1])+1)
                with self.assertRaises(AssertionError): self.compare(self.current, bad)
        bad = copy.deepcopy(rows); bad['depth_refusal'] = rows['multi_receiver']
        with self.assertRaises(AssertionError): self.compare(self.current, bad)
        bad = copy.deepcopy(rows); bad.pop('nested_actual')
        with self.assertRaises(AssertionError): self.compare(self.current, bad)
        bad = copy.deepcopy(rows); bad['extra'] = rows['nested_actual']
        with self.assertRaises(AssertionError): self.compare(self.current, bad)

    def test_pinned_expression_budget_and_stage_inputs(self):
        for name in ('source/shadow/Use.java', 'source/model/Product.java', 'expected.tsv', 'baseline.tsv', 'baseline-records.json',
                     'invalid/unknown/Use.java', 'invalid/void/Use.java', 'budget/Work.java', 'budget/expected.tsv',
                     'prior-invocation-updates.json', 'fixture-manifest.json'):
            with tempfile.TemporaryDirectory() as temp:
                base = Path(temp); root = base/'source-invocation-v1'; shutil.copytree(self.root, root)
                p = root/name; p.write_bytes(p.read_bytes()+b'changed')
                with patch.object(oracle, 'ORACLE', base), self.assertRaises(AssertionError): oracle.source_invocation_fixture()

    def test_whole_old_suite_forecast_only_two_explicit_changes(self):
        import json
        plan = json.loads((self.root/'prior-invocation-updates.json').read_text())
        self.assertEqual(len(plan['suites']), 6)
        for suite, update in plan['suites'].items():
            expected = {k:tuple(v) for k,v in update['previous_current'].items()}
            queries = {k:tuple(v) for k,v in update['queries'].items()}
            declarations = {k:tuple(v) for k,v in update['declarations'].items()}
            actual = oracle.current_source_invocation_expectations(suite, expected, queries, declarations)
            changed = {q for q in expected if expected[q] != actual[q]}
            self.assertEqual(changed, {'return_deep', 'return_argument'} if suite == 'reference-return-v1' else set())
            if changed:
                with self.assertRaises(AssertionError): oracle.current_source_invocation_expectations(suite, actual, queries, declarations)
            for index in range(3):
                args = [copy.deepcopy(expected), copy.deepcopy(queries), copy.deepcopy(declarations)]
                key = next(iter(args[index])); args[index][key] = ('wrong',)
                with self.assertRaises(AssertionError): oracle.current_source_invocation_expectations(suite, *args)
        with self.assertRaises(AssertionError): oracle.current_source_invocation_expectations('unknown-suite', {}, {}, {})

if __name__ == "__main__":
    unittest.main()
