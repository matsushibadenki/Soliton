"""Admission and numerical regressions for the bundled dense Gemma converter."""
import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest
import numpy as np

spec=importlib.util.spec_from_file_location('gemma',Path(__file__).with_name('strata-gemma3-prepare.py'))
g=importlib.util.module_from_spec(spec);spec.loader.exec_module(g)

class GemmaTests(unittest.TestCase):
    def test_layout_rejects_unsupported_features(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)
            for n in ['model.safetensors','tokenizer.model','tokenizer_config.json']: (p/n).touch()
            c={**g.EXPECTED,'rms_norm_eps':1e-6,'max_position_embeddings':32768}
            (p/'config.json').write_text(json.dumps(c)); self.assertEqual(g.inspect(p)['hidden_size'],1152)
            for key,value in [('head_dim',288),('model_type','gemma3'),('sliding_window_pattern',True),
                              ('layer_types',['full_attention']*26),('rope_scaling',{'factor':2}),('eos_token_id',[2]),('attention_bias',True)]:
                (p/'config.json').write_text(json.dumps({**c,key:value}))
                with self.assertRaises(ValueError): g.inspect(p)
    def test_refuses_bad_or_incomplete_tensor_data(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'model.safetensors'
            for content in [b'',struct.pack('<Q',2**32),struct.pack('<Q',2)+b'{}']:
                p.write_bytes(content)
                with self.assertRaises(ValueError): g.tensors(p)
    def test_shared_tokenizer_outside_embedding_vocab_is_ignored(self):
        tokens=[b'zero',b'one',b'two']; scores=[0,0,0]; types=[1,1,1]
        g.apply_added_tokens({'added_tokens_decoder':{'1':{'content':'<end_of_turn>','special':True},
            '2':{'content':'</s>','special':False}, '262144':{'content':'<image>','special':True}}},tokens,scores,types)
        self.assertEqual(tokens,[b'zero',b'<end_of_turn>',b'</s>'])
        self.assertEqual(types,[1,3,3])
        with self.assertRaises(ValueError):
            g.apply_added_tokens({'added_tokens_decoder':{'-1':{'content':'bad'}}},tokens,scores,types)
    def test_gemma_norm_offset_includes_qk_and_all_four_norms(self):
        a=np.array([-0.5,0.25],dtype=np.float32)
        for n in ['model.norm.weight','input_layernorm.weight','post_attention_layernorm.weight',
                  'pre_feedforward_layernorm.weight','post_feedforward_layernorm.weight','q_norm.weight','k_norm.weight']:
            np.testing.assert_array_equal(g.normalized(n,a),[0.5,1.25])
        np.testing.assert_array_equal(g.normalized('q_proj.weight',a),a)

if __name__=='__main__': unittest.main()
