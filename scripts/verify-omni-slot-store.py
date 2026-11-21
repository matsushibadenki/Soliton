"""Isolated Omni slot/store benchmark. Requires Python 3.11+ and prefix-store."""
import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import resource
import secrets
import signal
import socket
import stat
import subprocess
import tempfile
import time
from urllib import error, request


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()


def sha(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def runtime_tree(root):
    files = []
    for path in root.rglob("*"):
        resolved = path.resolve(strict=True)
        if not resolved.is_relative_to(root) or (path.is_symlink() and path.is_dir()):
            raise ValueError("Unreviewed link in native runtime")
        if path.is_file():
            files.append(path)
        elif not path.is_dir():
            raise ValueError("Unreviewed entry in native runtime")
    if len(files) > 1024:
        raise ValueError("Native runtime inventory is too large")
    digest = hashlib.sha256()
    for path in sorted(files):
        digest.update(path.relative_to(root).as_posix().encode() + b"\0")
        digest.update(sha(path).encode() + b"\n")
    return digest.hexdigest()


def measured(operation):
    start = time.perf_counter()
    result = operation()
    return result, round((time.perf_counter() - start) * 1000, 3)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("runtime", "model", "projector", "evidence", "image", "store", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--port", type=int, default=19344)
    lengths_option = parser.add_mutually_exclusive_group()
    lengths_option.add_argument("--sweep", action="store_true", help="Compare four prompt lengths")
    lengths_option.add_argument("--rows", type=int, nargs="+", help="Inventory row counts to compare")
    parser.add_argument("--shared-uncached", action="store_true",
                        help="Use a shared store, multi-turn histories and competing reads")
    parser.add_argument("--queued", action="store_true",
                        help="Compare simultaneous arrivals through a single-slot FIFO")
    parser.add_argument("--follow-up", action="store_true",
                        help="Include new turns and cache misses in each queued comparison")
    parser.add_argument("--probe-idle", action="store_true",
                        help="Observe the native slot after disconnecting a streamed response")
    args = parser.parse_args()
    if args.follow_up and not args.queued:
        parser.error("--follow-up requires --queued")
    if args.queued and not args.shared_uncached:
        parser.error("--queued requires --shared-uncached")
    lengths = args.rows if args.rows is not None else ((50, 105, 180, 300) if args.sweep else (180,))
    if any(not 1 <= rows <= 500 for rows in lengths) or len(set(lengths)) != len(lengths):
        parser.error("Row counts must be distinct integers between 1 and 500")
    if args.output.exists():
        raise FileExistsError(args.output)
    runtime = args.runtime.resolve()
    evidence = json.loads(args.evidence.read_text())
    identity = evidence["identity"]
    model_hash = sha(args.model)
    projector_hash = sha(args.projector)
    runtime_hash = runtime_tree(runtime)
    assert evidence["passed"] and evidence["shutdownClean"] and evidence["elapsedSeconds"] >= 1800
    assert evidence["scope"] == "omni_media_text_30min" and evidence["failures"] == 0
    assert identity["runtimeSha256"] == runtime_hash
    assert identity["modelSha256"] == model_hash and identity["projectorSha256"] == projector_hash
    assert identity["context"] == 4096 and identity["parallel"] == 1 and identity["reasoning"] is False
    assert identity["platform"] == "macos-aarch64"
    assert identity["osVersion"] == subprocess.check_output(["/usr/bin/sw_vers", "-productVersion"], text=True).strip()
    assert identity["hardware"] == subprocess.check_output(
        ["/usr/sbin/sysctl", "-n", "hw.model", "hw.memsize", "machdep.cpu.brand_string"], text=True,
    ).strip()

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(("127.0.0.1", args.port))
    url = f"http://127.0.0.1:{args.port}"
    key = secrets.token_hex(32)
    headers = {"Content-Type": "application/json", "Authorization": f"Bearer {key}"}

    def api(path, body=None):
        req = request.Request(url + path, None if body is None else canonical(body), headers)
        with request.urlopen(req, timeout=40) as response:
            return json.load(response)

    def slot(action):
        body = {} if action == "erase" else {"filename": "slot.bin"}
        return api("/slots/0?action=" + action, body)

    def completion(body):
        result = api("/v1/chat/completions", body)
        choice = result["choices"][0]
        return {
            "answer": choice["message"]["content"],
            "finish": choice["finish_reason"],
            "prompt": result["usage"]["prompt_tokens"],
            "completion": result["usage"]["completion_tokens"],
            "cached": result["timings"]["cache_n"],
        }

    def workload(rows, number, noun):
        prefix = " ".join(f"Inventory row {i} records {noun}." for i in range(rows))
        prompt = f"The secret is {number}. {prefix} What was the secret? Reply with digits only."
        return {"model": "omni-slot-store", "messages": [{"role": "user", "content": prompt}],
                "temperature": 0, "seed": 42, "max_tokens": 16, "cache_prompt": True}

    def probe_idle():
        body = {**workload(1, 42, "boxes"), "stream": True, "max_tokens": 1024,
                "messages": [{"role": "user", "content":
                              "List the integers from 1 to 1000 in order, separated by commas."}]}
        req = request.Request(url + "/v1/chat/completions", canonical(body), headers)
        with request.urlopen(req, timeout=20) as stream:
            assert stream.readline().startswith(b"data:")
            busy = api("/slots")
            assert len(busy) == 1 and busy[0]["id"] == 0 and busy[0]["is_processing"] is True
        start = time.perf_counter()
        probes = 0
        while time.perf_counter() - start < 20:
            probes += 1
            state = api("/slots")
            assert len(state) == 1 and state[0]["id"] == 0
            if state[0]["is_processing"] is False:
                elapsed = round((time.perf_counter() - start) * 1000, 3)
                assert completion(workload(50, 73, "chairs"))["answer"].strip() == "73"
                return {"busyBeforeDisconnect": True, "idleAfterDisconnect": True,
                        "idleWaitMs": elapsed, "polls": probes, "nextCompletionCorrect": True}
            time.sleep(0.1)
        raise TimeoutError("Native slot stayed busy after disconnect")

    def mixed_history(body, number, noun):
        content = body["messages"][0]["content"]
        return {**body, "messages": [
            {"role": "user", "content": f"We are reconciling the inventory of {noun}. Keep the current count for the handoff."},
            {"role": "assistant", "content": "Understood. Please send the latest inventory notes."},
            {"role": "user", "content": content.replace(
                f"The secret is {number}.", f"The corrected inventory count is {number}. Ignore the row indexes."
            ).replace("What was the secret? Reply with digits only.", "These are the supporting inventory notes.")},
            {"role": "assistant", "content": "I have recorded the corrected count."},
            {"role": "user", "content": "What is the corrected inventory count? Reply with digits only."},
        ]}

    def domain_history(body, rows, number):
        subject, item, notes = {
            42: ("inventory handoff", "confirmed stock count", (
                "supplier confirmed the shipment", "the damaged items were removed", "the shelf labels were reviewed",
            )),
            73: ("event planning", "confirmed room number", (
                "the organizer approved the layout", "the projector was requested", "the arrival instructions changed",
            )),
            91: ("support handoff", "confirmed incident number", (
                "the team checked the service logs", "the customer reported intermittent errors", "the rollback plan was reviewed",
            )),
        }[number]
        history = " ".join(f"Note {i}: {notes[i % len(notes)]}." for i in range(rows))
        return subject, {**body, "messages": [
            {"role": "user", "content": f"Help me prepare the {subject}. I will send the background notes."},
            {"role": "assistant", "content": "Send the notes and the final confirmed value."},
            {"role": "user", "content": f"The {item} is {number}. Here are the background notes: {history}"},
            {"role": "assistant", "content": "I've recorded the confirmed value for the handoff."},
            {"role": "user", "content": f"What is the {item}? Reply with digits only."},
        ]}

    report = {"identity": identity, "cases": []}
    if args.shared_uncached:
        report["storageMode"] = "shared-macos-f-nocache"
        report["storeQuotaBytes"] = 512 * 1024 * 1024
    mib = 1024 * 1024
    with tempfile.TemporaryDirectory(prefix="omni-slot-store-", dir=args.output.resolve().parent) as directory:
        work = Path(directory)
        stage = work / "stage"
        stage.mkdir(mode=0o700)
        payload = stage / "slot.bin"
        key_path = work / "identity.json"
        env = {k: v for k, v in os.environ.items() if not k.startswith(("LLAMA_", "GGML_"))}
        env["LLAMA_API_KEY"] = key

        def storage_command(store, operation, quota, identity_path, destination):
            return [str(args.store.resolve()), str(store), str(quota), operation,
                    str(identity_path), str(destination)]

        def storage(store, operation, quota=None):
            quota = quota if quota is not None else report.get("storeQuotaBytes", 128 * mib)
            return subprocess.run(
                storage_command(store, operation, quota, key_path, payload),
                capture_output=True, text=True, timeout=30,
            )

        def launch(log, limit=64 * mib):
            # The pinned runtime packs slot state and checkpoints into one file.
            # Let the native writer receive EFBIG instead of terminating on SIGXFSZ.
            def limit_file_size():
                signal.signal(signal.SIGXFSZ, signal.SIG_IGN)
                resource.setrlimit(resource.RLIMIT_FSIZE, (limit, limit))

            proc = subprocess.Popen(
                [str(runtime / "llama-server"), "-m", str(args.model.resolve()), "-mm", str(args.projector.resolve()),
                 "--host", "127.0.0.1", "--port", str(args.port), "--parallel", "1", "-c", "4096",
                 "--slot-save-path", str(stage), "--no-webui", "--reasoning", "off", "--temp", "0",
                 "--alias", "omni-slot-store"],
                env=env, stdout=log, stderr=log, preexec_fn=limit_file_size,
            )
            try:
                for _ in range(240):
                    if proc.poll() is not None:
                        raise RuntimeError("Omni server exited during startup")
                    try:
                        if api("/health")["status"] == "ok":
                            return proc
                    except OSError:
                        pass
                    time.sleep(0.25)
                raise TimeoutError("Omni server startup timed out")
            except BaseException:
                stop(proc)
                raise

        def stop(proc):
            if proc.poll() is None:
                proc.send_signal(signal.SIGINT)
                try:
                    proc.wait(timeout=20)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
            assert proc.returncode == 0

        with (work / "limit.log").open("w") as log:
            limit = 8 * mib
            proc = launch(log, limit)
            try:
                assert completion(workload(180, 42, "boxes"))["answer"].strip() == "42"
                try:
                    slot("save")
                except error.HTTPError as exc:
                    assert exc.code in (400, 500), exc.code
                    status = exc.code
                    exc.close()
                else:
                    raise AssertionError("Native save unexpectedly accepted an oversized payload")
                inventory = list(stage.iterdir())
                assert all(p == payload and stat.S_ISREG(p.lstat().st_mode) for p in inventory)
                written = sum(p.stat().st_size for p in inventory)
                assert written <= limit
                payload.unlink(missing_ok=True)
                slot("erase")
                assert completion(workload(50, 73, "chairs"))["answer"].strip() == "73"
                report["exportLimit"] = {"bytes": limit, "writtenBytes": written, "httpStatus": status,
                                         "serverRecovered": True}
            finally:
                stop(proc)

        with (work / "server.log").open("w") as log:
            proc = launch(log)
            try:
                template = api("/props")["chat_template"]
                assert isinstance(template, str) and template
                if args.probe_idle:
                    report["nativeIdleProbe"] = probe_idle()
                    print(json.dumps(report["nativeIdleProbe"]), flush=True)
                template_hash = hashlib.sha256(template.encode()).hexdigest()
                report["templateSha256"] = template_hash
                image = base64.b64encode(args.image.read_bytes()).decode()
                mixed = ["Reply only with 42.", [
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64," + image}},
                    {"type": "text", "text": "Name the color in one word."},
                ]]
                previous_key = work / "previous-identity.json"
                competing_payload = work / "competing.bin"
                previous_sha = None

                def run_burst(jobs, use_cache, store):
                    # One worker owns erase/restore/completion as a single FIFO job.
                    # A common arrival time models three requests arriving together.
                    def run_job(job, arrival):
                        started = time.perf_counter()
                        subject, body, key, expected_sha, cold, scenario = job
                        restore = use_cache and scenario != "miss"
                        slot("erase")
                        if restore:
                            key_path.write_bytes(canonical(key))
                            storage(store, "get-uncached").check_returncode()
                            assert sha(payload) == expected_sha
                            slot("restore")
                            payload.unlink()
                        elif use_cache:
                            miss_key = {**key, "prefix": hashlib.sha256(canonical(body)).hexdigest()}
                            key_path.write_bytes(canonical(miss_key))
                            miss = storage(store, "get-uncached")
                            assert miss.returncode != 0 and 'Error: "Cache miss"' in miss.stderr
                            assert not payload.exists()
                        outcome = completion(body)
                        ended = time.perf_counter()
                        assert all(cold[k] == outcome[k] for k in ("answer", "finish", "prompt", "completion"))
                        assert outcome["cached"] > 0 if restore else outcome["cached"] == 0
                        return {"domain": subject, "scenario": scenario, "restored": restore,
                                "promptTokens": outcome["prompt"],
                                "cachedTokens": outcome["cached"],
                                "queueMs": round((started - arrival) * 1000, 3),
                                "serviceMs": round((ended - started) * 1000, 3),
                                "totalMs": round((ended - arrival) * 1000, 3)}

                    with ThreadPoolExecutor(max_workers=1) as queue:
                        arrival = time.perf_counter()
                        pending = [queue.submit(run_job, job, arrival) for job in jobs]
                        return [future.result() for future in pending]

                if args.queued:
                    report["bursts"] = []
                for rows in lengths:
                    jobs = []
                    for number, noun in [(42, "boxes"), (73, "chairs"), (91, "books")]:
                        store = work / ("shared-store" if args.shared_uncached else f"store-{rows}-{number}")
                        slot("erase")
                        body = workload(rows, number, noun)
                        if args.queued:
                            subject, body = domain_history(body, rows, number)
                        elif args.shared_uncached:
                            body = mixed_history(body, number, noun)
                        cold, cold_ms = measured(lambda: completion(body))
                        assert cold["answer"].strip() == str(number) and cold["cached"] == 0
                        _, save_ms = measured(lambda: slot("save"))
                        inventory = list(stage.iterdir())
                        assert all(stat.S_ISREG(p.lstat().st_mode) for p in inventory)
                        stage_bytes = sum(p.stat().st_size for p in inventory)
                        assert stage_bytes <= 64 * mib and inventory == [payload]
                        payload_hash = sha(payload)
                        cache_key = {
                            "model": model_hash,
                            "tokenizer": template_hash,
                            "runtime": hashlib.sha256(canonical(identity)).hexdigest(),
                            "prefix": hashlib.sha256(canonical(body)).hexdigest(),
                            "format": "omni-native-chat-slot-v1",
                        }
                        key_path.write_bytes(canonical(cache_key))
                        if not report["cases"]:
                            assert storage(store, "put", quota=stage_bytes - 1).returncode != 0
                            assert not list(store.glob("*.cache"))
                            report["quotaRefused"] = True
                        result, put_ms = measured(lambda: storage(store, "put"))
                        result.check_returncode()
                        payload.unlink()

                        for media in mixed:
                            slot("erase")
                            unrelated = {**body, "messages": [{"role": "user", "content": media}]}
                            assert completion(unrelated)["answer"].strip()
                        slot("erase")
                        # Force one restore to cross a real native process boundary.
                        restarted = not report["cases"]
                        if restarted:
                            stop(proc)
                            proc = launch(log)
                            assert api("/props")["chat_template"] == template
                            key_path.write_bytes(canonical({**cache_key, "runtime": "0" * 64}))
                            assert storage(store, "get").returncode != 0 and not payload.exists()
                            key_path.write_bytes(canonical(cache_key))
                            report["wrongIdentityRefused"] = True

                        contention_ms = None
                        operation = "get-uncached" if args.shared_uncached else "get"
                        if args.shared_uncached and previous_sha is not None:
                            command = storage_command(store, operation, report["storeQuotaBytes"],
                                                      previous_key, competing_payload)
                            start = time.perf_counter()
                            with subprocess.Popen(command, stdout=subprocess.DEVNULL) as competitor:
                                try:
                                    result, get_ms = measured(lambda: storage(store, operation))
                                    status = competitor.wait(timeout=30)
                                finally:
                                    if competitor.poll() is None:
                                        competitor.kill()
                                contention_ms = round((time.perf_counter() - start) * 1000, 3)
                            assert status == 0 and sha(competing_payload) == previous_sha
                            competing_payload.unlink()
                        else:
                            result, get_ms = measured(lambda: storage(store, operation))
                        if args.shared_uncached:
                            previous_key.write_bytes(canonical(cache_key))
                            previous_sha = payload_hash
                        result.check_returncode()
                        assert sha(payload) == payload_hash
                        _, restore_ms = measured(lambda: slot("restore"))
                        warm, response_ms = measured(lambda: completion(body))
                        assert all(cold[field] == warm[field] for field in ("answer", "finish", "prompt", "completion"))
                        payload.unlink()
                        row = {
                            "rows": rows, "cold": cold, "restored": warm, "afterRestart": restarted, "stageBytes": stage_bytes,
                            "payloadSha256": payload_hash, "coldMs": cold_ms, "saveMs": save_ms, "putMs": put_ms,
                            "getMs": get_ms, "restoreMs": restore_ms, "responseMs": response_ms,
                            "restoreAndReplyMs": round(get_ms + restore_ms + response_ms, 3),
                            "saveRestoreAndReplyMs": round(save_ms + put_ms + get_ms + restore_ms + response_ms, 3),
                        }
                        if args.shared_uncached:
                            row["competingReadPairMs"] = contention_ms
                        report["cases"].append(row)
                        print(json.dumps(row), flush=True)
                        if args.queued:
                            scenario = "replay"
                            if args.follow_up and number != 42:
                                body = {**body, "messages": [*body["messages"],
                                    {"role": "assistant", "content": cold["answer"]},
                                    {"role": "user", "content": "I am writing the handoff now. What number should I enter? Reply with digits only."},
                                ]}
                                slot("erase")
                                cold = completion(body)
                                assert cold["answer"].strip() == str(number) and cold["cached"] == 0
                                scenario = "follow-up" if number == 73 else "miss"
                            jobs.append((subject, body, cache_key, payload_hash, cold, scenario))
                    if args.queued:
                        for order, restore in enumerate((False, True, True, False)):
                            ordered = jobs if order < 2 else jobs[1:] + jobs[:1]
                            samples = run_burst(ordered, restore, store)
                            burst = {"rows": rows, "restored": restore, "samples": samples}
                            report["bursts"].append(burst)
                            print(json.dumps(burst), flush=True)
            finally:
                stop(proc)
    with args.output.open("x") as handle:
        json.dump(report, handle, ensure_ascii=False, indent=2)
        handle.write("\n")


if __name__ == "__main__":
    main()
