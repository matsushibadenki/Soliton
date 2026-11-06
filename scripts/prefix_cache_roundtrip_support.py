"""Content identity helper shared by opt-in prefix-cache experiments."""
import hashlib


def digest_files(files):
    digest = hashlib.sha256()
    for path in sorted(files):
        digest.update(path.name.encode())
        with path.open('rb') as file:
            for chunk in iter(lambda: file.read(4 * 1024 * 1024), b''):
                digest.update(chunk)
    return digest.hexdigest()
