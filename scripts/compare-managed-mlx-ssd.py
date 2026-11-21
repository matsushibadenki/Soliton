"""Compare two qualified Gemma 2 profiles through an isolated, authenticated Soliton gateway on macOS."""

import argparse
from contextlib import contextmanager
import ctypes
from hashlib import sha256
from http.client import HTTPConnection
import json
import math
import os
import re
from pathlib import Path
import secrets
import signal
import socket
from statistics import median
import subprocess
import sys
import tempfile
import threading
import time
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from host_load_guard import HostLoadGuard

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "vLLM-apple"))
from vllm_apple.hardware import detect_memory


MIB = 1024**2
ROUTE = "/v1/chat/completions"


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class Footprint:
    """Sample Darwin physical footprint of the gateway and its current descendants."""

    def __init__(self, root):
        self.root = root
        self.lib = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
        self.peak = self.samples = 0
        self.stop = threading.Event()
        self.error = None

    def tree(self):
        pending, seen = [self.root], set()
        while pending:
            pid = pending.pop()
            if pid in seen:
                continue
            seen.add(pid)
            children = (ctypes.c_int * 256)()
            self.lib.proc_listchildpids(pid, children, ctypes.sizeof(children))
            if children[-1]:
                raise RuntimeError("Process tree exceeded sampler bound")
            pending.extend(child for child in children if child > 0)
        return seen

    def sample(self):
        pids = self.tree()
        total = 0
        for pid in pids:
            # Darwin rusage_info_v2: 16-byte UUID followed by 18 uint64s.
            info = (ctypes.c_uint64 * 20)()
            if self.lib.proc_pid_rusage(pid, 2, ctypes.byref(info)) != 0:
                if pid in self.tree():
                    raise OSError(ctypes.get_errno(), "proc_pid_rusage")
                return  # A short-lived store process exited during this sample.
            total += info[9]
        if total <= 0 or len(pids) < 2:
            raise RuntimeError("Missing live inference process")
        if pids != self.tree():
            return
        self.peak = max(total, self.peak)
        self.samples += 1

    def monitor(self):
        try:
            while not self.stop.wait(0.025):
                self.sample()
        except Exception as exc:
            self.error = exc

    def __enter__(self):
        self.sample()
        self.thread = threading.Thread(target=self.monitor, daemon=True)
        self.thread.start()
        return self

    def __exit__(self, *_):
        self.stop.set()
        self.thread.join()
        if self.error:
            raise self.error
        if self.samples == 0:
            raise RuntimeError("No complete physical-footprint samples")


def worker_limits(root):
    # Read only the diagnostic socket belonging to this gateway's source-guarded MLX worker.
    for pid in Footprint(root).tree():
        command = subprocess.run(
            ["/bin/ps", "-p", str(pid), "-o", "command="],
            capture_output=True,
            text=True,
        ).stdout
        port = re.search(r"--port (\d+)", command)
        if "mlx_kv_budget.py" in command and port:
            with urlopen(
                f"http://127.0.0.1:{port[1]}/vllm-apple/kv-cache", timeout=5
            ) as response:
                return json.load(response)
    raise RuntimeError("Managed MLX telemetry worker not found")


class Busy(Exception):
    """Admission refused before streaming began, so a new QA conversation can retry."""


class Gateway:
    def __init__(self, port, pid):
        self.port = port
        self.pid = pid
        self.origin = f"http://127.0.0.1:{port}"
        self.headers = {"Origin": self.origin, "Content-Type": "application/json"}

    def api(self, path, data=None):
        body = None if data is None else json.dumps(data).encode()
        try:
            with urlopen(
                Request(self.origin + path, body, self.headers), timeout=120
            ) as response:
                return json.load(response)
        except HTTPError as err:
            raise RuntimeError(f"{path}: {err.code}: {err.read(600).decode()}") from err

    def action(self, name, **kwargs):
        return self.api("/studio/action", {"action": name, **kwargs})

    def start(self):
        started = time.monotonic()
        self.action("start")
        while time.monotonic() - started < 120:
            view = self.api("/studio/state")
            if view["status"] == "ready":
                return round((time.monotonic() - started) * 1000, 2)
            if view["status"] == "offline":
                raise RuntimeError(str(view["logs"][-4:]))
            time.sleep(0.1)
        raise TimeoutError("Engine readiness")

    def inference(self, messages, interrupt=False, route=ROUTE, since=None):
        body = {
            "model": "default_model",
            "messages": messages,
            "temperature": 0,
            "seed": 0,
            "max_tokens": 128 if interrupt else 16,
            "stream": True,
            "stream_options": {"include_usage": True},
        }
        connection = HTTPConnection("127.0.0.1", self.port, timeout=60)
        text, usage, first, reason = "", None, None, None
        done = False
        with Footprint(self.pid) as memory:
            start = time.monotonic()
            try:
                connection.request("POST", route, json.dumps(body), self.headers)
                with connection.getresponse() as response:
                    if response.status != 200:
                        detail = response.read(512).decode()
                        if (
                            response.status in (409, 429, 503)
                            and "busy" in detail.lower()
                        ):
                            raise Busy(response.status, detail)
                        raise RuntimeError(str((response.status, detail)))
                    for line in response:
                        if not line.startswith(b"data:"):
                            continue
                        if line[5:].strip() == b"[DONE]":
                            done = True
                            break
                        frame = json.loads(line[5:])
                        usage = frame.get("usage") or usage
                        for choice in frame.get("choices") or []:
                            reason = choice.get("finish_reason") or reason
                            delta = choice.get("delta", {}).get("content")
                            if delta:
                                if first is None:
                                    first = time.monotonic()
                                text += delta
                        if interrupt and first is not None:
                            break
                    end = time.monotonic()
            finally:
                connection.close()
        assert first is not None and (
            interrupt or (done and usage and reason == "stop")
        ), (text, usage, reason)
        return {
            "answer": text,
            "usage": usage,
            "firstMs": round(1000 * (first - start), 2),
            "completionMs": round(1000 * (end - start), 2),
            "peakPhysicalBytes": memory.peak,
            **(
                {
                    "startToFirstMs": round((first - since) * 1000, 2),
                    "startToCompletedMs": round((end - since) * 1000, 2),
                }
                if since is not None
                else {}
            ),
            "memorySamples": memory.samples,
            "finish": reason,
        }


@contextmanager
def gateway(server, state, folder):
    port = free_port()
    statefile = folder / "studio.json"
    statefile.write_text(json.dumps({"settings": state}))
    with (folder / "studio.log").open("ab") as log:
        env = dict(os.environ, STUDIO_PORT=str(port), STUDIO_STATE_PATH=str(statefile))
        host = subprocess.Popen([server], env=env, stdout=log, stderr=log)
        try:
            client = Gateway(port, host.pid)
            for _ in range(150):
                try:
                    token = client.api("/studio/session", {})["token"]
                    break
                except OSError:
                    if host.poll() is not None:
                        raise RuntimeError("Gateway exited before session creation")
                    time.sleep(0.05)
            else:
                raise TimeoutError("Gateway startup")
            try:
                with urlopen(Request(client.origin + ROUTE, b"{}", client.headers)):
                    raise AssertionError("Anonymous chat was accepted")
            except HTTPError as exc:
                assert exc.code == 401
            client.headers["x-studio-token"] = token
            yield client
        finally:
            if host.poll() is None:
                host.send_signal(signal.SIGINT)
            host.wait(timeout=60)


def messages(tag, code):
    return [
        {
            "role": "user",
            "content": f"Notebook {tag}. "
            + "The inventory has already been checked. " * 24
            + f"The current handoff code is {code}. Reply with only the digits of the current handoff code.",
        }
    ]


def run(args):
    settings = {
        arm: json.loads(path.read_text())["settings"]
        for arm, path in [("off", args.mlx_state), ("on", args.ssd_state)]
    }
    ssd, mlx = settings["on"]["prefixCache"], settings["off"]["vllm"]
    assert (
        settings["on"]["engine"] == "mlxPrefix"
        and settings["off"]["engine"] == "vllmApple"
    )
    if args.ssd_state.resolve().parent not in Path(ssd["cacheRoot"]).resolve().parents:
        raise ValueError(
            "Use the cache certified under the dedicated SSD QA state directory"
        )
    assert Path(mlx["model"]).samefile(ssd["model"])
    assert Path(mlx["executable"]).parent == Path(ssd["python"]).parent
    assert (
        mlx["backend"],
        mlx["context"],
        mlx["kvCacheMiB"],
        mlx["allocatorCacheMiB"],
    ) == ("mlx_lm", 512, 64, 256)
    cert = json.loads(Path(ssd["evidence"]).read_text())
    profile = cert["profile"]
    assert (
        profile["context"] == 512
        and profile["memory_cache_bytes"] == 64 * MIB
        and profile["allocator_cache_bytes"] == 256 * MIB
    )
    assert cert["requires_eviction"] and cert["result"] == "passed"
    off_cert = json.loads(Path(mlx["evidence"]).read_text())
    assert off_cert["backend_versions"]["mlx_lm"] == "0.32.0" and off_cert["passed"]
    assert off_cert["architecture_evidence"]["context_tokens"] == 512
    # A new namespace gives each pair its own cold key without clearing even the QA store.
    nonce = secrets.token_hex(4)
    store = Path(ssd["cacheRoot"]) / "store"
    rows, cancellations, starts = [], [], []
    report = {
        "result": "running",
        "route": ROUTE,
        "fixtureNamespace": nonce,
        "order": ["off", "on", "on", "off"] * args.cycles,
        "profile": profile,
        "ssdQuotaMiB": ssd["quotaMiB"],
        "model": Path(mlx["model"]).name,
        "modelAndTokenizerDigest": {
            k: cert["cache_binding"]["identity"][k] for k in ("model", "tokenizer")
        },
        "scope": "Two managed execution paths, same weights and MLX installation. Cold means no matching application prefix; OS page cache retained. Memory is sampled process-tree physical footprint. Writes are logical cache payload and changed file lengths, not device I/O.",
        "mlxLm": "0.32.0",
        "evidenceSha256": {
            arm: sha256(Path(p).read_bytes()).hexdigest()
            for arm, p in [("on", ssd["evidence"]), ("off", mlx["evidence"])]
        },
        "rows": rows,
        "cancellations": cancellations,
        "starts": starts,
    }

    if args.restart_only:
        restart_to_answer(args, report, settings, nonce)
        return

    def save():
        args.output.write_text(json.dumps(report, indent=2) + "\n")

    guard = HostLoadGuard()
    try:
        for index, arm in enumerate(report["order"]):
            pair = index // 2
            prompt = messages(f"{nonce}-{pair}", "42")
            miss = messages(f"{nonce}-{pair}-new", "73")
            with (
                tempfile.TemporaryDirectory() as temp,
                gateway(args.server, settings[arm], Path(temp)) as api,
            ):

                def counters():
                    return api.api("/studio/prefix-cache") if arm == "on" else {}

                def ready():
                    ms = api.start()
                    starts.append({"pair": pair, "arm": arm, "ms": ms})
                    guard.set_owned(tuple([os.getpid(), *Footprint(api.pid).tree()]))
                    if arm == "on":
                        metrics = counters()
                        assert (
                            metrics["productionIntegration"]
                            and metrics["identity"] == cert["identity"]
                        )
                    else:
                        limits = worker_limits(api.pid)
                        assert limits["max_bytes"] == 64 * MIB
                        assert limits["allocator"]["effective_limit_bytes"] == 256 * MIB

                def measure(name, query, want, route=ROUTE):
                    memory = detect_memory()
                    assert (
                        memory.available_bytes >= 4 * 1024**3
                        and memory.pressure.value != "critical"
                    ), "Host memory headroom"
                    before = counters()
                    previous = {
                        p.name: p.stat().st_mtime_ns for p in store.glob("*.cache")
                    }
                    if name != "afterCancel":
                        guard.wait_quiet(limit=20, timeout=60)
                    mark = guard.mark()
                    retries = []
                    first_attempt = time.monotonic()
                    while True:
                        attempt = time.monotonic()
                        try:
                            row = api.inference(query, route=route)
                            break
                        except Busy as err:
                            if name != "afterCancel" or attempt - first_attempt > 15:
                                raise
                            retries.append(
                                {
                                    "status": err.args[0],
                                    "atMs": round((attempt - first_attempt) * 1000, 1),
                                }
                            )
                            time.sleep(0.1)
                            api.action("newConversation")
                    if name == "afterCancel":
                        row["busyBeforeRecovery"] = retries
                        for metric in ("firstMs", "completionMs"):
                            row[metric] = round(
                                row[metric] + (attempt - first_attempt) * 1000, 2
                            )
                    cpu = guard.interval(mark, guard.mark())
                    after = counters()
                    delta = {
                        k: after[k] - before[k]
                        for k in [
                            "hits",
                            "misses",
                            "writes",
                            "serializedBytes",
                            "committedBytes",
                            "errors",
                        ]
                        if k in after
                    }
                    current = {
                        p.name: p.stat().st_mtime_ns for p in store.glob("*.cache")
                    }
                    changed = [
                        store / n
                        for n, stamp in current.items()
                        if previous.get(n) != stamp
                    ]
                    assert len(changed) == delta.get("writes", 0)
                    row["changedStoreFileBytes"] = sum(
                        p.stat().st_size for p in changed
                    )
                    row["removedEntries"] = len(previous.keys() - current.keys())
                    assert (
                        sum((store / n).stat().st_size for n in current)
                        <= ssd["quotaMiB"] * MIB
                    )
                    row.update(
                        arm=arm,
                        pair=pair,
                        scenario=name,
                        route=route,
                        disk=delta,
                        backgroundCpu=cpu,
                        cpuBeforePercent=mark["busyPercent"],
                        availableBytesBefore=memory.available_bytes,
                    )
                    rows.append(row)
                    save()
                    assert row["answer"].strip() == want, (name, row["answer"], want)
                    u = row["usage"]
                    assert 0 < u["prompt_tokens"] <= 496 and u["completion_tokens"] > 0
                    if arm == "on":
                        assert delta["errors"] == 0
                        if name == "cold":
                            assert delta["writes"] == 1 and delta["hits"] == 0, delta
                        if name == "restart":
                            assert delta["hits"] == 1 and delta["writes"] == 0
                            assert (
                                u["prompt_tokens_details"]["cached_tokens"]
                                == u["prompt_tokens"] - 1
                            )
                    elif name in ("cold", "restart"):
                        assert u["prompt_tokens_details"]["cached_tokens"] == 0, u

                ready()
                measure("cold", prompt, "42")
                for _ in range(4):
                    measure("warm", prompt, "42")
                follow_up = prompt + [
                    {"role": "assistant", "content": rows[-1]["answer"]},
                    {
                        "role": "user",
                        "content": "Replace the old handoff code with 73. Reply with only the new digits.",
                    },
                ]
                measure("followUp", follow_up, "73")
                guard.set_owned((os.getpid(),))
                api.action("stop")
                ready()
                measure("restart", prompt, "42")
                measure("changed", miss, "73")
                # Desktop-style transport disconnect after visible content, followed by an independent conversation.
                api.action("newConversation")
                cancel_before = counters()
                interrupted = api.inference(
                    [
                        {
                            "role": "user",
                            "content": "Count from 1 to 1000 in order, separated by commas.",
                        }
                    ],
                    True,
                    route="/studio/chat",
                )
                at = time.monotonic()
                while time.monotonic() - at < 10:
                    view = api.api("/studio/state")
                    requests = view["requests"]
                    if (
                        not view["document"]["generating"]
                        and requests
                        and requests[0]["status"] == "cancelled"
                    ):
                        break
                    time.sleep(0.025)
                else:
                    raise AssertionError("Gateway did not record cancellation")
                cancel_after = counters()
                cancellations.append(
                    {
                        "disk": {
                            k: cancel_after[k] - cancel_before[k]
                            for k in ("writes", "serializedBytes", "committedBytes")
                            if k in cancel_after
                        },
                        "arm": arm,
                        "pair": pair,
                        "afterContent": interrupted["answer"],
                        "releaseObservedMs": round(1000 * (time.monotonic() - at), 2),
                    }
                )
                api.action("newConversation")
                # The next turn uses Rust-owned conversation history as in desktop chat.
                next_query = messages(f"{nonce}-{pair}-next", "91")
                measure("afterCancel", next_query, "91", route="/studio/chat")
                history = api.api("/studio/state")["document"]["messages"]
                assert [(m["role"], m["content"]) for m in history] == [
                    ("user", next_query[0]["content"]),
                    ("assistant", rows[-1]["answer"]),
                ]
                cancellations[-1]["busyBeforeRecovery"] = rows[-1]["busyBeforeRecovery"]
                cancellations[-1]["nextConversationAnswer"] = rows[-1]["answer"]
                cancellations[-1]["newHistoryOnly"] = True
                guard.set_owned((os.getpid(),))
                api.action("stop")
            print(
                f"Completed {index + 1}/{len(report['order'])}: {arm}, pair {pair}",
                flush=True,
            )

        mismatches = []
        for pair in range(len(report["order"]) // 2):
            for name in [
                "cold",
                "warm",
                "followUp",
                "restart",
                "changed",
                "afterCancel",
            ]:
                groups = [
                    [
                        (
                            r["answer"],
                            r["usage"]["prompt_tokens"],
                            r["usage"]["completion_tokens"],
                            r["finish"],
                        )
                        for r in rows
                        if (r["pair"], r["arm"], r["scenario"]) == (pair, arm, name)
                    ]
                    for arm in ("off", "on")
                ]
                assert len(groups[0]) == len(groups[1]) > 0
                if groups[0] != groups[1]:
                    mismatches.append(
                        {
                            "pair": pair,
                            "scenario": name,
                            "off": groups[0],
                            "on": groups[1],
                        }
                    )
        summary = {}
        for name in [
            "cold",
            "warm",
            "followUp",
            "restart",
            "changed",
            "afterCancel",
            "all",
        ]:
            summary[name] = {}
            for arm in ["off", "on"]:
                group = [
                    r
                    for r in rows
                    if r["arm"] == arm and (name == "all" or r["scenario"] == name)
                ]
                out = {
                    "n": len(group),
                    "peakMiB": round(
                        max(r["peakPhysicalBytes"] for r in group) / MIB, 1
                    ),
                }
                for metric in ["firstMs", "completionMs"]:
                    values = sorted(r[metric] for r in group)
                    out[metric] = {
                        "p50": round(median(values), 2),
                        "p95": values[math.ceil(0.95 * len(values)) - 1],
                    }
                out["logicalPayloadBytes"] = sum(
                    r["disk"].get("committedBytes", 0) for r in group
                )
                out["serializedBytes"] = sum(
                    r["disk"].get("serializedBytes", 0) for r in group
                )
                out["highCpuWindows"] = sum(
                    r["backgroundCpu"]["averageBusyPercent"] > 50
                    or r["backgroundCpu"]["sustainedHighSeconds"] >= 1
                    for r in group
                )
                summary[name][arm] = out
        report.update(
            result="passed",
            summary=summary,
            parity=not mismatches,
            parityDifferences=mismatches,
            pressureQualification={
                k: cert["gates"][k]
                for k in (
                    "automatic_eviction",
                    "eviction_restart_recompute",
                    "corruption_fallback",
                    "quota_fallback",
                )
            },
        )
    finally:
        guard.stop()
        save()


def restart_to_answer(args, report, settings, nonce):
    """Time from the ordinary Start command to content and completion, without polling diagnostics in between."""
    rows = []
    guard = HostLoadGuard()
    try:
        for index, arm in enumerate(report["order"]):
            pair = index // 2
            query = messages(f"{nonce}-restart-{pair}", "42")
            with (
                tempfile.TemporaryDirectory() as temp,
                gateway(args.server, settings[arm], Path(temp)) as api,
            ):
                api.start()
                seed = api.inference(query)
                assert seed["answer"].strip() == "42"
                api.action("stop")
                guard.wait_quiet(limit=20, timeout=60)
                cpu = guard.snapshot()["busyPercent"]
                begin = time.monotonic()
                ready = api.start()
                # No budget/metrics HTTP probes or sleeps between readiness and the request.
                row = api.inference(query, since=begin)
                assert (
                    row["answer"] == seed["answer"]
                    and row["usage"]["prompt_tokens"] == seed["usage"]["prompt_tokens"]
                )
                assert (
                    row["usage"]["completion_tokens"]
                    == seed["usage"]["completion_tokens"]
                )
                used = row["usage"]["prompt_tokens_details"]["cached_tokens"]
                assert used == (row["usage"]["prompt_tokens"] - 1 if arm == "on" else 0)
                if arm == "on":
                    counters = api.api("/studio/prefix-cache")
                    assert counters["hits"] == 1 and counters["writes"] == 0
                rows.append(
                    {
                        "arm": arm,
                        "pair": pair,
                        "readyMs": ready,
                        "firstMs": row["firstMs"],
                        "completionMs": row["completionMs"],
                        "startToFirstMs": row["startToFirstMs"],
                        "startToCompletedMs": row["startToCompletedMs"],
                        "usage": row["usage"],
                        "answer": row["answer"],
                        "quietBeforePercent": cpu,
                    }
                )
                api.action("stop")
            print(
                "Completed restart-to-answer",
                index + 1,
                "/",
                len(report["order"]),
                flush=True,
            )
        for pair in range(len(rows) // 2):
            x, y = [r for r in rows if r["pair"] == pair]
            assert x["answer"] == y["answer"] and all(
                x["usage"][k] == y["usage"][k]
                for k in ("prompt_tokens", "completion_tokens")
            )
        report.update(result="passed", parity=True)
    finally:
        guard.stop()
        report.update(
            scope="From the Start command to the replayed answer through the authenticated gateway. Prior seed and stop excluded. OS file caches retained.",
            rows=rows,
        )
        for key in ("starts", "cancellations"):
            report.pop(key, None)
        args.output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for field in ("mlx-state", "ssd-state", "server", "output"):
        parser.add_argument("--" + field, type=Path, required=True)
    parser.add_argument("--cycles", type=int, choices=range(1, 6), default=2)
    parser.add_argument(
        "--restart-only",
        action="store_true",
        help="Measure Start to first content and completion after reseeding the same history",
    )
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Choose a new report path and dedicated qualified QA state files")
    if not __debug__:
        parser.error("Run with validation assertions enabled")
    run(args)
