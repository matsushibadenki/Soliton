"""Read-only original startup verification at supplied context bounds; no engine.
VLLM_PYTHON SCRIPT VLLM_ROOT MODEL EXECUTABLE EVIDENCE CONTEXT EXPECT NEW_REPORT
EXPECT is accept or reject. Reports contain safe status codes, no exception text.
"""
import hashlib
import json
import os
from pathlib import Path
import sys
import time

root, model, executable, evidence = (Path(p).absolute() for p in sys.argv[1:5])
context = int(sys.argv[5])
expected = sys.argv[6]
assert expected in {"accept", "reject"}
output = Path(sys.argv[7])
assert not output.exists()
sys.path.insert(0, str(root))
from vllm_apple.architecture_evidence import verify_startup_evidence
from vllm_apple.hardware import detect_hardware
from vllm_apple.model import inspect_model
started = time.monotonic()
initial = evidence.read_bytes()
report = {"passed": False, "context": context, "expected": expected,
          "evidenceSha256": hashlib.sha256(initial).hexdigest()}
try:
    try:
        binding = verify_startup_evidence(evidence, inspect_model(model, backend="mlx_lm"),
            executable, "mlx_lm", detect_hardware(), context_tokens=context, concurrency=1)
        report.update(accepted=True, qualifiedContext=binding["context_tokens"])
    except ValueError as error:
        # Only this specific bounds refusal satisfies an expected rejection.
        if str(error) != "architecture evidence workload exceeds qualified bounds":
            raise RuntimeError("unexpected evidence verification failure") from None
        report.update(accepted=False, rejection="qualified_bounds_exceeded")
    assert evidence.read_bytes() == initial
    assert report["accepted"] == (expected == "accept")
    report["passed"] = True
finally:
    report["elapsedSeconds"] = time.monotonic() - started
    with output.open("x") as file:
        os.chmod(output, 0o600)
        json.dump(report, file, indent=2)
        file.write("\n")
print(json.dumps(report))
