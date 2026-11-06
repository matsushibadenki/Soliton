"""Bounded receipts for already committed states; never a substitute for restore checks."""
from collections import OrderedDict
import os
from pathlib import Path
import re
import stat


class CommittedPrefixes:
    def __init__(self, root, capacity=256):
        self.root = Path(root)
        self.capacity = capacity
        self.entries = OrderedDict()

    def _stamp(self, name):
        if not isinstance(name, str) or not re.fullmatch(r'[0-9a-f]{64}\.cache', name):
            raise ValueError('Invalid store receipt')
        directory = self.root.lstat()
        file = (self.root / name).lstat()
        if (not stat.S_ISDIR(directory.st_mode) or directory.st_mode & 0o077
                or directory.st_uid != os.getuid() or not stat.S_ISREG(file.st_mode)
                or file.st_mode & 0o077 or file.st_uid != os.getuid() or file.st_size <= 4104):
            raise ValueError('Non-private store receipt')
        return ((directory.st_dev, directory.st_ino, directory.st_mode, directory.st_uid),
                (file.st_dev, file.st_ino, file.st_size, file.st_mtime_ns,
                 file.st_ctime_ns, file.st_mode, file.st_uid))

    def remember(self, prefix, receipt):
        # Call only after a successful atomic put, or a verified/decoded restore.
        try:
            name = receipt['entry']
            stamp = self._stamp(name)
        except (OSError, ValueError, KeyError, TypeError):
            self.entries.pop(prefix, None)
            return
        self.entries[prefix] = (name, stamp)
        self.entries.move_to_end(prefix)
        while len(self.entries) > self.capacity:
            self.entries.popitem(last=False)

    def unchanged(self, prefix):
        prior = self.entries.get(prefix)
        if prior is None:
            return False
        try:
            same = self._stamp(prior[0]) == prior[1]
        except (OSError, ValueError):
            same = False
        if not same:
            self.entries.pop(prefix, None)
            return False
        self.entries.move_to_end(prefix)
        return True
