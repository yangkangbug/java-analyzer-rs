"""Current exact parser output. Historical contracts remain immutable.

Update this entry, not every acceptance caller, for future grammar additions.
The previous labeled-stage contract is preserved and remains explicitly callable.
"""
import copy
import hashlib
import importlib.util
from pathlib import Path

spec = importlib.util.spec_from_file_location('current_previous_labels', Path(__file__).with_name('labeled-statement-parser-contract.py'))
previous = importlib.util.module_from_spec(spec)
spec.loader.exec_module(previous)
canonical, require = previous.canonical, previous.require

# Fixed original diagnostic positions in checksum-pinned source files:
# (unexpected synchronized keyword start, following expected-semicolon point).
CHANGES = {
    'commons': [
        ('org/apache/commons/lang3/CharSet.java', ((7626, 7645),)),
        ('org/apache/commons/lang3/concurrent/LazyInitializer.java', ((5639, 5659),)),
        ('org/apache/commons/lang3/concurrent/MultiBackgroundInitializer.java', ((12111, 12131), (14828, 14848))),
        ('org/apache/commons/lang3/time/FastDateParser.java', ((25599, 25621),)),
    ],
    'gson': [
        ('gson/src/main/java/com/google/gson/JsonStreamParser.java', ((3444, 3464),)),
        ('gson/src/main/java/com/google/gson/internal/bind/DateTypeAdapter.java', ((3024, 3051), (3667, 3694))),
        ('gson/src/main/java/com/google/gson/internal/bind/DefaultDateTypeAdapter.java', ((4789, 4816), (5254, 5281))),
        ('gson/src/main/java/com/google/gson/internal/sql/SqlDateTypeAdapter.java', ((2180, 2200), (2642, 2662))),
        ('gson/src/main/java/com/google/gson/internal/sql/SqlTimeTypeAdapter.java', ((2141, 2161), (2582, 2602))),
    ],
}

def prior_reports(offsets):
    return [{'classification': 'parser_limit_or_invalid_syntax', 'message': message,
             'range': {'start_byte': at, 'end_byte': at + width, 'start_utf16': at, 'end_utf16': at + width}}
            for keyword, end in offsets
            for message, at, width in [('unexpected token', keyword, 12), ("expected ';'", end, 0)]]

def load(corpus, stage='latest'):
    require(stage == 'latest', 'unknown current parser stage')
    require(corpus in CHANGES, 'unknown current corpus')
    before, hashes = previous.load(corpus)
    expected = copy.deepcopy(before)
    for path, offsets in CHANGES[corpus]:
        rows = [row for row in expected['files'] if row['path'] == path]
        require(len(rows) == 1, 'exact synchronized file identity drift')
        require(rows[0] == {'path': path, 'compilation': {'project': {'gson': 'gson', 'commons': 'commons-lang'}[corpus], 'source_set': 'main'},
                           'reports': prior_reports(offsets)},
                'exact prior synchronized parser records drift')
        rows[0]['reports'] = []
    expected['report_count'] = sum(len(row['reports']) for row in expected['files'])
    expected['files_with_reports'] = sum(bool(row['reports']) for row in expected['files'])
    return expected, hashes

def check(corpus, actual, sources, stage='latest'):
    expected, hashes = load(corpus, stage)
    require(canonical(actual) == canonical(expected), 'complete current parser records differ')
    require(hashes == {name: hashlib.sha256(data).hexdigest() for name, data in sources.items()},
            'complete current source inventory differs')
    return expected
