"""Offline checks of editor acceptance calculations, not a Java semantic oracle."""
import copy
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location('editor_project', Path(__file__).with_name('check-editor-project.py'))
check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check)


class EditorChecks(unittest.TestCase):
    def test_strict_utf16_lines(self):
        text = 'a😀中\r\nx\ry\n'.encode()
        self.assertEqual(check.position(text, len('a😀'.encode())), {'encoding':'utf16_line','line':0,'character':3})
        self.assertEqual(check.position(text, len('a😀中\r\n'.encode())), {'encoding':'utf16_line','line':1,'character':0})
        self.assertEqual(check.position(text, len(text)), {'encoding':'utf16_line','line':3,'character':0})
        for byte in [2, len('a😀中\r'.encode()), -1, len(text)+1]:
            with self.assertRaises(ValueError): check.position(text, byte)

    def test_shift_exact_range_and_utf16_independently(self):
        data = 'class A {}'.encode()
        target = {'origin':'source', 'range':{'start_byte':6,'end_byte':7,'start_utf16':6,'end_utf16':7}}
        prefix = '// 😀\r\n'.encode()
        shifted = check.shift_target(target, data, prefix)
        self.assertEqual(shifted['range']['start_byte'], 6+len(prefix))
        self.assertEqual(shifted['range']['start_utf16'], 6+len('// 😀\r\n'.encode('utf-16-le'))//2)
        self.assertEqual(target['range']['start_byte'], 6)

    def test_wrong_target_or_truncation_is_not_a_pass(self):
        target = {'origin':'source','name':'A','range':{'start_byte':6,'end_byte':7}}
        row = {'status':'Resolved','targets':[target]}
        check.check_target(row, target)
        for wrong in [dict(row, status='Unresolved'), dict(row, targets=[]),
                      dict(row, targets=[dict(target, name='B')]), dict(row, targets=[target,target])]:
            with self.assertRaises(ValueError): check.check_target(wrong, target)
        wrong = copy.deepcopy(row); wrong['targets'][0]['range']['start_byte'] += 1
        with self.assertRaises(ValueError): check.check_target(wrong, target)


class ParserOverlayChecks(unittest.TestCase):
    def setUp(self):
        self.original = '// 🧭\nclass A {}'.encode()
        start = self.original.index(b'A')
        report = {'classification':'parser_limit_or_invalid_syntax', 'message':'unsupported',
                  'range':{'start_byte':start, 'end_byte':start+1,
                           'start_utf16':len(self.original[:start].decode().encode('utf-16-le'))//2,
                           'end_utf16':len(self.original[:start+1].decode().encode('utf-16-le'))//2}}
        self.baseline = {'source_count':2, 'files_with_reports':2, 'report_count':2,
                         'files':[{'path':'/edited.java','reports':[report]},
                                  {'path':'/unchanged.java','reports':[copy.deepcopy(report)]}]}
        self.prefixes = {'/edited.java':(self.original, check.PREFIX)}
        self.actual = copy.deepcopy(self.baseline)
        self.actual['files'][0]['reports'][0] = check.shift_target(report, self.original, check.PREFIX)
        self.actual['overlay_state'] = {'revision':41}

    def check(self, actual):
        check.check_parser_overlay(self.baseline, actual, self.prefixes)

    def test_exact_shifted_and_unchanged_reports(self):
        self.check(self.actual)
        self.assertNotIn('overlay_state', self.baseline)
        self.assertNotEqual(self.actual['files'][0]['reports'], self.baseline['files'][0]['reports'])
        self.assertEqual(self.actual['files'][1], self.baseline['files'][1])

    def test_dropped_extra_changed_report_and_counts(self):
        mutations = [lambda a:a['files'][0]['reports'].clear(),
                     lambda a:a['files'][0]['reports'].append(copy.deepcopy(a['files'][0]['reports'][0])),
                     lambda a:a['files'].pop(),
                     lambda a:a['files'].append(copy.deepcopy(a['files'][0])),
                     lambda a:a.__setitem__('source_count',3),
                     lambda a:a.__setitem__('files_with_reports',1),
                     lambda a:a.__setitem__('report_count',3),
                     lambda a:a['files'][0]['reports'][0].__setitem__('message','changed'),
                     lambda a:a['files'][0]['reports'][0].__setitem__('classification','changed')]
        for mutate in mutations:
            actual = copy.deepcopy(self.actual)
            mutate(actual)
            with self.subTest(mutation=mutate), self.assertRaises(ValueError):
                self.check(actual)

    def test_wrong_byte_utf16_and_unedited_ranges(self):
        for file_index in (0,1):
            for key in ('start_byte','end_byte','start_utf16','end_utf16'):
                actual = copy.deepcopy(self.actual)
                actual['files'][file_index]['reports'][0]['range'][key] += 1
                with self.subTest(file=file_index,key=key), self.assertRaises(ValueError):
                    self.check(actual)

    def test_dropping_report_and_adjusting_counts_still_fails(self):
        actual = copy.deepcopy(self.actual)
        actual['files'][0]['reports'].clear()
        actual['report_count'] -= 1
        actual['files_with_reports'] -= 1
        with self.assertRaises(ValueError):
            self.check(actual)


if __name__ == '__main__': unittest.main()
