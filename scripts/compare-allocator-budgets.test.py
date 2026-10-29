"""Negative comparison contracts; synthetic measurements never approve startup."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec=importlib.util.spec_from_file_location('comparison',Path(__file__).with_name('compare-allocator-budgets.py'))
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)

def fixture(budget):
    cases=[]
    for phase,n in [('prime',6),('warm-before',18),('anchor-cold',1),('anchor-hot',1),('churn',15),('revisit',1),('reheat',1),('recent',1),('seeded',3),('warm-after',18)]:
        for index in range(n):cases.append(dict(phase=phase,index=index,exact=True,promptTokens=100,cachedTokens=4,elapsedMs={0:100,256:100,512:80,768:70}[budget]*(10 if phase in {'churn','anchor-cold','revisit'} else 1)))
    ident=dict(backend_sha256='backend',runtime_sha256='runtime',environment_sha256=str(budget))
    return dict(passed=True,certification=False,shutdownClean=True,counterStopped=True,userStatePreserved=True,identityBefore=ident,identityAfter=ident,allocatorCacheMiB=budget,cases=cases,samples=[dict(physicalBytes=(1000 if budget==0 else 600)*1024**2,kvCache=dict(allocator=dict(cache_bytes=(1000 if budget==0 else budget)*1024**2))) for _ in range(26)])

class ComparisonContracts(unittest.TestCase):
    def run_reports(self,mutate=lambda reports:None):
        with tempfile.TemporaryDirectory() as d:
            reports=[fixture(b) for b in [0,256,512,512,256,0,768,768,256,0]];mutate(reports);paths=[]
            for index,r in enumerate(reports):
                p=Path(d)/f'{index}.json';p.write_text(json.dumps(r));paths.append(p)
            return module.compare(paths)
    def test_selects_speed_with_retained_memory_benefit(self):
        # 768 is faster but exceeds the free-cache retention criterion.
        self.assertEqual(self.run_reports()['selectedMiB'],512)
    def test_no_candidate_when_warm_speed_regresses(self):
        def mutate(reports):
            for r in reports:
                if r['allocatorCacheMiB'] in {512,768}:
                    for c in r['cases']:
                        if c['phase'].startswith('warm-'):c['elapsedMs']=200
        self.assertFalse(self.run_reports(mutate)['selectionPassed'])
    def test_input_count_change_invalidates_pairing(self):
        with self.assertRaisesRegex(AssertionError,'workload/count/reuse'):
            self.run_reports(lambda r:r[2]['cases'][0].update(promptTokens=101))
    def test_backend_change_cannot_be_attributed_to_budget(self):
        def mutate(r):
            r[3]['identityBefore']=dict(r[3]['identityBefore'],backend_sha256='changed');r[3]['identityAfter']=r[3]['identityBefore']
        with self.assertRaises(AssertionError):self.run_reports(mutate)
    def test_host_load_episode_prevents_latency_claim(self):
        with self.assertRaisesRegex(AssertionError,'host load episode'):
            self.run_reports(lambda r:r[6].update(loadAtEnd=[50,20,10]))
    def test_symmetric_micro_workload_is_compared_separately(self):
        with tempfile.TemporaryDirectory() as d:
            paths=[]
            for index,b in enumerate([0,256,512,768,768,512,256,0]):
                r=fixture(b)
                r.update(hostGuardMode='liveBackgroundCPU',hostGuardSha256='fixed',workloadKind='micro',workloadSha256='fixed',fixturesSha256='fixed')
                r['cases']=[c for c in r['cases'] if c['phase']!='churn' or c['index']<2]
                r['samples']=r['samples'][:13]
                for c in r['cases']:c.update(cpuMaxPercent=10,cpuAveragePercent=10,cpuSustainedHighSeconds=0)
                for sample in r['samples']:sample['cpuBusyPercent']=10
                r['loadAtEnd']=[200,100,100] # Lagging load alone does not override live CPU.
                p=Path(d)/f'{index}.json';p.write_text(json.dumps(r));paths.append(p)
            result=module.compare(paths)
            self.assertEqual(result['workloadKind'],'micro')
            self.assertEqual(result['selectedMiB'],512)
    def test_request_level_load_episode_is_not_hidden_by_end_load(self):
        with self.assertRaisesRegex(AssertionError,'host load episode'):
            self.run_reports(lambda r:r[6]['cases'][5].update(loadAfter=50))
    def test_memory_regression_prevents_selection(self):
        def mutate(reports):
            for r in reports:
                if r['allocatorCacheMiB'] in {512,768}:
                    for s in r['samples']:s['physicalBytes']=1100*1024**2
        self.assertIsNone(self.run_reports(mutate)['selectedMiB'])

if __name__=='__main__':unittest.main()
