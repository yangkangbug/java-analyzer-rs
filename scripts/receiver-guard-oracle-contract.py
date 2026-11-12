"""Exact additive refusal-state updates; historical source/goldens stay frozen."""
import hashlib
import json
import re

PLAN_SHA256 = '67f7255a63062c94f08e3fb37332000bd5dac65bd81c2fd225b6a84d2938b10f'


def apply(oracle, suite, expected, queries, declarations):
    path = oracle / 'reference-return-v1/receiver-guard-updates.json'
    data = path.read_bytes()
    assert hashlib.sha256(data).hexdigest() == PLAN_SHA256, 'frozen receiver guard update plan'
    plan = json.loads(data)
    assert set(plan['updates']) == {'primitive-v1', 'reference-v1'}
    assert suite in plan['updates'], 'unknown receiver guard suite'
    for name, checksum in plan['artifacts'].items():
        assert hashlib.sha256((oracle / name).read_bytes()).hexdigest() == checksum, name + ': historical bytes changed'
    # Reconstruct every marker from pinned bytes, so unrelated metadata cannot
    # be altered while retaining the one corrected key.
    frozen_queries, frozen_declarations = {}, {}
    marker = re.compile(rb"/\*([DQ]):([a-z_]+)\*/([A-Za-z_$][A-Za-z0-9_$]*)")
    prefix = suite + '/source/'
    for name in plan['artifacts']:
        if not name.startswith(prefix):
            continue
        for match in marker.finditer((oracle / name).read_bytes()):
            kind, identity, token = (p.decode() for p in match.groups())
            dest = frozen_queries if kind == 'Q' else frozen_declarations
            assert identity not in dest
            dest[identity] = (name[len(prefix):], match.start(3), match.end(3), token)
    assert queries == frozen_queries, 'complete frozen query metadata inventory'
    assert declarations == frozen_declarations, 'complete frozen declaration metadata inventory'
    update = plan['updates'][suite]
    q = update['query']
    assert list(queries[q]) == update['query_record'], 'complete frozen query metadata'
    assert list(expected[q]) == update['before'], 'exact old native/compiler tuple'
    marker = update['before'][1].split('@')[0]
    assert list(declarations[marker]) == update['selected_declaration'], 'unchanged independent compiler declaration'
    assert update['before'] == ['Unresolved', update['after'][1], '-']
    assert update['after'] == ['ReceiverNotBound', update['before'][1], '-']
    # Assert the complete existing golden query inventory, not just a matching key.
    historical = {}
    for line in (oracle / suite / 'expected.tsv').read_text().splitlines():
        if line and not line.startswith('#'):
            key, status, target, candidates = line.split('\t')
            assert key not in historical
            historical[key] = (status, target, candidates)
    # Primitive scalar-return corrections are an earlier explicit stage. Validate
    # their frozen plan as well rather than permitting arbitrary other updates.
    if suite == 'primitive-v1':
        for line in (oracle / 'return-v1/primitive-updates.tsv').read_text().splitlines():
            if line and not line.startswith('#'):
                key, before, after, target, candidates = line.split('\t')
                assert historical[key][0] == before and historical[key][1] == target
                historical[key] = (after, target, candidates)
    assert expected == historical, 'complete prior-stage result inventory'
    assert set(queries) == set(expected), 'complete query inventory'
    result = dict(expected)
    result[q] = tuple(update['after'])
    return result
