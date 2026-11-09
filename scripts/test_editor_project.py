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


if __name__ == '__main__': unittest.main()
