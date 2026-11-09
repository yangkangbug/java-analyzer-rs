#!/usr/bin/env python3
"""Offline fixed Gson unsaved-buffer acceptance. No Java, builds or downloads."""
import argparse
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import tempfile

ROOT = Path(__file__).resolve().parent.parent
PREFIX = '// Unsaved editor preview: 😀\r\n'.encode()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def position(data, byte):
    """Independent strict UTF-16 line calculation from original UTF-8 bytes."""
    require(0 <= byte <= len(data), 'byte outside source')
    require(not (byte and data[byte-1:byte+1] == b'\r\n'), 'inside CRLF')
    text = data[:byte].decode('utf-8')
    parts = re.split(r'\r\n|\r|\n', text)
    return {'encoding':'utf16_line', 'line':len(parts)-1,
            'character':len(parts[-1].encode('utf-16-le'))//2}


def shift_target(target, original, prefix):
    shifted = copy.deepcopy(target)
    start, end = target['range']['start_byte'], target['range']['end_byte']
    require(0 <= start <= end <= len(original), 'invalid original target')
    shifted['range'] = {'start_byte':start+len(prefix), 'end_byte':end+len(prefix),
                        'start_utf16':len((prefix+original[:start]).decode().encode('utf-16-le'))//2,
                        'end_utf16':len((prefix+original[:end]).decode().encode('utf-16-le'))//2}
    return shifted


def check_target(row, expected):
    require(row['status'] == 'Resolved', 'source declaration no longer resolves')
    require(row['targets'] == [expected], 'wrong exact source declaration/range')


def check_parser_overlay(baseline, actual, prefixed_sources):
    """A leading comment preserves every diagnostic except its shifted range.

    prefixed_sources maps absolute paths to (original UTF-8 bytes, prefix bytes).
    Both reports must also pass the ordinary parser schema/inventory validation.
    Overlay revision/versions are checked independently by the caller.
    """
    require('overlay_state' not in baseline, 'disk parser report has overlay state')
    expected = copy.deepcopy(baseline)
    for file in expected['files']:
        if file['path'] in prefixed_sources:
            original, prefix = prefixed_sources[file['path']]
            file['reports'] = [shift_target(report, original, prefix) for report in file['reports']]
    observed = copy.deepcopy(actual)
    observed.pop('overlay_state', None)
    require(observed == expected, 'comment-only overlay changed exact parser reports/inventory/counts')


def helpers():
    spec = importlib.util.spec_from_file_location('editor_second_project', ROOT/'scripts/check-second-project.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(native, inputs):
    helper = helpers()
    with tempfile.TemporaryDirectory(prefix='java-editor-acceptance-') as scratch:
        fixture = helper.prepare_fixture(inputs, Path(scratch)/'fixture')
        all_sources = {path: digest(path) for path in fixture.source_root.rglob('*.java')}
        input_hashes = {key:digest(path) for key,path in fixture.inputs.items()}
        base = helper.successful(native, fixture, 'declaration')
        helper.check_response('declaration', base, fixture)
        require('overlay_state' not in base['result'], 'legacy result gained overlay metadata')
        disk_parser = helper.successful(native, fixture, 'parser-report')['result']
        helper.check_parser(disk_parser, fixture)
        plan = fixture.plan[0]
        require(plan['id'] == 'cross_interface', 'predeclared query changed')
        target = fixture.expected[0]['targets'][0]
        query_path, target_path = plan['file'], target['path']
        require(fixture.sources[target_path][target['start']:target['end']] == b'TypeAdapterFactory',
                'independently reviewed original declaration changed')
        replacements = {path:PREFIX+fixture.sources[path] for path in (query_path,target_path)}
        overlay_data = {'schema_version':1,'revision':41,'files':[
            {'compilation':helper.compilation(fixture.owner[path]), 'file':'sources/'+path,
             'version':version,'text':replacements[path].decode()}
            for path,version in ((query_path,7),(target_path,12))]}
        overlays = helper.document(fixture, 'overlays.json', overlay_data)
        query_position = position(replacements[query_path], plan['byte']+len(PREFIX))
        requests = helper.document(fixture, 'editor-requests.json', {'schema_version':1,'queries':[
            {'compilation':plan['compilation'],'file':'sources/'+query_path,'position':query_position}]})
        expected = shift_target(helper.expected_target(target, fixture), fixture.sources[target_path], PREFIX)
        expected_state = {'revision':41,'files':[
            {'compilation':item['compilation'], 'file':str(fixture.workdir/item['file']),
             'version':item['version']} for item in overlay_data['files']]}

        def invoke(command, overlay_path=overlays, successful=True):
            args = [str(native),command,'--manifest',str(fixture.manifest),'--overlays',str(overlay_path)]
            if command == 'declaration': args += ['--requests',str(requests)]
            code,stdout,stderr = helper.bounded_helper().run_bounded(args, fixture.workdir.parent, 60, 16*1024*1024)
            require(code == (0 if successful else 2), f'{command} exit {code}: {stderr[:2000]!r}')
            require(not (stderr if successful else stdout), 'wrong output stream')
            response = json.loads(stdout if successful else stderr)
            if successful:
                helper.check_envelope(command, response, fixture)
                require(response['result']['overlay_state'] == expected_state, 'wrong revision/owner/version inventory')
            return response

        declaration = invoke('declaration')
        result = declaration['result']
        require(result['index_builds'] == 1 and len(result['queries']) == 1, 'batch index/result count')
        row = result['queries'][0]
        require(row['query_index'] == 0 and row['position'] == query_position, 'query identity/position')
        require(row['compilation'] == plan['compilation'] and
                row['source_compilation'] == helper.compilation(fixture.owner[query_path]), 'query ownership')
        require(row['file'] == str(fixture.source_root/query_path) and
                row['byte_offset'] == plan['byte']+len(PREFIX), 'wrong buffer coordinate')
        check_target(row, expected)
        mutated = copy.deepcopy(row); mutated['targets'][0]['range']['start_byte'] += 1
        try:
            check_target(mutated, expected)
        except ValueError:
            pass
        else:
            raise ValueError('checker accepted a wrong target mutation')
        changed_fixture = copy.copy(fixture)
        changed_fixture.sources = dict(fixture.sources, **replacements)
        parser = invoke('parser-report')['result']
        helper.check_parser(parser, changed_fixture)
        check_parser_overlay(disk_parser, parser, {
            str(fixture.source_root/path): (fixture.sources[path], PREFIX) for path in replacements})
        wrong_owner = copy.deepcopy(overlay_data)
        wrong_owner['files'][1]['compilation'] = plan['compilation']
        error = invoke('declaration', helper.document(fixture,'wrong-owner.json',wrong_owner), False)
        require(error['error']['code'] == 'invalid_overlay', 'wrong-owner rejection lost stable code')
        restored = helper.successful(native,fixture,'declaration')
        helper.check_response('declaration',restored,fixture)
        require('overlay_state' not in restored['result'], 'omitted overlay was retained')
        require({path:digest(path) for path in fixture.source_root.rglob('*.java')} == all_sources,
                'disk Java path inventory or source bytes changed')
        require({key:digest(path) for key,path in fixture.inputs.items()} == input_hashes, 'pinned input changed')
        return {'status':'passed', 'project_commit':fixture.metadata['commit'],
                'source_count':211,'baseline_queries':16,'editor_query':'cross_interface',
                'overlay_files':2,'prefix_utf8_bytes':len(PREFIX),'prefix_utf16_units':len(PREFIX.decode().encode('utf-16-le'))//2,
                'parser_reports':parser['report_count'],
                'files_with_parser_reports':parser['files_with_reports'],
                'controls':['exact_target_mutation','wrong_source_owner','omit_overlays_restores_disk',
                            'all_disk_java_hashes_unchanged','all_pinned_input_hashes_unchanged',
                            'comment_overlay_preserves_exact_current_disk_parser_reports'],
                'runtime':'native_rust_no_jvm','upstream_code_executed':False,
                'limits':'Fixed buffer/position evidence, not LSP, automatic stale rejection or semantic conformance'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('inputs_dir',type=Path)
    parser.add_argument('--native',type=Path,default=ROOT/'target/release/java-analyzer')
    args=parser.parse_args()
    native=args.native.resolve(strict=True)
    print(json.dumps(run(native,args.inputs_dir.resolve(strict=True)),indent=2))


if __name__ == '__main__': main()
