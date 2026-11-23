"""Offline contract checks against the installed upstream, without models or setup jobs."""
import importlib.util
import os
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import DEFAULT, patch

ROOT = Path(__file__).resolve().parents[1]
UPSTREAM = Path(os.environ.get('SOLITON_TEST_STRATA_ROOT', ROOT / 'Strata')).resolve()
sys.path.insert(0, str(UPSTREAM))


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class UpstreamCompatibility(unittest.TestCase):
    def test_split_mac_dispatch_uses_the_model_specific_setup(self):
        terminal = UPSTREAM / 'terminal'
        if not (terminal / 'setup.py').is_file():
            self.skipTest('split Terminal dispatcher is absent')
        with patch.object(sys, 'path', [str(terminal), *sys.path]):
            dispatcher = load('studio_strata_dispatch', terminal / 'setup.py')
            with patch.object(sys, 'platform', 'darwin'), patch.multiple(
                dispatcher, mac_setup=DEFAULT,
                mac_control_setup=DEFAULT,
                mac_inkling_setup=DEFAULT,
                mac_hy4_setup=DEFAULT,
            ) as targets:
                for arguments, control, expected in [
                    (['--model', 'Q2_0'], False, 'mac_setup'),
                    (['--model', 'Q2_0'], True, 'mac_control_setup'),
                    (['--family', 'inkling-2-mini'], False, 'mac_inkling_setup'),
                    (['--family', 'inkling-2-mini', '--model', 'SQ-20'], False, 'mac_inkling_setup'),
                    (['--family', 'hy4-flash'], False, 'mac_hy4_setup'),
                ]:
                    with self.subTest(arguments=arguments, control=control):
                        for target in targets.values():
                            target.reset_mock()
                        with patch.object(dispatcher, 'CONTROL', control), patch.object(
                            sys, 'argv', ['setup.py', *arguments]
                        ):
                            self.assertIs(dispatcher.main(), targets[expected].return_value)
                        targets[expected].assert_called_once()
                        self.assertEqual(sum(target.call_count for target in targets.values()), 1)

    def test_split_compiler_requirement_matches_distribution(self):
        setup = UPSTREAM / 'engine/metal/setup_mac.py'
        if not setup.is_file():
            self.skipTest('split Metal setup is absent')
        metal = load('studio_strata_compiler_contract', setup)
        host = dict(chip='Apple M4', macos='15.0', arm64=True, clt='')
        warning = 'xcode-select --install'
        self.assertIn(warning, ' '.join(metal._fit.mac_problems(host)))
        self.assertEqual(metal.mac_problems({**host, 'clt': '/installed'}), [])
        if (UPSTREAM / '.bundled-metal.json').is_file():
            self.assertEqual(metal.mac_problems(host), [])
        else:
            self.assertIn(warning, ' '.join(metal.mac_problems(host)))
        self.assertIn('macOS 15', ' '.join(metal.mac_problems({**host, 'macos': '14.6'})))

    def test_mac_setup_local_only_contract(self):
        setup = UPSTREAM / 'engine/metal/setup_mac.py'
        if not setup.is_file():
            setup = UPSTREAM / 'metal/setup_mac.py'
        metal = load('studio_upstream_contract', setup)
        wrapper = load('studio_local_wrapper', ROOT / 'crates/studio-core/src/local_setup.py')
        with tempfile.TemporaryDirectory(prefix='strata-upstream-contract-') as folder:
            base = Path(folder)
            stage, data = base / 'links', base / 'owned'
            stage.mkdir()
            weight = stage / metal.model_shard_names('Q2_0')[0]
            weight.touch()
            wrapper.configure(metal, stage, data)
            self.assertTrue(metal.S.done(weight))
            self.assertEqual(metal.S.data_folder(None, True, True, True), (data, []))
            self.assertEqual(metal.S.data_folder_for(SimpleNamespace(data_dir='/unused')), (data, []))
            for ram, stream in [(32, True), (128, True), (128, False)]:
                args = metal.engine_args(base / 'pack', [weight, stage / 'second.gguf'], base / 'mtp/rt', 8192, stream, ram)
                self.assertNotIn('--mtp', args)
                self.assertEqual(args[args.index('--max-context') + 1], '8192')
            with self.assertRaisesRegex(RuntimeError, 'forbids'):
                metal.S.download('https://invalid.example/model.gguf', stage / 'missing.gguf')
            self.assertEqual(sorted(p.name for p in stage.iterdir()), [weight.name])


def run_split_mac_suite():
    """Run the upstream cases with two stale assertions replaced by contracts."""
    upstream = load('studio_strata_mac_suite', UPSTREAM / 'terminal/tools/test_setup_mac.py')
    replacements = {
        'ControlMode.test_other_platforms_and_the_terminal_still_use_their_own_setup':
            'test_split_mac_dispatch_uses_the_model_specific_setup',
        'MacCheck.test_intel_old_macos_and_missing_tools_are_named':
            'test_split_compiler_requirement_matches_distribution',
    }
    suite = unittest.TestSuite()
    replaced = set()
    for group in unittest.defaultTestLoader.loadTestsFromModule(upstream):
        for case in group:
            name = case.id().split('.', 1)[1]
            if name in replacements:
                replacement = replacements[name]
                suite.addTest(UpstreamCompatibility(replacement))
                replaced.add(name)
            else:
                suite.addTest(case)
    if replaced != replacements.keys():
        raise RuntimeError('The upstream Mac suite changed; review the test replacements')
    return unittest.TextTestRunner().run(suite).wasSuccessful()


if __name__ == '__main__':
    if sys.argv[1:] == ['--split-mac-suite']:
        sys.exit(0 if run_split_mac_suite() else 1)
    unittest.main()
