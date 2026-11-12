#!/usr/bin/env python3
"""Additive pinned Gson strict-reference-phase gate. Offline; never executes Java."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile

_spec = importlib.util.spec_from_file_location('strict_string', Path(__file__).with_name('check-string-call-acceptance.py'))
string = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(string)
second = string.second
PLAN = second.CORPUS / 'strict-reference-probes.json'
PLAN_SHA256 = '8a5e58f8ac87a206f3c61fa5459f24e3e0f62f25cfe4ac2470945d484d16d0df'

_contract_spec = importlib.util.spec_from_file_location('qualified_contract', Path(__file__).with_name('qualified-primitive-contract.py'))
qualified = importlib.util.module_from_spec(_contract_spec)
_contract_spec.loader.exec_module(qualified)


def load_plan(path=PLAN):
    second.reject_symlinks(path.absolute())
    data = path.read_bytes()
    second.same(hashlib.sha256(data).hexdigest(), PLAN_SHA256, 'frozen complete strict reference plan digest')
    return json.loads(data, object_pairs_hook=second.unique_json_object)


def probes(fixture):
    plan = load_plan()
    second.same(plan['source_archive_sha256'], fixture.metadata['inputs']['sources']['sha256'],
                'strict reference source archive pin')
    for name, checksum in plan['historical_artifact_sha256'].items():
        second.same(second.digest(second.CORPUS / name), checksum, 'strict reference preserved artifact ' + name)
    for name, checksum in plan['source_sha256'].items():
        data = fixture.sources[name]
        second.same(hashlib.sha256(data).hexdigest(), checksum, 'strict reference source digest ' + name)
        second.reject_symlinks(fixture.source_root / name)
        second.same((fixture.source_root / name).read_bytes(), data, 'strict reference extracted bytes ' + name)
    queries = plan['queries']
    second.same(len(queries), 4, 'strict reference complete fixed plan')
    second.same(len({q['id'] for q in queries}), 4, 'strict reference unique query IDs')
    for query in queries:
        data = fixture.sources[query['file']]
        expression = query['expression'].encode()
        second.same(data[query['byte']:query['byte'] + len(expression)], expression,
                    query['id'] + ' exact argument expression')
        for stage in ('baseline', 'expected'):
            for target in query[stage]['targets']:
                data = fixture.sources[target['path']]
                second.same(data[target['start']:target['end']], target['name'].encode(),
                            query['id'] + ' exact declaration token')
    # Independent source/formal evidence, frozen before candidate observation.
    for item in plan['source_spellings']:
        data = fixture.sources[item['path']]
        spelling = item['spelling'].encode()
        second.same(data[item['byte']:item['byte'] + len(spelling)], spelling,
                    'independent source/formal spelling ' + item['spelling'])
    return queries


def frozen_qualified_probes(fixture):
    """Exact post-single-primitive stage, not the evolving current contract."""
    return qualified.overlay(second, fixture, probes(fixture))


_mixed_spec = importlib.util.spec_from_file_location('mixed_strict_contract', Path(__file__).with_name('mixed-strict-contract.py'))
mixed = importlib.util.module_from_spec(_mixed_spec)
_mixed_spec.loader.exec_module(mixed)


def frozen_mixed_probes(fixture):
    return mixed.overlay(second, fixture, frozen_qualified_probes(fixture))


_return_spec = importlib.util.spec_from_file_location('reference_return_contract', Path(__file__).with_name('reference-return-contract.py'))
returns = importlib.util.module_from_spec(_return_spec)
_return_spec.loader.exec_module(returns)


def frozen_return_probes(fixture):
    return returns.overlay(second, fixture, frozen_mixed_probes(fixture))


_null_spec = importlib.util.spec_from_file_location('null_actual_contract', Path(__file__).with_name('null-actual-contract.py'))
nulls = importlib.util.module_from_spec(_null_spec)
_null_spec.loader.exec_module(nulls)


def frozen_null_probes(fixture):
    return nulls.overlay(second, fixture, frozen_return_probes(fixture))


_binary_spec = importlib.util.spec_from_file_location('binary_argument_contract', Path(__file__).with_name('binary-argument-contract.py'))
binary_arguments = importlib.util.module_from_spec(_binary_spec)
_binary_spec.loader.exec_module(binary_arguments)


def frozen_binary_probes(fixture):
    return binary_arguments.overlay(second, fixture, frozen_null_probes(fixture))


_cast_spec = importlib.util.spec_from_file_location('cast_null_contract', Path(__file__).with_name('cast-null-contract.py'))
casts = importlib.util.module_from_spec(_cast_spec)
_cast_spec.loader.exec_module(casts)


def frozen_cast_probes(fixture):
    return casts.overlay(second, fixture, frozen_binary_probes(fixture))


INHERITED_PLAN_SHA256 = '1b40aa2f447a11ebfcdda3d74015634316b071b80da0f801dba8e981208463bc'


def inherited_plan(path=None):
    path = second.CORPUS / 'inherited-source-probes.json' if path is None else path
    second.reject_symlinks(path.absolute())
    data = path.read_bytes()
    second.same(hashlib.sha256(data).hexdigest(), INHERITED_PLAN_SHA256, 'complete inherited-source plan')
    return json.loads(data, object_pairs_hook=second.unique_json_object)


def inherited_probes(fixture, historical):
    plan = inherited_plan()
    second.same(plan['source_archive_sha256'], fixture.metadata['inputs']['sources']['sha256'], 'inherited source archive')
    for name, checksum in plan['historical_artifact_sha256'].items():
        second.same(second.digest(second.CORPUS / name), checksum, 'preserved inherited-stage artifact ' + name)
    for name, checksum in plan['source_sha256'].items():
        data = fixture.sources[name]
        second.same(hashlib.sha256(data).hexdigest(), checksum, 'inherited source bytes ' + name)
        second.reject_symlinks(fixture.source_root / name)
        second.same((fixture.source_root / name).read_bytes(), data, 'unchanged extracted inherited source')
    queries = plan['queries']
    second.same(len(queries), 35, 'exact inherited-stage query count')
    second.same(len(historical), 29, 'exact cast predecessor count')
    for q, old in zip(queries[:29], historical):
        second.same(q, dict(old, baseline=old['expected']), 'complete preserved predecessor query')
    second.same([q['byte'] for q in queries[29:]], [5305,2883,3001,6712,10817,6612], 'exact inherited additions')
    second.same([q['byte'] for q in queries if q['baseline'] != q['expected']], [5305,2883], 'only two inherited improvements')
    second.same(len({q['id'] for q in queries}), 35, 'unique inherited query IDs')
    for q in queries:
        expression = q['expression'].encode()
        second.same(fixture.sources[q['file']][q['byte']:q['byte']+len(expression)], expression, 'exact inherited expression')
        for stage in ('baseline','expected'):
            for target in q[stage]['targets']:
                second.expected_target(target, fixture)  # validates owner compilation and UTF-16 materialization
                if target['origin'] == 'source':
                    second.same(fixture.sources[target['path']][target['start']:target['end']], target['name'].encode(), 'exact inherited declaration')
    return queries


def frozen_inherited_probes(fixture):
    return inherited_probes(fixture, frozen_cast_probes(fixture))


OVERRIDE_V1_SHA256 = '95ede0ef806796ee459348b523d2834d11e3cfd97bfdd804d4dc2867db3c39ae'
OVERRIDE_PLAN_SHA256 = '70298e4fe5c27f8ee2e2ec64be2246ac586754f05377ea831bb6a230a67d4eca'
OVERRIDE_DIAGNOSIS_SHA256 = '320489e023f00dc402f86d20e115fd9ce38aa6c98d00a39ea63751366cd1b4b6'


def override_plan(path=None):
    def pinned(path, checksum, label):
        second.reject_symlinks(path.absolute())
        data=path.read_bytes()
        second.same(hashlib.sha256(data).hexdigest(), checksum, label)
        return json.loads(data, object_pairs_hook=second.unique_json_object)
    original=pinned(second.CORPUS/'source-override-probes.json', OVERRIDE_V1_SHA256, 'preserved original override forecast')
    diagnosis=pinned(second.CORPUS/'source-override-deferred-diagnosis.json', OVERRIDE_DIAGNOSIS_SHA256, 'fixed annotated-local diagnostic')
    path=second.CORPUS/'source-override-probes-v2.json' if path is None else path
    revised=pinned(path, OVERRIDE_PLAN_SHA256, 'complete override V2 contract')
    for field in ('diagnostic_source','diagnostic_log','real_source_log'):
        second.same(hashlib.sha256(diagnosis[field].encode()).hexdigest(), diagnosis[field+'_sha256'], 'retained diagnostic bytes '+field)
    second.same(diagnosis['results'], [{'annotation':a,'status':s,'candidate_count':n} for a,s,n in [('', 'Resolved',1),('@A','Ambiguous',2),('@A()','Ambiguous',2),('@SuppressWarnings("deprecation")','Ambiguous',2)]], 'independent diagnostic outcomes')
    second.same(diagnosis['real_source_sha256'], {k:v for k,v in original['source_sha256'].items() if 'JsonNull' in k}, 'original JsonNull source hashes')
    expected=json.loads(json.dumps(original))
    q=expected['queries'][35]
    q['expected']=q['baseline']
    expected['scope_revision']={'supersedes':'source-override-probes.json','supersedes_sha256':OVERRIDE_V1_SHA256,
        'disposition':'deferred_annotated_local_receiver','query_id':q['id'],'reason':diagnosis['reason'],
        'original_expected':original['queries'][35]['expected'],'revised_expected':q['expected'],
        'diagnosis':'source-override-deferred-diagnosis.json','diagnosis_sha256':OVERRIDE_DIAGNOSIS_SHA256,
        'first_candidate_failure_sha256':diagnosis['first_candidate_failure_sha256']}
    second.same(revised, expected, 'only JsonNull expected result revised; all original records retained')
    return revised


def override_probes(fixture, historical):
    plan = override_plan()
    second.same(plan['source_archive_sha256'], fixture.metadata['inputs']['sources']['sha256'], 'override source archive')
    for name, checksum in plan['historical_artifact_sha256'].items():
        second.same(second.digest(second.CORPUS / name), checksum, 'preserved override-stage artifact ' + name)
    for name, checksum in plan['source_sha256'].items():
        data = fixture.sources[name]
        second.same(hashlib.sha256(data).hexdigest(), checksum, 'override source bytes ' + name)
        second.reject_symlinks(fixture.source_root / name)
        second.same((fixture.source_root / name).read_bytes(), data, 'unchanged override source')
    queries = plan['queries']
    second.same(len(queries), 36, 'exact override-stage query count')
    second.same(len(historical), 35, 'exact inherited predecessor count')
    promoted = {31:'com.google.gson.JsonArray',32:'com.google.gson.JsonObject',33:'com.google.gson.JsonPrimitive'}
    for index, (q, old) in enumerate(zip(queries[:35], historical)):
        expected = dict(old, baseline=old['expected'])
        if index in promoted:
            expected['expected'] = {'status':'Resolved', 'targets':[target for target in old['expected']['targets'] if target['owner']==promoted[index]]}
        second.same(q, expected, 'exact complete predecessor record and explicit override promotion')
    second.same([q['byte'] for q in queries if q['baseline'] != q['expected']], [3001,6712,10817], 'only three real override improvements')
    second.same((queries[35]['file'],queries[35]['byte'],queries[35]['expression']),
        ('gson/src/test/java/com/google/gson/JsonNullTest.java',1261,'deepCopy()'), 'exact additional JsonNull site')
    second.same(queries[35]['expected'], queries[35]['baseline'], 'JsonNull annotated-local remains exact deferred refusal')
    second.same(len({q['id'] for q in queries}), 36, 'unique override query IDs')
    for q in queries:
        expression = q['expression'].encode()
        second.same(fixture.sources[q['file']][q['byte']:q['byte']+len(expression)], expression, 'exact override expression')
        for stage in ('baseline','expected'):
            for target in q[stage]['targets']:
                second.expected_target(target, fixture)
                if target['origin']=='source':
                    second.same(fixture.sources[target['path']][target['start']:target['end']], target['name'].encode(), 'exact override declaration')
    return queries


def frozen_override_probes(fixture):
    return override_probes(fixture, frozen_inherited_probes(fixture))


INVOCATION_PLAN_SHA256 = 'c893191907577f4568e2f335d0e159d862c78a7180ded2a3457b8439d5fac5fd'


def invocation_plan(path=None):
    path=second.CORPUS/'source-invocation-probes.json' if path is None else path
    second.reject_symlinks(path.absolute())
    data=path.read_bytes()
    second.same(hashlib.sha256(data).hexdigest(), INVOCATION_PLAN_SHA256, 'complete bounded invocation plan')
    return json.loads(data, object_pairs_hook=second.unique_json_object)


def invocation_probes(fixture, historical):
    plan=invocation_plan()
    second.same(plan['source_archive_sha256'], fixture.metadata['inputs']['sources']['sha256'], 'invocation source archive')
    for name, checksum in plan['historical_artifact_sha256'].items():
        second.same(second.digest(second.CORPUS/name), checksum, 'preserved invocation artifact '+name)
    for name, checksum in plan['source_sha256'].items():
        data=fixture.sources[name]
        second.same(hashlib.sha256(data).hexdigest(), checksum, 'invocation source bytes '+name)
        second.reject_symlinks(fixture.source_root/name)
        second.same((fixture.source_root/name).read_bytes(), data, 'unchanged invocation source')
    queries=plan['queries']
    second.same(len(queries),44,'exact bounded invocation query count')
    second.same(len(historical),36,'exact override V2 predecessor count')
    for q,old in zip(queries[:36],historical):
        second.same(q,dict(old,baseline=old['expected']),'complete unchanged predecessor record including deferred JsonNull')
    offsets=[2405,1473,1543,1613,3188,3246,6826,6888]
    second.same([q['byte'] for q in queries[36:]],offsets,'exact eight invocation sites')
    second.same([q['byte'] for q in queries if q['baseline']!=q['expected']],offsets,'only eight invocation improvements')
    second.same(len({q['id'] for q in queries}),44,'unique invocation query IDs')
    for q in queries:
        expression=q['expression'].encode()
        second.same(fixture.sources[q['file']][q['byte']:q['byte']+len(expression)],expression,'exact invocation expression')
        for stage in ('baseline','expected'):
            for target in q[stage]['targets']:
                second.expected_target(target,fixture)
                if target['origin']=='source':
                    second.same(fixture.sources[target['path']][target['start']:target['end']],target['name'].encode(),'exact invocation declaration')
    return queries


def current_probes(fixture):
    return invocation_probes(fixture, frozen_override_probes(fixture))


def capture_proof_stage(native, fixture, stage):
    """Explicit additive capture; legacy feature captures retain their old entry points."""
    captures={
        'inherited_source':(inherited_plan,frozen_inherited_probes,INHERITED_PLAN_SHA256,'cast_null','39fa8de8'),
        'source_override':(override_plan,frozen_override_probes,OVERRIDE_PLAN_SHA256,'inherited_source','dcb31d23'),
        'source_invocation':(invocation_plan,current_probes,INVOCATION_PLAN_SHA256,'source_override','68d12d9c'),
    }
    second.require(stage in captures, 'supported additive capture stage')
    plan_loader,probe_loader,checksum,previous,commit=captures[stage]
    plan=plan_loader()
    second.same(second.digest(native),plan['baseline_native_sha256'],'exact frozen predecessor binary')
    queries=probe_loader(fixture)
    before = second.all_source_hashes(fixture)
    request = second.document(fixture, 'inherited-capture.json', {'schema_version':1, 'queries':[
        {'compilation':q['compilation'], 'file':'sources/'+q['file'], 'position':{'encoding':'utf8','offset':q['byte']}} for q in queries]})
    response = second.successful(native, fixture, 'declaration', requests=request, strict_json=True)
    second.check_reference_call_response(response, queries, fixture, 'baseline')
    preservation = string.run(native, fixture, proof_stage=previous, parser_stage='current')
    after = second.all_source_hashes(fixture)
    second.same(before, after, 'capture sources unchanged')
    second.same(after, preservation['source_sha256_after'], 'capture preservation sources')
    return {'validation':'baseline_verified_not_acceptance', 'comparison_stage':stage+'_baseline_'+commit,
            'plan_sha256':checksum, 'native_sha256':second.digest(native),
            'queries':second.portable_queries(response,fixture), 'preservation':preservation,
            'source_sha256_before':before, 'source_sha256_after':after, 'source_bytes_unchanged':True}


def check_response(response, queries, fixture, capture_baseline=False):
    second.require(type(capture_baseline) is bool, 'comparison mode must be boolean')
    second.check_reference_call_response(response, queries, fixture,
                                         'baseline' if capture_baseline else 'expected')


def run(native, fixture, capture_baseline=False):
    proof_stage, parser_stage = second.proof_stages(capture_baseline)
    queries = probes(fixture) if capture_baseline else current_probes(fixture)
    before = second.all_source_hashes(fixture)
    request = second.document(fixture, 'strict-reference-requests.json', {
        'schema_version': 1, 'queries': [
            {'compilation': q['compilation'], 'file': 'sources/' + q['file'],
             'position': {'encoding': 'utf8', 'offset': q['byte']}} for q in queries]})
    response = second.successful(native, fixture, 'declaration', requests=request, strict_json=True)
    check_response(response, queries, fixture, capture_baseline)
    # Always use current string/constructor/reference/local/original expectations.
    preservation = string.run(native, fixture, proof_stage=proof_stage, parser_stage=parser_stage)
    after = second.all_source_hashes(fixture)
    second.same(before, after, 'all source bytes unchanged throughout strict reference gate')
    second.same(after, preservation['source_sha256_after'], 'complete preservation inventory')
    return {
        'validation': 'baseline_verified_not_acceptance' if capture_baseline else 'passed',
        'comparison_stage': 'baseline' if capture_baseline else 'expected',
        'plan_sha256': PLAN_SHA256 if capture_baseline else INVOCATION_PLAN_SHA256,
        'historical_strict_plan_sha256': PLAN_SHA256, 'native_sha256': second.digest(native),
        'queries': second.portable_queries(response, fixture),
        'preservation': preservation, 'source_sha256_before': before, 'source_sha256_after': after,
        'source_bytes_unchanged': True, 'javac_real_gson_equivalence': False,
        'upstream_code_executed': False,
        'analysis_environment': {'PATH': '/nonexistent', 'java_executed': False},
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('inputs_dir', type=Path)
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument('--output', type=Path)
    captures = parser.add_mutually_exclusive_group()
    captures.add_argument('--capture-baseline', action='store_true')
    captures.add_argument('--capture-proof-stage', choices=['inherited_source','source_override','source_invocation'])
    args = parser.parse_args()
    native = args.native.resolve()
    second.require(native.is_file(), 'native executable must exist')
    with tempfile.TemporaryDirectory(prefix='strict-reference-') as tmp:
        fixture = second.prepare_fixture(args.inputs_dir, Path(tmp) / 'fixture')
        report = capture_proof_stage(native, fixture, args.capture_proof_stage) if args.capture_proof_stage else run(native, fixture, args.capture_baseline)
        text = json.dumps(report, indent=2) + '\n'
        if args.output:
            second.reject_symlinks(args.output.absolute())
            with args.output.open('x', encoding='utf-8') as output:
                output.write(text)
        print(text, end='')


if __name__ == '__main__':
    main()
