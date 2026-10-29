"""Admission/resource routing contracts, no model or tokenizer imports."""
import ast
from pathlib import Path
from types import SimpleNamespace
import time
import unittest

SOURCE=Path(__file__).with_name('allocator-budget-trial.py')

def namespace(micro=True,cpu=20):
    function=next(n for n in ast.parse(SOURCE.read_text()).body if isinstance(n,ast.FunctionDef) and n.name=='chat')
    values=dict(micro=micro,os=SimpleNamespace(getloadavg=lambda:(200,200,200)),time=time,
        guard=SimpleNamespace(wait_quiet=lambda:0,mark=lambda:{'busyPercent':cpu},interval=lambda a,b:dict(averageBusyPercent=cpu,peakBusyPercent=cpu,sustainedHighSeconds=0)),
        count=lambda messages:32,report=dict(cases=[]),
        request_json=lambda path,body:dict(choices=[dict(message=dict(content='OK'))],usage=dict(prompt_tokens=32,prompt_tokens_details=dict(cached_tokens=4))))
    exec(compile(ast.Module(body=[function],type_ignores=[]),str(SOURCE),'exec'),values)
    return values

class TrialRouting(unittest.TestCase):
    def test_micro_priming_does_not_fall_into_lagging_load_guard(self):
        n=namespace(cpu=70);n['chat']([{'role':'user','content':'test'}],'OK','prime',0)
        self.assertTrue(n['report']['cases'][0]['exact'])
    def test_micro_measured_request_uses_live_cpu_not_lagging_load(self):
        n=namespace();n['chat']([{'role':'user','content':'test'}],'OK','warm-before',0)
        self.assertEqual(n['report']['cases'][0]['cpuAveragePercent'],20)
    def test_micro_busy_request_still_rejects(self):
        n=namespace(cpu=70)
        with self.assertRaises(AssertionError):n['chat']([], 'OK','warm-before',0)
        self.assertFalse(n['report']['cases'])
    def test_diverse_history_legacy_load_guard_is_retained(self):
        n=namespace(micro=False)
        with self.assertRaises(AssertionError):n['chat']([], 'OK','prime',0)
        self.assertFalse(n['report']['cases'])

if __name__=='__main__':unittest.main()
