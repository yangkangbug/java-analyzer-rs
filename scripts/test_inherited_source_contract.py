"""Exact shared inherited data contract; no native or JVM required."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import unittest
from unittest import mock
import test_cast_null_acceptance as prior
s = prior.gate.strict
second = s.second

class InheritedControls(unittest.TestCase):
    response_for = prior.CastNullControls.response_for
    # Use the common synthetic source builder, not inherited feature tests.
    def setUp(self):
        prior.CastNullControls.setUp(self)
        self.historical = self.queries
        self.plan = s.inherited_plan()
        self.queries = self.plan['queries']
        buffers = {p: bytearray(b) for p,b in self.fixture.sources.items()}
        for name in self.plan['source_sha256']:
            if name not in buffers:
                buffers[name] = bytearray(b' ' * 20000)
            self.fixture.owner[name] = 'gson/test' if '/src/test/' in name else 'gson/main'
        for q in self.queries:
            expression=q['expression'].encode(); buffers[q['file']][q['byte']:q['byte']+len(expression)] = expression
            for target in q['baseline']['targets'] + q['expected']['targets']:
                if target['origin']=='source':buffers[target['path']][target['start']:target['end']] = target['name'].encode()
        self.fixture.sources = {p:bytes(b) for p,b in buffers.items()}
        for p,b in self.fixture.sources.items():
            dest=self.fixture.source_root/p;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(b)
        self.fixture.inputs={'java_base':Path('/synthetic/java.base.jmod')}
        self.fixture.metadata['inputs']['java_base']={'filename':'java.base.jmod'}
        self.response=self.response_for('expected')
    def check(self, response, baseline=False):
        second.check_reference_call_response(response,self.queries,self.fixture,'baseline' if baseline else 'expected')
    def test_complete_baseline_and_current_are_disjoint(self):
        self.check(self.response);self.check(self.response_for('baseline'),True)
        for response,mode in [(self.response,True),(self.response_for('baseline'),False)]:
            with self.assertRaises(second.AcceptanceError):self.check(response,mode)
        self.assertEqual([5305,2883],[q['byte'] for q in self.queries if q['baseline']!=q['expected']])
    def test_all_rows_candidates_identity_and_positions(self):
        for mode in (False,True):
            original=self.response_for('baseline' if mode else 'expected')
            for i in range(35):
                for key,value in [('query_index',999),('byte_offset',0),('file','wrong'),('compilation',{}),('source_compilation',{}),('position',{}),('status','Wrong')]:
                    changed=copy.deepcopy(original);changed['result']['queries'][i][key]=value
                    with self.assertRaises(second.AcceptanceError):self.check(changed,mode)
                for action in ('omit','duplicate','target_omit','target_duplicate'):
                    changed=copy.deepcopy(original);rows=changed['result']['queries']
                    if action=='omit':rows.pop(i)
                    elif action=='duplicate':rows.append(copy.deepcopy(rows[i]))
                    elif action=='target_omit':rows[i]['targets'].pop()
                    else:rows[i]['targets'].append(copy.deepcopy(rows[i]['targets'][0]))
                    with self.assertRaises(second.AcceptanceError):self.check(changed,mode)
                for j,t in enumerate(original['result']['queries'][i]['targets']):
                    for key in ('path','owner','name','kind','origin'):
                        changed=copy.deepcopy(original);changed['result']['queries'][i]['targets'][j][key]='wrong'
                        with self.assertRaises(second.AcceptanceError):self.check(changed,mode)
                    if 'range' in t:
                        for key in t['range']:
                            changed=copy.deepcopy(original);changed['result']['queries'][i]['targets'][j]['range'][key]+=1
                            with self.assertRaises(second.AcceptanceError):self.check(changed,mode)
    def test_frozen_plan_and_exact_predecessor(self):
        plan=copy.deepcopy(self.plan);plan['source_sha256']={p:hashlib.sha256(b).hexdigest() for p,b in self.fixture.sources.items()}
        with mock.patch.object(s,'inherited_plan',return_value=plan):
            self.assertEqual(self.queries,s.inherited_probes(self.fixture,self.historical))
            for action in ('omit','extra','metadata','result'):
                h=copy.deepcopy(self.historical)
                if action=='omit':h.pop()
                elif action=='extra':h.append(copy.deepcopy(h[0]))
                elif action=='metadata':h[0]['byte']=1
                else:h[0]['expected']['status']='Wrong'
                with self.assertRaises(second.AcceptanceError):s.inherited_probes(self.fixture,h)
        path=self.fixture.workdir/'bad-plan.json';path.write_text(json.dumps(self.plan))
        with self.assertRaises(second.AcceptanceError):s.inherited_plan(path)
    def test_capture_keeps_parser_and_proof_stages_orthogonal(self):
        with mock.patch.object(second,'digest',return_value=self.plan['baseline_native_sha256']), \
             mock.patch.object(s,'current_probes',return_value=self.queries), \
             mock.patch.object(second,'all_source_hashes',return_value={}), \
             mock.patch.object(second,'document',return_value=Path('requests')), \
             mock.patch.object(second,'successful',return_value=self.response_for('baseline')), \
             mock.patch.object(s.string,'run',return_value={'source_sha256_after':{}}) as keep:
            report=s.capture_proof_stage(Path('native'),self.fixture,'inherited_source')
            self.assertEqual(report['validation'],'baseline_verified_not_acceptance')
            keep.assert_called_once_with(Path('native'),self.fixture,proof_stage='cast_null',parser_stage='current')

    def test_capture_rejects_wrong_stage_and_binary_before_execution(self):
        with mock.patch.object(second,'digest',return_value='wrong'),mock.patch.object(second,'successful') as execute:
            for stage in ('inherited_source','cast_null','auto',None):
                with self.assertRaises(second.AcceptanceError):s.capture_proof_stage(Path('native'),self.fixture,stage)
            execute.assert_not_called()

class CommonsPreservationControls(unittest.TestCase):
    def setUp(self):
        spec=importlib.util.spec_from_file_location('commons_inherited',Path(__file__).with_name('check-cli-acceptance.py'))
        self.c=importlib.util.module_from_spec(spec);spec.loader.exec_module(self.c)
        path=self.c.CORPUS/'inherited-source-preservation.json'
        self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(),self.c.INHERITED_PRESERVATION_SHA256)
        self.plan=json.loads(path.read_text());self.root=Path('/synthetic/sources')
        rows=copy.deepcopy(self.plan['queries'])
        for row in rows:
            row['file']=str(self.root/row['file'])
            for target in row['targets']:target['path']=str(self.root/target['path'])
        self.response={'result':{'index_builds':1,'queries':rows}}
    def test_complete_preservation_rows_and_mutations(self):
        self.c.check_inherited_preservation(self.response,self.plan,self.root)
        for i in range(4):
            for key,value in [('byte_offset',0),('query_index',99),('position',{}),('compilation',{}),('source_compilation',{}),('status','Unresolved')]:
                changed=copy.deepcopy(self.response);changed['result']['queries'][i][key]=value
                with self.assertRaises(ValueError):self.c.check_inherited_preservation(changed,self.plan,self.root)
            for key in ('owner','kind','name','origin','range','compilation','source_available'):
                changed=copy.deepcopy(self.response);changed['result']['queries'][i]['targets'][0][key]='wrong'
                with self.assertRaises(ValueError):self.c.check_inherited_preservation(changed,self.plan,self.root)
            for action in ('omit','extra','targetextra'):
                changed=copy.deepcopy(self.response);rows=changed['result']['queries']
                if action=='omit':rows.pop(i)
                elif action=='extra':rows.append(copy.deepcopy(rows[i]))
                else:rows[i]['targets'].append(copy.deepcopy(rows[i]['targets'][0]))
                with self.assertRaises(ValueError):self.c.check_inherited_preservation(changed,self.plan,self.root)

if __name__=='__main__':unittest.main()
