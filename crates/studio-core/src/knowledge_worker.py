"""Offline MLX adapter experiment. Invoked only by the Rust job supervisor.

The model directory is read-only input. Only the experiment directory is written.
Evaluation prompts are never included in the training data.
"""
import contextlib
import hashlib
import importlib.metadata
import json
import math
from pathlib import Path
import sys
import time
from types import SimpleNamespace

PROTOCOL = sys.stdout


def emit(kind, **values):
    print(json.dumps({"kind": kind, **values}, ensure_ascii=False), file=PROTOCOL, flush=True)


def fingerprint(model):
    files = sorted(p for p in model.iterdir() if p.is_file() and p.suffix in {".json", ".safetensors", ".model"})
    if not any(p.suffix == ".safetensors" for p in files):
        raise ValueError("A local MLX Safetensors model is required (GGUF is not supported).")
    digest = hashlib.sha256()
    for path in files:
        digest.update(path.name.encode())
        with path.open("rb") as source:
            for chunk in iter(lambda: source.read(4 * 1024 * 1024), b""):
                digest.update(chunk)
    return digest.hexdigest()


def run(folder):
    draft = json.loads((folder / "manifest.json").read_text())["draft"]
    source = Path(draft["model"]).resolve(strict=True)
    if not source.is_dir() or not (source / "config.json").is_file():
        raise ValueError("Select a local MLX model folder with config.json.")
    emit("phase", phase="checking")
    original = fingerprint(source)
    # Imports stay inside run so dataset/scoring tests do not need an accelerator.
    import mlx.core as mx
    from mlx_lm import load, stream_generate
    from mlx_lm.sample_utils import make_sampler
    from mlx_lm.lora import CONFIG_DEFAULTS, train_model
    from mlx_lm.tuner.datasets import load_dataset
    from mlx_lm.tuner.callbacks import TrainingCallback

    class Progress(TrainingCallback):
        def on_train_loss_report(self, report):
            if not math.isfinite(report["train_loss"]):
                raise ValueError("Training loss became non-finite. Reduce the learning rate; this adapter is not usable.")
            emit("progress", iteration=report["iteration"], loss=report["train_loss"], peakMemoryGb=report["peak_memory"])

    mx.random.seed(0)
    mx.set_cache_limit(256 * 1024 * 1024)
    model, tokenizer = load(str(source), tokenizer_config={"trust_remote_code": False}, trust_remote_code=False)
    if not tokenizer.chat_template:
        raise ValueError("This workflow requires a model with a chat template.")
    if draft["layers"] > len(model.layers):
        raise ValueError("Requested adapter layers exceed this model's layer count.")

    def prompt_for(prompt, response=None):
        messages = [{"role": "user", "content": prompt}]
        if response is not None:
            messages.append({"role": "assistant", "content": response})
        return tokenizer.apply_chat_template(messages, tokenize=True, add_generation_prompt=response is None)

    # Do not silently truncate training answers or evaluation questions.
    for example in draft["training"]:
        if len(prompt_for(example["prompt"], example["response"])) > 512:
            raise ValueError("A training example exceeds the 512-token training limit.")
    for case in draft["evaluation"]:
        if len(prompt_for(case["prompt"])) > 512:
            raise ValueError("An evaluation question exceeds 512 tokens.")

    def evaluate(phase):
        emit("phase", phase=phase)
        model.eval()
        for index, case in enumerate(draft["evaluation"]):
            mx.random.seed(0)
            started = time.monotonic()
            text, tokens, finish = "", 0, None
            for result in stream_generate(model, tokenizer, prompt=prompt_for(case["prompt"]),
                                          max_tokens=draft["maxTokens"], sampler=make_sampler(temp=0)):
                text += result.text
                tokens = result.generation_tokens
                finish = result.finish_reason
            emit("answer", phase=phase, index=index, text=text[:16000], tokens=tokens,
                 truncated=len(text) > 16000 or finish != "stop",
                 elapsedMs=round((time.monotonic() - started) * 1000))

    evaluate("baseline")
    emit("phase", phase="training")
    data = folder / "data"
    data.mkdir()
    records = [{"messages": [{"role": "user", "content": row["prompt"]},
                              {"role": "assistant", "content": row["response"]}]} for row in draft["training"]]
    (data / "train.jsonl").write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in records))
    args = dict(CONFIG_DEFAULTS)
    args.update(model=str(source), train=True, data=str(data), num_layers=draft["layers"],
                batch_size=1, iters=draft["steps"], learning_rate=draft["learningRate"],
                adapter_path=str(folder / "adapter"), max_seq_length=512, mask_prompt=True,
                steps_per_report=1, steps_per_eval=draft["steps"] + 1,
                save_every=draft["steps"], val_batches=0, grad_checkpoint=True, seed=0,
                trust_remote_code=False, report_to=None)
    args = SimpleNamespace(**args)
    with contextlib.redirect_stdout(sys.stderr):
        train_set, valid_set, _ = load_dataset(args, tokenizer)
        train_model(args, model, train_set, valid_set, Progress())
    # Reload the saved adapter; evaluation therefore covers the deliverable on disk.
    del model
    import gc
    gc.collect()
    mx.clear_cache()
    model, tokenizer = load(str(source), adapter_path=str(folder / "adapter"),
                            tokenizer_config={"trust_remote_code": False}, trust_remote_code=False)
    evaluate("candidate")
    emit("phase", phase="verifying")
    if fingerprint(source) != original:
        raise ValueError("Source model changed during the experiment; results are invalid.")
    adapter = folder / "adapter" / "adapters.safetensors"
    if not adapter.is_file():
        raise ValueError("Training did not produce an adapter.")
    if not all(bool(mx.all(mx.isfinite(tensor)).item()) for tensor in mx.load(str(adapter)).values()):
        raise ValueError("Adapter contains non-finite weights; results are invalid.")
    digest = hashlib.sha256()
    with adapter.open("rb") as file:
        for chunk in iter(lambda: file.read(4 * 1024 * 1024), b""):
            digest.update(chunk)
    # A successful process is not a quality verdict. Rust computes all score gates.
    emit("complete", sourceSha256=original,
         adapterSha256=digest.hexdigest(),
         versions={name: importlib.metadata.version(name) for name in ("mlx", "mlx-lm")})


if __name__ == "__main__":
    try:
        run(Path(sys.argv[1]).resolve(strict=True))
    except Exception as exc:
        emit("error", message=str(exc)[:2000])
        sys.exit(1)
