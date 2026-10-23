"""Opt-in real tokenizer ID parity. No weights, downloads, or model processes.
Run with the qualified backend's Python and the local model directory.
"""
import ast
import hashlib
import json
import os
from pathlib import Path
import random
import sys

os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"
root = Path(sys.argv[1]).resolve()
source = Path(__file__).resolve().parents[1] / "crates/studio-core/src/token_budget.py"
# Load the selector without running the worker or replacing any upstream imports.
node = next(n for n in ast.parse(source.read_text()).body if isinstance(n, ast.FunctionDef) and n.name == "qualified_tokenizer")
namespace = {"Path": Path, "json": json}
exec(compile(ast.Module(body=[node], type_ignores=[]), str(source), "exec"), namespace)
fast = namespace["qualified_tokenizer"](root)
assert fast is not None, "model/version is not in the measured implementation allowlist"
assert not any(k == "transformers" or k.startswith(("torch", "mlx")) for k in sys.modules), "heavy dependency imported by fast path"
# Changing a sidecar, serialized vocabulary, or dependency must leave the fast path.
import shutil
import subprocess
import tempfile
from unittest.mock import patch
with tempfile.TemporaryDirectory(prefix="soliton-parity-") as directory:
    copy = Path(directory)
    for name in ["tokenizer.json", "tokenizer_config.json", "tokenizer.model", "special_tokens_map.json"]:
        shutil.copyfile(root / name, copy / name)
    # Exercise the actual framed worker, then invalidate its loaded serialization.
    worker = subprocess.Popen([sys.executable, "-I", "-u", "-c", source.read_text(), str(copy), "--soliton-token-counter"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        worker.stdin.write(b'{"id":1,"request":{"prompt":"hello"}}\n'); worker.stdin.flush()
        assert json.loads(worker.stdout.readline()) == {"id": 1, "prompt_tokens": 2}
        (copy / "chat_template.jinja").write_text("unknown override")
        worker.stdin.write(b'{"id":2,"request":{"prompt":"hello"}}\n'); worker.stdin.flush()
        assert worker.wait(timeout=10) != 0
        assert worker.stdout.read() == b""
        assert b"tokenizer files changed" in worker.stderr.read()
    finally:
        if worker.poll() is None:
            worker.kill(); worker.wait()
        worker.stdin.close(); worker.stdout.close(); worker.stderr.close()
    (copy / "chat_template.jinja").write_text("unknown override")
    assert namespace["qualified_tokenizer"](copy) is None
    (copy / "chat_template.jinja").unlink()
    with patch("importlib.metadata.version", return_value="unqualified"):
        assert namespace["qualified_tokenizer"](copy) is None
    for name in ["tokenizer.json", "tokenizer_config.json", "tokenizer.model", "special_tokens_map.json"]:
        with (copy / name).open("ab") as file:
            file.write(b" ")
        assert namespace["qualified_tokenizer"](copy) is None, name
        shutil.copyfile(root / name, copy / name)
from transformers import AutoTokenizer
reference = AutoTokenizer.from_pretrained(root, local_files_only=True, trust_remote_code=False)
texts = ["", "hello", "日本語\n改行", "你好，世界", "  leading trailing \n", "\t\r\n", "é e\u0301 👩🏽‍💻", "<bos><eos><start_of_turn>model\n", "{{ 7*7 }}", "\x00\u200b\u2028", "a" * 10000]
rng = random.Random(20261022)
alphabet = "abc 日本語中文\n\t😀é\u0301<>_012"
texts += ["".join(rng.choices(alphabet, k=rng.randrange(0, 1024))) for _ in range(200)]
counts = {"raw": 0, "chat": 0, "rejected": 0}
digest = hashlib.sha256()
for text in texts:
    a, b = reference.encode(text), fast.encode(text)
    assert a == b, ("raw", repr(text))
    digest.update(json.dumps(a).encode()); counts["raw"] += 1
    for messages in [[{"role": "user", "content": text}], [{"role": "user", "content": text}, {"role": "assistant", "content": "回答 answer"}, {"role": "user", "content": "次は？"}]]:
        for thinking in [False, True]:
            kwargs = dict(tokenize=True, add_generation_prompt=True, return_dict=False, tools=None, enable_thinking=thinking)
            a = reference.apply_chat_template(messages, **kwargs)
            b = fast.apply_chat_template(messages, **kwargs)
            assert a == b, ("chat", repr(messages), thinking)
            digest.update(json.dumps(a).encode()); counts["chat"] += 1
for messages in [[{"role": "system", "content": "test"}], [{"role": "assistant", "content": "test"}], [{"role": "user", "content": "one"}, {"role": "user", "content": "two"}]]:
    for tokenizer in [reference, fast]:
        try:
            tokenizer.apply_chat_template(messages, tokenize=True, add_generation_prompt=True, return_dict=False, tools=None)
        except Exception as error:
            assert "System role not supported" in str(error) or "roles must alternate" in str(error)
        else:
            raise AssertionError("invalid conversation accepted")
    counts["rejected"] += 1
print(json.dumps({"passed": True, "counts": counts, "tokenIdsSha256": digest.hexdigest(), "heavyImportsInFastPath": False, "fallbackGuards": 6}))
