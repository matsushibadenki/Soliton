"""Real-model Inkling gateway smoke. Run from the repository root with pinned local assets."""

import argparse
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
from urllib.error import HTTPError
from urllib.request import Request, urlopen

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--app", type=Path, help="Soliton.app copied from a mounted DMG")
parser.add_argument("--soak-seconds", type=int, default=0)
parser.add_argument("--output", type=Path)
args = parser.parse_args()
if args.soak_seconds < 0:
    parser.error("--soak-seconds must be nonnegative")

repo = Path.cwd()
root = repo / ".soliton-runtime/inkling-ad-iq3"
lib = ctypes.CDLL("/usr/lib/libproc.dylib")
report = {
    "model": "AD-IQ3_S",
    "runtime": "b12600",
    "context": 4096,
    "parallel": 1,
    "host": "Apple M4 / 32 GiB",
}
samples = []


def swap():
    return subprocess.check_output(
        ["/usr/sbin/sysctl", "-n", "vm.swapusage"], text=True
    ).strip()


with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    cfg = {
        "backend": "strata-inkling-metal",
        "model_name": "Inkling 2 Mini AD-IQ3_S",
        "exe": str(root / "llama-b12600/llama-server"),
        "model": str(root / "model.gguf"),
        "args": [],
    }
    (tmp / "strata-inkling.json").write_text(json.dumps(cfg))
    state = tmp / "state.json"
    state.write_text(
        json.dumps(
            {
                "settings": {
                    "engine": "strata",
                    "strataRoot": str(
                        tmp / "app-data/Strata-metal-v1"
                        if args.app
                        else repo / "Strata"
                    ),
                    "config": str(tmp / "strata-inkling.json"),
                    "mock": False,
                    "python": str(tmp / "app-data/python-3.12.16/bin/python3")
                    if args.app
                    else "python3",
                    "temperature": 0,
                    "maxTokens": 128,
                    "language": "en",
                }
            }
        )
    )
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 8080 if args.app else 0))
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
            ) as resp:
                assert resp.status == status
                return json.load(resp)
        except HTTPError as e:
            raw = e.read().decode()
            assert e.code == status, (route, e.code, raw)
            return json.loads(raw)

    def act(action):
        return req("/studio/action", {"action": action})

    def snapshot():
        return req("/studio/state")

    def until(fn, timeout=90):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                result = fn()
                if result:
                    return result
            except OSError:
                pass
            time.sleep(0.1)
        raise RuntimeError("Timed out")

    def host():
        executable = (
            args.app.resolve() / "Contents/MacOS/soliton"
            if args.app
            else repo / "target/debug/studio-server"
        )
        env = {k: v for k, v in os.environ.items() if k != "STRATA_ROOT"}
        begun = time.monotonic()
        h = subprocess.Popen(
            [executable],
            env={
                **env,
                "STUDIO_STATE_PATH": str(state),
                "STUDIO_PORT": str(port),
                "SOLITON_DATA_DIR": str(tmp / "app-data"),
            },
            stdout=subprocess.DEVNULL,
        )
        headers.pop("x-studio-token", None)
        try:
            headers["x-studio-token"] = until(lambda: req("/studio/session", {}))[
                "token"
            ]
            report.setdefault("appToSessionSeconds", []).append(
                round(time.monotonic() - begun, 3)
            )
            return h
        except BaseException:
            stop(h)
            raise

    def stop(h):
        h.send_signal(signal.SIGINT)
        h.wait(timeout=30)

    def start():
        begun = time.monotonic()
        act("start")
        view = until(ready)
        assert view["backend"] == "Inkling Metal (experimental)"
        report.setdefault("engineStarts", []).append(
            {
                "startToReadySeconds": round(time.monotonic() - begun, 3),
                "startup": view.get("startup"),
            }
        )
        return view

    def ready():
        v = snapshot()
        return v if v["status"] == "ready" else None

    def body(text, limit=128):
        return dict(
            model="inkling-2-mini",
            messages=[dict(role="user", content=text)],
            max_tokens=limit,
            temperature=0,
        )

    def desktop(text):
        with urlopen(
            Request(origin + "/studio/chat", json.dumps(body(text)).encode(), headers),
            timeout=180,
        ) as stream:
            for line in stream:
                pass
        return until(completed)

    def completed():
        view = snapshot()
        return view if not view["document"]["generating"] else None

    def disconnect(route, payload):
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=90)
        conn.request(
            "POST",
            route,
            json.dumps(payload),
            headers,
        )
        with conn.getresponse() as response:
            assert response.status == 200, response.read(300)
            for line in response:
                if line.startswith(b"data:") and line[5:].strip() != b"[DONE]":
                    choices = json.loads(line[5:]).get("choices") or []
                    delta = choices[0]["delta"] if choices else {}
                    if delta.get("content") or delta.get("reasoning_content"):
                        cancelled_on = (
                            "content" if delta.get("content") else "reasoning_content"
                        )
                        conn.sock.shutdown(socket.SHUT_RDWR)
                        break
            else:
                raise RuntimeError("No generation delta before cancellation")
        conn.close()
        return cancelled_on

    def soak():
        results = report["soak"] = {
            "requestedSeconds": args.soak_seconds,
            "completed": [],
            "cancellations": [],
            "passed": False,
        }
        begun = time.monotonic()
        while time.monotonic() - begun < args.soak_seconds:
            turn = len(results["completed"])
            code = str(7300 + turn % 100)
            query = body(
                "Reference notes:"
                + " sample" * 3820
                + f"\nRemember code {code}. Reply only READY.",
                128,
            )
            query["messages"].extend(
                [
                    {"role": "assistant", "content": "READY"},
                    {
                        "role": "user",
                        "content": "What is the remembered code? Reply with digits only.",
                    },
                ]
            )
            if turn % 5 == 0:
                cancelled = {**query, "stream": True}
                cancelled["messages"] = [
                    *query["messages"][:-1],
                    {
                        "role": "user",
                        "content": "Count upwards from 1 and separate numbers with commas.",
                    },
                ]
                phase = disconnect("/v1/chat/completions", cancelled)

                def observed_cancel():
                    rows = snapshot()["requests"]
                    return rows and rows[0]["status"] == "cancelled"

                until(observed_cancel, timeout=15)
                results["cancellations"].append({"beforeTurn": turn, "delta": phase})
            started = time.monotonic()
            reply = req("/v1/chat/completions", query)
            used = reply["usage"]["prompt_tokens"]
            answer = reply["choices"][0]["message"]["content"]
            assert 3800 <= used <= 4096 - 128 and answer.strip() == code, (
                used,
                answer,
                code,
            )
            results["completed"].append(
                {
                    "turn": turn,
                    "promptTokens": used,
                    "answer": answer,
                    "seconds": round(time.monotonic() - started, 3),
                }
            )
            if turn % 5 == 0:
                print(
                    "soak",
                    round(time.monotonic() - begun),
                    len(results["completed"]),
                    flush=True,
                )
        results["elapsedSeconds"] = round(time.monotonic() - begun, 3)
        results["passed"] = True

    report["swapBefore"] = swap()
    h = host()
    done = threading.Event()

    def monitor(pid):
        i = 0
        while not done.is_set():
            data = (ctypes.c_uint64 * 20)()
            if lib.proc_pid_rusage(pid, 2, ctypes.byref(data)) == 0:
                samples.append((data[8], data[9]))
            if i % 4 == 0:
                report.setdefault("swapSamples", []).append(swap())
            i += 1
            done.wait(0.25)

    try:
        start()
        pid = int(
            subprocess.check_output(
                ["/usr/bin/pgrep", "-P", str(h.pid), "llama-server"]
            )
        )
        watcher = threading.Thread(target=monitor, args=(pid,))
        watcher.start()
        route = "/v1/chat/completions"
        token = headers.pop("x-studio-token")
        req(route, body("Reply OK"), 401)
        headers["x-studio-token"] = token
        report["anonymousStatus"] = 401
        prompt = "Reference notes:" + " sample" * 3970 + "\nReply with OK."
        e = req(route, body(prompt, 4095), 422)
        count = int(re.search(r"prompt \((\d+)\)", e["error"]["message"])[1])
        reserve = 4096 - count
        assert 16 < reserve < 200, (count, reserve)
        req(route, body(prompt, reserve + 1), 422)
        preview = "/studio/chat/budget"
        draft = {
            "conversationId": snapshot()["document"]["conversations"]["activeId"],
            "content": prompt,
            "max_tokens": reserve,
        }
        before = snapshot()["document"]["messages"]
        assert req(preview, draft) == {
            "inputTokens": count,
            "outputTokens": reserve,
            "context": 4096,
            "fits": True,
        }
        assert req(preview, {**draft, "max_tokens": reserve + 1})["fits"] is False
        assert snapshot()["document"]["messages"] == before
        print("boundary", count, reserve, flush=True)
        b = req(route, body(prompt, reserve))
        assert b["usage"]["prompt_tokens"] == count, b["usage"]
        report["boundary"] = {
            "promptTokens": count,
            "outputReservation": reserve,
            "overStatus": 422,
            "actualUsage": b["usage"],
        }
        v = desktop("Remember code 7429. Reply only READY.")
        assert v["document"]["messages"][-1]["content"].strip() == "READY"
        cancelled_on = disconnect(
            "/studio/chat", body("Count from 1 to 1000, separated by commas.", 512)
        )
        v = until(completed)
        assert v["requests"][0]["status"] == "cancelled", v["requests"][0]["status"]
        partial = v["document"]["messages"][-1]["content"]
        assert v["document"]["messages"][-2]["role"] == "user"
        v = desktop("What code did I ask you to remember? Reply only with digits.")
        assert v["document"]["messages"][-1]["content"].strip() == "7429"
        original = [(m["role"], m["content"]) for m in v["document"]["messages"]]
        if args.soak_seconds:
            soak()
        done.set()
        watcher.join()
        stop(h)
        h = None
        h = host()
        start()
        saved = [(m["role"], m["content"]) for m in snapshot()["document"]["messages"]]
        assert saved == original
        v = desktop("What is the remembered code? Reply only with digits.")
        assert v["document"]["messages"][-1]["content"].strip() == "7429"
        act("newConversation")
        v = desktop("The code in this conversation is 8316. Reply with only that code.")
        assert (
            v["document"]["messages"][-1]["content"].strip() == "8316"
            and len(v["document"]["messages"]) == 2
        )
        report["conversation"] = {
            "cancelledOn": cancelled_on,
            "partialAnswer": partial,
            "nextAnswer": "7429",
            "restartHistoryMatched": True,
            "restartAnswer": "7429",
            "newConversationAnswer": "8316",
        }
    finally:
        done.set()
        if "watcher" in locals():
            watcher.join()
        report["swapEnd"] = swap()
        swap_readings = [
            report["swapBefore"],
            *report.get("swapSamples", []),
            report["swapEnd"],
        ]
        report["swapObservationCount"] = len(swap_readings)
        report["maxSwapUsedMiB"] = max(
            float(re.search(r"used = ([0-9.]+)M", row)[1]) for row in swap_readings
        )
        report.pop("swapSamples", None)
        report["samples"] = len(samples)
        if samples:
            report.update(
                peakRssBytes=max(r for r, _ in samples),
                peakPhysicalBytes=max(p for _, p in samples),
            )
        output = args.output or root / "gateway-report.json"
        output.write_text(json.dumps(report, indent=2) + "\n")
        if h:
            stop(h)
