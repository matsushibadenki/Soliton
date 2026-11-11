"""Evidence contract regressions; no GPU, network or credentials required."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import time
import unittest
from prefix_cache_contract import PROFILE, REQUIRED_GATES, BINDING_SCHEMA, binding_digest, identity_binding, verify_evidence
from unittest.mock import patch
import prefix_cache_contract as contract


class EvidenceContract(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory(prefix='prefix-evidence-test-')
        self.path = Path(self.folder.name) / 'evidence.json'
        self.quota = 256 * 1024**2
        self.bound=dict(binding_schema=BINDING_SCHEMA,identity={'model':'a'*64,'tokenizer':'b'*64,'runtime':'c'*64,'format':'test'},
            profile=copy.deepcopy(PROFILE),cache_root=self.folder.name,quota_bytes=self.quota,
            cache_volume=dict(volume_uuid_sha256='d'*64,filesystem='apfs',internal=True,solid_state=True,bus_protocol='Apple Fabric'))
        self.identity=binding_digest(self.bound)
        self.report = dict(binding_schema=BINDING_SCHEMA,cache_binding=self.bound,result='passed', identity=self.identity, profile=PROFILE,
                           quota_bytes=self.quota, created_at=time.time(), duration_seconds=1800,
                           successes=100, failures=0, rss_growth_bytes=0,
                           gates={key: True for key in REQUIRED_GATES})

    def tearDown(self):
        self.folder.cleanup()

    def write(self, report):
        self.path.write_text(json.dumps(report))
        self.path.chmod(0o600)

    def test_publication_validates_before_exposing_and_preserves_existing_report(self):
        spec = importlib.util.spec_from_file_location(
            'qualify_prefix_cache', Path(__file__).with_name('qualify-prefix-cache.py')
        )
        controller = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(controller)
        with self.assertRaises(ValueError):
            controller.publish_evidence(
                self.path, {**self.report, 'duration_seconds': 1799}, self.identity, self.quota
            )
        self.assertFalse(self.path.exists())
        controller.publish_evidence(self.path, self.report, self.identity, self.quota)
        original = self.path.read_bytes()
        verify_evidence(self.path, self.identity, self.quota)
        with self.assertRaises(FileExistsError):
            controller.publish_evidence(
                self.path, {**self.report, 'successes': 101}, self.identity, self.quota
            )
        self.assertEqual(self.path.read_bytes(), original)
        self.assertEqual(list(self.path.parent.iterdir()), [self.path])

    def test_device_renumbering_does_not_change_binding_but_volume_path_and_quota_do(self):
        root=Path(self.folder.name)
        volume=self.bound['cache_volume']
        with patch.object(contract,'volume_snapshot',side_effect=[(volume,10),(volume,99)]):
            first,dev1=identity_binding(self.bound['identity'],root,self.quota)
            second,dev2=identity_binding(self.bound['identity'],root,self.quota)
        self.assertNotEqual(dev1,dev2)
        self.assertEqual(binding_digest(first),binding_digest(second))
        for key,value in [('cache_root','/different'),('quota_bytes',self.quota+1),
                          ('cache_volume',{**volume,'volume_uuid_sha256':'e'*64})]:
            other=copy.deepcopy(first);other[key]=value
            self.assertNotEqual(binding_digest(first),binding_digest(other))

    def test_legacy_and_tampered_binding_cannot_be_reused(self):
        for change in [dict(binding_schema=1),dict(cache_binding=None)]:
            report=copy.deepcopy(self.report);report.update(change);self.write(report)
            with self.assertRaises(ValueError):verify_evidence(self.path,self.identity,self.quota)
        report=copy.deepcopy(self.report);report['cache_binding']['cache_volume']['filesystem']='hfs';self.write(report)
        with self.assertRaises(ValueError):verify_evidence(self.path,self.identity,self.quota)

    def test_eviction_evidence_requires_pressure_and_restart_gates(self):
        self.report['requires_eviction'] = True
        self.write(self.report)
        with self.assertRaises(ValueError): verify_evidence(self.path,self.identity,self.quota)
        self.report['gates'].update(automatic_eviction=True,eviction_restart_recompute=True)
        self.write(self.report)
        self.assertTrue(verify_evidence(self.path,self.identity,self.quota)['requires_eviction'])
        self.report['requires_eviction'] = 'yes'
        self.write(self.report)
        with self.assertRaises(ValueError): verify_evidence(self.path,self.identity,self.quota)

    def test_qualified_private_report(self):
        self.write(self.report)
        self.assertEqual(verify_evidence(self.path, self.identity, self.quota)['result'], 'passed')

    def test_identity_profile_and_numeric_guards(self):
        cases = [('identity', 'b'*64), ('quota_bytes', self.quota+1), ('result', 'preflight'),
                 ('duration_seconds', 1799), ('duration_seconds', float('nan')),
                 ('duration_seconds', float('inf')), ('successes', 99), ('successes', True),
                 ('failures', False), ('failures', 1), ('rss_growth_bytes', 128*1024**2+1),
                 ('created_at', time.time()-8*86400), ('created_at', time.time()+86400)]
        for key, value in cases:
            with self.subTest(key=key, value=value):
                report = copy.deepcopy(self.report); report[key] = value; self.write(report)
                with self.assertRaises(ValueError): verify_evidence(self.path, self.identity, self.quota)
        report = copy.deepcopy(self.report); report['profile']['context'] = 1024; self.write(report)
        with self.assertRaises(ValueError): verify_evidence(self.path, self.identity, self.quota)

    def test_all_gates_required_and_boolean(self):
        for gate in REQUIRED_GATES:
            report = copy.deepcopy(self.report); del report['gates'][gate]; self.write(report)
            with self.assertRaises(ValueError): verify_evidence(self.path, self.identity, self.quota)
            report['gates'][gate] = 1; self.write(report)
            with self.assertRaises(ValueError): verify_evidence(self.path, self.identity, self.quota)

    def test_public_oversized_and_symlink_reports_refused(self):
        self.write(self.report); self.path.chmod(0o644)
        with self.assertRaises(ValueError): verify_evidence(self.path, self.identity, self.quota)
        self.path.write_text(' ' * 65537); self.path.chmod(0o600)
        with self.assertRaises(ValueError): verify_evidence(self.path, self.identity, self.quota)
        self.path.unlink(); self.path.symlink_to('/etc/passwd')
        with self.assertRaises(OSError): verify_evidence(self.path, self.identity, self.quota)


if __name__ == '__main__': unittest.main()
