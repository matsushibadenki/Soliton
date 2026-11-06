"""Assessment regressions for extended real conversation fixtures; no accelerator required."""
import importlib.util
from pathlib import Path
import unittest
import subprocess
import sys
import tempfile

spec=importlib.util.spec_from_file_location('multiturn',Path(__file__).with_name('validate-prefix-multiturn.py'))
module=importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ExtendedFormats(unittest.TestCase):
    def test_invalid_near_limit_configuration_does_not_create_output(self):
        with tempfile.TemporaryDirectory() as folder:
            output=Path(folder)/'not-created'
            result=subprocess.run([sys.executable,str(Path(module.__file__)),
                '--python',sys.executable,'--model',folder,'--store',folder,
                '--output-dir',str(output),'--near-limit','--auto-eviction','--quota-mib','128'],
                capture_output=True,text=True)
            self.assertEqual(result.returncode,2)
            self.assertIn('requires --auto-eviction --quota-mib 256',result.stderr)
            self.assertFalse(output.exists())

    def test_extended_turns_preserve_language_specific_fact_and_combined_formats(self):
        rows=[dict(label='en-7',answer='Maple'),dict(label='en-8',answer='14:00'),
              dict(label='en-9',answer='Maple | 14:00'),dict(label='ja-9',answer='牛乳: 1 / パン: 3'),
              dict(label='zh-9',answer='大阪 | 09:30')]
        self.assertTrue(all(row['passed'] for row in module.format_checks(rows)))

    def test_newlines_and_spaces_inside_factual_values_are_not_normalized_away(self):
        rows=[dict(label='en-9',answer='Maple |\n14:00'),dict(label='en-7',answer='Ma ple'),
              dict(label='en-8',answer='14 : 0 0')]
        self.assertFalse(any(row['passed'] for row in module.format_checks(rows)))

    def test_format_failures_do_not_accept_extra_prose_or_wrong_fact(self):
        rows=[dict(label='en-7',answer='The room is Maple.'),dict(label='ja-7',answer='11'),
              dict(label='zh-8',answer='09:30。')]
        self.assertFalse(any(row['passed'] for row in module.format_checks(rows)))


if __name__=='__main__':unittest.main()
