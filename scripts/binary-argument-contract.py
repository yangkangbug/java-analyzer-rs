"""Frozen additive binary-argument expectations; no executable or network work."""
import copy
import hashlib
import json

PLAN_NAME = 'binary-argument-probes.json'
PLAN_SHA256 = 'd2cccac5da9beb1c75bd759ee71505ecc2d77d8be312d1c83ce719d2cc8e6a60'


def load_plan(second, path=None):
    path = second.CORPUS / PLAN_NAME if path is None else path
    second.reject_symlinks(path.absolute())
    data = path.read_bytes()
    second.same(hashlib.sha256(data).hexdigest(), PLAN_SHA256, 'frozen complete binary argument plan')
    return json.loads(data, object_pairs_hook=second.unique_json_object)


def overlay(second, fixture, historical_queries):
    plan = load_plan(second)
    second.same(plan['source_archive_sha256'], fixture.metadata['inputs']['sources']['sha256'],
                'binary argument source archive')
    for name, checksum in plan['historical_artifact_sha256'].items():
        second.same(second.digest(second.CORPUS / name), checksum, 'preserved binary argument artifact ' + name)
    for name, checksum in plan['source_sha256'].items():
        data = fixture.sources[name]
        second.same(hashlib.sha256(data).hexdigest(), checksum, 'binary argument source ' + name)
        second.reject_symlinks(fixture.source_root / name)
        second.same((fixture.source_root / name).read_bytes(), data, 'unchanged binary argument source bytes')
    queries = plan['queries']
    second.same(len(queries), 9, 'exact binary-stage nine query inventory')
    second.same(len(historical_queries), 8, 'exact previous null-v2 eight queries')
    target = {'origin': 'source', 'path': 'gson/src/main/java/com/google/gson/JsonObject.java',
              'kind': 'Method', 'owner': 'com.google.gson.JsonObject', 'name': 'add', 'start': 2345, 'end': 2348}
    for index, (q, old) in enumerate(zip(queries[:8], historical_queries)):
        metadata = lambda row: {k: v for k, v in row.items() if k not in ('id', 'baseline', 'expected')}
        second.same(metadata(q), metadata(old), 'unchanged prior query metadata')
        second.same(q['baseline'], old['expected'], 'exact prior null-v2 result including deferred refusal')
        if index == 6:
            second.same((q['file'], q['byte'], q['expression']),
                        ('gson/src/test/java/com/google/gson/JsonObjectTest.java', 2139, 'add(propertyName, null)'),
                        'only frozen deferred query is promoted')
            second.same(q['baseline'], {'status': 'CallNotResolved', 'targets': [target]}, 'exact deferred predecessor')
            second.same(q['expected'], {'status': 'Resolved', 'targets': [target]}, 'exact declared recovery target')
        else:
            second.same(q['expected'], old['expected'], 'all other prior queries remain unchanged')
    extra = queries[8]
    second.same({k: v for k, v in extra.items() if k not in ('id', 'byte', 'expression')},
                {k: v for k, v in queries[6].items() if k not in ('id', 'byte', 'expression')},
                'new non-null query shares exact owner, target and independent baseline')
    second.same((extra['byte'], extra['expression']), (1653, 'add(propertyName, value)'), 'fixed non-null query')
    data = fixture.sources['gson/src/test/java/com/google/gson/JsonObjectTest.java']
    declaration = b'JsonPrimitive value = new JsonPrimitive("blah");'
    second.same(data[1592:1592 + len(declaration)], declaration, 'independent non-null actual declaration')
    for q in queries:
        data = fixture.sources[q['file']]; expression = q['expression'].encode()
        second.same(data[q['byte']:q['byte'] + len(expression)], expression, 'exact binary-argument expression')
        for stage in ('baseline', 'expected'):
            for target in q[stage]['targets']:
                data = fixture.sources[target['path']]
                second.same(data[target['start']:target['end']], target['name'].encode(), 'exact declaration token')
    for item in plan['source_spellings']:
        data = fixture.sources[item['path']]; spelling = item['spelling'].encode()
        second.same(data[item['byte']:item['byte'] + len(spelling)], spelling, 'independent declaration spelling')
    return copy.deepcopy(queries)
