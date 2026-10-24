"""Negative contracts: cache measurements may refresh flags, not certify new bounds/backends."""
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vLLM-apple'))
from tests.test_architecture_evidence import body, identity
from vllm_apple.architecture_evidence import bind_evidence

class RefreshGuardTests(unittest.TestCase):
    def trial(self, context, mutate_identity, expected):
        with tempfile.TemporaryDirectory(prefix='soliton-cache-refresh-') as directory:
            directory = Path(directory)
            # All-a identities are deliberately synthetic and cannot match a real backend.
            fixture = body()
            fixture['context_reevaluation'].update(configured_context_tokens=4096, effective_context_tokens=4096)
            fixture['model_memory_fit']['context_tokens'] = 4096
            fixture['qualification_limits']['context_tokens'] = 4096
            prior = bind_evidence(fixture, identity(), context_tokens=4096, concurrency=1)
            old = directory / 'old-synthetic.json'
            old.write_text(json.dumps(prior)); old.chmod(0o600)
            qualification = copy.deepcopy(prior)
            mutate_identity(qualification['architecture_evidence']['identity'])
            evidence = directory / 'evidence.json'
            evidence.write_text(json.dumps(qualification)); evidence.chmod(0o600)
            sha = hashlib.sha256(evidence.read_bytes()).hexdigest()
            row = {'exact': True, 'messageCount': 13, 'promptTokens': 3900}
            cases = [{**row, 'index': n % 16, 'language': ['en','ja','zh'][n % 3]} for n in range(38)]
            matrix = {'passed': True, 'context': 4096, 'evidenceSha256': sha, 'checks': {'lifecycle': True},
                'sharedRequestMetrics': {'bounded': True}, 'cacheMemory': {'passed': True,
                'boundary': {'passed': True, 'acceptedTotal': 4096, 'rejectedTotal': 4097},
                'observedAnchorReuseLosses': 2, 'sharedCachedTelemetry': {'verified': True},
                'afterStop': {'ownedTreeGone': True}, 'cases': cases}}
            matrix_path = directory / 'matrix.json'; matrix_path.write_text(json.dumps(matrix))
            gates = {'passed': True, 'evidenceSha256': ['',sha], 'checks': [
                {'context': c, 'startupRefused': True, 'noInferenceOrCounter': True, 'historyPreserved': True} for c in [4096,4097]]}
            gates_path = directory / 'gates.json'; gates_path.write_text(json.dumps(gates))
            state = directory / 'state.json'
            state.write_text(json.dumps({'messages': [], 'settings': {'vllm': {
                'context': context, 'root': str(ROOT / 'vLLM-apple'), 'python': sys.executable,
                'evidence': str(old)}}})); state.chmod(0o600)
            before = state.read_bytes(); output = directory / 'activation.json'
            result = subprocess.run([sys.executable, str(ROOT / 'scripts/activate-context-profile.py'),
                str(ROOT / 'vLLM-apple'), str(evidence), str(matrix_path), str(gates_path), str(output), str(state)],
                capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(expected, result.stderr)
            self.assertEqual(state.read_bytes(), before)
            self.assertFalse(output.exists())

    def test_cache_trial_cannot_increase_context(self):
        self.trial(1024, lambda identity: None, 'cache trial cannot raise the existing context')

    def test_backend_change_requires_full_matrix(self):
        self.trial(4096, lambda identity: identity.update(backend_sha256='b' * 64), 'non-environment changes require the full context matrix')

if __name__ == '__main__':
    unittest.main()
