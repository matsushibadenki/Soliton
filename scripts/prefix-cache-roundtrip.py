"""Opt-in real MLX cache round-trip through the Rust SSD store (no server changes).
Run with an existing MLX-LM Python: ... scripts/prefix-cache-roundtrip.py --model /absolute/model
"""
import argparse
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import subprocess
import tempfile


def digest_files(files):
    digest = hashlib.sha256()
    for path in sorted(files):
        digest.update(path.name.encode())
        with path.open('rb') as file:
            for chunk in iter(lambda: file.read(4 * 1024 * 1024), b''):
                digest.update(chunk)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--model', required=True, type=Path)
    args = parser.parse_args()
    model_path = args.model.resolve(strict=True)
    if not args.model.is_absolute() or not model_path.is_dir():
        parser.error('An existing absolute local model folder is required')
    for key in ['HF_HUB_OFFLINE', 'TRANSFORMERS_OFFLINE']:
        os.environ[key] = '1'
    import mlx.core as mx
    import mlx_lm
    from mlx_lm import load, stream_generate
    from mlx_lm.models.cache import make_prompt_cache, save_prompt_cache, load_prompt_cache
    from mlx_lm.sample_utils import make_sampler
    root = Path(__file__).resolve().parents[1]
    executable = root / 'target/debug/examples/prefix-store'
    if not executable.is_file():
        parser.error('Build first: cargo build -p studio-core --example prefix-store')
    assets = [p for p in model_path.iterdir() if p.is_file() and p.suffix in {'.json', '.model', '.safetensors', '.txt'}]
    model_hash = digest_files(assets)
    tokenizer_hash = digest_files([p for p in assets if p.suffix != '.safetensors'])
    runtime_files = list(Path(mlx_lm.__file__).parent.rglob('*.py')) + [Path(mx.__file__)]
    runtime_hash = digest_files(runtime_files)
    model, tokenizer = load(str(model_path), tokenizer_config={'trust_remote_code': False}, trust_remote_code=False)
    mx.set_cache_limit(256 * 1024 * 1024)
    prompt = tokenizer.apply_chat_template([{'role': 'user', 'content': 'The fictional code word is soliton. ' * 20 + 'What is the code word? Answer briefly.'}], tokenize=True, add_generation_prompt=True)
    prefix = prompt[:-1]
    identity = dict(model=model_hash, tokenizer=tokenizer_hash, runtime=runtime_hash,
                    prefix=hashlib.sha256(json.dumps(prefix, separators=(',', ':')).encode()).hexdigest(),
                    format='mlx-lm/' + importlib.metadata.version('mlx-lm') + '/prompt-cache-v1')
    cache = make_prompt_cache(model)
    mx.eval(model(mx.array([prefix]), cache=cache))
    with tempfile.TemporaryDirectory(prefix='soliton-prefix-') as temp:
        folder = Path(temp)
        original, restored = folder / 'original.safetensors', folder / 'restored.safetensors'
        save_prompt_cache(str(original), cache, {'prefix_sha256': identity['prefix']})
        identity_path = folder / 'identity.json'
        identity_path.write_text(json.dumps(identity))
        quota = max(16384, original.stat().st_size * 3)
        for action, payload in [('put', original), ('get', restored)]:
            subprocess.run([str(executable), str(folder / 'store'), str(quota), action, str(identity_path), str(payload)], check=True)
        def file_hash(path):
            with path.open('rb') as file:
                return hashlib.file_digest(file, 'sha256').hexdigest()
        assert file_hash(original) == file_hash(restored)
        restored_cache, metadata = load_prompt_cache(str(restored), return_metadata=True)
        assert metadata['prefix_sha256'] == identity['prefix']
        def generate(tokens, state=None):
            mx.random.seed(0)
            return [part.token for part in stream_generate(model, tokenizer, prompt=tokens, prompt_cache=state, max_tokens=16, sampler=make_sampler(temp=0))]
        cold, warm = generate(prompt), generate(prompt[-1:], restored_cache)
        assert cold and cold == warm, 'Restored cache changed the generated token IDs'
        assert model_hash == digest_files(assets), 'Source model changed'
        print(json.dumps(dict(result='passed', prefixTokens=len(prefix), comparedOutputTokens=len(cold), payloadBytes=original.stat().st_size,
                              modelSha256=model_hash, runtimeSha256=runtime_hash, productionIntegration=False)))


if __name__ == '__main__':
    main()
