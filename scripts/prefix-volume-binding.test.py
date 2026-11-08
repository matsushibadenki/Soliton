"""Volume mapping and fail-closed metadata tests; no GPU or DiskManagement required."""
import copy
from pathlib import Path
import stat
import tempfile
import os
import shutil
import subprocess
import sys
import time
from types import SimpleNamespace
from unittest.mock import patch
import unittest
import prefix_cache_contract as contract


class Volume(unittest.TestCase):
    def setUp(self):
        self.folder=tempfile.TemporaryDirectory();self.root=Path(self.folder.name)
        self.device=self.root.stat().st_dev
        self.info=dict(DeviceNode='/dev/disk9s9',MountPoint=str(self.root),VolumeUUID='12345678-1234-4234-8234-123456789ABC',FilesystemType='apfs',BusProtocol='Apple Fabric',Internal=True,SolidState=True,WritableVolume=True)
    def tearDown(self):self.folder.cleanup()

    def test_uuid_is_canonical_and_does_not_expose_raw_value(self):
        result=contract._volume_info(self.info,'/dev/disk9s9',self.device)
        self.assertEqual(len(result['volume_uuid_sha256']),64)
        self.assertNotIn('12345678',str(result))
        other=dict(self.info,VolumeUUID=self.info['VolumeUUID'].lower())
        self.assertEqual(result,contract._volume_info(other,'/dev/disk9s9',self.device))

    def test_missing_wrong_or_noncanonical_volume_data_fails(self):
        cases=[dict(VolumeUUID=None),dict(VolumeUUID='0'*32),dict(DeviceNode='/dev/disk1'),
               dict(FilesystemType='smbfs'),dict(BusProtocol=None),dict(Internal=1),dict(WritableVolume=False),dict(MountPoint='relative')]
        for change in cases:
            with self.subTest(change=change):
                with self.assertRaises((ValueError,OSError)):contract._volume_info({**self.info,**change},'/dev/disk9s9',self.device)
        with self.assertRaises(ValueError):contract._volume_info(self.info,'/dev/disk9s9',self.device+1)

    def test_real_private_root_and_matching_device_are_required(self):
        actual_lstat=os.lstat
        def metadata(path,*args,**kwargs):
            if str(path)=='/dev/disk9s9':return SimpleNamespace(st_mode=stat.S_IFBLK,st_rdev=self.device)
            return actual_lstat(path,*args,**kwargs)
        with patch.object(contract.sys,'platform','darwin'),patch.object(contract,'_device_node',return_value='/dev/disk9s9'),patch.object(contract,'_diskutil_info',return_value=self.info),patch.object(contract.os,'lstat',metadata):
            value,device=contract.volume_snapshot(self.root)
            self.assertEqual(device,self.device);self.assertEqual(value['filesystem'],'apfs')
        self.root.chmod(0o755)
        with patch.object(contract.sys,'platform','darwin'):
            with self.assertRaises(ValueError):contract.volume_snapshot(self.root)

    def test_root_replacement_during_probe_is_rejected(self):
        moved=Path(str(self.root)+'.replaced')
        def replace(_node):
            self.root.rename(moved);self.root.mkdir(mode=0o700)
            return self.info
        try:
            with patch.object(contract.sys,'platform','darwin'),patch.object(contract,'_device_node',return_value='/dev/disk9s9'),patch.object(contract,'_diskutil_info',side_effect=replace):
                with self.assertRaisesRegex(ValueError,'changed during volume probe'):contract.volume_snapshot(self.root)
        finally:
            if moved.exists():shutil.rmtree(moved)

    def test_bounded_process_output_and_timeout_reap_owned_child(self):
        real_popen=subprocess.Popen;children=[]
        def launch(code):
            def wrapped(_args,**kwargs):
                process=real_popen([sys.executable,'-c',code],**kwargs);children.append(process);return process
            return wrapped
        for code in ["import sys;sys.stdout.write('x'*70000)","import time;time.sleep(10)"]:
            started=time.monotonic()
            with patch.object(contract.subprocess,'Popen',side_effect=launch(code)):
                with self.assertRaises(ValueError):contract._diskutil_info('/dev/disk9s9')
            self.assertIsNotNone(children[-1].poll())
            self.assertLess(time.monotonic()-started,5)

    def test_failed_probe_or_changed_device_never_falls_back_to_device_number(self):
        with patch.object(contract.sys,'platform','darwin'),patch.object(contract,'_device_node',return_value='/dev/disk9s9'),patch.object(contract,'_diskutil_info',side_effect=ValueError('unavailable')):
            with self.assertRaises(ValueError):contract.volume_snapshot(self.root)
        with patch.object(contract.sys,'platform','linux'):
            with self.assertRaises(ValueError):contract.volume_snapshot(self.root)


if __name__=='__main__':unittest.main()
