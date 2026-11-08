"""Explicit isolated real-model probe; never issues or edits qualification evidence."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import urllib.error
import urllib.request

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--isolated', action='store_true', required=True)
p.add_argument('--controller', type=Path, required=True)
p.add_argument('--executable', type=Path, required=True)
p.add_argument('--model', type=Path, required=True)
p.add_argument('--output', type=Path, required=True)
p.add_argument('--port', type=int, default=19233)
a = p.parse_args()
assert a.port not in (8080, 8081) and not a.output.exists()
sys.path.insert(0, str(a.controller.resolve()))
from vllm_apple.backend import BackendConfig, BackendProcess

os.environ.update(PYTORCH_ENABLE_MPS_FALLBACK='1', VLLM_APPLE_KV_CACHE_MIB='1536',
                  VLLM_APPLE_ALLOCATOR_CACHE_MIB='512')
report = {'passed': False, 'certification': False, 'scope': 'Direct private text backend contract, not performance or vision qualification'}
hits = []
class Capture(BaseHTTPRequestHandler):
    def do_GET(self):
        hits.append(self.path)
        self.send_response(500)
        self.end_headers()
    def log_message(self, *_):
        pass
capture = ThreadingHTTPServer(('127.0.0.1', 0), Capture)
thread = threading.Thread(target=capture.serve_forever, daemon=True)
thread.start()
backend = BackendProcess(BackendConfig(str(a.model.resolve()), a.executable.resolve(),
                                      port=a.port, max_model_len=4096, backend_kind='mlx_lm'))
def request(body):
    req = urllib.request.Request(backend.base_url + '/v1/chat/completions',
                                 json.dumps(body).encode(), {'Content-Type': 'application/json'})
    with urllib.request.urlopen(req, timeout=180) as response:
        raw = response.read(4 * 1024**2 + 1)
    assert len(raw) <= 4 * 1024**2
    return raw
body = {'model': 'default_model', 'messages': [{'role': 'user', 'content':
        'The fictional code word is soliton. ' * 20 + 'What is the code word? Answer briefly.'}],
        'max_tokens': 16, 'temperature': 0}
try:
    backend.start()
    first = json.loads(request(body))
    expected = first['choices'][0]['message']['content']
    assert 'soliton' in expected.lower()
    image_url = f'http://127.0.0.1:{capture.server_port}/must-not-fetch'
    invalid = [[], [{'role': 'user', 'content': [{'type': 'image_url', 'image_url': {'url': image_url}}]}],
               [{'role': 'user', 'content': [{'type': 'image_url', 'image_url': {'url': 'data:image/png;base64,AAAA'}}]}],
               [{'role': 'user', 'content': [{'type': 'text', 'text': 'plain'}]}],
               [{'role': 'assistant', 'content': '', 'tool_calls': []}]]
    report['refusals'] = []
    for messages in invalid:
        try:
            request({**body, 'messages': messages})
            raise AssertionError('Unsupported messages accepted')
        except urllib.error.HTTPError as error:
            assert error.code == 400
            report['refusals'].append(error.code)
    assert hits == [], 'URL inliner ran before text-profile validation'
    raw = request({**body, 'stream': True, 'stream_options': {'include_usage': True}}).decode()
    text, usage, done = '', None, False
    for line in raw.splitlines():
        if not line.startswith('data:'):
            continue
        data = line[5:].strip()
        if data == '[DONE]':
            done = True
            continue
        event = json.loads(data)
        for choice in event.get('choices', []):
            text += choice.get('delta', {}).get('content', '')
        usage = event.get('usage') or usage
    assert done and text == expected and usage['prompt_tokens'] == first['usage']['prompt_tokens']
    assert first['usage']['prompt_tokens_details']['cached_tokens'] == 0
    assert usage['prompt_tokens_details']['cached_tokens'] == usage['prompt_tokens'] - 1
    with urllib.request.urlopen(backend.base_url + '/vllm-apple/kv-cache', timeout=10) as response:
        kv = json.load(response)
    assert kv['max_bytes'] == 1536 * 1024**2 and kv['retained_bytes'] <= kv['max_bytes']
    assert kv['allocator']['configured_limit_bytes'] == 512 * 1024**2
    report.update(passed=True, first=first, repeated={'answer': text, 'usage': usage},
                  urlFetches=len(hits), kv=kv,
                  wrapperSha256=hashlib.sha256((a.controller/'vllm_apple/mlx_kv_budget.py').read_bytes()).hexdigest())
except Exception as error:
    report['error'] = str(error)
    raise
finally:
    backend.stop()
    capture.shutdown()
    capture.server_close()
    thread.join(timeout=2)
    report['shutdownClean'] = not backend.running
    report['passed'] &= report['shutdownClean']
    with a.output.open('x') as output:
        json.dump(report, output, indent=2)
        output.write('\n')
    a.output.chmod(0o600)
print(json.dumps({'passed': report['passed'], 'urlFetches': len(hits), 'shutdownClean': report['shutdownClean']}))
