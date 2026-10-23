"""Protocol/file-change/EOF checks with a tokenizer stub; never loads weights."""
import json
import pathlib
import select
import subprocess
import sys
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "crates/studio-core/src/token_budget.py"
BOOTSTRAP = """
import sys, types
from pathlib import Path
class Tokenizer:
    @classmethod
    def from_pretrained(cls, *args, **kw):
        assert kw == {'local_files_only': True, 'trust_remote_code': False}
        return cls()
    def apply_chat_template(self, messages, **kw):
        assert kw['return_dict'] is False and kw['add_generation_prompt'] is True
        return list(range(sum(len(m['content']) for m in messages)))
    def encode(self, prompt):
        return list(range(len(prompt)))
m=types.ModuleType('transformers');m.AutoTokenizer=Tokenizer;sys.modules['transformers']=m
exec(Path(sys.argv[3]).read_text())
"""

class CounterProtocol(unittest.TestCase):
    def test_reuse_metadata_change_and_eof(self):
        with tempfile.TemporaryDirectory(prefix="soliton-token-test-") as tmp:
            model = pathlib.Path(tmp)
            (model / "vocab.bpe").write_text("initial")
            def start():
                return subprocess.Popen([sys.executable, "-I", "-u", "-c", BOOTSTRAP, tmp, "--soliton-token-counter", str(SCRIPT)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
            process = start()
            try:
                for ident, text in [(1, "hello"), (2, "你好")]:
                    process.stdin.write((json.dumps({"id": ident, "request": {"messages": [{"role": "user", "content": text}]}})+"\n").encode()); process.stdin.flush()
                    self.assertTrue(select.select([process.stdout], [], [], 5)[0])
                    self.assertEqual(json.loads(process.stdout.readline()), {"id": ident, "prompt_tokens": len(text)})
                # A vocabulary extension without a JSON suffix must also invalidate.
                (model / "vocab.bpe").write_text("changed")
                process.stdin.write(b'{"id":3,"request":{"prompt":"hello"}}\n'); process.stdin.flush()
                self.assertNotEqual(process.wait(timeout=5), 0)
                self.assertEqual(process.stdout.read(), b"")
            finally:
                if process.poll() is None: process.kill(); process.wait()
                process.stdin.close(); process.stdout.close()
            process = start()
            process.stdin.close()
            self.assertEqual(process.wait(timeout=5), 0)
            process.stdout.close()

if __name__ == "__main__": unittest.main()
