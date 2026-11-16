"""Exact additive label-syntax stage over the frozen traditional-switch output."""
import copy
import hashlib
import importlib.util
from pathlib import Path

spec = importlib.util.spec_from_file_location('label_previous_switch', Path(__file__).with_name('traditional-switch-parser-contract.py'))
switch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(switch)
canonical, require = switch.canonical, switch.require

# Exact frozen file identity and complete prior diagnostics, not message filtering.
CHANGES = {
    'commons': [('org/apache/commons/lang3/StringUtils.java', {'project': 'commons-lang', 'source_set': 'main'}, (125269,)),
                ('org/apache/commons/lang3/reflect/MethodUtils.java', {'project': 'commons-lang', 'source_set': 'main'}, (24832,))],
    'gson': [('gson/src/main/java/com/google/gson/stream/JsonReader.java', {'project': 'gson', 'source_set': 'main'}, (22453, 34910, 47629))],
}

def prior_reports(offsets):
    return [{'classification': 'parser_limit_or_invalid_syntax', 'message': message,
             'range': {'start_byte': at, 'end_byte': at + width, 'start_utf16': at, 'end_utf16': at + width}}
            for at in offsets for message, width in [("expected ';'", 0), ('unexpected token', 1)]]

def load(corpus, stage='labels'):
    require(stage == 'labels', 'unknown labeled parser stage')
    require(corpus in CHANGES, 'unknown labeled corpus')
    before, hashes = switch.load(corpus)
    expected = copy.deepcopy(before)
    for path, compilation, offsets in CHANGES[corpus]:
        rows = [row for row in expected['files'] if row['path'] == path]
        require(len(rows) == 1, 'exact labeled file identity drift')
        require(rows[0] == {'path': path, 'compilation': compilation, 'reports': prior_reports(offsets)},
                'exact prior labeled parser records drift')
        rows[0]['reports'] = []
    expected['report_count'] = sum(len(row['reports']) for row in expected['files'])
    expected['files_with_reports'] = sum(bool(row['reports']) for row in expected['files'])
    return expected, hashes

def check(corpus, actual, sources, stage='labels'):
    expected, hashes = load(corpus, stage)
    require(canonical(actual) == canonical(expected), 'complete labeled parser records differ')
    require(hashes == {name: hashlib.sha256(data).hexdigest() for name, data in sources.items()},
            'complete labeled source inventory differs')
    return expected
