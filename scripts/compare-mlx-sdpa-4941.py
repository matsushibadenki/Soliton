"""Compare two isolated MLX builds with deterministic SDPA and decode workloads."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import resource
import statistics
import subprocess
import sys
import time

BASE = "58d91ee81a86dd9710123ec42e7ff91f6d1d21b1"
PATCH = "720859602efef9242b7105268976958957764c29"


def instrument(source):
    path = source / "mlx/backend/metal/scaled_dot_product_attention.cpp"
    text = path.read_text()
    if '"SDPA_ROUTE %s %d' in text:
        return
    text = text.replace("#include <algorithm>", "#include <algorithm>\n#include <cstdio>\n#include <cstdlib>", 1)
    marker = "  check_kernel_threadgroup_size(kernel, group_dims, hash_name);"
    index = text.index(marker, text.index("void sdpa_vector_2pass("))
    text = text[:index] + '''  if (std::getenv("SOLITON_SDPA_TRACE")) {
    std::fprintf(stderr, "SDPA_ROUTE %s %d\\n", kname.c_str(), N);
  }
''' + text[index:]
    path.write_text(text)


def file_digest(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def check_runtime(mx):
    source = Path(os.environ["SOLITON_MLX_TREE"])
    binary = Path(mx.__file__).resolve()
    if not binary.is_relative_to(source.resolve()):
        raise RuntimeError(f"Unexpected MLX import: {binary}")
    metal = sorted((source / "python/mlx").rglob("mlx.metallib"))
    if len(metal) != 1:
        raise RuntimeError(f"Expected one packaged Metal library, got {metal}")
    return {
        "coreSha256": file_digest(binary),
        "metalSha256": file_digest(metal[0]),
        "device": mx.device_info(mx.gpu),
        "os": os.uname().release,
        "version": mx.__version__,
    }


def operation(mx, args):
    rows = []
    for dim, heads, kv_heads in [(256, 8, 4), (512, 32, 4)]:
        for length in args.lengths:
            if dim == 256 and length != 4096:
                continue
            for dtype in args.dtypes:
                mx.reset_peak_memory()
                mx.random.seed(4941)
                shapes = [(1, heads, 1, dim), (1, kv_heads, length, dim)]
                q, k, v = [mx.random.uniform(-0.5, 0.5, shape=shape).astype(getattr(mx, dtype))
                           for shape in (shapes[0], shapes[1], shapes[1])]
                mx.eval(q, k, v)

                def evaluate():
                    return mx.fast.scaled_dot_product_attention(
                        q, k, v, scale=dim ** -0.5)

                os.environ["SOLITON_SDPA_TRACE"] = "1"
                try:
                    output = evaluate()
                    mx.eval(output)
                finally:
                    os.environ.pop("SOLITON_SDPA_TRACE")
                for _ in range(3):
                    mx.eval(evaluate())
                samples = []
                for _ in range(args.repetitions):
                    start = time.perf_counter_ns()
                    mx.eval(evaluate())
                    samples.append((time.perf_counter_ns() - start) / 1e6)
                peak = mx.get_peak_memory()
                import numpy as np

                key, value = [np.asarray(x.astype(mx.float32))[0] for x in (k, v)]
                query = np.asarray(q.astype(mx.float32))[0, :, 0].astype(np.float64)
                reference = []
                for start in range(0, heads, heads // kv_heads):
                    kv = start // (heads // kv_heads)
                    scores = query[start:start + heads // kv_heads] @ key[kv].astype(np.float64).T
                    scores *= dim ** -0.5
                    weights = np.exp(scores - scores.max(axis=-1, keepdims=True))
                    reference.extend((weights / weights.sum(axis=-1, keepdims=True)) @ value[kv])
                result = np.asarray(output.astype(mx.float32))[0, :, 0]
                if not np.isfinite(result).all():
                    raise RuntimeError("Nonfinite attention output")
                error = float(np.max(np.abs(reference - result)))
                if error > 0.001:
                    raise RuntimeError(f"SDPA reference error exceeds 0.001: {error}")
                rows.append({"headDim": dim, "kvLength": length, "dtype": dtype,
                             "operationMs": samples, "peakMlxBytes": peak,
                             "maxAbsoluteError": error,
                             "output": result.tolist()})
                del q, k, v, key, value, output
                mx.clear_cache()
    return rows


def recall_prompt(tokenizer, limit, fixture, *, include_message=False):
    codes = ("58316", "72940", "41683", "95724")
    fields = ("North Transfer handoff code", "East Yard locker PIN",
              "South Depot inventory number", "West Terminal seal number")
    field = fields[fixture % len(fields)]
    code = codes[fixture % len(codes)]
    position = (15, 75, 40, 90)[fixture % len(codes)]

    def render(count):
        split = count * position // 100
        lines = [f"Record {i:05d}: archive item inspected and returned to storage."
                 for i in range(count)]
        lines.insert(split, f"Record TARGET: the {field} is {code}.")
        content = ("Use the archive to answer the question below.\n" + "\n".join(lines)
                   + f"\nWhat is the {field}? Reply with the digits only.")
        message = {"role": "user", "content": content}
        tokens = tokenizer.apply_chat_template(
            [message], add_generation_prompt=True, enable_thinking=False, tokenize=True,
        )
        return tokens, message

    low, high = 0, limit
    tokens, message = render(0)
    while low <= high:
        middle = (low + high) // 2
        candidate, candidate_message = render(middle)
        if len(candidate) <= limit:
            tokens, message = candidate, candidate_message
            low = middle + 1
        else:
            high = middle - 1
    if include_message:
        return tokens, code, position, message
    return tokens, code, position


def model(mx, args):
    from mlx_lm import load
    from mlx_lm.generate import generate_step

    path = args.model.resolve(strict=True)
    loaded, tokenizer = load(str(path))
    limit = args.context - args.output_tokens
    expected = position = None
    if args.chat_recall:
        tokens, expected, position = recall_prompt(tokenizer, limit, args.fixture)
        if not limit - 32 <= len(tokens) <= limit:
            raise RuntimeError("Chat template prompt missed the context target")
        prompt = mx.array(tokens)
    else:
        suffix = tokenizer.encode("\nThe handoff code is 7429. What is the handoff code? Answer with digits.",
                                  add_special_tokens=False)
        fill = tokenizer.encode(" reference", add_special_tokens=False)
        needed = limit - len(suffix)
        if needed < 1 or not fill:
            raise ValueError("Prompt exceeds context budget")
        prompt = mx.array((fill * ((needed + len(fill) - 1) // len(fill)))[:needed] + suffix)
    prompt_hash = hashlib.sha256(json.dumps(prompt.tolist()).encode()).hexdigest()
    results = []
    for _ in range(args.repetitions):
        mx.clear_cache()
        mx.reset_peak_memory()
        started = time.perf_counter()
        tokens, timestamps = [], []
        for token, _ in generate_step(prompt, loaded, max_tokens=args.output_tokens,
                                      prefill_step_size=512):
            tokens.append(int(token))
            timestamps.append(time.perf_counter())
            # Observe decode dispatch outside the first-token/prefill phase.
            if len(tokens) == 1:
                os.environ["SOLITON_SDPA_TRACE"] = "1"
            elif len(tokens) == 3:
                os.environ.pop("SOLITON_SDPA_TRACE", None)
        os.environ.pop("SOLITON_SDPA_TRACE", None)
        if len(tokens) != args.output_tokens:
            raise RuntimeError(f"Generated {len(tokens)} tokens instead of {args.output_tokens}")
        eos = next((i for i, token in enumerate(tokens) if token in tokenizer.eos_token_ids), len(tokens))
        answer = tokenizer.decode(tokens[:eos]).strip()
        results.append({"inputTokens": prompt.size, "promptSha256": prompt_hash,
                        "answer": answer, "expected": expected, "factPositionPercent": position,
                        "correct": answer == expected if expected else None, "tokenIds": tokens, "text": tokenizer.decode(tokens),
                        "timeToFirstTokenSeconds": timestamps[0]-started,
                        "decodeTokensPerSecond": (len(tokens)-1)/(timestamps[-1]-timestamps[0]),
                        "completionSeconds": timestamps[-1]-started, "peakMlxBytes": mx.get_peak_memory(),
                        "peakProcessRssBytes": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss})
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--instrument-source", type=Path, nargs="+")
    parser.add_argument("--base", type=Path)
    parser.add_argument("--patched", type=Path)
    parser.add_argument("--dependencies", type=Path, help="Isolated MLX-LM site-packages directory")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--lengths", type=int, nargs="+", default=[4096, 16384, 32768, 60000])
    parser.add_argument("--dtypes", choices=("bfloat16", "float16", "float32"), nargs="+",
                        default=("bfloat16", "float16", "float32"))
    parser.add_argument("--order", choices=("abba", "baab"), default="abba")
    parser.add_argument("--model", type=Path)
    parser.add_argument("--context", type=int, default=4096)
    parser.add_argument("--chat-recall", action="store_true", help="Use the checkpoint's actual chat template")
    parser.add_argument("--candidate", choices=("full", "bf16"), default="full")
    parser.add_argument("--cycles", type=int, choices=(1, 2), default=1)
    parser.add_argument("--fixture", type=int, default=0, help=argparse.SUPPRESS)
    parser.add_argument("--output-tokens", type=int, default=32)
    parser.add_argument("--repetitions", type=int, default=7)
    parser.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.instrument_source:
        for source in args.instrument_source:
            instrument(source)
        return
    if args.repetitions < 1 or args.output_tokens < 4:
        parser.error("Use at least one repetition and four output tokens")
    if not args.model and any(length < 4096 for length in args.lengths):
        parser.error("Use at least 4,096 keys to exercise the generic two-pass route")
    if args.worker:
        import mlx.core as mx
        runtime = check_runtime(mx)
        rows = model(mx, args) if args.model else operation(mx, args)
        print(json.dumps({"runtime": runtime, "rows": rows}))
        return
    if not all((args.base, args.patched, args.dependencies, args.output)) or args.output.exists():
        parser.error("Specify both trees, dependencies and a new output file")
    header = Path("mlx/backend/metal/kernels/sdpa_vector.h")
    baseline = (args.base / header).read_text()
    patched = (args.patched / header).read_text()
    expected = baseline.replace(
        "template <typename T, int D, int V = D, int TK = 4>",
        "// Larger TK spills registers for head dim 512 (up to 3.6x slower on M5 Max).\n"
        "template <typename T, int D, int V = D, int TK = (D + V > 512 ? 2 : 4)>",
        1,
    )
    if args.candidate == "bf16":
        expected = expected.replace("D + V > 512 ?", "D + V > 512 && metal::is_same_v<T, bfloat16_t> ?")
    if expected == baseline or expected != patched:
        raise RuntimeError(f"The Metal header does not match the {args.candidate} variant of PR #4941")
    dispatch = Path("mlx/backend/metal/scaled_dot_product_attention.cpp")
    if (args.base / dispatch).read_bytes() != (args.patched / dispatch).read_bytes():
        raise RuntimeError("The dispatch source differs between arms")
    runs = []
    order = ["base" if lane == "a" else "patched" for lane in args.order]
    for index, arm in enumerate(order * args.cycles):
        source = getattr(args, arm).resolve()
        env = {**os.environ, "PYTHONPATH": os.pathsep.join(map(str, (source/"python", args.dependencies.resolve()))),
               "SOLITON_MLX_TREE": str(source), "MLX_ENABLE_TF32": "0", "HF_HUB_OFFLINE": "1"}
        env.pop("SOLITON_SDPA_TRACE", None)
        start = time.monotonic()
        if args.chat_recall:
            env.pop("MLX_SDPA_D512_MIN_KL", None)
        fixture = index // 2 if args.chat_recall else args.fixture
        worker = subprocess.run([sys.executable, __file__, *sys.argv[1:], "--worker", "--fixture", str(fixture)],
                                env=env, text=True, capture_output=True)
        if worker.returncode:
            raise RuntimeError(worker.stderr[-6000:])
        data = json.loads(worker.stdout.splitlines()[-1])
        data["routes"] = re.findall(r"^SDPA_ROUTE (\S+) (\d+)$", worker.stderr, re.M)
        data.update(arm=arm, elapsedSeconds=time.monotonic()-start)
        if not args.model:
            for row, route in zip(data["rows"], data["routes"], strict=True):
                if not (route[0].startswith("sdpa_vector_2pass_1_") and "_gqa_" not in route[0]
                        and int(route[1]) == row["kvLength"]):
                    raise RuntimeError(f"Unexpected dispatch: {route}")
        runs.append(data)
        print(f"{arm}: {len(data['rows'])} observations", flush=True)
    for arm in ("base", "patched"):
        hashes = {(r["runtime"]["coreSha256"], r["runtime"]["metalSha256"]) for r in runs if r["arm"]==arm}
        if len(hashes)!=1:
            raise RuntimeError(f"Runtime changed between {arm} trials")
    if runs[0]["runtime"]["metalSha256"] == runs[1]["runtime"]["metalSha256"]:
        raise RuntimeError("Metal library is identical in both arms")
    summary = []
    if args.model:
        references = {}
        for run in runs:
            for row in run["rows"]:
                references.setdefault(row["promptSha256"], set()).add(tuple(row["tokenIds"]))
        summary = [{"tokenIdsMatch": all(len(tokens) == 1 for tokens in references.values())}]
        if args.chat_recall:
            for old, new in zip(runs[::2], runs[1::2], strict=True):
                if [r["promptSha256"] for r in old["rows"]] != [r["promptSha256"] for r in new["rows"]]:
                    raise RuntimeError("Chat prompts differ between paired arms")
            for run in runs:
                if not any(name.startswith("sdpa_vector_2pass_1_") and name.endswith("_bfloat16_t_512_512")
                           for name, _ in run["routes"]):
                    raise RuntimeError("Model did not use the generic D512 route with standard settings")
        for arm in ("base", "patched"):
            rows = [row for run in runs if run["arm"] == arm for row in run["rows"]]
            summary[0][arm] = {
                key: statistics.median(row[key] for row in rows)
                for key in ("timeToFirstTokenSeconds", "decodeTokensPerSecond", "completionSeconds")
            }
            if args.chat_recall:
                summary[0][arm].update(correct=sum(row["correct"] for row in rows), trials=len(rows),
                                       peakMlxBytes=max(row["peakMlxBytes"] for row in rows),
                                       peakProcessRssBytes=max(row["peakProcessRssBytes"] for row in rows))
    else:
        for index, row in enumerate(runs[0]["rows"]):
            arms = [[r["rows"][index] for r in runs if r["arm"] == arm] for arm in ("base", "patched")]
            medians = [statistics.median(v for sample in arm for v in sample["operationMs"]) for arm in arms]
            max_delta = max(abs(a-b) for arm in arms for sample in arm
                            for ref, out in zip(row["output"], sample["output"], strict=True)
                            for a, b in zip(ref, out, strict=True))
            summary.append({"headDim": row["headDim"], "kvLength": row["kvLength"], "dtype": row["dtype"],
                            "baseMedianMs": medians[0], "patchedMedianMs": medians[1],
                            "maxPairedOutputDelta": max_delta, "speedup": medians[0]/medians[1]})
        for run in runs:
            for row in run["rows"]:
                output = row.pop("output")
                row["outputSha256"] = hashlib.sha256(json.dumps(output).encode()).hexdigest()
    metadata = {
        "baseRevision": BASE, "patchCommit": PATCH, "runs": runs, "summary": summary,
        "sdpaD512MinKvLengthOverride": None if args.chat_recall else os.environ.get("MLX_SDPA_D512_MIN_KL"),
        "chatRecall": args.chat_recall, "candidate": args.candidate, "order": args.order.upper(), "cycles": args.cycles,
        "metricScope": "whole model" if args.model else "synchronized SDPA operation",
    }
    if args.model:
        metadata.update(modelConfigSha256=file_digest(args.model / "config.json"),
                        context=args.context, outputTokens=args.output_tokens)
    args.output.write_text(json.dumps(metadata, indent=2) + "\n")


if __name__ == "__main__":
    main()
