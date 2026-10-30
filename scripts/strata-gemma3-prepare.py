"""Soliton's opt-in Strata dense Metal path. Local BF16/F16 -> GGUF; no torch/downloads."""
import hashlib
import json
import math
import os
from pathlib import Path
import struct
import sys

# Deliberately bounded to the tested 1B text layout; never reinterpret arbitrary Gemma models.
EXPECTED = dict(bos_token_id=2, eos_token_id=[1,106], pad_token_id=0, model_type='gemma3_text', architectures=['Gemma3ForCausalLM'],
                hidden_size=1152, intermediate_size=6912, num_hidden_layers=26,
                num_attention_heads=4, num_key_value_heads=1, head_dim=256,
                vocab_size=262144, max_position_embeddings=32768, sliding_window=512, sliding_window_pattern=6,
                rope_theta=1000000, rope_local_base_freq=10000,
                query_pre_attn_scalar=256, hidden_activation='gelu_pytorch_tanh')


def inspect(folder):
    folder = Path(folder)
    c = json.loads((folder / 'config.json').read_text())
    for key, value in EXPECTED.items():
        if c.get(key) != value or isinstance(c.get(key), bool):
            raise ValueError('Unsupported Gemma 3 layout: ' + key)
    for key in ['rope_scaling', 'attn_logit_softcapping', 'final_logit_softcapping']:
        if c.get(key) is not None:
            raise ValueError('Unsupported Gemma 3 option: ' + key)
    if c.get('attention_bias', False) or c.get('rms_norm_eps') != 1e-6:
        raise ValueError('Unsupported Gemma 3 attention/norm settings')
    layers = c.get('layer_types')
    expected_layers = ['full_attention' if (i+1) % 6 == 0 else 'sliding_attention' for i in range(26)]
    if layers is not None and layers != expected_layers:
        raise ValueError('Inconsistent Gemma 3 attention pattern')
    for name in ['model.safetensors', 'tokenizer.model', 'tokenizer_config.json']:
        if not (folder / name).is_file():
            raise ValueError('Missing local model file: ' + name)
    return c


def tensors(path):
    """Validate every tensor range and expected shape before creating any output."""
    with path.open('rb') as f:
        raw = f.read(8)
        if len(raw) != 8:
            raise ValueError('Truncated safetensors header')
        length = struct.unpack('<Q', raw)[0]
        if not 2 <= length <= 16*1024*1024:
            raise ValueError('Invalid safetensors header size')
        header = json.loads(f.read(length))
    expected = {'model.embed_tokens.weight': [262144,1152], 'model.norm.weight':[1152]}
    for i in range(26):
        base = f'model.layers.{i}.'
        for name, shape in {'input_layernorm':[1152], 'post_attention_layernorm':[1152],
                            'pre_feedforward_layernorm':[1152], 'post_feedforward_layernorm':[1152],
                            'self_attn.q_norm':[256], 'self_attn.k_norm':[256],
                            'self_attn.q_proj':[1024,1152], 'self_attn.k_proj':[256,1152],
                            'self_attn.v_proj':[256,1152], 'self_attn.o_proj':[1152,1024],
                            'mlp.gate_proj':[6912,1152], 'mlp.up_proj':[6912,1152],
                            'mlp.down_proj':[1152,6912]}.items():
            expected[base+name+'.weight'] = shape
    if set(header)-{'__metadata__'} != set(expected):
        raise ValueError('Missing or unexpected Gemma 3 tensors')
    end = path.stat().st_size - 8 - length
    ranges = []
    for name, shape in expected.items():
        t = header[name]
        start, stop = t['data_offsets']
        width = {'BF16':2, 'F16':2, 'F32':4}.get(t['dtype'])
        if t['shape'] != shape or width is None or start < 0 or stop > end or stop-start != math.prod(shape)*width:
            raise ValueError('Invalid tensor shape/range: ' + name)
        ranges.append((start, stop))
    cursor = 0
    for start, stop in sorted(ranges):
        if start != cursor:
            raise ValueError('Overlapping or missing tensor data')
        cursor = stop
    if cursor != end:
        raise ValueError('Truncated/trailing safetensors data')
    return header, 8+length


def normalized(name, data):
    # HF Gemma RMSNorm stores the delta from 1.0 (including Q/K norms).
    return data + 1.0 if name.endswith('norm.weight') else data


def apply_added_tokens(tc, tokens, scores, types):
    n = len(tokens)
    for token_id, item in tc.get('added_tokens_decoder', {}).items():
        i = int(token_id)
        if i < 0:
            raise ValueError('Negative added token ID')
        if i >= n:
            continue  # Official shared tokenizer includes tokens outside the 1B embedding table.
        tokens[i] = item['content'].encode()
        scores[i] = -1000.0
        types[i] = 3 if item.get('special') or item['content'] == '</s>' else 4


def convert(folder, output, root):
    import numpy as np
    from sentencepiece import SentencePieceProcessor
    sys.path.insert(0, str(root / 'third_party/gemma3-gguf'))
    import gguf
    c = inspect(folder)
    source = folder / 'model.safetensors'
    header, offset = tensors(source)
    w = gguf.GGUFWriter(output, 'gemma3')
    w.add_name('Gemma 3 1B IT - Strata dense Metal')
    for method, value in [('context_length',c['max_position_embeddings']), ('embedding_length',1152),
                          ('block_count',26), ('feed_forward_length',6912), ('head_count',4),
                          ('head_count_kv',1), ('key_length',256), ('value_length',256),
                          ('layer_norm_rms_eps',1e-6), ('rope_freq_base',1000000.0),
                          ('rope_freq_base_swa',10000.0), ('sliding_window',512),
                          ('sliding_window_pattern',6), ('file_type',1)]:
        getattr(w, 'add_'+method)(value)
    sp = SentencePieceProcessor(model_file=str(folder / 'tokenizer.model'))
    n = c['vocab_size']
    if sp.vocab_size() != n:
        raise ValueError('Unexpected Gemma vocabulary size')
    tokens = [sp.id_to_piece(i).encode() for i in range(n)]
    scores = [sp.get_score(i) for i in range(n)]
    types = [2 if sp.is_unknown(i) else 3 if sp.is_control(i) else 5 if sp.is_unused(i) else 6 if sp.is_byte(i) else 1 for i in range(n)]
    tc = json.loads((folder/'tokenizer_config.json').read_text())
    apply_added_tokens(tc, tokens, scores, types)
    w.add_tokenizer_model('llama'); w.add_tokenizer_pre('default')
    w.add_token_list(tokens); w.add_token_scores(scores); w.add_token_types(types)
    gguf.SpecialVocab(folder, n_vocab=n).add_to_gguf(w)
    # SpecialVocab does not interpret HF's EOS list. Preserve both stop tokens explicitly.
    w.add_eos_token_id(1)
    w.add_eot_token_id(106)
    w.add_add_space_prefix(False)
    mapping = gguf.get_tensor_name_map(gguf.MODEL_ARCH.GEMMA3,26)
    ordered = [(name,t) for name,t in header.items() if name != '__metadata__']
    for name,t in ordered:
        dest = mapping.get_name(name, try_suffixes=(".weight",))
        if dest is None:
            raise ValueError('Unmapped tensor: '+name)
        dtype = np.dtype('float32' if len(t['shape']) == 1 else 'float16')
        w.add_tensor_info(dest,t['shape'],dtype,math.prod(t['shape'])*dtype.itemsize)
    w.write_header_to_file(); w.write_kv_data_to_file(); w.write_ti_data_to_file()
    with source.open('rb') as f:
        for i,(name,t) in enumerate(ordered):
            start,end = t['data_offsets']; f.seek(offset+start)
            raw = f.read(end-start)
            if t['dtype'] == 'BF16':
                data = (np.frombuffer(raw,dtype='<u2').astype(np.uint32)<<16).view(np.float32)
            else:
                data = np.frombuffer(raw,dtype='<f2' if t['dtype']=='F16' else '<f4').astype(np.float32)
            data = normalized(name,data).astype(np.float32 if len(t['shape'])==1 else np.float16).reshape(t['shape'])
            if not np.isfinite(data).all():
                raise ValueError('Non-finite Gemma tensor: '+name)
            w.write_tensor_data(data)
            print(f'[Gemma 3] {i+1}/{len(ordered)} {name}',flush=True)
    w.close()


def main():
    root,stage,data = map(Path,sys.argv[1:4])
    if len(sys.argv)>4 and sys.argv[4]=='--inspect':
        inspect(stage); tensors(stage/'model.safetensors'); return
    engine=root/'build-metal/strata-gemma-server'
    manifest=json.loads((root/'.bundled-gemma3.json').read_text())
    if hashlib.sha256(engine.read_bytes()).hexdigest()!=manifest['sha256']:
        raise ValueError('Bundled Gemma backend checksum mismatch')
    data.mkdir(parents=True,exist_ok=True)
    out=data/'gemma3-f16.gguf'; partial=data/'gemma3-f16.gguf.partial'
    # Atomic publication: cancelled/failed conversion never produces a usable config.
    try:
        convert(stage,partial,root)
        os.replace(partial,out)
    finally:
        partial.unlink(missing_ok=True)
    cfg={'backend':'strata-gemma3-metal','model_name':'google/gemma-3-1b-it',
         'exe':str(engine),'cwd':str(root),'model':str(out),'source_model':str((stage/'model.safetensors').resolve()),
         'args':['--model',str(out),'--ctx-size','2048','--parallel','1','--n-gpu-layers','99',
                 '--jinja','--alias','strata-gemma3','--no-webui','--no-context-shift'],
         'llama_commit':manifest['llama_commit']}
    target=root/'strata-gemma3-1b.json'; tmp=target.with_suffix('.json.partial')
    tmp.write_text(json.dumps(cfg,indent=2)); os.replace(tmp,target)
    print('[Gemma 3] Ready: '+str(target),flush=True)

if __name__=='__main__':
    main()
