import importlib.util
from pathlib import Path
import unittest
import subprocess
import sys
spec=importlib.util.spec_from_file_location('retention',Path(__file__).with_name('compare-kv-retention.py'));m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
def trials():
 return [dict(identityBefore={k:'same' for k in ['backend','backend_sha256','config_sha256','hardware_sha256','model_root_sha256','runtime_sha256']},identityAfter={k:'same' for k in ['backend','backend_sha256','config_sha256','hardware_sha256','model_root_sha256','runtime_sha256']},budgetMiB=b,passed=True,shutdownClean=True,counterStopped=True,fixturesSha256='same',cases=[dict(label='case',answer='code',completionTokens=1,phase='revisit',cachedTokens=99 if b>1024 else 0,promptTokens=100,firstContentMs=10 if b>1024 else 100,cpuBeforePercent=10,cpu=dict(averageBusyPercent=10,sustainedHighSeconds=0))],memory=[dict(physicalBytes=b*1024**2,availableBytes=5*1024**3)]) for b in m.ORDER]
class Comparison(unittest.TestCase):
 def test_matched_complete_blocks_and_memory_are_reported(self):
  result=m.summarize(trials());self.assertEqual(result[1024]['fullHits'],0);self.assertEqual(result[1536]['fullHits'],2);self.assertGreater(result[2048]['sampledPeakPhysicalBytes'],result[1024]['sampledPeakPhysicalBytes'])
 def test_partial_or_failed_comparison_refuses(self):
  with self.assertRaises(AssertionError):m.summarize(trials()[:-1])
  rows=trials();rows[2]['passed']=False
  with self.assertRaises(AssertionError):m.summarize(rows)
 def test_busy_windows_do_not_authorize_latency_claims(self):
  rows=trials();rows[0]['cases'][0]['cpu']['averageBusyPercent']=60
  self.assertFalse(m.summarize(rows)[1024]['latencyEligible'])
 def test_non_budget_identity_changes_refuse(self):
  rows=trials();rows[1]['identityAfter']['runtime_sha256']='changed'
  with self.assertRaises(AssertionError):m.summarize(rows)
 def test_output_or_fixture_changes_refuse(self):
  for field in ['answer','promptTokens','completionTokens']:
   rows=trials();rows[1]['cases'][0][field]='changed'
   with self.assertRaises(AssertionError):m.summarize(rows)
  rows=trials();rows[1]['fixturesSha256']='changed'
  with self.assertRaises(AssertionError):m.summarize(rows)
 def test_nonfinite_first_content_refuses(self):
  rows=trials();rows[2]['cases'][0]['firstContentMs']=float('nan')
  with self.assertRaises(AssertionError):m.summarize(rows)
 def test_optimized_python_cannot_disable_validation(self):
  result=subprocess.run([sys.executable,'-O',str(Path(__file__).with_name('compare-kv-retention.py')),'--help'],capture_output=True,text=True)
  self.assertNotEqual(result.returncode,0)
  self.assertIn('Validation assertions must be enabled',result.stderr)
if __name__=='__main__':unittest.main()
