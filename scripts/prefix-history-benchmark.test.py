"""Comparison eligibility tests; no model/GPU/OS CPU monitor required."""
import copy
import importlib.util
import json
import sys
import tempfile
from types import SimpleNamespace
from pathlib import Path
import unittest
from unittest.mock import patch

spec=importlib.util.spec_from_file_location('history_benchmark',Path(__file__).with_name('benchmark-prefix-history.py'))
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)


def fixture():
    return [dict(label='case',round=r,slot=i,mode=mode,firstContentMs=100 if mode=='fresh' else 120,
        completionMs=200,exact=True,cacheVerified=True,quietBeforePercent=10,
        cpu=dict(averageBusyPercent=15,sustainedHighSeconds=0))
        for r in range(2) for i,mode in enumerate(['fresh','restore','restore','fresh'])]


class Comparison(unittest.TestCase):
    def test_counterbalanced_plan_and_paired_round_statistics(self):
        rows=fixture()
        for row in rows:
            row['mode']=module.block_modes(row['round'],'counterbalanced')[row['slot']]
            row['firstContentMs']=100 if row['mode']=='fresh' else (80 if row['round']==0 else 140)
        result=module.summarize(rows,['case'],2,'counterbalanced')['case']
        self.assertEqual([r['order'] for r in result['pairedRounds']],['ABBA','BAAB'])
        self.assertAlmostEqual(result['pairedChangeMedianPercent'],10)
        self.assertEqual(result['restore']['firstContentRangeMs'],[80,140])
        self.assertGreater(result['pairedChangeRangePercent'][1],0)
        with self.assertRaises(AssertionError):module.summarize(rows,['case'],3,'counterbalanced')
        with self.assertRaises(ValueError):module.block_modes(0,'unknown')

    def test_nonfinite_load_and_reversed_timing_refuse(self):
        for key,value in [('quietBeforePercent',float('nan')),('completionMs',50),
                          ('cpu',dict(averageBusyPercent=float('nan'),sustainedHighSeconds=0)),
                          ('cpu',dict(averageBusyPercent=0,sustainedHighSeconds=-1))]:
            rows=fixture();rows[0][key]=value
            with self.assertRaises(AssertionError):module.summarize(rows,['case'],2)

    def test_existing_qualification_uses_full_original_verifier(self):
        response=dict(verified=True,binding_schema=2,identity='a'*64)
        with patch.object(module.subprocess,'run',return_value=SimpleNamespace(returncode=0,stdout=json.dumps(response).encode())) as run:
            actual=module.verify_qualification('python',Path('/runtime'),'model','store','root','evidence',{'TOKEN':'private'})
        self.assertEqual(actual,response)
        command=run.call_args.args[0]
        self.assertIn('--verify-only',command)
        self.assertIn('--evidence',command)
        self.assertNotIn('--qualification-mode',command)
        self.assertEqual(run.call_args.kwargs['timeout'],90)
        for result in [SimpleNamespace(returncode=1,stdout=b''),
                       SimpleNamespace(returncode=0,stdout=b'x'*65537),
                       SimpleNamespace(returncode=0,stdout=b'{"verified":false,"binding_schema":2}'),
                       SimpleNamespace(returncode=0,stdout=b'{"verified":true,"binding_schema":1}'),
                       SimpleNamespace(returncode=0,stdout=b'{"verified":true,"binding_schema":2,"identity":"bad"}')]:
            with patch.object(module.subprocess,'run',return_value=result):
                with self.assertRaises(RuntimeError):module.verify_qualification('python',Path('/runtime'),'model','store','root','evidence',{})

    def test_busy_preflight_refuses_before_model_worker_or_seed(self):
        clock=[0];stopped=[]
        class Guard:
            def snapshot(self):return dict(at=clock[0],busyPercent=35,globalBusyPercent=35,ownedBusyPercent=0)
            def stop(self):stopped.append(True)
        def sleep(duration):clock[0]+=duration
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);fixtures=root/'fixtures.json';store=root/'store'
            store.write_bytes(b'test executable')
            fixtures.write_text(json.dumps(dict(result='passed',fresh_text_parity=1,canonical=[dict(label='case')])) )
            argv=['benchmark','--python',sys.executable,'--model','unused-model','--store',str(store),
                  '--fixtures',str(fixtures),'--output-dir',str(root/'out'),'--labels','case']
            with patch.object(sys,'argv',argv),patch.object(module,'HostLoadGuard',return_value=Guard()),patch.object(module.time,'monotonic',side_effect=lambda:clock[0]),patch.object(module.time,'sleep',side_effect=sleep),patch.object(module.subprocess,'Popen',side_effect=AssertionError('Model worker must not start')):
                with self.assertRaises(module.QuietHostError):module.main()
            report=json.loads((root/'out/comparison.json').read_text())
            self.assertEqual(report['result'],'rejected')
            self.assertEqual(report['failureStage'],'host_preflight')
            self.assertEqual(report['quietFailure']['minimumPercent'],35)
            self.assertEqual(report['seeds'],{})
            self.assertEqual(report['samples'],[])
            self.assertEqual(stopped,[True])

    def test_quiet_gate_resets_after_a_busy_sample(self):
        values=iter([5,25,5,5,5]);clock=[0];rows=[]
        class Guard:
            def snapshot(self):
                value=next(values);clock[0]+=.01
                return dict(at=clock[0],busyPercent=value,globalBusyPercent=value,ownedBusyPercent=0)
        def sleep(duration):clock[0]+=duration
        with patch.object(module.time,'monotonic',side_effect=lambda:clock[0]),patch.object(module.time,'sleep',side_effect=sleep):
            self.assertEqual(module.quiet(Guard(),record=rows),5)
        self.assertEqual(len(rows),5)

    def test_sustained_background_load_never_satisfies_quiet_gate(self):
        clock=[0]
        class Guard:
            def snapshot(self):return dict(at=clock[0],busyPercent=30,globalBusyPercent=30,ownedBusyPercent=0)
        def sleep(duration):clock[0]+=duration
        with patch.object(module.time,'monotonic',side_effect=lambda:clock[0]),patch.object(module.time,'sleep',side_effect=sleep):
            with self.assertRaises(RuntimeError):module.quiet(Guard(),timeout=1)

    def test_slower_restoration_is_reported_without_success_bias(self):
        result=module.summarize(fixture(),['case'],2)['case']
        self.assertEqual(result['fresh']['requests'],4)
        self.assertAlmostEqual(result['restoreFirstContentChangePercent'],20)

    def test_incomplete_or_reordered_round_is_rejected(self):
        rows=fixture()
        with self.assertRaises(AssertionError):module.summarize(rows[:-1],['case'],2)
        rows[0],rows[1]=rows[1],rows[0]
        with self.assertRaises(AssertionError):module.summarize(rows,['case'],2)

    def test_load_or_nonmatching_answer_invalidates_entire_pilot(self):
        for change in [dict(exact=False),dict(cacheVerified=False),dict(quietBeforePercent=21),
                       dict(cpu=dict(averageBusyPercent=51,sustainedHighSeconds=0)),
                       dict(cpu=dict(averageBusyPercent=20,sustainedHighSeconds=1))]:
            rows=fixture();rows[3].update(change)
            with self.assertRaises(AssertionError):module.summarize(rows,['case'],2)

    def test_missing_or_nonfinite_latency_cannot_become_a_result(self):
        for value in [float('nan'),float('inf'),0,-1]:
            rows=fixture();rows[2]['firstContentMs']=value
            with self.assertRaises(AssertionError):module.summarize(rows,['case'],2)


if __name__=='__main__':unittest.main()
