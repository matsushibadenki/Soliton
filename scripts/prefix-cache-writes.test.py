import os
from pathlib import Path
import tempfile
import unittest
from prefix_cache_writes import CommittedPrefixes


class Receipts(unittest.TestCase):
    def test_changes_deletion_permissions_and_replacement_invalidate(self):
        with tempfile.TemporaryDirectory() as root:
            name = 'a' * 64 + '.cache'
            path = Path(root) / name
            path.write_bytes(b'x' * 5000)
            path.chmod(0o600)
            memo = CommittedPrefixes(root)
            self.assertFalse(memo.unchanged('p'))
            memo.remember('p', {'entry': name})
            self.assertTrue(memo.unchanged('p'))
            before = path.stat()
            with path.open('r+b') as file:
                file.write(b'y')
            os.utime(path, ns=(before.st_atime_ns, before.st_mtime_ns))
            self.assertFalse(memo.unchanged('p'))  # ctime catches restored mtime
            memo.remember('p', {'entry': name})
            path.chmod(0o644)
            self.assertFalse(memo.unchanged('p'))
            path.chmod(0o600)
            memo.remember('p', {'entry': name})
            replacement = Path(root) / 'replacement'
            replacement.write_bytes(b'x' * 5000)
            replacement.chmod(0o600)
            replacement.replace(path)
            self.assertFalse(memo.unchanged('p'))
            memo.remember('p', {'entry': name})
            path.unlink()
            self.assertFalse(memo.unchanged('p'))
            path.symlink_to('/dev/null')
            memo.remember('p', {'entry': name})
            self.assertFalse(memo.unchanged('p'))

    def test_receipts_are_bounded_and_failed_or_invalid_receipts_do_not_suppress_retry(self):
        with tempfile.TemporaryDirectory() as root:
            memo = CommittedPrefixes(root, capacity=2)
            for i in range(3):
                name = str(i) * 64 + '.cache'
                path = Path(root) / name
                path.write_bytes(b'x' * 5000)
                path.chmod(0o600)
                memo.remember(str(i), {'entry': name})
            self.assertFalse(memo.unchanged('0'))
            self.assertTrue(memo.unchanged('1'))
            for receipt in [None, {}, {'entry': '../escape'}, {'entry': 'f' * 64 + '.cache'}]:
                memo.remember('1', receipt)
                self.assertFalse(memo.unchanged('1'))
            self.assertTrue(memo.unchanged('2'))
            self.assertLessEqual(len(memo.entries), 2)

    def test_replacing_store_directory_invalidates_receipt(self):
        with tempfile.TemporaryDirectory() as parent:
            root = Path(parent) / 'store'
            root.mkdir(mode=0o700)
            name = 'a' * 64 + '.cache'
            path = root / name
            path.write_bytes(b'x' * 5000)
            path.chmod(0o600)
            memo = CommittedPrefixes(root)
            memo.remember('p', {'entry': name})
            self.assertTrue(memo.unchanged('p'))
            root.rename(Path(parent) / 'old')
            root.mkdir(mode=0o700)
            # Even the very same file inode under a replaced directory is not the receipt.
            (Path(parent) / 'old' / name).rename(root / name)
            self.assertFalse(memo.unchanged('p'))


if __name__ == '__main__':
    unittest.main()
