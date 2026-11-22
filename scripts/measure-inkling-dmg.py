"""Compare matched Inkling text and image turns on an isolated Apple Silicon DMG app."""

import argparse
import ctypes
import json
import os
import re
import signal
import struct
import subprocess
import tempfile
import threading
import time
import zlib
from pathlib import Path
from urllib.error import URLError
from urllib.request import Request, urlopen

ORIGIN = "http://127.0.0.1:8080"
TARGET = 3960
RESERVE = 128
MAX_INITIAL_SWAP_MIB = 256


def sysctl(key):
    return subprocess.check_output(["/usr/sbin/sysctl", "-n", key], text=True).strip()


def host_memory():
    swap = float(re.search(r"used = ([0-9.]+)M", sysctl("vm.swapusage"))[1])
    vm = subprocess.check_output(["/usr/bin/vm_stat"], text=True)
    return {
        "swapMiB": swap,
        "pressureLevel": int(sysctl("kern.memorystatus_vm_pressure_level")),
        "swapInsPages": int(re.search(r"^Swapins:\s+(\d+)\.", vm, re.M)[1]),
        "swapOutsPages": int(re.search(r"^Swapouts:\s+(\d+)\.", vm, re.M)[1]),
    }


def red_png(path, width, height):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    pixels = (b"\x00" + b"\xff\x00\x00" * width) * height
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(pixels))
        + chunk(b"IEND", b"")
    )


def detail_png(path, codes, font_size, scaled):
    from PIL import Image, ImageDraw, ImageFont

    image = Image.new("RGB", (1536, 1024), "white")
    draw = ImageDraw.Draw(image)
    title = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", 64)
    detail = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", font_size)
    draw.text((768, 210), f"TOP: {codes[0]}", font=title, fill="black", anchor="mm")
    for x, label, code in zip((384, 1152), ("LEFT", "RIGHT"), codes[1:]):
        draw.text((x, 610), label, font=title, fill="black", anchor="mm")
        draw.text((x, 755), code, font=detail, fill="black", anchor="mm")
    if scaled:
        image = image.resize((768, 512), Image.Resampling.LANCZOS)
    image.save(path)
    return list(image.size)


def reading_png(path, kind, labels, codes, font_size, scaled):
    from PIL import Image, ImageDraw, ImageFont

    image = Image.new("RGB", (1536, 1024), "#eef1f5")
    draw = ImageDraw.Draw(image)
    face = "/System/Library/Fonts/Menlo.ttc"
    heading = ImageFont.truetype(face, 42)
    label_font = ImageFont.truetype(face, 29)
    value_font = ImageFont.truetype(face, font_size)
    if kind == "document":
        draw.rectangle((136, 65, 1400, 959), fill="white", outline="#c7cdd6", width=2)
        draw.text((234, 142), "INVENTORY HANDOFF", font=heading, fill="#17283a")
        draw.text((234, 232), "Reference sheet   /   Revision 02", font=label_font, fill="#526175")
        for index, (label, code) in enumerate(zip(labels, codes)):
            y = 330 + index * 162
            draw.line((233, y, 1301, y), fill="#acb7c5", width=2)
            draw.text((250, y + 51), label, font=label_font, fill="#17283a")
            draw.text((1060, y + 52), code, font=value_font, fill="#121820", anchor="lm")
    elif kind == "document-columns":
        draw.rectangle((112, 71, 1424, 953), fill="white", outline="#c7cdd6", width=2)
        draw.text((216, 139), "SERVICE TRANSFER RECORD", font=heading, fill="#233242")
        draw.text((216, 244), "Packet 04  /  verification copy", font=label_font, fill="#526175")
        for index, (label, code) in enumerate(zip(labels, codes)):
            x = 211 + index * 385
            draw.rectangle((x, 365, x + 344, 678), fill="#f9f7ef", outline="#c2b994", width=2)
            draw.text((x + 20, 398), label, font=label_font, fill="#17283a")
            draw.text((x + 172, 564), code, font=value_font, fill="#17283a", anchor="mm")
    elif kind == "screenshot-log":
        draw.rounded_rectangle((92, 87, 1444, 935), radius=24, fill="#202632")
        draw.text((147, 145), "Run monitor  /  Events", font=heading, fill="#eef2fa")
        draw.text((147, 235), "TIME       EVENT                         CODE", font=label_font, fill="#afbed1")
        for index, (label, code) in enumerate(zip(labels, codes)):
            y = 332 + index * 153
            draw.rectangle((130, y, 1405, y + 105), fill="#303b4b")
            draw.text((156, y + 34), f"09:{12 + index}      {label}", font=label_font, fill="#dfe8f4")
            draw.text((1120, y + 52), code, font=value_font, fill="#c9f2d8", anchor="lm")
    else:
        draw.rounded_rectangle((70, 80, 1466, 944), radius=24, fill="white", outline="#a7b2c1", width=2)
        draw.rectangle((72, 127, 1464, 235), fill="#1d3654")
        draw.text((112, 157), "Deployments  /  Run details", font=heading, fill="white")
        draw.rectangle((72, 235, 305, 940), fill="#e8edf4")
        for index, tab in enumerate(("Overview", "Deployments", "Settings")):
            draw.text((106, 290 + index * 70), tab, font=label_font, fill="#314760")
        for index, (label, code) in enumerate(zip(labels, codes)):
            y = 303 + index * 184
            draw.rounded_rectangle((368, y, 1395, y + 133), radius=14, fill="#f0f4fa")
            draw.text((404, y + 22), label, font=label_font, fill="#203853")
            draw.text((1252, y + 70), code, font=value_font, fill="#121820", anchor="mm")
    if scaled:
        image = image.resize((768, 512), Image.Resampling.LANCZOS)
    image.save(path)
    return list(image.size)


def wait_for(check, process, timeout=180):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"Soliton exited with {process.returncode}")
        try:
            if result := check():
                return result
        except URLError:
            pass
        time.sleep(0.25)
    raise TimeoutError("Expected Soliton state was not reached")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("app", type=Path, help="Soliton.app copied from the verified DMG")
    parser.add_argument("output", type=Path, help="New JSON report path")
    parser.add_argument("--mode", choices=("text", "image"), required=True)
    parser.add_argument("--detail", choices=("original", "scaled", "text"),
                        help="Read matched synthetic codes instead of running the color workload")
    parser.add_argument("--extended-detail", action="store_true",
                        help="Add document and screenshot cases with a 384-token output budget")
    parser.add_argument("--layout-suite", action="store_true",
                        help="Add a column document and a dark event-log screenshot")
    parser.add_argument("--after-boot", type=int, metavar="EPOCH",
                        help="Require a host boot strictly newer than this Unix timestamp")
    args = parser.parse_args()
    if args.layout_suite and not args.extended_detail:
        parser.error("--layout-suite requires --extended-detail")
    if args.extended_detail and not args.detail:
        parser.error("--extended-detail requires --detail")
    reserve = 384 if args.extended_detail else RESERVE
    targets = (1200, 1450, 4096 - reserve if args.extended_detail else TARGET)
    if args.detail and (args.detail == "text") != (args.mode == "text"):
        parser.error("Use --mode text for --detail text and --mode image for image fixtures")
    if args.output.exists():
        parser.error("Use a new report path")
    if sysctl("machdep.cpu.brand_string") != "Apple M4" or int(sysctl("hw.memsize")) != 32 * 1024**3:
        parser.error("This workload requires an Apple M4 with 32 GiB")
    boot_epoch = int(re.search(r"sec = (\d+)", sysctl("kern.boottime"))[1])
    if args.after_boot is not None and boot_epoch <= args.after_boot:
        parser.error(f"Host boot {boot_epoch} must be newer than {args.after_boot}")
    initial = host_memory()
    if initial["swapMiB"] > MAX_INITIAL_SWAP_MIB or initial["pressureLevel"] != 1:
        parser.error(f"Start with at most {MAX_INITIAL_SWAP_MIB} MiB swap and normal memory pressure. Observed {initial}")
    try:
        with urlopen(ORIGIN, timeout=1):
            pass
    except URLError as exc:
        if not isinstance(exc.reason, ConnectionRefusedError):
            raise
    else:
        parser.error("Port 8080 already serves an app")

    app = (args.app / "Contents/MacOS/soliton").resolve(strict=True)
    root = Path(".soliton-runtime/inkling-ad-iq3").resolve()
    headers = {"Origin": ORIGIN, "Content-Type": "application/json"}
    report = {
        "app": str(app), "host": "Apple M4 / 32 GiB",
        "context": 4096, "inputTokensByTurn": list(targets), "mode": args.mode,
        "reservedTokens": reserve, "detail": args.detail,
        "bootEpoch": boot_epoch, "layoutSuite": args.layout_suite,
        "initial": initial, "hostSamples": [], "turns": [],
    }
    began = time.monotonic()

    def request(path, body=None, timeout=180):
        data = None if body is None else json.dumps(body).encode()
        return urlopen(Request(ORIGIN + path, data, headers), timeout=timeout)

    def call(path, body=None, timeout=180):
        with request(path, body, timeout) as reply:
            return json.load(reply)

    def action(name):
        return call("/studio/action", {"action": name})

    def state():
        return call("/studio/state", timeout=5)

    with tempfile.TemporaryDirectory() as directory:
        tmp = Path(directory)
        assets = tmp / "assets"
        assets.mkdir(mode=0o700)
        data = tmp / "app-data"
        cfg = tmp / "inkling.json"
        cfg.write_text(json.dumps({
            "backend": "strata-inkling-metal", "model_name": "Inkling 2 Mini", "args": [],
            "exe": str(root / "llama-b12600/llama-server"), "model": str(root / "model.gguf"),
            "mmproj": str(root / "mmproj-f16.gguf"),
        }))
        settings = tmp / "state.json"
        settings.write_text(json.dumps({"settings": {
            "engine": "strata", "strataRoot": str(data / "Strata-metal-v1"),
            "config": str(cfg), "mock": False, "temperature": 0, "maxTokens": reserve,
            "python": str(data / "python-3.12.16/bin/python3"), "language": "en",
            "omni": {"mediaRoot": str(assets)},
        }}))
        env = {**os.environ, "SOLITON_DATA_DIR": str(data), "STUDIO_STATE_PATH": str(settings)}
        env.pop("STRATA_ROOT", None)
        stop = threading.Event()
        phase = "startup"
        engine_pid = None
        libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
        memory = []
        failures = []
        with args.output.with_suffix(".app.log").open("x") as log:
            app_process = subprocess.Popen([app], env=env, stdout=log, stderr=log)

            def sample():
                try:
                    tick = 0
                    while not stop.is_set():
                        elapsed = round(time.monotonic() - began, 3)
                        current_phase = phase
                        if tick % 4 == 0:
                            report["hostSamples"].append({
                                "seconds": elapsed, "phase": current_phase, **host_memory(),
                            })
                        if engine_pid is not None:
                            processes = []
                            for pid in (app_process.pid, engine_pid):
                                counters = (ctypes.c_uint64 * 20)()
                                if libproc.proc_pid_rusage(pid, 2, ctypes.byref(counters)) != 0:
                                    break
                                processes.append((counters[8], counters[9]))
                            if len(processes) == 2:
                                memory.append((elapsed, current_phase, processes))
                        tick += 1
                        stop.wait(0.25)
                except Exception as exc:
                    failures.append(str(exc))

            watcher = threading.Thread(target=sample)
            watcher.start()
            try:
                session = wait_for(lambda: call("/studio/session", {}, timeout=1), app_process)
                headers["x-studio-token"] = session["token"]
                # Prove the HTTP session belongs to the newly started isolated process.
                assert str(data) in state()["root"], "Unexpected Soliton data root"
                action("start")
                wait_for(lambda: state()["status"] == "ready", app_process)
                assert state()["inklingVision"], "Pinned image profile unavailable"
                engine_pid = int(subprocess.check_output(
                    ["/usr/bin/pgrep", "-P", str(app_process.pid), "llama-server"],
                ))
                mode = args.mode
                cases = [
                    ("cedar", 256, 256, "card", 52, ("K28", "M47", "R63"), ("TOP", "LEFT", "RIGHT")),
                    ("granite", 768, 512, "document", 36, ("H36", "V52", "N84"),
                     ("LOT CODE", "SHELF CODE", "AUDIT CODE")),
                    ("orbit", 1536, 1024, "screenshot", 24, ("T72", "P65", "C39"),
                     ("BUILD ID", "REGION ID", "ROLLBACK ID")),
                ]
                if args.layout_suite:
                    cases.extend([
                        ("harbor", 1536, 1024, "document-columns", 24, ("F26", "J83", "W54"),
                         ("INTAKE", "TRANSFER", "RECEIPT")),
                        ("meadow", 1536, 1024, "screenshot-log", 20, ("A67", "D92", "U45"),
                         ("START EVENT", "SYNC EVENT", "EXIT EVENT")),
                    ])
                for index, (topic, width, height, kind, font_size, codes, labels) in enumerate(cases, 1):
                    phase = f"{index}-{mode}"
                    fixture = tmp / f"{topic}.png"
                    if not args.extended_detail:
                        kind, labels = "card", ("TOP", "LEFT", "RIGHT")
                    dimensions = None
                    if mode == "image":
                        if args.detail:
                            scaled = args.detail == "scaled"
                            if kind == "card":
                                dimensions = detail_png(fixture, codes, font_size, scaled)
                            else:
                                dimensions = reading_png(fixture, kind, labels, codes, font_size, scaled)
                        else:
                            red_png(fixture, width, height)
                            dimensions = [width, height]
                    media = {"kind": "image", "path": str(fixture)}
                    action("newConversation")
                    cid = state()["document"]["conversations"]["activeId"]
                    for turn in range(1, 4):
                        attaching = mode == "image" and turn == 1
                        has_image = mode == "image"
                        if turn == 1:
                            question = "The attached image has one color." if has_image else "The color is red."
                            question += " What is the color? Answer with its English name only."
                        else:
                            question = "What was the original color? Answer with its English name only."
                        expected = "red"
                        if args.detail:
                            label = labels[turn - 1]
                            question = f"What is the exact {label} code? Reply with just the three characters."
                            expected = codes[turn - 1]
                            if args.detail == "text" and turn == 1:
                                legend = ", ".join(f"{key}: {code}" for key, code in zip(labels, codes))
                                question = f"The reference card lists {legend}. {question}"
                        target = targets[turn - 1]
                        words = 1000 if turn == 3 else 1
                        budget_time = time.monotonic()
                        for _ in range(10):
                            content = f"{topic} reference notes:" + " sample" * words + "\n" + question
                            draft = {"conversationId": cid, "content": content, "max_tokens": reserve}
                            if attaching:
                                draft["media"] = media
                            budget = call("/studio/chat/budget", draft)
                            diff = target - budget["inputTokens"]
                            if diff == 0 and budget["fits"]:
                                break
                            words += diff
                            if words <= 0 or diff == 0:
                                raise RuntimeError(f"Conversation {phase} turn {turn} cannot fit: {budget}")
                        else:
                            raise RuntimeError(f"Could not match {target} prompt tokens for {phase} turn {turn}")
                        preview_seconds = time.monotonic() - budget_time
                        message = {"role": "user", "content": content}
                        if attaching:
                            message["attachments"] = [call("/studio/omni/assets/import", media)]
                        started = time.monotonic()
                        usage = {}
                        answer = ""
                        ttft = None
                        with request("/studio/chat", {
                            "conversationId": cid, "messages": [message], "max_tokens": reserve,
                            "temperature": 0, "stream": True,
                        }) as stream:
                            for line in stream:
                                if not line.startswith(b"data:") or line[5:].strip() == b"[DONE]":
                                    continue
                                item = json.loads(line[5:])
                                usage = item.get("usage") or usage
                                for choice in item.get("choices", []):
                                    delta = choice.get("delta", {}).get("content") or ""
                                    if delta and ttft is None:
                                        ttft = time.monotonic() - started
                                    answer += delta
                        assert usage.get("prompt_tokens") == target, (phase, turn, usage)
                        cached = usage.get("prompt_tokens_details", {}).get("cached_tokens")
                        if turn == 1 and (cached is None or cached > 64):
                            raise RuntimeError(f"Unexpected cross-conversation KV reuse in {phase}: {cached}")
                        row = {
                            "phase": phase, "turn": turn, "previewTokens": budget["inputTokens"],
                            "imageDimensions": dimensions, "expected": expected,
                            "fixtureKind": kind if args.detail else None,
                            "correct": answer.strip().casefold() == expected.casefold(),
                            "originalDetailFontPx": font_size if args.detail else None,
                            "usage": usage, "answer": answer.strip(), "previewSeconds": preview_seconds,
                            "firstOutputSeconds": ttft, "completionSeconds": time.monotonic() - started,
                        }
                        report["turns"].append(row)
                        print(json.dumps(row), flush=True)
                        wait_for(lambda: not state()["document"]["generating"], app_process)
                if failures:
                    raise RuntimeError(f"Memory sampling failed: {failures}")
                report["passed"] = True
            finally:
                stop.set()
                watcher.join()
                try:
                    if app_process.poll() is None:
                        app_process.send_signal(signal.SIGINT)
                        app_process.wait(timeout=30)
                finally:
                    report["samplingErrors"] = failures
                    report["final"] = host_memory()
                    report["processSamples"] = [
                        {"seconds": when, "phase": label, "appRssBytes": app_mem[0],
                         "appPhysicalBytes": app_mem[1], "engineRssBytes": engine[0],
                         "enginePhysicalBytes": engine[1]}
                        for when, label, (app_mem, engine) in memory
                    ]
                    args.output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
