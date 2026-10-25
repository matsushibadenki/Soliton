"""Activate measured 4096 context or refresh environment-only evidence at that bound.
Run with vLLM-Apple's configured Python:
SCRIPT VLLM_ROOT EVIDENCE MATRIX_REPORT GATES_REPORT NEW_ACTIVATION_REPORT STATE...
Always calls the original complete startup verifier; reports never cache approval.
"""
import copy
import hashlib
import itertools
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile

if not __debug__:
    raise RuntimeError("activation requires validation assertions enabled")

root, evidence, matrix_path, gates_path, output = [Path(p).absolute() for p in sys.argv[1:6]]
states = [Path(p).absolute() for p in sys.argv[6:]]
assert states and len(set(states)) == len(states)
assert not output.exists()
hash_bytes = lambda b: hashlib.sha256(b).hexdigest()
evidence_bytes = evidence.read_bytes()
qualification = json.loads(evidence_bytes)
context = qualification["architecture_evidence"]["context_tokens"]
assert qualification["architecture_evidence"]["concurrency"] == 1
assert context == 4096, "only the measured 4096 profile is supported"
matrix = json.loads(matrix_path.read_bytes())
gates = json.loads(gates_path.read_bytes())
assert matrix["passed"] is True and matrix["context"] == context
assert matrix["evidenceSha256"] == hash_bytes(evidence_bytes)
assert all(matrix["checks"].values())
assert matrix["sharedRequestMetrics"]["bounded"] is True
allocator_upgrade = matrix.get("allocatorCacheMiB", 0) == 256
policy_upgrade = matrix.get("kvCacheMiB", 0) == 1024
refresh_only = "cacheMemory" in matrix and "contextMatrix" not in matrix
if policy_upgrade:
    assert not refresh_only, "a new KV policy requires the complete context matrix"
    assert matrix["cacheMemory"]["passed"] and len(matrix["cacheMemory"]["cases"]) == 38
    assert matrix["cacheMemory"]["afterStop"]["ownedTreeGone"]
    assert len(matrix["cacheMemory"]["seededCases"]) == 3
    assert all(c["exact"] and c["memory"]["kvCache"]["retained_bytes"] <= 1024**3 for c in matrix["cacheMemory"]["seededCases"])
    assert matrix["checks"].get("backendKvBudgetArgument") is True
    if allocator_upgrade:
        assert matrix["checks"].get("backendAllocatorBudget") is True
        assert matrix["allocatorComparison"]["lowerPhysicalPeak"] is True
        assert matrix["allocatorComparison"]["lowerAllocatorCachePeak"] is True
        samples = matrix["contextMatrix"]["memory"] + matrix["cacheMemory"]["samples"] + [c["memory"] for c in matrix["cacheMemory"]["seededCases"]]
        assert len(samples) == 153
        assert all(s["kvCache"]["allocator"]["configured_limit_bytes"] == 256 * 1024**2 and 0 <= s["kvCache"]["allocator"]["effective_limit_bytes"] <= 256 * 1024**2 for s in samples)
        assert {(c.get("allocatorCacheMiB"), c["context"]) for c in gates["checks"]} == {(0,4096),(128,4096),(256,4097)}
        assert all(c["kvCacheMiB"] == 1024 for c in gates["checks"])
        os.environ["VLLM_APPLE_ALLOCATOR_CACHE_MIB"] = "256"
    else:
        assert {(c.get("kvCacheMiB"), c["context"]) for c in gates["checks"]} == {(0,4096),(512,4096),(1024,4097)}
    # Match the explicit setting that Rust will inject, keeping all other flags.
    os.environ["VLLM_APPLE_KV_CACHE_MIB"] = "1024"
if refresh_only:
    cache = matrix["cacheMemory"]
    assert cache["passed"] and cache["boundary"] == {"passed": True, "acceptedTotal": context, "rejectedTotal": context + 1}
    assert cache["observedAnchorReuseLosses"] == 2
    assert cache["sharedCachedTelemetry"]["verified"] and cache["afterStop"]["ownedTreeGone"]
    cases = cache["cases"]
    assert len(cases) == 38 and {c["index"] for c in cases} == set(range(16))
    assert {c["language"] for c in cases} == {"en", "ja", "zh"}
    assert all(c["exact"] is True and c["messageCount"] == 13 and 3500 <= c["promptTokens"] <= context - 64 for c in cases)
else:
    assert matrix["contextMatrix"]["passed"] is True
    required = set(itertools.product(["en", "ja", "zh"], [False, True], ["start", "middle", "end"], range(3), [False, True]))
    cases = matrix["contextMatrix"]["cases"]
    assert len(cases) == len(required)
    assert {(c["language"], c["history"], c["position"], c["round"], c["stream"]) for c in cases} == required
    assert all(c["exact"] is True and c["acceptedTotal"] == context and c["rejectedTotal"] == context + 1
               and context - 128 <= c["promptTokens"] <= context - 32
               and c["messageCount"] == (13 if c["history"] else 1) for c in cases)
assert gates["passed"] is True and gates["evidenceSha256"][1] == hash_bytes(evidence_bytes)
assert {c["context"] for c in gates["checks"]} == {4096, 4097}
assert all(c["startupRefused"] and c["noInferenceOrCounter"] and c["historyPreserved"] for c in gates["checks"])

# A live application could overwrite a preferences file when it next saves.
processes = subprocess.check_output(["/bin/ps", "-axo", "command="], text=True).splitlines()
assert not any(Path(line.split()[0]).name in {"soliton", "studio-server"}
               for line in processes if line.split()), "close settings owners before activation"
sys.path.insert(0, str(root))
from vllm_apple.architecture_evidence import verify_startup_evidence, validate_evidence
from vllm_apple.hardware import detect_hardware
from vllm_apple.model import inspect_model
pending = []
for state_path in states:
    info = state_path.lstat()
    assert stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
    original = state_path.read_bytes()
    document = json.loads(original)
    settings = document["settings"]["vllm"]
    if refresh_only:
        assert settings["context"] == context, "cache trial cannot raise the existing context"
        previous = json.loads(Path(settings["evidence"]).read_bytes())
        prior_identity = previous["architecture_evidence"]["identity"]
        validate_evidence(previous, prior_identity, context_tokens=context, concurrency=1)
        current_identity = qualification["architecture_evidence"]["identity"]
        assert all(prior_identity[k] == current_identity[k] for k in prior_identity if k != "environment_sha256"), "non-environment changes require the full context matrix"
    elif allocator_upgrade:
        assert settings["context"] == context and settings["kvCacheMiB"] == 1024
        assert settings.get("allocatorCacheMiB", 0) == 0
    elif policy_upgrade:
        assert settings["context"] == context and settings.get("kvCacheMiB", 0) == 0
    else:
        assert settings["context"] < context, "profile must increase the selected context"
    assert Path(settings["root"]).absolute() == root
    assert Path(settings["python"]).absolute() == Path(sys.executable).absolute()
    verify_startup_evidence(evidence, inspect_model(Path(settings["model"]), backend=settings["backend"]),
        Path(settings["executable"]), settings["backend"], detect_hardware(), context_tokens=context, concurrency=1)
    changed = copy.deepcopy(document)
    changed["settings"]["vllm"].update(context=context, evidence=str(evidence))
    if policy_upgrade:
        changed["settings"]["vllm"]["kvCacheMiB"] = 1024
    if allocator_upgrade:
        changed["settings"]["vllm"]["allocatorCacheMiB"] = 256
    restored = copy.deepcopy(changed)
    restored["settings"]["vllm"]["context"] = settings["context"]
    restored["settings"]["vllm"]["evidence"] = settings["evidence"]
    if policy_upgrade:
        if "kvCacheMiB" in settings:
            restored["settings"]["vllm"]["kvCacheMiB"] = settings["kvCacheMiB"]
        else:
            restored["settings"]["vllm"].pop("kvCacheMiB")
    if allocator_upgrade:
        if "allocatorCacheMiB" in settings:
            restored["settings"]["vllm"]["allocatorCacheMiB"] = settings["allocatorCacheMiB"]
        else:
            restored["settings"]["vllm"].pop("allocatorCacheMiB")
    assert restored == document
    pending.append((state_path, original, changed))
assert evidence.read_bytes() == evidence_bytes
assert all(path.read_bytes() == original for path, original, _ in pending), "settings changed during validation"
report = {"passed": False, "context": context, "evidenceSha256": hash_bytes(evidence_bytes), "states": []}
for index, (path, original, changed) in enumerate(pending):
    backup = output.with_name(output.stem + f"-backup-{index}.json")
    with os.fdopen(os.open(backup, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "wb") as file:
        file.write(original)
    encoded = (json.dumps(changed, ensure_ascii=False, indent=2) + "\n").encode()
    fd, temp = tempfile.mkstemp(prefix=".soliton-context-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as file:
            file.write(encoded)
            file.flush()
            os.fsync(file.fileno())
        assert path.read_bytes() == original
        os.replace(temp, path)
    finally:
        if os.path.exists(temp):
            os.unlink(temp)
    assert json.loads(path.read_bytes()) == changed
    report["states"].append({"path": str(path), "beforeSha256": hash_bytes(original),
        "afterSha256": hash_bytes(encoded), "onlyContextAndEvidenceChanged": not policy_upgrade, "onlyKvPolicyAndEvidenceChanged": policy_upgrade and not allocator_upgrade, "onlyAllocatorPolicyAndEvidenceChanged": allocator_upgrade, "backup": str(backup)})
report["passed"] = True
with output.open("x") as file:
    os.chmod(output, 0o600)
    json.dump(report, file, indent=2)
    file.write("\n")
print(json.dumps({"passed": True, "context": context, "updatedStates": len(pending)}))
