import unittest
import threading
import time
from unittest.mock import Mock
from host_load_guard import busy_percent, HostLoadGuard

class GuardTests(unittest.TestCase):
    def test_idle_and_busy_ticks(self):
        self.assertEqual(busy_percent([0,0,0,0],[10,10,80,0]),20)
        self.assertEqual(busy_percent([0,0,0,0],[0,0,100,0]),0)
    def test_wrapping_ticks_preserve_accounting(self):
        self.assertEqual(busy_percent([0,0,0xfffffff0,0],[10,10,64,0]),20)
    def test_missing_interval_is_not_assumed_idle(self):
        with self.assertRaises(AssertionError):busy_percent([1,1,1,1],[1,1,1,1])
    def test_unchanged_kernel_snapshot_does_not_publish_idle(self):
        g=object.__new__(HostLoadGuard)
        g.lock=threading.Lock();g.ready=threading.Event();g.error=None;g.samples=[];g.generation=0;g.owned_ns=Mock(return_value=0)
        g.stopped=Mock();g.stopped.wait.side_effect=[False,False,True]
        g.ticks=Mock(side_effect=[[0,0,0,0],[0,0,0,0],[10,10,80,0]])
        g.monitor()
        self.assertIsNone(g.error)
        self.assertEqual(len(g.samples),1)
        self.assertEqual(g.samples[0]['busyPercent'],20)
    def test_native_owned_cpu_matches_process_clock_units(self):
        guard=HostLoadGuard()
        try:
            before=guard.owned_ns();clock=time.process_time_ns()
            sum(range(5000000))
            after=guard.owned_ns();elapsed=time.process_time_ns()-clock
            self.assertTrue(.8<(after-before)/elapsed<1.2)
        finally:guard.stop()
    def test_native_guard_is_live(self):
        guard=HostLoadGuard()
        try:self.assertTrue(0<=guard.snapshot()['busyPercent']<=100)
        finally:guard.stop()

if __name__=='__main__':unittest.main()
