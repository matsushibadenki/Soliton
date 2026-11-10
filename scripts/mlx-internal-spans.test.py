import concurrent.futures
import copy
from pathlib import Path
import runpy
from types import SimpleNamespace, ModuleType
from unittest.mock import patch
import tempfile
import unittest

m = runpy.run_path(str(Path(__file__).with_name('mlx-internal-spans.py')))
Ledger = m['SpanLedger']


class Spans(unittest.TestCase):
    def test_unknown_unmeasured_zero_and_failure_preserve_semantics(self):
        clock = iter([0, 0, 1000000, 4000000])
        ledger = Ledger(lambda: next(clock))
        self.assertIsNone(ledger.snapshot()['spans']['cache.fetch']['last_ms'])
        value = object()
        self.assertIs(ledger.wrap('cache.fetch', lambda: value)(), value)
        error = ValueError('private error must not be retained')
        def fail():
            raise error
        with self.assertRaises(ValueError) as raised:
            ledger.wrap('cache.fetch', fail)()
        self.assertIs(raised.exception, error)
        row = ledger.snapshot()['spans']['cache.fetch']
        self.assertEqual(row, dict(calls=2, failures=1, total_ms=3.0, last_ms=3.0, max_ms=3.0))
        self.assertNotIn('private', str(ledger.snapshot()))
        with self.assertRaises(ValueError):
            with ledger.measure('unbounded user controlled name'):
                pass

    def test_decorator_and_concurrent_bounded_scalar_snapshot(self):
        ledger = Ledger()
        @ledger.wrap('qualification.verify')
        def verify(x):
            return x
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            self.assertEqual(list(pool.map(verify, range(100))), list(range(100)))
        snapshot = ledger.snapshot()
        self.assertEqual(snapshot['spans']['qualification.verify']['calls'], 100)
        snapshot['spans']['qualification.verify']['calls'] = -1
        self.assertEqual(ledger.snapshot()['spans']['qualification.verify']['calls'], 100)
        self.assertEqual(len(snapshot['spans']), len(m['NAMES']))

    def test_wrong_release_and_source_fail_before_hooks_or_budget_start(self):
        main = m['main']
        globals_ = main.__globals__
        touched = []
        budget = {'reviewed_sources': lambda *args: False,
                  'main': lambda **kwargs: touched.append('budget')}
        with patch.object(globals_['importlib'].metadata, 'version', return_value='0.32.3'), patch.object(globals_['runpy'], 'run_path', return_value=budget), patch.dict(globals_, install=lambda *args:touched.append('install')):
            with self.assertRaises(ValueError):
                main()
        with tempfile.TemporaryDirectory() as directory:
            source=Path(directory)/'unreviewed.py';source.write_text('# unreviewed')
            server=SimpleNamespace(__file__=str(source))
            cache=SimpleNamespace(__file__=str(source))
            package=ModuleType('mlx_lm');package.server=server
            models=ModuleType('mlx_lm.models');models.cache=cache
            for reviewed_pair in (False, True):
                # Even an admitted server/cache pair must reject changed generate.
                candidate_budget = {**budget, 'reviewed_sources': lambda *args: reviewed_pair}
                with patch.object(globals_['importlib'].metadata,'version',return_value='0.32.2'), patch.object(globals_['runpy'],'run_path',return_value=candidate_budget), patch.object(globals_['importlib'],'import_module',return_value=SimpleNamespace(__file__=str(source))), patch.dict('sys.modules',{'mlx_lm':package,'mlx_lm.models':models}), patch.dict(globals_,install=lambda *args:touched.append('install')):
                    with self.assertRaises(ValueError):
                        main()
        self.assertEqual(touched, [])

    def test_hooks_keep_results_arguments_close_and_failure(self):
        calls = []
        class Cache:
            def fetch_nearest_cache(self, *args, **kwargs):
                calls.append((args, copy.deepcopy(kwargs)))
                return ('cache', [1])
            def insert_cache(self, *args):
                return args
            def trim_to(self, *args):
                raise RuntimeError('preserved')
        class Prompt:
            def prompt(self, tokens):
                return tokens
        class Generation:
            def next(self):
                return ['same token']
        closed = []
        def stream(*args, **kwargs):
            try:
                yield 'first'
                yield 'second'
            finally:
                closed.append(True)
        server = SimpleNamespace(stream_generate=stream)
        generate = SimpleNamespace(PromptProcessingBatch=Prompt, GenerationBatch=Generation)
        ledger = Ledger()
        m['install'](ledger, server, SimpleNamespace(LRUPromptCache=Cache), generate)
        self.assertEqual(Cache().fetch_nearest_cache('model', [1,2], a=3), ('cache', [1]))
        self.assertEqual(calls, [(('model',[1,2]), {'a':3})])
        self.assertEqual(Cache().insert_cache('x'), ('x',))
        with self.assertRaises(RuntimeError):
            Cache().trim_to(0)
        self.assertEqual(Prompt().prompt([[1,2]]), [[1,2]])
        self.assertEqual(Generation().next(), ['same token'])
        self.assertEqual(list(server.stream_generate()), ['first','second'])
        it = server.stream_generate()
        self.assertEqual(next(it), 'first')
        it.close()
        self.assertEqual(closed, [True, True])
        rows = ledger.snapshot()['spans']
        self.assertEqual(rows['cache.trim']['failures'], 1)
        self.assertEqual(rows['sequential.prefill_first']['calls'], 2)
        self.assertEqual(rows['sequential.decode']['failures'], 0)


if __name__ == '__main__':
    unittest.main()
