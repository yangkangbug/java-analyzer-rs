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
            for i in range(len(self.queries)):
                for key,value in [('query_index',999),('byte_offset',0),('file','wrong'),('compilation',{}),('source_compilation',{}),('position',{}),('status','Wrong')]:
                    changed=copy.deepcopy(original);changed['result']['queries'][i][key]=value
                    with self.assertRaises(second.AcceptanceError):self.check(changed,mode)
                for action in ('omit','duplicate','target_omit','target_duplicate'):
                    changed=copy.deepcopy(original);rows=changed['result']['queries']
                    if action=='omit':rows.pop(i)
                    elif action=='duplicate':rows.append(copy.deepcopy(rows[i]))
                    elif action=='target_omit':
                        rows[i]['targets'].pop() if rows[i]['targets'] else rows[i]['targets'].append({})
                    else:rows[i]['targets'].append(copy.deepcopy(rows[i]['targets'][0]) if rows[i]['targets'] else {})
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
             mock.patch.object(s,'frozen_inherited_probes',return_value=self.queries), \
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

class SourceOverrideControls(unittest.TestCase):
    response_for = InheritedControls.response_for
    check = InheritedControls.check
    test_all_rows_candidates_identity_and_positions = InheritedControls.test_all_rows_candidates_identity_and_positions
    def setUp(self):
        InheritedControls.setUp(self)
        self.historical=self.queries
        self.plan=s.override_plan();self.queries=self.plan['queries']
        buffers={p:bytearray(b) for p,b in self.fixture.sources.items()}
        for name in self.plan['source_sha256']:
            if name not in buffers:buffers[name]=bytearray(b' '*20000)
            self.fixture.owner[name]='gson/test' if '/src/test/' in name else 'gson/main'
        for q in self.queries:
            expression=q['expression'].encode();buffers[q['file']][q['byte']:q['byte']+len(expression)]=expression
            for target in q['baseline']['targets']+q['expected']['targets']:
                if target['origin']=='source':buffers[target['path']][target['start']:target['end']]=target['name'].encode()
        self.fixture.sources={p:bytes(b) for p,b in buffers.items()}
        for p,b in self.fixture.sources.items():
            dest=self.fixture.source_root/p;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(b)
        self.response=self.response_for('expected')
    def test_three_promotions_and_no_old_new_union(self):
        self.check(self.response);self.check(self.response_for('baseline'),True)
        for response,mode in [(self.response,True),(self.response_for('baseline'),False)]:
            with self.assertRaises(second.AcceptanceError):self.check(response,mode)
        self.assertEqual([3001,6712,10817],[q['byte'] for q in self.queries if q['baseline']!=q['expected']])
        self.assertEqual(self.queries[34]['baseline'],self.queries[34]['expected'])
        self.assertEqual(self.queries[35]['baseline'],self.queries[35]['expected'])
        original=json.loads((second.CORPUS/'source-override-probes.json').read_text())
        false_success=copy.deepcopy(self.response)
        false_success['result']['queries'][35]['status']='Resolved'
        false_success['result']['queries'][35]['targets']=[second.expected_target(x,self.fixture) for x in original['queries'][35]['expected']['targets']]
        with self.assertRaises(second.AcceptanceError):self.check(false_success)

    def test_exact_old_fullrecords_and_new_plan_pin(self):
        plan=copy.deepcopy(self.plan);plan['source_sha256']={p:hashlib.sha256(b).hexdigest() for p,b in self.fixture.sources.items()}
        with mock.patch.object(s,'override_plan',return_value=plan):
            self.assertEqual(self.queries,s.override_probes(self.fixture,self.historical))
            for index in range(35):
                old=copy.deepcopy(self.historical);old[index]['expected']['targets'].pop()
                with self.assertRaises(second.AcceptanceError):s.override_probes(self.fixture,old)
            old=copy.deepcopy(self.historical);old[31]['byte']=3002
            with self.assertRaises(second.AcceptanceError):s.override_probes(self.fixture,old)
        path=self.fixture.workdir/'bad-override.json';path.write_text(json.dumps(self.plan))
        with self.assertRaises(second.AcceptanceError):s.override_plan(path)
    def test_new_capture_uses_previous_proof_current_parser(self):
        with mock.patch.object(second,'digest',return_value=self.plan['baseline_native_sha256']), \
             mock.patch.object(s,'frozen_override_probes',return_value=self.queries), \
             mock.patch.object(second,'all_source_hashes',return_value={}), \
             mock.patch.object(second,'document',return_value=Path('requests')), \
             mock.patch.object(second,'successful',return_value=self.response_for('baseline')), \
             mock.patch.object(s.string,'run',return_value={'source_sha256_after':{}}) as keep:
            report=s.capture_proof_stage(Path('native'),self.fixture,'source_override')
            self.assertEqual(report['validation'],'baseline_verified_not_acceptance')
            keep.assert_called_once_with(Path('native'),self.fixture,proof_stage='inherited_source',parser_stage='current')
        with mock.patch.object(second,'digest',return_value='wrong'),mock.patch.object(second,'successful') as execute:
            with self.assertRaises(second.AcceptanceError):s.capture_proof_stage(Path('native'),self.fixture,'source_override')
            execute.assert_not_called()

class SourceInvocationControls(unittest.TestCase):
    response_for = InheritedControls.response_for
    check = InheritedControls.check
    test_all_rows_candidates_identity_and_positions = InheritedControls.test_all_rows_candidates_identity_and_positions
    def setUp(self):
        SourceOverrideControls.setUp(self)
        self.historical=self.queries
        self.plan=s.invocation_plan();self.queries=self.plan['queries']
        buffers={p:bytearray(b) for p,b in self.fixture.sources.items()}
        for name in self.plan['source_sha256']:
            if name not in buffers:buffers[name]=bytearray(b' '*20000)
            self.fixture.owner[name]='gson/test' if '/src/test/' in name else 'gson/main'
        for q in self.queries:
            expression=q['expression'].encode();buffers[q['file']][q['byte']:q['byte']+len(expression)]=expression
            for target in q['baseline']['targets']+q['expected']['targets']:
                if target['origin']=='source':buffers[target['path']][target['start']:target['end']]=target['name'].encode()
        self.fixture.sources={p:bytes(b) for p,b in buffers.items()}
        for p,b in self.fixture.sources.items():
            dest=self.fixture.source_root/p;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(b)
        self.response=self.response_for('expected')
    def test_eight_improvements_with_every_previous_record_unchanged(self):
        self.check(self.response);self.check(self.response_for('baseline'),True)
        for response,mode in [(self.response,True),(self.response_for('baseline'),False)]:
            with self.assertRaises(second.AcceptanceError):self.check(response,mode)
        self.assertEqual([2405,1473,1543,1613,3188,3246,6826,6888],[q['byte'] for q in self.queries if q['baseline']!=q['expected']])
        self.assertEqual(self.queries[35]['baseline'],self.queries[35]['expected'])
    def test_exact_predecessor_and_frozen_plan(self):
        plan=copy.deepcopy(self.plan);plan['source_sha256']={p:hashlib.sha256(b).hexdigest() for p,b in self.fixture.sources.items()}
        with mock.patch.object(s,'invocation_plan',return_value=plan):
            self.assertEqual(self.queries,s.invocation_probes(self.fixture,self.historical))
            for index in range(36):
                old=copy.deepcopy(self.historical);old[index]['expected']['status']='Wrong'
                with self.assertRaises(second.AcceptanceError):s.invocation_probes(self.fixture,old)
            old=copy.deepcopy(self.historical);old.pop()
            with self.assertRaises(second.AcceptanceError):s.invocation_probes(self.fixture,old)
        path=self.fixture.workdir/'bad-invocation.json';path.write_text(json.dumps(self.plan))
        with self.assertRaises(second.AcceptanceError):s.invocation_plan(path)
    def test_capture_stage_and_binary_are_explicit(self):
        with mock.patch.object(second,'digest',return_value=self.plan['baseline_native_sha256']), \
             mock.patch.object(s,'current_probes',return_value=self.queries), \
             mock.patch.object(second,'all_source_hashes',return_value={}), \
             mock.patch.object(second,'document',return_value=Path('requests')), \
             mock.patch.object(second,'successful',return_value=self.response_for('baseline')), \
             mock.patch.object(s.string,'run',return_value={'source_sha256_after':{}}) as keep:
            report=s.capture_proof_stage(Path('native'),self.fixture,'source_invocation')
            self.assertEqual(report['validation'],'baseline_verified_not_acceptance')
            keep.assert_called_once_with(Path('native'),self.fixture,proof_stage='source_override',parser_stage='current')
        with mock.patch.object(second,'digest',return_value='wrong'),mock.patch.object(second,'successful') as execute:
            with self.assertRaises(second.AcceptanceError):s.capture_proof_stage(Path('native'),self.fixture,'source_invocation')
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
                else:rows[i]['targets'].append(copy.deepcopy(rows[i]['targets'][0]) if rows[i]['targets'] else {})
                with self.assertRaises(ValueError):self.c.check_inherited_preservation(changed,self.plan,self.root)

class BinaryMethodControls(unittest.TestCase):
    def setUp(self):
        spec=importlib.util.spec_from_file_location('commons_binary',Path(__file__).with_name('check-cli-acceptance.py'))
        self.c=importlib.util.module_from_spec(spec);spec.loader.exec_module(self.c)
        self.plan=self.c.binary_method_plan();self.root=Path('/synthetic/sources');self.platform=Path('/synthetic/java.base.jmod')
    def response(self, mode):
        rows=copy.deepcopy([q[mode] for q in self.plan['queries']])
        for row in rows:
            row['file']=str(self.root/row['file'])
            for target in row['targets']:target['path']=target['path'].replace('@PLATFORM@',str(self.platform))
        result={'index_builds':1,'queries':rows}
        if mode=='expected_details':result['target_details_version']=1
        return {'result':result}
    def check(self, value, mode):
        self.c.check_binary_methods(value,self.plan,self.root,self.platform,
             baseline=mode=='baseline',details=mode=='expected_details')
    def test_three_disjoint_modes(self):
        for mode in ['baseline','expected','expected_details']:
            self.check(self.response(mode),mode)
            for other in ['baseline','expected','expected_details']:
                if other!=mode:
                    with self.assertRaises(ValueError):self.check(self.response(other),mode)
    def test_full_inventory_and_identity_mutations(self):
        for mode in ['baseline','expected','expected_details']:
            for i in range(10):
                for key,value in [('query_index',99),('file','/wrong'),('byte_offset',0),('position',{}),('compilation',{}),('source_compilation',{}),('status','Wrong')]:
                    d=self.response(mode);d['result']['queries'][i][key]=value
                    with self.assertRaises(ValueError):self.check(d,mode)
                for key in self.response(mode)['result']['queries'][i]['targets'][0]:
                    d=self.response(mode);d['result']['queries'][i]['targets'][0][key]='wrong'
                    with self.assertRaises(ValueError):self.check(d,mode)
                for action in ['omit','extra','targetomit','targetextra']:
                    d=self.response(mode);rows=d['result']['queries']
                    if action=='omit':rows.pop(i)
                    elif action=='extra':rows.append(copy.deepcopy(rows[i]))
                    elif action=='targetomit':rows[i]['targets'].pop()
                    else:rows[i]['targets'].append(copy.deepcopy(rows[i]['targets'][0]))
                    with self.assertRaises(ValueError):self.check(d,mode)
    def test_default_cannot_leak_details_or_drop_context(self):
        for key,value in [('descriptor','()I'),('linking_context',{}),('identity_precision','exact')]:
            d=self.response('expected');d['result']['queries'][0]['targets'][0][key]=value
            with self.assertRaises(ValueError):self.check(d,'expected')
        for mode in ['baseline','expected','expected_details']:
            d=self.response(mode);d['target_details_version']=1
            with self.assertRaises(ValueError):self.check(d,mode)
        d=self.response('expected_details');del d['result']['target_details_version']
        with self.assertRaises(ValueError):self.check(d,'expected_details')
        d=self.response('expected_details');d['result']['queries'][0]['targets'][0]['linking_context']=None
        with self.assertRaises(ValueError):self.check(d,'expected_details')
    def test_v1_refusal_forecast_is_not_accepted_as_current(self):
        old=json.loads((self.c.CORPUS/'binary-method-probes.json').read_text())
        for mode in ['expected','expected_details']:
            d=self.response(mode)
            for i in (3,4,8):
                row=copy.deepcopy(old['queries'][i][mode]);row['file']=str(self.root/row['file'])
                for t in row['targets']:t['path']=t['path'].replace('@PLATFORM@',str(self.platform))
                d['result']['queries'][i]=row
            with self.assertRaises(ValueError):self.check(d,mode)
    def test_v2_success_forecast_not_accepted_for_deferred_sites(self):
        old=self.c.binary_method_plan_v2()
        for mode in ['expected','expected_details']:
            d=self.response(mode)
            for i in (0,1,2):
                row=copy.deepcopy(old['queries'][i][mode]);row['file']=str(self.root/row['file'])
                for t in row['targets']:t['path']=t['path'].replace('@PLATFORM@',str(self.platform))
                d['result']['queries'][i]=row
            with self.assertRaises(ValueError):self.check(d,mode)
    def test_revision_reconstruction_even_with_candidate_hash_stub(self):
        import tempfile
        original_sha=self.c.sha256
        with tempfile.TemporaryDirectory() as td:
            p=Path(td)/'revision.json'
            for kind in ['positive','baseline','extra_change']:
                d=copy.deepcopy(self.plan)
                if kind=='positive':d['queries'][5]['expected']['status']='OverloadNotResolved'
                elif kind=='baseline':d['queries'][3]['baseline']['status']='Ambiguous'
                else:d['scope_revision']['changes'].append(copy.deepcopy(d['scope_revision']['changes'][0]))
                p.write_text(json.dumps(d))
                with mock.patch.object(self.c,'sha256',side_effect=lambda path:self.c.BINARY_METHOD_PLAN_SHA256 if path==p else original_sha(path)):
                    with self.assertRaises(ValueError):self.c.binary_method_plan(p)
    def test_plan_hash_rejects_mutation(self):
        import tempfile
        with tempfile.TemporaryDirectory() as td:
            p=Path(td)/'plan.json';p.write_text(json.dumps(self.plan)+' ')
            with self.assertRaises(ValueError):self.c.binary_method_plan(p)
            link=Path(td)/'linked.json';link.symlink_to(self.c.CORPUS/'binary-method-probes.json')
            with self.assertRaises(ValueError):self.c.binary_method_plan(link)

if __name__=='__main__':unittest.main()
