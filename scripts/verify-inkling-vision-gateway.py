"""Run the qualified Inkling vision candidate against a dedicated Soliton state file."""

import argparse
import base64
import ctypes
import http.client
import json
import os
import re
import signal
import socket
import subprocess
import tempfile
import threading
import time
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

repo = Path.cwd()
root = repo / ".soliton-runtime/inkling-ad-iq3"
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument(
    "--image", type=Path, required=True, help="A local PNG of a solid red region"
)
parser.add_argument("--seconds", type=int, default=1800)
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
if args.seconds < 0 or args.output.exists():
    parser.error("Use a nonnegative duration and a new report path")
png = args.image.resolve(strict=True)
data = "data:image/png;base64," + base64.b64encode(png.read_bytes()).decode()
report = {
    "host": "Apple M4 / 32 GiB",
    "model": "AD-IQ3_S",
    "projector": "F16",
    "context": 4096,
    "parallel": 1,
    "cases": [],
    "cancellations": [],
}


def swap():
    return float(
        re.search(
            r"used = ([0-9.]+)M",
            subprocess.check_output(
                ["/usr/sbin/sysctl", "-n", "vm.swapusage"], text=True
            ),
        )[1]
    )


report["initialSwapMiB"] = swap()
done = threading.Event()
samples = []
swaps = []
lib = ctypes.CDLL("/usr/lib/libproc.dylib")
with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    assets = tmp / "assets"
    assets.mkdir(mode=0o700)
    config = tmp / "inkling.json"
    config.write_text(
        json.dumps(
            {
                "backend": "strata-inkling-metal",
                "exe": str(root / "llama-b12600/llama-server"),
                "model": str(root / "model.gguf"),
                "args": [],
                "mmproj": str(root / "mmproj-f16.gguf"),
            }
        )
    )
    state = tmp / "state.json"
    state.write_text(
        json.dumps(
            {
                "settings": {
                    "engine": "strata",
                    "strataRoot": str(repo / "Strata"),
                    "config": str(config),
                    "mock": False,
                    "python": "python3",
                    "temperature": 0,
                    "maxTokens": 128,
                    "language": "en",
                    "omni": {"mediaRoot": str(assets)},
                }
            }
        )
    )
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    origin = f"http://127.0.0.1:{port}"
    headers = {"Origin": origin, "Content-Type": "application/json"}

    def req(route, body=None, status=200):
        try:
            with urlopen(
                Request(
                    origin + route,
                    None if body is None else json.dumps(body).encode(),
                    headers,
                ),
                timeout=240,
            ) as response:
                assert status == response.status, (status, response.status)
                return json.load(response)
        except HTTPError as e:
            reason = e.read().decode()
            assert status == e.code, (route, e.code, reason)
            return json.loads(reason)

    def poll(fn, seconds=120):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            try:
                r = fn()
                if r:
                    return r
            except URLError:
                pass
            time.sleep(0.1)
        raise TimeoutError("expected state")

    def monitor(pid):
        i = 0
        while not done.is_set():
            info = (ctypes.c_uint64 * 20)()
            if lib.proc_pid_rusage(pid, 2, ctypes.byref(info)) == 0:
                samples.append((info[8], info[9]))
            if i % 4 == 0:
                swaps.append(swap())
            i += 1
            done.wait(0.25)

    diag = "/v1/chat/completions/input_tokens"
    chat = "/v1/chat/completions"

    def request(history, cap=128):
        return {
            "model": "inkling-2-mini",
            "messages": history,
            "temperature": 0,
            "max_tokens": cap,
        }

    def text(role, content):
        return {"role": role, "content": content}

    question = "What is the dominant color of this image? Answer with the English color name only."

    def image(content):
        return text(
            "user",
            [
                {"type": "text", "text": content},
                {"type": "image_url", "image_url": {"url": data}},
            ],
        )

    def exchange(history, cap=128, expected=None):
        body = request(history, cap)
        before = req(diag, body)
        assert before["fits"], before
        reply = req(chat, body)
        got = reply["usage"]["prompt_tokens"]
        answer = (reply["choices"][0]["message"]["content"] or "").strip()
        assert got == before["inputTokens"], (before, got)
        if expected:
            assert answer.lower().strip(".") == expected, answer
        return {"inputTokens": got, "outputReservation": cap, "answer": answer}

    def cancel(history):
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=120)
        try:
            conn.request(
                "POST", chat, json.dumps({**request(history), "stream": True}), headers
            )
            with conn.getresponse() as r:
                assert r.status == 200, r.read(350)
                for line in r:
                    if line.startswith(b"data:") and line[5:].strip() != b"[DONE]":
                        frame = json.loads(line[5:])
                        for choice in frame.get("choices") or []:
                            if any(
                                choice.get("delta", {}).get(name)
                                for name in ("reasoning_content", "content")
                            ):
                                conn.sock.shutdown(socket.SHUT_RDWR)
                                return
                raise RuntimeError("no generation delta")
        finally:
            conn.close()

    with (root / "vision-gateway.log").open("w") as log:
        host = subprocess.Popen(
            [repo / "target/debug/studio-server"],
            env={
                **os.environ,
                "STUDIO_PORT": str(port),
                "STUDIO_STATE_PATH": str(state),
            },
            stdout=log,
            stderr=log,
        )
        try:
            headers["x-studio-token"] = poll(lambda: req("/studio/session", {}))[
                "token"
            ]
            token = headers.pop("x-studio-token")
            req(diag, request([image(question)]), 401)
            headers["x-studio-token"] = token
            began = time.monotonic()
            req("/studio/action", {"action": "start"})
            poll(lambda: req("/studio/state")["status"] == "ready")
            report["startSeconds"] = time.monotonic() - began
            pid = int(
                subprocess.check_output(
                    ["/usr/bin/pgrep", "-P", str(host.pid), "llama-server"]
                )
            )
            thread = threading.Thread(target=monitor, args=(pid,))
            thread.start()
            seed = exchange([image(question)], expected="red")
            history = [
                image(question),
                text("assistant", seed["answer"]),
                text(
                    "user",
                    "What color was the original picture? Reply with the color only.",
                ),
            ]
            follow = exchange(history, expected="red")
            report["short"] = [seed, follow]
            current = req("/studio/state")["document"]
            preview = req(
                "/studio/chat/budget",
                {
                    "conversationId": current["conversations"]["activeId"],
                    "content": question,
                    "max_tokens": 128,
                    "media": {"path": str(png), "kind": "image"},
                },
            )
            assert preview["inputTokens"] == seed["inputTokens"] and not list(
                assets.iterdir()
            )
            report["composerPreview"] = preview
            long = history[:-1] + [
                text(
                    "user", "Notes:" + " sample" * 3940 + "\n" + history[-1]["content"]
                )
            ]
            n = req(diag, request(long))["inputTokens"]
            cap = 4096 - n
            assert 1 <= cap <= 256, (n, cap)
            assert not req(diag, request(long, cap + 1))["fits"]
            req(chat, request(long, cap + 1), 422)
            report["boundary"] = exchange(long, cap)
            print("boundary and short", report["boundary"], report["short"], flush=True)
            began = time.monotonic()
            while time.monotonic() - began < args.seconds:
                index = len(report["cases"])
                near = history[:-1] + [
                    text(
                        "user",
                        f"Note {index}:"
                        + " sample" * 3780
                        + "\n"
                        + history[-1]["content"],
                    )
                ]
                if index % 5 == 0:
                    cancel(
                        near[:-1]
                        + [
                            text(
                                "user",
                                near[-1]["content"] + " Then count from 1 to 500.",
                            )
                        ]
                    )
                    poll(
                        lambda: (rows := req("/studio/state")["requests"])
                        and rows[0]["status"] == "cancelled",
                        15,
                    )
                    report["cancellations"].append(index)
                report["cases"].append(exchange(near, expected="red"))
                if index % 5 == 0:
                    print(
                        "soak", round(time.monotonic() - began), index + 1, flush=True
                    )
            report["elapsedSeconds"] = time.monotonic() - began
            report["passed"] = True
        finally:
            done.set()
            if "thread" in locals():
                thread.join()
            report["finalSwapMiB"] = swap()
            report["peakSwapMiB"] = max(
                [report["initialSwapMiB"], report["finalSwapMiB"], *swaps]
            )
            report["swapSamples"] = len(swaps)
            if samples:
                report.update(
                    peakRssBytes=max(a for a, _ in samples),
                    peakPhysicalBytes=max(b for _, b in samples),
                    memorySamples=len(samples),
                )
            args.output.write_text(json.dumps(report, indent=2) + "\n")
            host.send_signal(signal.SIGINT)
            host.wait(timeout=30)
