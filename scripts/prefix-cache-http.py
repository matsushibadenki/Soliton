"""Explicit opt-in, authenticated loopback MLX HTTP experiment (not production).
Requires reviewed MLX-LM 0.32.0, a local Gemma 2 2B model and Rust prefix-store CLI.
"""
import argparse
import hashlib
import importlib
import importlib.metadata
import importlib.util
import io
import json
import os
import platform
from pathlib import Path
import secrets
import shutil
import subprocess
import tempfile
import threading
import sys
from http.server import ThreadingHTTPServer
from prefix_cache_roundtrip_support import digest_files
from prefix_cache_contract import PROFILE, BINDING_SCHEMA, identity_binding, binding_digest, verify_evidence
from prefix_cache_writes import CommittedPrefixes

REVIEWED = {'server': '57392576de114af80ede723976b9dd156dc234f856b9943b8b44b8b78bc27f79',
            'generate': '4a3bf57f5679dac73791f069268866ab5b9336e58582d436b31133984e4b6971',
            'models.cache': 'c96cb169fd5e0ec4584a162dd61dc62ab08ccdca03a9850f8a491ce6809145e6'}


class RequestAdmission:
    """One inference at a time; bounded grace for a prior HTTP handler's teardown.

    A client may consume Content-Length bytes before the serving thread returns
    through finally. Immediate try-lock can reject that client's next serial call.
    No request retries or parallel model generation are introduced.
    """
    def __init__(self):
        self.lock = threading.Lock()

    def acquire(self):
        return self.lock.acquire(timeout=0.1)

    def release(self):
        self.lock.release()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--model', required=True, type=Path)
    parser.add_argument('--cache-root', required=True, type=Path)
    parser.add_argument('--quota-mib', type=int, default=256)
    parser.add_argument('--port', type=int, default=8082)
    parser.add_argument('--store-executable', type=Path)
    parser.add_argument('--evidence', type=Path)
    parser.add_argument('--qualification-mode', action='store_true')
    parser.add_argument('--verify-only', action='store_true')
    args = parser.parse_args()
    token = os.environ.get('SOLITON_PREFIX_HTTP_TOKEN', '')
    if len(token) < 24 or not token.isascii() or any(not 33 <= ord(c) <= 126 for c in token):
        parser.error('Set SOLITON_PREFIX_HTTP_TOKEN to a private token of at least 24 characters')
    if not 64 <= args.quota_mib <= 4096 or not 1024 <= args.port <= 65535:
        parser.error('Quota must be 64–4096 MiB and port 1024–65535')
    model_path = args.model.resolve(strict=True)
    if not args.model.is_absolute() or not model_path.is_dir():
        parser.error('Select an absolute local model directory')
    config = json.loads((model_path / 'config.json').read_text())
    if config.get('model_type') != 'gemma2' or config.get('num_hidden_layers') != 26 or config.get('hidden_size') != 2304:
        parser.error('This experiment is limited to the reviewed Gemma 2 2B layout')
    root = args.cache_root
    if not root.is_absolute():
        parser.error('Cache root must be absolute')
    root.mkdir(mode=0o700, parents=False, exist_ok=True)
    if root.is_symlink() or not root.is_dir() or root.stat().st_mode & 0o077:
        parser.error('Cache root must be a real owner-only directory')
    os.environ['HF_HUB_OFFLINE'] = '1'
    os.environ['TRANSFORMERS_OFFLINE'] = '1'
    import mlx.core as mx
    import mlx_lm
    from mlx_lm import server
    from mlx_lm.models import cache as cache_module
    if importlib.metadata.version('mlx-lm') != '0.32.0':
        parser.error('Requires reviewed MLX-LM 0.32.0')
    for name, expected in REVIEWED.items():
        module = importlib.import_module('mlx_lm.' + name)
        if hashlib.sha256(Path(module.__file__).read_bytes()).hexdigest() != expected:
            parser.error('Backend source changed; review cache integration before use')
    executable = args.store_executable or (Path(__file__).resolve().parents[1] / 'target/debug/examples/prefix-store')
    executable = executable.resolve(strict=True)
    if not executable.is_file():
        parser.error('Build cargo example prefix-store first')
    assets = [p for p in model_path.iterdir() if p.is_file() and p.suffix in {'.json', '.safetensors', '.model', '.txt'}]
    dependency_files = []
    dependency_versions = {}
    for package in ['mlx', 'transformers', 'tokenizers', 'jinja2', 'numpy', 'safetensors']:
        specification = importlib.util.find_spec(package)
        if specification and (specification.origin or specification.submodule_search_locations):
            package_root = Path(specification.origin).parent if specification.origin else Path(next(iter(specification.submodule_search_locations)))
            dependency_files.extend(p for p in package_root.rglob('*') if p.is_file() and p.suffix in {'.py','.so','.dylib'})
            dependency_versions[package] = importlib.metadata.version(package)
        else:
            dependency_versions[package] = 'missing'
    if len(dependency_files) > 20000:
        parser.error('Runtime dependency inventory exceeds the reviewed limit')
    identity = dict(model=digest_files(assets), tokenizer=digest_files([p for p in assets if p.suffix != '.safetensors']),
                    runtime=hashlib.sha256((digest_files(dependency_files + list(Path(mlx_lm.__file__).parent.rglob('*.py')) + [Path(mx.__file__), Path(sys.executable), executable, Path(__file__), Path(__file__).with_name('prefix_cache_roundtrip_support.py'), Path(__file__).with_name('prefix_cache_contract.py'), Path(__file__).with_name('prefix_cache_writes.py')]) + json.dumps({k:v for k,v in os.environ.items() if k.startswith(('MLX_', 'METAL_', 'VECLIB_', 'OMP_', 'TOKENIZERS_')) or k=='PYTORCH_ENABLE_MPS_FALLBACK'}, sort_keys=True) + json.dumps(dependency_versions,sort_keys=True) + platform.machine() + str(platform.mac_ver()) + importlib.metadata.version('mlx') + subprocess.check_output(['/usr/sbin/sysctl','-n','hw.model','hw.memsize','machdep.cpu.brand_string'],text=True).strip()).encode()).hexdigest(),
                    format='mlx-lm/0.32.0/gemma2-2b/http-prefix-v1')
    quota = args.quota_mib * 1024**2
    cache_binding, observed_device = identity_binding(identity, root, quota)
    binding = binding_digest(cache_binding)
    if args.qualification_mode and args.evidence:
        parser.error('Qualification mode cannot use an evidence file')
    if not args.qualification_mode:
        if not args.evidence:
            parser.error('Normal startup requires --evidence; explicit --qualification-mode is for isolated certification only')
        verify_evidence(args.evidence, binding, quota)
    if args.verify_only:
        print(json.dumps(dict(identity=binding,profile=PROFILE,binding_schema=BINDING_SCHEMA,cache_binding=cache_binding,cache_device_observed=observed_device,verified=not args.qualification_mode)))
        return
    mx.set_cache_limit(256 * 1024**2)
    stats = dict(hits=0, misses=0, writes=0, errors=0, skippedWrites=0, serializedBytes=0, committedBytes=0)
    committed = CommittedPrefixes(root / 'store')
    context = threading.local()
    def disk(action, key, path, folder):
        manifest = folder / 'identity.json'
        manifest.write_text(json.dumps(key))
        result = subprocess.run([str(executable), str(root / 'store'), str(quota), action, str(manifest), str(path)],
                                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=20)
        if result.returncode != 0:
            return None
        if len(result.stdout) > 4096:
            raise ValueError('Invalid store receipt')
        return json.loads(result.stdout)
    def identity_for(tokens):
        return dict(identity, prefix=hashlib.sha256(json.dumps(tokens, separators=(',', ':')).encode()).hexdigest())

    class Cache(cache_module.LRUPromptCache):
        def fetch_nearest_cache(self, model, tokens):
            if len(tokens) > 512 or len(tokens) < 2:
                raise ValueError('This experiment requires 2–512 input tokens')
            context.tokens = list(tokens)
            context.saved = False
            state, rest = super().fetch_nearest_cache(model, tokens)
            if state is not None and rest:
                return state, rest
            try:
                with tempfile.TemporaryDirectory(dir=root, prefix='stage-') as temp:
                    folder = Path(temp)
                    path = folder / 'state.safetensors'
                    key = identity_for(tokens[:-1])
                    receipt = disk('get', key, path, folder)
                    if receipt is not None:
                        restored, metadata = cache_module.load_prompt_cache(str(path), return_metadata=True)
                        if metadata.get('prefix') != identity_for(tokens[:-1])['prefix'] or len(restored) != 26 or any(getattr(c, 'offset', None) != len(tokens)-1 for c in restored):
                            raise ValueError('Restored cache shape/offset mismatch')
                        stats['hits'] += 1
                        committed.remember(key['prefix'], receipt)
                        context.saved = True
                        return restored, tokens[-1:]
            except Exception:
                stats['errors'] += 1
            stats['misses'] += 1
            return None, tokens

    original_generate = server.stream_generate
    def generate(*positional, **kwargs):
        old_progress = kwargs.get('prompt_progress_callback')
        state = kwargs.get('prompt_cache')
        tokens = getattr(context, 'tokens', None)
        def progress(processed, total):
            if old_progress:
                old_progress(processed, total)
            if not tokens or getattr(context, 'saved', True) or processed != total-1:
                return
            context.saved = True
            key = identity_for(tokens[:-1])
            if committed.unchanged(key['prefix']):
                stats['skippedWrites'] += 1
                return
            try:
                size = sum(c.nbytes for c in state)
                if not 0 < size < min(quota//3, 1024**3) or shutil.disk_usage(root).free < 2*size + 16*1024**2:
                    return
                if len(state) != 26 or any(getattr(c, 'offset', None) != len(tokens)-1 for c in state):
                    return
                with tempfile.TemporaryDirectory(dir=root, prefix='stage-') as temp:
                    folder = Path(temp)
                    path = folder / 'state.safetensors'
                    cache_module.save_prompt_cache(str(path), state, {'prefix': key['prefix']})
                    size = path.stat().st_size
                    stats['serializedBytes'] += size
                    receipt = disk('put', key, path, folder)
                    if receipt is not None:
                        stats['writes'] += 1
                        stats['committedBytes'] += size
                        committed.remember(key['prefix'], receipt)
                    else:
                        stats['errors'] += 1
            except Exception:
                stats['errors'] += 1
        kwargs['prompt_progress_callback'] = progress
        try:
            yield from original_generate(*positional, **kwargs)
        finally:
            context.tokens = None
            context.saved = True
    server.stream_generate = generate

    class Provider(server.ModelProvider):
        def load(self, model_path_arg, adapter_path=None, draft_model_path=None):
            if model_path_arg not in {'default_model', str(model_path)} or adapter_path is not None or draft_model_path not in {None, 'default_model'}:
                raise ValueError('Dynamic models/adapters are disabled')
            result = super().load(str(model_path), None, None)
            self.is_batchable = False
            return result
    admission = RequestAdmission()
    class Handler(server.APIHandler):
        def authorized(self):
            return secrets.compare_digest(self.headers.get('Authorization', '').encode('utf-8'), ('Bearer ' + token).encode('utf-8'))
        def reply(self, status, payload):
            data = json.dumps(payload).encode()
            self.send_response(status)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(data)))
            self.send_header('Connection', 'close')
            self.end_headers()
            self.wfile.write(data)
            self.close_connection = True
        def do_GET(self):
            if not self.authorized():
                return self.reply(401, {'error': 'Authentication required'})
            if self.path == '/v1/models':
                return self.reply(200, {'object':'list','data':[{'id':'default_model','object':'model','owned_by':'local'}]})
            if self.path == '/health':
                return self.reply(200, {'status':'ok','model':'default_model','inference_ready':True,'identity':binding})
            if self.path == '/studio/ssd-cache':
                return self.reply(200, dict(stats, identity=binding, profile=PROFILE, binding_schema=BINDING_SCHEMA, cache_binding=cache_binding, cache_device_observed=observed_device, quota_bytes=quota, experimental=args.qualification_mode, productionIntegration=not args.qualification_mode))
            return self.reply(404, {'error': 'Not found'})
        def do_POST(self):
            self.connection.settimeout(30)
            if not self.authorized():
                return self.reply(401, {'error': 'Authentication required'})
            if self.path not in {'/v1/chat/completions','/tokenize'}:
                return self.reply(404, {'error': 'Not found'})
            if not admission.acquire():
                self.log_message('SSD admission busy after bounded handoff grace')
                return self.reply(503, {'error': 'One request at a time'})
            try:
                lengths = self.headers.get_all('Content-Length', [])
                if len(lengths) != 1 or self.headers.get('Transfer-Encoding') or not 0 < int(lengths[0]) <= 65536:
                    return self.reply(400, {'error': 'Invalid request size/framing'})
                raw = self.rfile.read(int(lengths[0]))
                body = json.loads(raw)
                if not isinstance(body, dict) or set(body)-{'model','messages','stream','max_tokens','temperature','seed','stream_options'}:
                    return self.reply(400, {'error': 'Unsupported request fields'})
                if body.get('model','default_model') != 'default_model' or body.get('temperature',0) != 0 or body.get('seed',0) != 0 or type(body.get('max_tokens',32)) is not int or not 1 <= body.get('max_tokens',32) <= 128:
                    return self.reply(400, {'error': 'Requires default_model, greedy seed 0 and 1–128 output tokens'})
                if type(body.get('stream', False)) is not bool:
                    return self.reply(400, {'error': 'stream must be a boolean'})
                messages = body.get('messages')
                if not isinstance(messages,list) or not 1 <= len(messages) <= 32 or any(not isinstance(m,dict) or set(m)!={'role','content'} or m['role'] not in {'system','user','assistant'} or not isinstance(m['content'],str) for m in messages):
                    return self.reply(400, {'error': 'Text messages only'})
                if body.get('stream_options', {'include_usage':True}) not in ({'include_usage':True}, {'include_usage':False}):
                    return self.reply(400, {'error':'Unsupported stream options'})
                input_tokens = provider.tokenizer.apply_chat_template(messages, tokenize=True, add_generation_prompt=True)
                if len(input_tokens) < 2 or len(input_tokens) + body.get('max_tokens',32) > 512:
                    return self.reply(422, {'error':'Prompt plus output exceeds the qualified 512-token context'})
                if self.path == '/tokenize':
                    return self.reply(200, {'count':len(input_tokens),'context':512,'identity':binding})
                body.update(seed=0, temperature=0, max_tokens=body.get('max_tokens',32))
                encoded = json.dumps(body).encode()
                self.headers.replace_header('Content-Length',str(len(encoded)))
                self.rfile = io.BytesIO(encoded)
                return super().do_POST()
            except (ValueError, TypeError, TimeoutError):
                return self.reply(400, {'error': 'Invalid request'})
            finally:
                admission.release()
    cli = argparse.Namespace(model=str(model_path),adapter_path=None,draft_model=None,num_draft_tokens=0,trust_remote_code=False,
        chat_template='',use_default_chat_template=False,temp=0.0,top_p=1.0,top_k=0,min_p=0,max_tokens=128,
        chat_template_args={},prompt_cache_size=1,prompt_cache_bytes=None,pipeline=False,allowed_origins=[],
        decode_concurrency=1,prompt_concurrency=1,prefill_step_size=512,kv_bits=None,kv_group_size=64,quantized_kv_start=5000)
    provider = Provider(cli)
    provider.load('default_model')
    generator = server.ResponseGenerator(provider, Cache(max_size=1,max_bytes=64*1024**2))
    try:
        server._run_http_server('127.0.0.1',args.port,generator,server_class=ThreadingHTTPServer,handler_class=Handler)
    finally:
        generator.stop_and_join()


if __name__ == '__main__':
    main()
