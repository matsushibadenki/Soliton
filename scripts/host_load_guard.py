"""Live Darwin CPU guard for measurements; load averages are retained as context.

No processes are stopped. Missing/stale/invalid readings fail closed. Ticks use
public HOST_CPU_LOAD_INFO (user/system/idle/nice), polled every 250 ms (kernel snapshots may update more slowly).
"""
import ctypes
import os
import threading
import time


def busy_percent(before, after):
    assert len(before)==len(after)==4
    delta=[(int(b)-int(a)) & 0xffffffff for a,b in zip(before,after)]
    total=sum(delta)
    assert 0<total<1_000_000,'invalid CPU tick interval'
    return 100*(total-delta[2])/total


class TimebaseInfo(ctypes.Structure):
    _fields_=[('numer',ctypes.c_uint32),('denom',ctypes.c_uint32)]


class TaskInfo(ctypes.Structure):
    _fields_=[('values',ctypes.c_uint64*6),('tail',ctypes.c_int32*12)]


class HostLoadGuard:
    def __init__(self):
        self.library=ctypes.CDLL('/usr/lib/libSystem.B.dylib')
        self.library.mach_host_self.restype=ctypes.c_uint32
        self.host=self.library.mach_host_self()
        self.library.mach_timebase_info.argtypes=[ctypes.POINTER(TimebaseInfo)]
        self.library.mach_timebase_info.restype=ctypes.c_int
        timebase=TimebaseInfo()
        assert self.library.mach_timebase_info(ctypes.byref(timebase))==0 and timebase.denom>0
        self.numer=int(timebase.numer);self.denom=int(timebase.denom)
        self.proc=ctypes.CDLL('/usr/lib/libproc.dylib')
        self.proc.proc_pidinfo.argtypes=[ctypes.c_int,ctypes.c_int,ctypes.c_uint64,ctypes.c_void_p,ctypes.c_int]
        self.proc.proc_pidinfo.restype=ctypes.c_int
        self.owned_pids=(os.getpid(),);self.generation=0
        self.library.host_statistics.argtypes=[ctypes.c_uint32,ctypes.c_int,ctypes.POINTER(ctypes.c_int),ctypes.POINTER(ctypes.c_uint32)]
        self.library.host_statistics.restype=ctypes.c_int
        self.lock=threading.Lock();self.stopped=threading.Event();self.ready=threading.Event()
        self.samples=[];self.error=None
        self.thread=threading.Thread(target=self.monitor,daemon=True);self.thread.start()
        assert self.ready.wait(5),'CPU guard unavailable'
        self.snapshot()
    def ticks(self):
        values=(ctypes.c_uint32*4)();count=ctypes.c_uint32(4)
        status=self.library.host_statistics(self.host,3,ctypes.cast(values,ctypes.POINTER(ctypes.c_int)),ctypes.byref(count))
        assert status==0 and count.value==4,'CPU statistics unavailable'
        return list(values)
    def owned_ns(self,pids=None):
        total=0
        for pid in (pids or self.owned_pids):
            value=TaskInfo();size=ctypes.sizeof(value)
            assert self.proc.proc_pidinfo(pid,4,0,ctypes.byref(value),size)==size,'owned CPU accounting unavailable'
            total+=int(value.values[2])+int(value.values[3])
        # PROC_PIDTASKINFO CPU totals are Mach absolute ticks, not nanoseconds.
        # Do not add threads_user/system: these are subsets of the totals.
        return total*self.numer//self.denom
    def set_owned(self,pids):
        self.owned_pids=tuple(pids);self.generation+=1
        deadline=time.monotonic()+5
        while time.monotonic()<deadline:
            with self.lock:
                if self.samples and self.samples[-1]['generation']==self.generation:return
            assert self.error is None,'owned CPU monitor failed'
            time.sleep(.05)
        raise RuntimeError('owned CPU monitor did not become ready')
    def monitor(self):
        try:
            generation=self.generation;before=self.ticks();own=self.owned_ns();at=time.monotonic_ns()
            while not self.stopped.wait(.25):
                after=self.ticks();own_after=self.owned_ns();now=time.monotonic_ns()
                if generation!=self.generation:
                    generation=self.generation;before=self.ticks();own=self.owned_ns();at=time.monotonic_ns();continue
                if after==before:continue
                global_busy=busy_percent(before,after)
                assert own_after>=own and now>at,'invalid owned CPU interval'
                owned_busy=100*(own_after-own)/(now-at)/os.cpu_count()
                assert 0<=owned_busy<=120,'invalid owned CPU utilization'
                row=dict(generation=generation,at=time.monotonic(),busyPercent=max(0,global_busy-owned_busy),globalBusyPercent=global_busy,ownedBusyPercent=owned_busy,load1m=os.getloadavg()[0])
                before=after;own=own_after;at=now
                with self.lock:
                    self.samples.append(row)
                    if len(self.samples)>10000:self.samples=self.samples[-10000:]
                self.ready.set()
        except BaseException as error:
            self.error=type(error).__name__;self.ready.set()
    def snapshot(self):
        assert self.error is None,'CPU guard read failed'
        with self.lock:
            assert self.samples,'CPU guard has no sample'
            value=dict(self.samples[-1])
        assert time.monotonic()-value['at']<2,'stale CPU guard sample'
        return value
    def wait_quiet(self,limit=40,timeout=10):
        start=time.monotonic()
        while self.snapshot()['busyPercent']>limit:
            assert time.monotonic()-start<timeout,'background CPU did not settle'
            time.sleep(.1)
        return (time.monotonic()-start)*1000
    def mark(self):
        value=self.snapshot();value['ticks']=self.ticks();value['ownedNs']=self.owned_ns();value['markedAt']=time.monotonic();return value
    def interval(self,before,after):
        # A sub-second response can finish within one cached kernel snapshot.
        # Wait for an observable interval AFTER the response timer has stopped.
        # This adds no fake idle reading and does not enter request latency.
        deadline=time.monotonic()+2
        while after['ticks']==before['ticks']:
            assert time.monotonic()<deadline, 'CPU request interval unavailable'
            time.sleep(.02);after=self.mark()
        global_value=busy_percent(before['ticks'],after['ticks'])
        own_value=100*(after['ownedNs']-before['ownedNs'])/((after['markedAt']-before['markedAt'])*1e9)/os.cpu_count()
        assert own_value>=0
        value=max(0,global_value-own_value)
        with self.lock:
            overlapping=[s['busyPercent'] for s in self.samples if before['markedAt']<=s['at']<=after['markedAt']]
        high_since=None;sustained=0
        with self.lock:
            rows=[s for s in self.samples if before['markedAt']<=s['at']<=after['markedAt']]
        for row in rows:
            if row['busyPercent']>=75:
                if high_since is None:high_since=row['at']
                sustained=max(sustained,row['at']-high_since)
            else:high_since=None
        return dict(averageBusyPercent=value,peakBusyPercent=max([value,before['busyPercent'],after['busyPercent'],*overlapping]),sustainedHighSeconds=sustained)
    def stop(self):
        self.stopped.set();self.thread.join(timeout=2)
