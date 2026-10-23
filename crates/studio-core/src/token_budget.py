"""Local text token counting only; no weights, downloads or remote model code."""
import json
import sys
from pathlib import Path


def stamp():
    # Tokenizer files cannot silently change under a reused object. No weights read.
    root = Path(sys.argv[1])
    entries = []
    for p in sorted(root.rglob("*")):
        if p.is_file():
            st = p.stat()
            entries.append((str(p.relative_to(root)), st.st_dev, st.st_ino, st.st_size, st.st_mtime_ns, st.st_ctime_ns))
    return tuple(entries)


def qualified_tokenizer(root):
    """Exact, measured Gemma serialization only; all other files use AutoTokenizer.

    This is an implementation allowlist, not backend compatibility evidence.
    Startup evidence/version checks and per-request file stamps remain mandatory.
    No optional dependency probes are patched or disabled.
    """
    import hashlib
    from importlib.metadata import version
    expected = {
        "tokenizer.json": "3f289bc05132635a8bc7aca7aa21255efd5e18f3710f43e3cdb96bcd41be4922",
        "tokenizer_config.json": "f3a9ecd05833ba49de8432fff27b66bb061ad6a69d17df158de03dc07420e02a",
        "tokenizer.model": "61a7b147390c64585d6c3543dd6fc636906c9af3865a5548f27f31aee1d4c8e2",
        "special_tokens_map.json": "db82f8bd9b25d14f9c788e6bde64de84d42f1c2538f1c245ba6cb3e872d14b18",
    }
    names = set(expected) | {"README.md", "config.json", "model.safetensors", "model.safetensors.index.json", ".git", ".gitattributes"}
    if not {p.name for p in root.iterdir()} <= names or not set(expected) <= {p.name for p in root.iterdir()}:
        return None
    if any(version(p) != v for p, v in {
        "transformers": "5.19.0", "tokenizers": "0.23.3", "jinja2": "3.1.6"
    }.items()):
        return None
    for name, digest in expected.items():
        with (root / name).open("rb") as file:
            if hashlib.file_digest(file, "sha256").hexdigest() != digest:
                return None
    from tokenizers import Tokenizer
    from jinja2.sandbox import ImmutableSandboxedEnvironment
    from jinja2.exceptions import TemplateError
    config = json.loads((root / "tokenizer_config.json").read_text())
    environment = ImmutableSandboxedEnvironment(trim_blocks=True, lstrip_blocks=True)
    def raise_exception(message):
        raise TemplateError(message)
    environment.globals["raise_exception"] = raise_exception
    template = environment.from_string(config["chat_template"])
    backend = Tokenizer.from_file(str(root / "tokenizer.json"))

    class QualifiedGemma:
        def apply_chat_template(self, messages, **kwargs):
            # Exact pinned template uses only messages, bos_token and generation flag.
            # enable_thinking/tools are unused by this template, just as upstream.
            text = template.render(messages=messages, bos_token=config["bos_token"],
                                   add_generation_prompt=kwargs["add_generation_prompt"])
            return backend.encode(text, add_special_tokens=False).ids

        def encode(self, prompt):
            # Match the pinned Transformers 5 Gemma implementation: raw input
            # uses the serialized BOS postprocessor, regardless of config flag.
            return backend.encode(prompt, add_special_tokens=True).ids

    return QualifiedGemma()


before = stamp()
tokenizer = qualified_tokenizer(Path(sys.argv[1]))
if tokenizer is None:
    from transformers import AutoTokenizer
    tokenizer = AutoTokenizer.from_pretrained(sys.argv[1], local_files_only=True, trust_remote_code=False)
if stamp() != before:
    raise RuntimeError("tokenizer files changed during load")


def count(request):
    if stamp() != before:
        raise RuntimeError("tokenizer files changed; restart and revalidate")
    if "messages" in request:
        tokens = tokenizer.apply_chat_template(
            request["messages"], tokenize=True, add_generation_prompt=True, return_dict=False,
            tools=None, **request.get("chat_template_kwargs", {})
        )
    else:
        tokens = tokenizer.encode(request["prompt"])
    if not isinstance(tokens, list) or not all(type(token) is int for token in tokens):
        raise RuntimeError("invalid tokenizer result")
    if stamp() != before:
        raise RuntimeError("tokenizer files changed during count")
    return len(tokens)


if "--soliton-token-counter" in sys.argv:
    while True:
        line = sys.stdin.buffer.readline(2 * 1024 * 1024 + 1)
        if not line:
            break
        if len(line) > 2 * 1024 * 1024 or not line.endswith(b"\n"):
            raise RuntimeError("invalid frame")
        frame = json.loads(line)
        result = {"id": frame["id"], "prompt_tokens": count(frame["request"])}
        print(json.dumps(result), flush=True)
        del frame, result, line
else:
    print(json.dumps({"prompt_tokens": count(json.load(sys.stdin))}))
