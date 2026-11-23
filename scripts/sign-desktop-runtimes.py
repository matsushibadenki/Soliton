"""Sign Mach-O files inside the desktop runtime archives before signing the app."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile

MAGIC = {bytes.fromhex(value) for value in ("feedface", "cefaedfe", "feedfacf",
                                            "cffaedfe", "cafebabe", "bebafeca",
                                            "cafebabf", "bfbafeca")}


def sign_archive(archive: Path, identity: str) -> int:
    with tempfile.TemporaryDirectory(dir=archive.parent) as folder:
        root = Path(folder) / "runtime"
        root.mkdir()
        with tarfile.open(archive) as incoming:
            incoming.extractall(root, filter="data")
        count = 0
        for binary in sorted(root.rglob("*")):
            if binary.is_symlink() or not binary.is_file():
                continue
            with binary.open("rb") as handle:
                magic = handle.read(4)
            if magic not in MAGIC or "Mach-O" not in subprocess.check_output(
                ["/usr/bin/file", "-b", str(binary)], text=True
            ):
                continue
            subprocess.run(["/usr/bin/codesign", "--force", "--sign", identity, "--timestamp",
                            "--options", "runtime", str(binary)], check=True)
            subprocess.run(["/usr/bin/codesign", "--verify", "--strict", str(binary)], check=True)
            count += 1
        if archive.name == "strata.tar.gz":
            metal = root / "engine/build-metal/strata"
            if not metal.is_file():
                metal = root / "build-metal/strata"
            for marker, binary in ((".bundled-metal.json", metal),
                                   (".bundled-gemma3.json", root / "build-metal/strata-gemma-server")):
                path = root / marker
                metadata = json.loads(path.read_text())
                with binary.open("rb") as handle:
                    metadata["sha256"] = hashlib.file_digest(handle, "sha256").hexdigest()
                path.write_text(json.dumps(metadata) + "\n")
        if not count:
            raise RuntimeError(f"No Mach-O binaries found in {archive.name}")
        packed = Path(folder) / archive.name
        subprocess.run(["/usr/bin/tar", "-czf", str(packed), "-C", str(root), "."], check=True)
        packed.replace(archive)
        return count


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("resources", type=Path)
    parser.add_argument("identity")
    args = parser.parse_args()
    for name in ("strata.tar.gz", "python.tar.gz"):
        print(f"{name}: {sign_archive(args.resources / name, args.identity)} signed Mach-O files",
              flush=True)
