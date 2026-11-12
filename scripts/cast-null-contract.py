"""Frozen exact cast-null additive contract. No old/new union."""
import hashlib
import json
PLAN_NAME = 'cast-null-probes.json'
PLAN_SHA256 = '3f2b471279027e7f71d4d07452e4a928ba6af67a1c18b7993c2447fe244a3e37'
def load_plan(second, path=None):
    path = second.CORPUS / PLAN_NAME if path is None else path
    second.reject_symlinks(path.absolute())
    data = path.read_bytes()
    second.same(hashlib.sha256(data).hexdigest(), PLAN_SHA256, 'complete frozen cast-null plan')
    return json.loads(data, object_pairs_hook=second.unique_json_object)
def overlay(second, fixture, historical):
    plan = load_plan(second)
    second.same(plan['source_archive_sha256'], fixture.metadata['inputs']['sources']['sha256'], 'cast-null archive')
    for name, checksum in plan['historical_artifact_sha256'].items():
        second.same(second.digest(second.CORPUS / name), checksum, 'preserved artifact ' + name)
    for name, checksum in plan['source_sha256'].items():
        data = fixture.sources[name]
        second.same(hashlib.sha256(data).hexdigest(), checksum, 'cast-null source ' + name)
        second.reject_symlinks(fixture.source_root / name)
        second.same((fixture.source_root / name).read_bytes(), data, 'extracted source bytes')
    for item in plan['source_spellings']:
        spelling = item['spelling'].encode()
        second.same(fixture.sources[item['path']][item['byte']:item['byte']+len(spelling)], spelling, 'independent source/formal spelling')
    queries = plan['queries']
    second.same(len(queries), 29, '29 exact cast-null stage queries')
    second.same(len(historical), 9, '9 exact binary predecessor queries')
    for q, old in zip(queries[:9], historical):
        expected = dict(old, baseline=old['expected'])
        second.same(q, expected, 'complete unchanged binary predecessor record')
    second.same([q['byte'] for q in queries[9:]], [6484,6850,7239,7315,7638,7980,8392,8502,8883,8920,8955,8990,9024,9057,9095,9129,9164,9573,9959,9994], 'exact twenty cast-null sites')
    second.same([q['byte'] for q in queries if q['baseline'] != q['expected']], [9164,9573], 'only two cast-null improvements')
    second.same(len({q['id'] for q in queries}), 29, 'unique query IDs')
    for q in queries:
        data = fixture.sources[q['file']]
        expression = q['expression'].encode()
        second.same(data[q['byte']:q['byte']+len(expression)], expression, 'full cast-null expression')
        for stage in ('baseline','expected'):
            for t in q[stage]['targets']:
                second.same(fixture.sources[t['path']][t['start']:t['end']], t['name'].encode(), 'exact declaration token')
    return queries
