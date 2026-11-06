"""CPU-only candidate CLI pressure/receipt checks; no inference or qualification claim."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class EvictionCLI(unittest.TestCase):
    def test_pressure_receipts_restore_and_unknown_protection(self):
        with tempfile.TemporaryDirectory(prefix='soliton-eviction-cli-') as temp:
            folder = Path(temp)
            payload = folder/'state.bin'
            payload.write_bytes(bytes(range(256))*80)  # 20 KiB opaque state, not MLX tensors.
            quota = 100000
            outcomes = {}
            for policy, binary in [('refuse','prefix-store'),('evict','prefix-store-evict')]:
                store = folder/policy
                receipts, failures, evictions = [], 0, 0
                executable = ROOT/'target/debug/examples'/binary
                def run(action, manifest, path):
                    return subprocess.run([str(executable),str(store),str(quota),action,str(manifest),str(path)],capture_output=True,text=True)
                first_manifest = None
                for i in range(10):
                    manifest = folder/f'identity-{i}.json'
                    manifest.write_text(json.dumps(dict(model='a'*64,tokenizer='b'*64,runtime='c'*64,
                        prefix=hashlib.sha256(str(i).encode()).hexdigest(),format='opaque-test-v1')))
                    if i == 0: first_manifest = manifest
                    result = run('put', manifest, payload)
                    if result.returncode: failures += 1
                    else:
                        receipt = json.loads(result.stdout)
                        self.assertEqual(receipt['bytes'],payload.stat().st_size)
                        self.assertTrue((store/receipt['entry']).is_file())
                        receipts.append(receipt)
                        evictions += receipt.get('eviction',{}).get('removedEntries',0)
                    if i == 0: (store/'protected.txt').write_text('keep')
                    self.assertLessEqual(sum(p.stat().st_size for p in store.iterdir() if p.name!='.lock'),quota)
                    self.assertEqual((store/'protected.txt').read_text(),'keep')
                if policy == 'refuse': self.assertGreater(failures,0);self.assertEqual(evictions,0)
                else:
                    self.assertEqual(failures,0);self.assertGreater(evictions,0)
                    self.assertFalse((store/receipts[0]['entry']).exists())
                    # Reinsert a deleted key, then verify opaque payload SHA-256 on get.
                    result=run('put',first_manifest,payload);self.assertEqual(result.returncode,0,result.stderr)
                    output=folder/'restored.bin'
                    result=run('get',first_manifest,output);self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(hashlib.sha256(output.read_bytes()).digest(),hashlib.sha256(payload.read_bytes()).digest())
                    # Existing worker receipts invalidate on deletion; no receipt bypass is added.
                    import sys
                    sys.path.insert(0,str(ROOT/'scripts'))
                    from prefix_cache_writes import CommittedPrefixes
                    cache=CommittedPrefixes(store)
                    receipt=json.loads(result.stdout)
                    cache.remember('probe',receipt);self.assertTrue(cache.unchanged('probe'))
                    (store/receipt['entry']).unlink();self.assertFalse(cache.unchanged('probe'))
                outcomes[policy]=dict(writes=len(receipts),refusedWrites=failures,evictedEntries=evictions)
            print(json.dumps(dict(passed=True,quotaBytes=quota,payloadBytes=payload.stat().st_size,trials=outcomes)))


if __name__ == '__main__': unittest.main()
