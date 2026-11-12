"""Pinned null selection overlays for five older current oracle suites."""
import hashlib
import json
import re

PLAN_SHA256 = 'e36c67d997df2a5a4d0258f559b2fc98bfbb9d47ed439b47dea402c0294d6e20'


def apply(oracle, suite, expected, queries, declarations):
    data = (oracle / 'null-actual-v1/prior-null-updates.json').read_bytes()
    assert hashlib.sha256(data).hexdigest() == PLAN_SHA256
    plan = json.loads(data)
    assert suite in plan['updates']
    for name, checksum in plan['artifacts'].items():
        assert hashlib.sha256((oracle / name).read_bytes()).hexdigest() == checksum, name + ': prior artifact changed'
    frozen_queries, frozen_declarations = {}, {}
    marker = re.compile(rb'/\*([DQ]):([a-z_]+)\*/([A-Za-z_$][A-Za-z0-9_$]*)')
    prefix = suite + '/source/'
    for name in plan['artifacts']:
        if name.startswith(prefix):
            for m in marker.finditer((oracle / name).read_bytes()):
                kind, key, token = (p.decode() for p in m.groups())
                dest = frozen_queries if kind == 'Q' else frozen_declarations
                assert key not in dest
                dest[key] = (name[len(prefix):], m.start(3), m.end(3), token)
    assert queries == frozen_queries and declarations == frozen_declarations, 'complete frozen metadata'
    prior = {}
    for line in (oracle / suite / 'expected.tsv').read_text().splitlines():
        if line and not line.startswith('#'):
            q, status, target, candidates = line.split('\t')
            assert q not in prior
            prior[q] = status, target, candidates
    if suite == 'reference-v1':
        update = json.loads((oracle / 'reference-return-v1/receiver-guard-updates.json').read_bytes())['updates'][suite]
        assert list(prior[update['query']]) == update['before']
        prior[update['query']] = tuple(update['after'])
    assert expected == prior, 'complete previous current-stage results'
    update = plan['updates'][suite]; q = update['query']
    assert list(expected[q]) == update['before']
    assert update['after'] == ['Resolved', expected[q][1], expected[q][1]]
    result = dict(expected); result[q] = tuple(update['after'])
    return result
