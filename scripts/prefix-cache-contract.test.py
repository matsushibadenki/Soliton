"""Evidence contract regressions; no GPU, network or credentials required."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import time
import unittest
from prefix_cache_contract import PROFILE, REQUIRED_GATES, verify_evidence


class EvidenceContract(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory(prefix='prefix-evidence-test-')
        self.path = Path(self.folder.name) / 'evidence.json'
        self.identity = 'a' * 64
        self.quota = 256 * 1024**2
        self.report = dict(result='passed', identity=self.identity, profile=PROFILE,
                           quota_bytes=self.quota, created_at=time.time(), duration_seconds=1800,
                           successes=100, failures=0, rss_growth_bytes=0,
                           gates={key: True for key in REQUIRED_GATES})

    def tearDown(self):
        self.folder.cleanup()

    def write(self, report):
        self.path.write_text(json.dumps(report))
        self.path.chmod(0o600)

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
