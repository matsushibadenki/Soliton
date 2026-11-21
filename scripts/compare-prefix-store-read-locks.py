"""Compare optimized prefix-store builds on simultaneous verified macOS reads."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("before", "after", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise FileExistsError(args.output)
    size = 32 * 1024 * 1024
    samples = []
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        store = root / "store"
        digests = []
        keys = []
        for i in range(2):
            key = root / f"{i}.json"
            payload = root / f"input-{i}"
            key.write_text(json.dumps(dict(model="a"*64, tokenizer="b"*64, runtime="c"*64,
                                           prefix=str(i)*64, format="opaque-test-v1")))
            block = bytes(range(256)) if i == 0 else bytes(reversed(range(256)))
            payload.write_bytes(block * (size // len(block)))
            digests.append(hashlib.sha256(payload.read_bytes()).digest())
            keys.append(key)
            subprocess.run([args.before, store, str(4 * size), "put", key, payload],
                           check=True, timeout=30, stdout=subprocess.DEVNULL)

        def restore(binary, i):
            subprocess.run([binary, store, str(4 * size), "get-uncached", keys[i], root / f"out-{i}"],
                           check=True, timeout=30, stdout=subprocess.DEVNULL)

        with ThreadPoolExecutor(max_workers=2) as workers:
            for name in ("before", "after", "after", "before") * 4:
                for readers in (1, 2):
                    start = time.perf_counter()
                    jobs = [workers.submit(restore, getattr(args, name), i) for i in range(readers)]
                    for job in jobs:
                        job.result()
                    elapsed = round((time.perf_counter() - start) * 1000, 3)
                    for i in range(readers):
                        output = root / f"out-{i}"
                        assert hashlib.sha256(output.read_bytes()).digest() == digests[i]
                        output.unlink()
                    samples.append(dict(build=name, readers=readers, wallMs=elapsed))
    medians = {name: {str(n): statistics.median(r["wallMs"] for r in samples
                                               if r["build"] == name and r["readers"] == n)
                      for n in (1, 2)} for name in ("before", "after")}
    with args.output.open("x") as out:
        json.dump(dict(bytesPerEntry=size, order="ABBA x4", medianMs=medians, samples=samples),
                  out, indent=2)
        out.write("\n")
    print(json.dumps(medians))


if __name__ == "__main__":
    main()
