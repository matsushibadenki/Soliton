import importlib.util
from pathlib import Path
import tempfile
import unittest

spec=importlib.util.spec_from_file_location('check_distribution',Path(__file__).with_name('check-distribution.py'))
c=importlib.util.module_from_spec(spec);spec.loader.exec_module(c)

class DistributionChecks(unittest.TestCase):
    def test_compiler_and_linker_versions_are_not_macos_versions(self):
        for tool in ['LD','4','CLANG']:
            text=f'cmd LC_BUILD_VERSION\n minos 11.0\n sdk 27.0\n ntools 1\n tool {tool}\n version 23.1.3\n'
            self.assertEqual(c.load_versions(text),['11.0'])
    def test_old_and_multi_arch_deployment_commands(self):
        text='cmd LC_VERSION_MIN_MACOSX\n version 10.15\n sdk 14.0\ncmd LC_BUILD_VERSION\n minos 15.0\n tool LD\n version 27037.1'
        self.assertEqual(c.load_versions(text),['10.15','15.0'])
        self.assertEqual(c.load_versions('unrecognized output'),[])
    def test_version_comparison_is_numeric(self):
        self.assertGreater(c.version('15.10'),c.version('15.2'))
        self.assertEqual(c.version('15'),c.version('15.0.0'))
    def test_system_dependencies_are_allowed_and_homebrew_is_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d); binary=root/'bin/app'
            self.assertEqual(c.dependency_errors(['/usr/lib/libSystem.B.dylib','/System/Library/Frameworks/Metal.framework/Metal'],binary,root),[])
            self.assertTrue(c.dependency_errors(['/opt/homebrew/lib/libcrypto.dylib'],binary,root))
    def test_relocated_libraries_must_exist_within_bundle(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);(root/'bin').mkdir();(root/'lib').mkdir();(root/'lib/a.dylib').touch();binary=root/'bin/app'
            self.assertEqual(c.dependency_errors(['@loader_path/../lib/a.dylib'],binary,root),[])
            self.assertEqual(c.dependency_errors(['@rpath/a.dylib'],binary,root,['@loader_path/../lib']),[])
            self.assertTrue(c.dependency_errors(['@rpath/missing.dylib'],binary,root,['@loader_path/../lib']))
    def test_loader_symlinks_cannot_escape_the_distribution(self):
        with tempfile.TemporaryDirectory() as d:
            base=Path(d);root=base/'bundle';root.mkdir();outside=base/'private.dylib';outside.touch();(root/'escape.dylib').symlink_to(outside)
            self.assertTrue(c.dependency_errors(['@loader_path/escape.dylib'],root/'app',root))

if __name__=='__main__': unittest.main()
