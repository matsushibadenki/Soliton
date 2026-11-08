"""CPU-only compatibility boundaries; these fixtures never issue startup evidence."""
import copy
from pathlib import Path
import runpy
import sys
import tempfile
from types import ModuleType, SimpleNamespace
import unittest
from unittest.mock import patch

wrapper = runpy.run_path(str(Path(__file__).with_name('mlx-retained-kv.py')))


class ReviewedRelease(unittest.TestCase):
    def test_release_and_exact_source_pair_are_bound_together(self):
        pairs = wrapper['REVIEWED_SOURCE_PAIRS']
        self.assertEqual({key: len(value) for key, value in pairs.items()}, {'0.32.0': 2, '0.32.2': 1})
        all_pairs = set().union(*pairs.values())
        for release in ['0.32.0', '0.32.2', '0.32.1', '0.32.3', '0.32.2.dev0']:
            for server, cache in all_pairs:
                self.assertEqual(wrapper['reviewed_sources'](release, server, cache),
                                 (server, cache) in pairs.get(release, ()))
        for release, allowed in pairs.items():
            for server, cache in allowed:
                self.assertFalse(wrapper['reviewed_sources'](release, '0' * 64, cache))
                self.assertFalse(wrapper['reviewed_sources'](release, server, '0' * 64))
                self.assertFalse(wrapper['reviewed_sources'](release, server[:-1], cache))

    def test_plain_text_does_not_mutate_input_or_invoke_network_path(self):
        calls = []
        server = SimpleNamespace(_inline_image_urls=lambda messages: calls.append(messages))
        wrapper['install_text_profile'](server, '0.32.2')
        messages = [{'role': 'user', 'content': '画像ではない plain text 中文'},
                    {'role': 'assistant', 'content': ''}, {'role': 'user', 'content': 'next'}]
        before = copy.deepcopy(messages)
        self.assertIsNone(server._inline_image_urls(messages))
        self.assertEqual(messages, before)
        self.assertEqual(calls, [])

    def test_new_vision_or_tool_surface_is_rejected_before_url_inlining(self):
        calls = []
        server = SimpleNamespace(_inline_image_urls=lambda messages: calls.append(messages))
        wrapper['install_text_profile'](server, '0.32.2')
        for messages in [None, {}, [], [{'role': 'tool', 'content': 'x'}],
                         [{'role': 'user', 'content': None}],
                         [{'role': 'user', 'content': [{'type': 'text', 'text': 'x'}]}],
                         [{'role': 'user', 'content': [{'type': 'image_url', 'image_url': {'url': 'http://127.0.0.1:1/private'}}]}],
                         [{'role': 'user', 'content': [{'type': 'image_url', 'image_url': {'url': 'data:image/png;base64,AAAA'}}]}],
                         [{'role': 'user', 'content': 'x', 'images': ['x']}],
                         [{'role': 'assistant', 'content': '', 'tool_calls': []}],
                         [{'role': 'user', 'content': 'x'}] * 1001]:
            with self.subTest(messages=str(messages)[:100]), self.assertRaises(ValueError):
                server._inline_image_urls(messages)
        self.assertEqual(calls, [])

    def test_old_release_hook_is_unchanged(self):
        original = lambda messages: None
        server = SimpleNamespace(_inline_image_urls=original)
        wrapper['install_text_profile'](server, '0.32.0')
        self.assertIs(server._inline_image_urls, original)

    def test_changed_source_is_refused_before_installing_any_adapter(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / 'changed.py'
            source.write_text('# changed source, not a reviewed wheel')
            server = SimpleNamespace(__file__=str(source))
            cache = SimpleNamespace(__file__=str(source))
            package = ModuleType('mlx_lm')
            package.server = server
            models = ModuleType('mlx_lm.models')
            models.cache = cache
            with patch.dict(sys.modules, {'mlx_lm': package, 'mlx_lm.models': models}), \
                    patch('importlib.metadata.version', return_value='0.32.2'), \
                    patch.dict('os.environ', {'VLLM_APPLE_KV_CACHE_MIB': '1536'}):
                with self.assertRaisesRegex(ValueError, 'source changes'):
                    wrapper['main']()
            self.assertEqual(vars(server), {'__file__': str(source)})
            self.assertEqual(vars(cache), {'__file__': str(source)})


if __name__ == '__main__':
    unittest.main()
