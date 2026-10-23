"""Alternating cold CPU-worker trials; run with the qualified backend Python.
Usage: python SCRIPT BEFORE_SOURCE AFTER_SOURCE MODEL NEW_REPORT
The paired native benchmark separately verifies real inference and exclusivity.
"""
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys
import time

before, after, model, output = map(Path, sys.argv[1:])
reference = str(before) == "--auto-tokenizer"
sources = {"before": after.read_text().replace("tokenizer = qualified_tokenizer(Path(sys.argv[1]))", "tokenizer = None") if reference else before.read_text(), "after": after.read_text()}
if reference:
    assert sources["before"] != sources["after"], "selector not found"
request = {"id": 1, "request": {"messages": [{"role": "user", "content": "Answer with exactly this token and nothing else: cobalt"}]}}
report = {"environment": {"python": sys.version.split()[0], "loadAverageAtStart": os.getloadavg()}, "schema": 1, "trials": [], "sourceSha256": {}, "rssMeaning": "Resident set sampled after first count; not peak or unique physical pages"}
for label, source in sources.items():
    report["sourceSha256"][label] = hashlib.sha256(source.encode()).hexdigest()
report["reference"] = "AutoTokenizer-only worker copy" if reference else str(before)
for trial in range(5):
    for label, path in [("before", before), ("after", after)]:
        trial_load = os.getloadavg()
        started = time.monotonic()
        worker = subprocess.Popen([sys.executable, "-I", "-u", "-c", sources[label], str(model.resolve()), "--soliton-token-counter"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env={**os.environ, "HF_HUB_OFFLINE": "1", "TRANSFORMERS_OFFLINE": "1"})
        try:
            worker.stdin.write((json.dumps(request)+"\n").encode()); worker.stdin.flush()
            # communicate(timeout) bounds a broken trial while EOF also checks cleanup.
            import select
            assert select.select([worker.stdout], [], [], 30)[0], "count timeout"
            result = json.loads(worker.stdout.readline())
            elapsed = (time.monotonic()-started)*1000
            rss = int(subprocess.check_output(["/bin/ps", "-p", str(worker.pid), "-o", "rss="]))
            warm = []
            for ident in range(2, 22):
                request["id"] = ident
                tick = time.monotonic()
                worker.stdin.write((json.dumps(request)+"\n").encode()); worker.stdin.flush()
                assert select.select([worker.stdout], [], [], 30)[0], "warm count timeout"
                reply = json.loads(worker.stdout.readline())
                assert reply == {"id": ident, "prompt_tokens": result["prompt_tokens"]}
                warm.append((time.monotonic()-tick)*1000)
            request["id"] = 1
            report["trials"].append({"trial": trial, "label": label, "firstCountMs": elapsed, "rssKiB": rss, "result": result, "warmMedianMs": statistics.median(warm), "loadAverageAtStart": trial_load})
            print(f"{label} trial {trial + 1}/5: first={elapsed:.1f} ms RSS={rss / 1024:.1f} MiB", file=sys.stderr, flush=True)
            worker.stdin.close()
            assert worker.wait(timeout=10) == 0, worker.stderr.read().decode()
        finally:
            if worker.poll() is None:
                worker.kill(); worker.wait()
            worker.stdout.close(); worker.stderr.close()
            if not worker.stdin.closed: worker.stdin.close()
assert all(r["result"] == report["trials"][0]["result"] for r in report["trials"])
report["summary"] = {label: {"medianFirstCountMs": statistics.median(r["firstCountMs"] for r in report["trials"] if r["label"] == label), "medianWarmCountMs": statistics.median(r["warmMedianMs"] for r in report["trials"] if r["label"] == label), "medianRssMiB": statistics.median(r["rssKiB"] for r in report["trials"] if r["label"] == label)/1024} for label in ["before", "after"]}
report["passed"] = all(report["summary"]["after"][k] < report["summary"]["before"][k] for k in ["medianFirstCountMs", "medianRssMiB"])
with output.open("x") as file:
    os.chmod(output, 0o600)
    json.dump(report, file, indent=2); file.write("\n")
print(json.dumps(report["summary"]))
assert report["passed"]
