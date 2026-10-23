"""Offline contract checks against the installed upstream, without models or setup jobs."""
import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'Strata'))


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class UpstreamCompatibility(unittest.TestCase):
    def test_mac_setup_local_only_contract(self):
        metal = load('studio_upstream_contract', ROOT / 'Strata/metal/setup_mac.py')
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


if __name__ == '__main__':
    unittest.main()
