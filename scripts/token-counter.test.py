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
    seen = set()
    def apply_chat_template(self, messages, **kw):
        assert kw['return_dict'] is False and kw['add_generation_prompt'] is True
        key = (repr(messages), kw.get('enable_thinking'))
        assert key not in self.seen, 'unexpected repeated tokenization'
        self.seen.add(key)
        return list(range(sum(len(m['content']) for m in messages) + int(kw.get('enable_thinking', False))))
    def encode(self, prompt):
        return list(range(len(prompt)))
m=types.ModuleType('transformers');m.AutoTokenizer=Tokenizer;sys.modules['transformers']=m
source = Path(sys.argv[3]).read_text()
# Exercise the reviewed-template path with the stub.
exec(source.replace("tokenizer = qualified_tokenizer(Path(sys.argv[1]))",
                    "tokenizer = Tokenizer()", 1))
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
                for ident, text, thinking, limit in [(1, "hello", False, 16), (2, "你好", False, 16), (3, "hello", False, 64), (4, "hello", True, 64)]:
                    process.stdin.write((json.dumps({"id": ident, "request": {"messages": [{"role": "user", "content": text}], "chat_template_kwargs": {"enable_thinking": thinking}, "max_tokens": limit}})+"\n").encode()); process.stdin.flush()
                    self.assertTrue(select.select([process.stdout], [], [], 5)[0])
                    self.assertEqual(json.loads(process.stdout.readline()), {"id": ident, "prompt_tokens": len(text) + int(thinking)})
                # A vocabulary extension without a JSON suffix must also invalidate.
                (model / "vocab.bpe").write_text("changed")
                process.stdin.write(b'{"id":5,"request":{"messages":[{"role":"user","content":"hello"}],"chat_template_kwargs":{"enable_thinking":false},"max_tokens":16}}\n'); process.stdin.flush()
                self.assertNotEqual(process.wait(timeout=5), 0)
                self.assertEqual(process.stdout.read(), b"")
            finally:
                if process.poll() is None: process.kill(); process.wait()
                process.stdin.close(); process.stdout.close()
            process = start()
            process.stdin.close()
            self.assertEqual(process.wait(timeout=5), 0)
            process.stdout.close()

    def test_nested_linked_tokenizer_file_mutation_refuses_cached_count(self):
        with tempfile.TemporaryDirectory(prefix="soliton-token-linked-") as tmp:
            root = pathlib.Path(tmp)
            model = root / "model"
            nested = model / "nested"
            nested.mkdir(parents=True)
            vocab = root / "vocab"
            vocab.write_text("initial")
            (nested / "vocab.bpe").symlink_to(vocab)
            command = [sys.executable, "-I", "-u", "-c", BOOTSTRAP,
                       str(model), "--soliton-token-counter", str(SCRIPT)]
            with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL) as worker:
                try:
                    frame = b'{"id":1,"request":{"prompt":"hello"}}\n'
                    worker.stdin.write(frame)
                    worker.stdin.flush()
                    self.assertTrue(select.select([worker.stdout], [], [], 5)[0])
                    self.assertEqual(json.loads(worker.stdout.readline())["prompt_tokens"], 5)
                    vocab.write_text("changed")
                    worker.stdin.write(frame)
                    worker.stdin.flush()
                    self.assertNotEqual(worker.wait(timeout=5), 0)
                    self.assertEqual(worker.stdout.read(), b"")
                finally:
                    if worker.poll() is None:
                        worker.kill()
                        worker.wait()

if __name__ == "__main__": unittest.main()
