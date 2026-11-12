"""Frozen additive mixed-argument expectations; no executable or network work."""
import copy
import hashlib
import json

PLAN_NAME = 'mixed-strict-probes.json'
PLAN_SHA256 = '64d4519c43fdb8e485db1279ad24d9216b000d3193064b5dab1cc281903b92d6'


def load_plan(second, path=None):
    path = second.CORPUS / PLAN_NAME if path is None else path
    second.reject_symlinks(path.absolute())
    data = path.read_bytes()
    second.same(hashlib.sha256(data).hexdigest(), PLAN_SHA256, 'frozen complete mixed strict plan')
    return json.loads(data, object_pairs_hook=second.unique_json_object)


def overlay(second, fixture, historical_queries):
    plan = load_plan(second)
    second.same(plan['source_archive_sha256'], fixture.metadata['inputs']['sources']['sha256'],
                'mixed strict source archive')
    for name, checksum in plan['historical_artifact_sha256'].items():
        second.same(second.digest(second.CORPUS / name), checksum, 'preserved mixed strict artifact ' + name)
    for name, checksum in plan['source_sha256'].items():
        data = fixture.sources[name]
        second.same(hashlib.sha256(data).hexdigest(), checksum, 'mixed strict source ' + name)
        second.reject_symlinks(fixture.source_root / name)
        second.same((fixture.source_root / name).read_bytes(), data, 'unchanged mixed strict source bytes')
    queries = plan['queries']
    second.same(len(queries), 4, 'mixed strict complete query count')
    second.same(len(historical_queries), 4, 'historical qualified query count')
    changed = []
    for q, old in zip(queries, historical_queries):
        metadata = lambda row: {k: v for k, v in row.items() if k not in ('id', 'baseline', 'expected')}
        second.same(second.canonical_json(metadata(q)), second.canonical_json(metadata(old)),
                    'unchanged historical query position and expression')
        second.same(second.canonical_json(q['baseline']), second.canonical_json(old['expected']),
                    'mixed baseline is exact previously accepted qualified result')
        if q['expected'] != q['baseline']:
            changed.append(q['byte'])
            winner = [t for t in q['baseline']['targets'] if t['start'] == 4463 and t['end'] == 4466]
            second.same(len(winner), 1, 'unique independently selected set(int,JsonElement) candidate')
            second.same(q['expected'], {'status': 'Resolved', 'targets': winner}, 'only planned winner changes')
        data = fixture.sources[q['file']]
        expr = q['expression'].encode()
        second.same(data[q['byte']:q['byte'] + len(expr)], expr, 'mixed strict exact call expression')
        for stage in ('baseline', 'expected'):
            for target in q[stage]['targets']:
                data = fixture.sources[target['path']]
                second.same(data[target['start']:target['end']], target['name'].encode(), 'exact target token')
    second.same(changed, [2412], 'only set(0,b) deliberately changes')
    for item in plan['source_spellings']:
        data = fixture.sources[item['path']]
        spelling = item['spelling'].encode()
        second.same(data[item['byte']:item['byte'] + len(spelling)], spelling, 'independent formal/source spelling')
    return copy.deepcopy(queries)
