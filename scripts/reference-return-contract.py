"""Frozen additive reference-return expectations; no executable or network work."""
import copy
import hashlib
import json

PLAN_NAME = 'reference-return-probes.json'
PLAN_SHA256 = '0370da36211be7d68ee780d46301944381e114f6f8aac454c11dd4f556da8dd6'


def load_plan(second, path=None):
    path = second.CORPUS / PLAN_NAME if path is None else path
    second.reject_symlinks(path.absolute())
    data = path.read_bytes()
    second.same(hashlib.sha256(data).hexdigest(), PLAN_SHA256, 'frozen complete reference return plan')
    return json.loads(data, object_pairs_hook=second.unique_json_object)


def overlay(second, fixture, historical_queries):
    plan = load_plan(second)
    second.same(plan['source_archive_sha256'], fixture.metadata['inputs']['sources']['sha256'],
                'reference return source archive')
    for name, checksum in plan['historical_artifact_sha256'].items():
        second.same(second.digest(second.CORPUS / name), checksum, 'preserved reference return artifact ' + name)
    for name, checksum in plan['source_sha256'].items():
        data = fixture.sources[name]
        second.same(hashlib.sha256(data).hexdigest(), checksum, 'reference return source ' + name)
        second.reject_symlinks(fixture.source_root / name)
        second.same((fixture.source_root / name).read_bytes(), data, 'unchanged reference return source bytes')
    queries = plan['queries']
    second.same(len(queries), 5, 'reference return complete query count')
    second.same(len(historical_queries), 4, 'historical mixed query count')
    changed = []
    for q, old in zip(queries, historical_queries):
        metadata = lambda row: {k: v for k, v in row.items() if k not in ('id', 'baseline', 'expected')}
        second.same(second.canonical_json(metadata(q)), second.canonical_json(metadata(old)),
                    'unchanged historical query position and expression')
        second.same(second.canonical_json(q['baseline']), second.canonical_json(old['expected']),
                    'return baseline is exact previously accepted mixed result')
        if q['expected'] != q['baseline']:
            changed.append(q['byte'])
            winner = [{'origin': 'source', 'path': 'gson/src/main/java/com/google/gson/JsonElement.java',
                       'kind': 'Method', 'owner': 'com.google.gson.JsonElement', 'name': 'getAsString',
                       'start': 6434, 'end': 6445}]
            second.same(len(winner), 1, 'unique independently selected JsonElement.getAsString candidate')
            second.same(q['expected'], {'status': 'Resolved', 'targets': winner}, 'only planned winner changes')
        data = fixture.sources[q['file']]
        expr = q['expression'].encode()
        second.same(data[q['byte']:q['byte'] + len(expr)], expr, 'reference return exact call expression')
        for stage in ('baseline', 'expected'):
            for target in q[stage]['targets']:
                data = fixture.sources[target['path']]
                second.same(data[target['start']:target['end']], target['name'].encode(), 'exact target token')
    second.same(changed, [1986], 'only existing outer1986 deliberately changes')
    extra = queries[4]
    original = queries[3]
    second.same({k: v for k, v in extra.items() if k not in ('id', 'byte')},
                {k: v for k, v in original.items() if k not in ('id', 'byte')}, 'second outer query exact same static target')
    second.same(extra['byte'], 2489, 'fixed second outer query')
    data = fixture.sources[extra['file']]
    second.same(data[2482:2482 + len(b'get(0).getAsString()')], b'get(0).getAsString()', 'independent second inner and outer expression')
    for item in plan['source_spellings']:
        data = fixture.sources[item['path']]
        spelling = item['spelling'].encode()
        second.same(data[item['byte']:item['byte'] + len(spelling)], spelling, 'independent formal/source spelling')
    return copy.deepcopy(queries)


def original_overlay(second, fixture, expectations):
    """One correction in the complete frozen original 16-query inventory."""
    historical = json.loads((second.CORPUS / 'mixed-strict-probes.json').read_text(),
                            object_pairs_hook=second.unique_json_object)['queries']
    current = overlay(second, fixture, historical)
    plan = json.loads((second.CORPUS / 'query-plan.json').read_text(),
                     object_pairs_hook=second.unique_json_object)['queries']
    second.same(fixture.plan, plan, 'complete unchanged original query plan')
    old = json.loads((second.CORPUS / 'expected.json').read_text(), object_pairs_hook=second.unique_json_object)
    constructor = json.loads((second.CORPUS / 'constructor-call-probe.json').read_text(),
                            object_pairs_hook=second.unique_json_object)
    second.same(old[constructor['query_index']], constructor['baseline'], 'exact constructor predecessor')
    old[constructor['query_index']] = constructor['expected']
    second.same(expectations, old, 'complete original results at pre-return stage')
    result = copy.deepcopy(expectations)
    changes = 0
    target = current[3]
    for i, q in enumerate(plan):
        if q['file'] == target['file'] and q['byte'] == target['byte']:
            metadata = {k: v for k, v in q.items() if k not in ('id', 'goal')}
            second.same(metadata, {k: target[k] for k in metadata}, 'exact original outer metadata')
            second.same(result[i], target['baseline'], 'exact original outer baseline record')
            result[i] = copy.deepcopy(target['expected'])
            changes += 1
    second.same(changes, 1, 'only one original query changes')
    return result
