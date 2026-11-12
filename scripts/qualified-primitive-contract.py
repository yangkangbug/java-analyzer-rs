"""Frozen additive primitive-argument expectations; no executable or network work."""
import copy
import hashlib
import json

PLAN_NAME = 'qualified-primitive-probes.json'
PLAN_SHA256 = 'c9f2e08b78dabf9572f9456ae1d7e0c68ab169b57c0d2ea2408a7cf39834685e'


def load_plan(second, path=None):
    path = second.CORPUS / PLAN_NAME if path is None else path
    second.reject_symlinks(path.absolute())
    data = path.read_bytes()
    second.same(hashlib.sha256(data).hexdigest(), PLAN_SHA256, 'frozen complete qualified primitive plan')
    return json.loads(data, object_pairs_hook=second.unique_json_object)


def overlay(second, fixture, historical_queries):
    plan = load_plan(second)
    second.same(plan['source_archive_sha256'], fixture.metadata['inputs']['sources']['sha256'],
                'qualified primitive source archive')
    for name, checksum in plan['historical_artifact_sha256'].items():
        second.same(second.digest(second.CORPUS / name), checksum, 'preserved qualified primitive artifact ' + name)
    for name, checksum in plan['source_sha256'].items():
        data = fixture.sources[name]
        second.same(hashlib.sha256(data).hexdigest(), checksum, 'qualified primitive source ' + name)
        second.reject_symlinks(fixture.source_root / name)
        second.same((fixture.source_root / name).read_bytes(), data, 'unchanged qualified primitive source bytes')
    queries = plan['queries']
    second.same(len(queries), 4, 'qualified primitive complete query count')
    second.same(len(historical_queries), 4, 'historical strict query count')
    changed = []
    for q, old in zip(queries, historical_queries):
        metadata = lambda row: {k: v for k, v in row.items() if k not in ('id', 'baseline', 'expected')}
        second.same(second.canonical_json(metadata(q)), second.canonical_json(metadata(old)),
                    'unchanged historical query position and expression')
        second.same(second.canonical_json(q['baseline']), second.canonical_json(old['expected']),
                    'qualified baseline is exact previously accepted strict result')
        if q['expected'] != q['baseline']:
            changed.append(q['byte'])
            winner = [t for t in q['baseline']['targets'] if t['start'] == 5494 and t['end'] == 5500]
            second.same(len(winner), 1, 'unique independently selected remove(int) candidate')
            second.same(q['expected'], {'status': 'Resolved', 'targets': winner}, 'only planned winner changes')
        data = fixture.sources[q['file']]
        expr = q['expression'].encode()
        second.same(data[q['byte']:q['byte'] + len(expr)], expr, 'qualified primitive exact call expression')
        for stage in ('baseline', 'expected'):
            for target in q[stage]['targets']:
                data = fixture.sources[target['path']]
                second.same(data[target['start']:target['end']], target['name'].encode(), 'exact target token')
    second.same(changed, [1976], 'only remove(1) deliberately changes')
    for item in plan['source_spellings']:
        data = fixture.sources[item['path']]
        spelling = item['spelling'].encode()
        second.same(data[item['byte']:item['byte'] + len(spelling)], spelling, 'independent formal/source spelling')
    return copy.deepcopy(queries)
