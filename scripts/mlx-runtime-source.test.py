"""CPU-only guard tests; no model or GPU imported."""
import runpy
import unittest
from pathlib import Path
wrapper=runpy.run_path(str(Path(__file__).resolve().parents[1]/'vLLM-apple/vllm_apple/mlx_kv_budget.py'))
class ReviewedPairs(unittest.TestCase):
    def test_exact_pairs_only(self):
        pairs=wrapper['REVIEWED_SOURCE_PAIRS'];self.assertEqual(len(pairs),2)
        for server,cache in pairs:self.assertTrue(wrapper['reviewed_sources'](server,cache))
    def test_mixed_or_mutated_files_are_refused(self):
        a,b=sorted(wrapper['REVIEWED_SOURCE_PAIRS'])
        for server,cache in [(a[0],b[1]),(b[0],a[1]),('0'*64,a[1]),(a[0],'0'*64)]:
            self.assertFalse(wrapper['reviewed_sources'](server,cache))
if __name__=='__main__':unittest.main()
