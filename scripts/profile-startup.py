"""Read-only qualification phase profiling; no weights loaded into an engine.
Use vLLM-Apple's Python: SCRIPT VLLM_ROOT MODEL BACKEND_EXECUTABLE EVIDENCE NEW_REPORT
Calls the original verifier in full; timings are observations, never cached approvals.
"""
import json
import os
from pathlib import Path
import sys
import time

root, model_path, executable, evidence, output = map(lambda p: Path(p).resolve(), sys.argv[1:])
sys.path.insert(0, str(root))
report = {"schema": 1, "phasesMs": {}, "passed": False, "loadAverageAtStart": os.getloadavg()}

def timed(label, fn):
    def run(*args, **kwargs):
        started = time.perf_counter()
        try:
            return fn(*args, **kwargs)
        finally:
            report["phasesMs"][label] = (time.perf_counter() - started) * 1000
    return run

try:
    started = time.perf_counter()
    from vllm_apple import architecture_evidence as module
    from vllm_apple.hardware import detect_hardware
    from vllm_apple.model import inspect_model
    report["phasesMs"]["imports"] = (time.perf_counter()-started)*1000
    hardware = timed("hardware", detect_hardware)()
    model = timed("modelMetadata", inspect_model)(model_path, backend="mlx_lm")
    # Instrument only this diagnostic process, preserving every verifier call/result.
    # Restore even on failure; these wrappers are never installed in the engine.
    originals = {name: getattr(module, name) for name in ["build_model_integrity_manifest", "fingerprint_backend"]}
    try:
        for name, fn in originals.items():
            setattr(module, name, timed(name, fn))
        binding = json.loads(evidence.read_text())["architecture_evidence"]
        timed("fullEvidenceVerification", module.verify_startup_evidence)(
            evidence, model, executable, "mlx_lm", hardware,
            context_tokens=binding["context_tokens"], concurrency=binding["concurrency"])
    finally:
        for name, fn in originals.items():
            setattr(module, name, fn)
    report["passed"] = True
finally:
    report["loadAverageAtEnd"] = os.getloadavg()
    with output.open("x") as file:
        os.chmod(output, 0o600)
        json.dump(report, file, indent=2); file.write("\n")
print(json.dumps(report))
