"""Frozen additive null-actual expectations; no executable or network work."""
import copy
import hashlib
import json

PLAN_NAME = 'null-actual-probes-v2.json'
INITIAL_PLAN_NAME = 'null-actual-probes.json'
INITIAL_PLAN_SHA256 = 'c0dd9c79f5bccba0c42c744b4375d9d013a0df1334548a15eae4617bd6acf443'
PLAN_SHA256 = '33cab5ec727e2a2f934615979f78a30d3ad01dc00e2ee145f77b19244c164bde'


def load_plan(second, path=None):
    path = second.CORPUS / PLAN_NAME if path is None else path
    second.reject_symlinks(path.absolute())
    data = path.read_bytes()
    second.same(hashlib.sha256(data).hexdigest(), PLAN_SHA256, 'frozen complete null actual plan')
    plan = json.loads(data, object_pairs_hook=second.unique_json_object)
    prior_bytes = (second.CORPUS / INITIAL_PLAN_NAME).read_bytes()
    second.same(hashlib.sha256(prior_bytes).hexdigest(), INITIAL_PLAN_SHA256, 'unchanged original failed forecast plan')
    old = json.loads(prior_bytes, object_pairs_hook=second.unique_json_object)
    revision = plan['scope_revision']
    second.same(revision['revision'], 2, 'explicit reviewed second contract')
    second.same((revision['query_id'], revision['file'], revision['byte'], revision['disposition']),
                ('null_add', old['queries'][6]['file'], 2139, 'deferred_refusal'), 'only deferred query identity')
    second.same(revision['prior_expected'], old['queries'][6]['expected'], 'preserve original positive forecast')
    second.same(revision['revised_expected'], old['queries'][6]['baseline'], 'exact deferred refusal, never either/or')
    second.same(hashlib.sha256((second.CORPUS / revision['diagnosis_file']).read_bytes()).hexdigest(),
                revision['diagnosis_sha256'], 'pinned failure-isolation evidence')
    expected = copy.deepcopy(old)
    expected['selected_before_implementation'] = False
    expected['supersedes'] = {'file': INITIAL_PLAN_NAME, 'sha256': INITIAL_PLAN_SHA256}
    expected['scope_revision'] = revision
    expected['historical_artifact_sha256'][INITIAL_PLAN_NAME] = INITIAL_PLAN_SHA256
    expected['historical_artifact_sha256'][revision['diagnosis_file']] = revision['diagnosis_sha256']
    expected['queries'][6]['expected'] = copy.deepcopy(old['queries'][6]['baseline'])
    second.same(plan, expected, 'only explicit reviewed revision differs; all eight records otherwise frozen')
    return plan


def overlay(second, fixture, historical_queries):
    plan = load_plan(second)
    second.same(plan['source_archive_sha256'], fixture.metadata['inputs']['sources']['sha256'],
                'null actual source archive')
    for name, checksum in plan['historical_artifact_sha256'].items():
        second.same(second.digest(second.CORPUS / name), checksum, 'preserved null actual artifact ' + name)
    for name, checksum in plan['source_sha256'].items():
        data = fixture.sources[name]
        second.same(hashlib.sha256(data).hexdigest(), checksum, 'null actual source ' + name)
        second.reject_symlinks(fixture.source_root / name)
        second.same((fixture.source_root / name).read_bytes(), data, 'unchanged null actual source bytes')
    queries = plan['queries']
    second.same(len(queries), 8, 'exact null-stage eight query inventory')
    second.same(len(historical_queries), 5, 'exact previous return-stage five queries')
    for q, old in zip(queries[:5], historical_queries):
        metadata = lambda row: {k: v for k, v in row.items() if k not in ('id', 'baseline', 'expected')}
        second.same(metadata(q), metadata(old), 'unchanged prior query metadata')
        second.same(q['baseline'], old['expected'], 'exact prior return-stage result')
        second.same(q['expected'], old['expected'], 'prior five queries remain unchanged')
    for q, wanted in zip(queries[5:], [
            ('JsonArrayTest.java', 2527, 'set(0, null)', 'JsonArray.java', 'JsonArray', 'set', 4463, 4466, True),
            ('JsonObjectTest.java', 2139, 'add(propertyName, null)', 'JsonObject.java', 'JsonObject', 'add', 2345, 2348, False),
            ('JsonObjectTest.java', 2489, 'add(null, JsonNull.INSTANCE)', 'JsonObject.java', 'JsonObject', 'add', 2345, 2348, False)]):
        file, at, expr, target_file, owner, name, start, end, selected = wanted
        second.same((q['file'], q['byte'], q['expression'], q['spelling'], q['compilation']),
                    ('gson/src/test/java/com/google/gson/' + file, at, expr, name, {'project': 'gson', 'source_set': 'test'}),
                    'complete independently chosen null query metadata')
        target = {'origin': 'source', 'path': 'gson/src/main/java/com/google/gson/' + target_file,
                  'kind': 'Method', 'owner': 'com.google.gson.' + owner, 'name': name, 'start': start, 'end': end}
        second.same(q['baseline'], {'status': 'CallNotResolved', 'targets': [target]}, 'exact null baseline refusal')
        second.same(q['expected'], {'status': 'Resolved' if selected else 'CallNotResolved', 'targets': [target]},
                    'exact one selection, deferred String-local refusal and unchanged field refusal')
    for q in queries:
        data = fixture.sources[q['file']]; expression = q['expression'].encode()
        second.same(data[q['byte']:q['byte'] + len(expression)], expression, 'exact null source expression')
        for stage in ('baseline', 'expected'):
            for target in q[stage]['targets']:
                data = fixture.sources[target['path']]
                second.same(data[target['start']:target['end']], target['name'].encode(), 'exact declaration token')
    for item in plan['source_spellings']:
        data = fixture.sources[item['path']]; spelling = item['spelling'].encode()
        second.same(data[item['byte']:item['byte'] + len(spelling)], spelling, 'independent declaration spelling')
    return copy.deepcopy(queries)
