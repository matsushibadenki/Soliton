//! Opaque backend-state store and stopped-engine capacity maintenance.
//! Callers must supply content hashes for model, tokenizer, runtime and token prefix.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
/// Wall-clock intervals for the CLI's successful store write.
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PutTimings {
    pub payload_and_header_ns: u128,
    pub durability_ns: u128,
}

const MAX_HEADER: u64 = 4096;
const MAX_ENTRY: u64 = 1024 * 1024 * 1024;
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn hash(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub model: String,
    pub tokenizer: String,
    pub runtime: String,
    pub prefix: String,
    pub format: String,
}
impl Identity {
    /// Stable opaque entry name, also used in successful CLI write/read receipts.
    pub fn key(&self) -> io::Result<String> {
        if [&self.model, &self.tokenizer, &self.runtime, &self.prefix]
            .iter()
            .any(|s| s.len() != 64 || !s.bytes().all(|c| c.is_ascii_hexdigit()))
            || self.format.is_empty()
            || self.format.len() > 128
        {
            return Err(invalid(
                "Expected content SHA-256 identities and bounded format version",
            ));
        }
        Ok(hash(&serde_json::to_vec(self)?))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u8,
    identity: Identity,
    bytes: u64,
    sha256: String,
}
pub struct Store {
    root: PathBuf,
    quota: u64,
}
struct StoreLock(File);
impl Drop for StoreLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            // Explicit unlock prevents a concurrent fork from extending a finished operation's lock.
            unsafe {
                libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}
impl Store {
    pub fn open(root: &Path, quota: u64) -> io::Result<Self> {
        if quota == 0 || quota > 64 * MAX_ENTRY {
            return Err(invalid("Quota must be 1 byte to 64 GiB"));
        }
        match fs::create_dir(root) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e),
        }
        Self::existing(root, quota)
    }
    /// Opens a private existing store without creating directories.
    pub fn existing(root: &Path, quota: u64) -> io::Result<Self> {
        if quota == 0 || quota > 64 * MAX_ENTRY {
            return Err(invalid("Quota must be 1 byte to 64 GiB"));
        }
        let meta = fs::symlink_metadata(root)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(invalid("Cache root must be a real directory"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                return Err(invalid("Cache directory must be owner-only"));
            }
            use std::os::unix::fs::MetadataExt;
            if meta.uid() != unsafe { libc::geteuid() } {
                return Err(invalid("Cache directory must belong to the current user"));
            }
        }
        Ok(Self {
            root: fs::canonicalize(root)?,
            quota,
        })
    }
    fn lock(&self) -> io::Result<StoreLock> {
        self.lock_with(false)
    }
    fn lock_with(&self, nonblocking: bool) -> io::Result<StoreLock> {
        let file = private_options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join(".lock"))?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            if unsafe {
                libc::flock(
                    file.as_raw_fd(),
                    libc::LOCK_EX | if nonblocking { libc::LOCK_NB } else { 0 },
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        #[cfg(not(unix))]
        return Err(invalid("Prototype requires Unix file locking"));
        Ok(StoreLock(file))
    }
    fn usage(&self) -> io::Result<(u64, usize)> {
        let (mut bytes, mut entries) = (0u64, 0);
        for item in fs::read_dir(&self.root)? {
            let item = item?;
            let name = item.file_name();
            let name = name.to_string_lossy();
            if name == ".lock" {
                continue;
            }
            let meta = fs::symlink_metadata(item.path())?;
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err(invalid("Unexpected cache directory entry"));
            }
            bytes = bytes
                .checked_add(meta.len())
                .ok_or_else(|| invalid("Cache size overflow"))?;
            if name.ends_with(".cache") {
                entries += 1;
            }
        }
        Ok((bytes, entries))
    }
    /// Rejects full stores instead of silently deleting other conversations.
    pub fn put(&self, identity: &Identity, source: &mut impl Read, bytes: u64) -> io::Result<()> {
        self.put_impl(identity, source, bytes, None)
    }

    pub fn put_timed(
        &self,
        identity: &Identity,
        source: &mut impl Read,
        bytes: u64,
        timings: &mut PutTimings,
    ) -> io::Result<()> {
        self.put_impl(identity, source, bytes, Some(timings))
    }

    fn put_impl(
        &self,
        identity: &Identity,
        source: &mut impl Read,
        bytes: u64,
        timings: Option<&mut PutTimings>,
    ) -> io::Result<()> {
        identity.key()?;
        if bytes == 0 || bytes > MAX_ENTRY {
            return Err(invalid("State must be 1 byte to 1 GiB"));
        }
        let _lock = self.lock()?;
        self.put_locked(identity, source, bytes, timings)
    }
    fn put_locked(
        &self,
        identity: &Identity,
        source: &mut impl Read,
        bytes: u64,
        timings: Option<&mut PutTimings>,
    ) -> io::Result<()> {
        let key = identity.key()?;
        let (used, count) = self.usage()?;
        let destination = self.root.join(format!("{key}.cache"));
        let old = match fs::symlink_metadata(&destination) {
            Ok(m) if m.is_file() && !m.file_type().is_symlink() => m.len(),
            Ok(_) => return Err(invalid("Invalid destination")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => 0,
            Err(e) => return Err(e),
        };
        // Includes temporary file overhead: an overwrite cannot exceed the disk quota either.
        if count >= 256 && old == 0
            || used
                .checked_add(bytes)
                .and_then(|n| n.checked_add(MAX_HEADER + 8))
                .is_none_or(|n| n > self.quota)
        {
            return Err(invalid("SSD cache quota or entry limit reached"));
        }
        let temp = self.root.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let payload_started = timings.as_ref().map(|_| std::time::Instant::now());
            let mut file = private_options()
                .write(true)
                .read(true)
                .create_new(true)
                .open(&temp)?;
            // Fixed header region keeps large backend state out of Rust/JS buffers.
            file.write_all(&[0u8; 4104])?;
            let mut digest = Sha256::new();
            let mut remaining = bytes;
            let mut buffer = [0u8; 65536];
            while remaining > 0 {
                let take = remaining.min(buffer.len() as u64) as usize;
                let n = source.read(&mut buffer[..take])?;
                if n == 0 {
                    return Err(invalid("Truncated backend state"));
                }
                digest.update(&buffer[..n]);
                file.write_all(&buffer[..n])?;
                remaining -= n as u64;
            }
            if source.read(&mut [0u8; 1])? != 0 {
                return Err(invalid("Backend state exceeds declared size"));
            }
            let header = serde_json::to_vec(&Header {
                version: 1,
                identity: identity.clone(),
                bytes,
                sha256: hex(&digest.finalize()),
            })?;
            if header.len() > MAX_HEADER as usize {
                return Err(invalid("Header too large"));
            }
            file.seek(SeekFrom::Start(0))?;
            file.write_all(&(header.len() as u64).to_le_bytes())?;
            file.write_all(&header)?;
            let durability_started = payload_started.map(|_| std::time::Instant::now());
            file.sync_all()?;
            fs::rename(&temp, &destination)?;
            File::open(&self.root)?.sync_all()?;
            if let (Some(timings), Some(payload), Some(durability)) =
                (timings, payload_started, durability_started)
            {
                timings.payload_and_header_ns = (durability - payload).as_nanos();
                timings.durability_ns = durability.elapsed().as_nanos();
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
    /// Opt-in pressure policy for isolated qualification; ordinary put remains refusal-only.
    /// Eviction is oldest-save-first, not LRU. Destination and unknown/hardlinked files are protected.
    /// Once reclamation starts, a later write failure can leave other recomputable states evicted.
    pub fn put_evicting(
        &self,
        identity: &Identity,
        source: &mut impl Read,
        bytes: u64,
    ) -> io::Result<Eviction> {
        let key = identity.key()?;
        let required = bytes
            .checked_add(MAX_HEADER + 8)
            .ok_or_else(|| invalid("State size overflow"))?;
        if bytes == 0 || bytes > MAX_ENTRY || required > self.quota {
            return Err(invalid("State cannot fit in SSD quota"));
        }
        let _lock = self.lock()?;
        let (report, candidates) = self.inventory()?;
        let (used, count) = self.usage()?;
        let destination = self.root.join(format!("{key}.cache"));
        let overwrite = match fs::symlink_metadata(&destination) {
            Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => true,
            Ok(_) => return Err(invalid("Invalid destination")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(e) => return Err(e),
        };
        let mut result = Eviction::default();
        if used.checked_add(required).is_some_and(|n| n <= self.quota) && (count < 256 || overwrite)
        {
            self.put_locked(identity, source, bytes, None)?;
            return Ok(result);
        }
        let target = report.target_bytes.saturating_sub(required);
        let count_target = if overwrite { 256 } else { 255 };
        let mut planned = Vec::new();
        let (mut remaining, mut entries) = (used, count);
        for candidate in candidates {
            if remaining <= target && entries <= count_target {
                break;
            }
            if candidate.path == destination {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if candidate.metadata.nlink() != 1 {
                    continue;
                }
            }
            remaining -= candidate.metadata.len();
            if !candidate.orphan {
                entries -= 1;
            }
            planned.push(candidate);
        }
        // No deletions if protected files make the incoming reservation impossible.
        if remaining
            .checked_add(required)
            .is_none_or(|n| n > self.quota)
            || entries > count_target
        {
            return Err(invalid("Protected SSD files prevent reservation"));
        }
        // Recheck the entire plan before deleting any entry.
        for candidate in &planned {
            if !fs::symlink_metadata(&candidate.path)
                .is_ok_and(|m| same_file(&m, &candidate.metadata))
            {
                return Err(invalid("SSD eviction candidate changed"));
            }
        }
        for candidate in planned {
            if !fs::symlink_metadata(&candidate.path)
                .is_ok_and(|m| same_file(&m, &candidate.metadata))
            {
                return Err(invalid("SSD eviction candidate changed"));
            }
            fs::remove_file(&candidate.path)?;
            result.removed_entries += 1;
            result.removed_bytes += candidate.metadata.len();
        }
        if result.removed_entries > 0 {
            File::open(&self.root)?.sync_all()?;
        }
        self.put_locked(identity, source, bytes, None)?;
        Ok(result)
    }
    /// Returns a verified, positioned file handle plus exact payload length, never GPU arrays.
    pub fn get(&self, identity: &Identity) -> io::Result<Option<(File, u64)>> {
        let Some((mut file, header)) = self.read_verified(identity, &mut io::sink())? else {
            return Ok(None);
        };
        file.seek(SeekFrom::Start(4104))?;
        Ok(Some((file, header.bytes)))
    }

    /// Copy and verify the same bytes before publishing a private staging file.
    pub fn restore_to(
        &self,
        identity: &Identity,
        destination: &Path,
    ) -> io::Result<Option<(u64, u128)>> {
        let temp = destination.with_file_name(format!(".{}.tmp", uuid::Uuid::new_v4()));
        let mut output = private_options().write(true).create_new(true).open(&temp)?;
        let result = (|| {
            let Some((_, header)) = self.read_verified(identity, &mut output)? else {
                return Ok(None);
            };
            let durability = std::time::Instant::now();
            output.sync_all()?;
            // Publish without overwriting an existing destination.
            fs::hard_link(&temp, destination)?;
            Ok(Some((header.bytes, durability.elapsed().as_nanos())))
        })();
        drop(output);
        let cleanup = fs::remove_file(temp);
        match result {
            Err(error) => Err(error),
            Ok(value) => cleanup.map(|()| value),
        }
    }

    fn read_verified(
        &self,
        identity: &Identity,
        output: &mut impl Write,
    ) -> io::Result<Option<(File, Header)>> {
        let key = identity.key()?;
        let _lock = self.lock()?;
        let mut file = match private_options()
            .read(true)
            .open(self.root.join(format!("{key}.cache")))
        {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        if !file.metadata()?.is_file() {
            return Err(invalid("Invalid cache file"));
        }
        let mut length = [0; 8];
        file.read_exact(&mut length)?;
        let length = u64::from_le_bytes(length);
        if length == 0 || length > MAX_HEADER {
            return Err(invalid("Invalid header length"));
        }
        let mut header = vec![0; length as usize];
        file.read_exact(&mut header)?;
        let header: Header = serde_json::from_slice(&header)?;
        if header.version != 1
            || header.identity != *identity
            || header.bytes == 0
            || header.bytes > MAX_ENTRY
            || file.metadata()?.len() != 4104 + header.bytes
        {
            return Err(invalid("Cache identity, version or size mismatch"));
        }
        file.seek(SeekFrom::Start(4104))?;
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 65536];
        let mut remaining = header.bytes;
        while remaining > 0 {
            let length = remaining.min(buffer.len() as u64) as usize;
            file.read_exact(&mut buffer[..length])?;
            digest.update(&buffer[..length]);
            output.write_all(&buffer[..length])?;
            remaining -= length as u64;
        }
        if file.read(&mut buffer[..1])? != 0 || hex(&digest.finalize()) != header.sha256 {
            return Err(invalid("Cache checksum mismatch"));
        }
        Ok(Some((file, header)))
    }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Eviction {
    pub removed_entries: u64,
    pub removed_bytes: u64,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capacity {
    pub quota_bytes: u64,
    pub target_bytes: u64,
    pub used_bytes: u64,
    pub cache_bytes: u64,
    pub temporary_bytes: u64,
    pub unknown_bytes: u64,
    pub reclaimable_bytes: u64,
    pub entry_count: u64,
    pub temporary_count: u64,
    pub unknown_count: u64,
    pub can_trim: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimResult {
    pub before: Capacity,
    pub after: Capacity,
    pub removed_entries: u64,
    pub removed_bytes: u64,
    pub failed_entries: u64,
    pub target_met: bool,
}
struct Candidate {
    path: PathBuf,
    metadata: fs::Metadata,
    orphan: bool,
}
fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev()
            && a.ino() == b.ino()
            && a.len() == b.len()
            && a.mode() == b.mode()
            && a.uid() == b.uid()
            && a.mtime() == b.mtime()
            && a.mtime_nsec() == b.mtime_nsec()
            && a.ctime() == b.ctime()
            && a.ctime_nsec() == b.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        let _ = (a, b);
        false
    }
}
impl Store {
    fn inventory(&self) -> io::Result<(Capacity, Vec<Candidate>)> {
        let mut report = Capacity {
            quota_bytes: self.quota,
            target_bytes: self.quota * 3 / 4,
            ..Default::default()
        };
        let mut candidates = Vec::new();
        for (index, item) in fs::read_dir(&self.root)?.enumerate() {
            if index >= 4096 {
                return Err(invalid("Cache inventory exceeds 4096 files"));
            }
            let item = item?;
            let name = item.file_name();
            let name = name.to_string_lossy();
            if name == ".lock" {
                continue;
            }
            let path = item.path();
            let meta = fs::symlink_metadata(&path)?;
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err(invalid("Unexpected cache directory entry"));
            }
            report.used_bytes = report
                .used_bytes
                .checked_add(meta.len())
                .ok_or_else(|| invalid("Cache size overflow"))?;
            #[cfg(unix)]
            let private = {
                use std::os::unix::fs::MetadataExt;
                meta.mode() & 0o077 == 0 && meta.uid() == unsafe { libc::geteuid() }
            };
            #[cfg(not(unix))]
            let private = false;
            let recognized = private
                && name.strip_suffix(".cache").is_some_and(|key| {
                    if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return false;
                    }
                    // Inventory reads only bounded metadata, not the state payload. Restore still checks SHA-256.
                    let check = (|| -> io::Result<bool> {
                        let mut file = private_options().read(true).open(&path)?;
                        if !same_file(&meta, &file.metadata()?) {
                            return Ok(false);
                        }
                        let mut n = [0; 8];
                        file.read_exact(&mut n)?;
                        let n = u64::from_le_bytes(n);
                        if n == 0 || n > MAX_HEADER {
                            return Ok(false);
                        }
                        let mut bytes = vec![0; n as usize];
                        file.read_exact(&mut bytes)?;
                        let header: Header = serde_json::from_slice(&bytes)?;
                        Ok(header.version == 1
                            && header.bytes > 0
                            && header.bytes <= MAX_ENTRY
                            && meta.len() == 4104 + header.bytes
                            && header.identity.key()? == key)
                    })();
                    check.unwrap_or(false)
                });
            let temporary = private
                && name.strip_suffix(".tmp").is_some_and(|id| {
                    uuid::Uuid::parse_str(id).is_ok_and(|u| {
                        u.to_string() == id
                            && u.get_version_num() == 4
                            && u.get_variant() == uuid::Variant::RFC4122
                    })
                });
            if recognized {
                report.cache_bytes += meta.len();
                report.entry_count += 1;
                report.reclaimable_bytes += meta.len();
                candidates.push(Candidate {
                    path,
                    metadata: meta,
                    orphan: false,
                });
            } else if temporary {
                report.temporary_bytes += meta.len();
                report.temporary_count += 1;
                if meta
                    .modified()?
                    .elapsed()
                    .is_ok_and(|age| age.as_secs() >= 86400)
                {
                    report.reclaimable_bytes += meta.len();
                    candidates.push(Candidate {
                        path,
                        metadata: meta,
                        orphan: true,
                    });
                }
            } else {
                report.unknown_bytes += meta.len();
                report.unknown_count += 1;
            }
        }
        report.can_trim = candidates.iter().any(|c| c.orphan)
            || (report.used_bytes > report.target_bytes && !candidates.is_empty());
        candidates.sort_by(|a, b| {
            b.orphan
                .cmp(&a.orphan)
                .then_with(|| a.metadata.modified().ok().cmp(&b.metadata.modified().ok()))
                .then_with(|| a.path.cmp(&b.path))
        });
        Ok((report, candidates))
    }
    pub fn capacity(&self) -> io::Result<Capacity> {
        let _lock = self.lock_with(true)?;
        Ok(self.inventory()?.0)
    }
    /// Explicit maintenance, called only while Soliton's engine is stopped.
    /// Old recognized states are recomputable; unknown files and recent temporary files stay intact.
    pub fn trim(&self) -> io::Result<TrimResult> {
        let _lock = self.lock_with(true)?;
        let (before, candidates) = self.inventory()?;
        let mut used = before.used_bytes;
        let (mut removed_entries, mut removed_bytes, mut failed_entries) = (0, 0, 0);
        for candidate in candidates {
            if !candidate.orphan && used <= before.target_bytes {
                break;
            }
            let unchanged = fs::symlink_metadata(&candidate.path)
                .is_ok_and(|m| same_file(&m, &candidate.metadata));
            if !unchanged || fs::remove_file(&candidate.path).is_err() {
                failed_entries += 1;
                continue;
            }
            removed_entries += 1;
            removed_bytes += candidate.metadata.len();
            used -= candidate.metadata.len();
        }
        if removed_entries > 0 {
            File::open(&self.root)?.sync_all()?;
        }
        let after = self.inventory()?.0;
        let target_met = after.used_bytes <= after.target_bytes;
        Ok(TrimResult {
            before,
            after,
            removed_entries,
            removed_bytes,
            failed_entries,
            target_met,
        })
    }
}
fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    options
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eviction_reserves_incoming_overhead_protects_newer_entries_and_repairs_receipts() {
        let root = std::env::temp_dir().join(format!("soliton-evict-{}", uuid::Uuid::new_v4()));
        let store = Store::open(&root, 20000).unwrap();
        let mut ids = Vec::new();
        for i in 0..4 {
            let mut id = identity();
            id.prefix = hash(&[i]);
            store.put(&id, &mut &b"payload"[..], 7).unwrap();
            File::open(root.join(format!("{}.cache", id.key().unwrap())))
                .unwrap()
                .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(i as u64))
                .unwrap();
            ids.push(id);
        }
        let mut incoming = identity();
        incoming.prefix = hash(b"new");
        let (mut pinned_reader, _) = store.get(&ids[0]).unwrap().unwrap();
        let evicted = store
            .put_evicting(&incoming, &mut &b"incoming"[..], 8)
            .unwrap();
        assert_eq!(evicted.removed_entries, 2);
        assert_eq!(evicted.removed_bytes, 2 * 4111);
        assert!(store.capacity().unwrap().used_bytes <= 15000);
        assert!(store.get(&ids[0]).unwrap().is_none());
        let mut pinned_payload = Vec::new();
        pinned_reader.read_to_end(&mut pinned_payload).unwrap();
        assert_eq!(pinned_payload, b"payload");
        assert!(store.get(&ids[3]).unwrap().is_some());
        assert!(store.get(&incoming).unwrap().is_some());
        // Reinsert an evicted key; receipts must be checked against on-disk existence by the worker.
        assert_eq!(
            store
                .put_evicting(&ids[0], &mut &b"payload"[..], 7)
                .unwrap()
                .removed_entries,
            0
        );
        assert!(store.get(&ids[0]).unwrap().is_some());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn eviction_impossible_or_invalid_reservations_preserve_all_entries() {
        let root = std::env::temp_dir().join(format!("soliton-evict-{}", uuid::Uuid::new_v4()));
        let store = Store::open(&root, 15000).unwrap();
        let id = identity();
        store.put(&id, &mut &b"old"[..], 3).unwrap();
        fs::write(root.join("protected"), vec![0; 9000]).unwrap();
        let mut incoming = identity();
        incoming.prefix = hash(b"new");
        // Even after every recognized entry is removed, protected data leaves insufficient room.
        assert!(store
            .put_evicting(&incoming, &mut &b"new"[..], 3000)
            .is_err());
        assert!(store.get(&id).unwrap().is_some());
        for bytes in [0, 15000, u64::MAX] {
            assert!(store
                .put_evicting(&incoming, &mut &b"new"[..], bytes)
                .is_err());
        }
        assert!(store.get(&id).unwrap().is_some());
        assert_eq!(fs::metadata(root.join("protected")).unwrap().len(), 9000);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn eviction_failed_write_preserves_overwrite_target_unknown_files_and_quota() {
        let root = std::env::temp_dir().join(format!("soliton-evict-{}", uuid::Uuid::new_v4()));
        let store = Store::open(&root, 17000).unwrap();
        let target = identity();
        store.put(&target, &mut &b"original"[..], 8).unwrap();
        for i in 0..3 {
            let mut id = identity();
            id.prefix = hash(&[i]);
            store.put(&id, &mut &b"other"[..], 5).unwrap();
        }
        fs::write(root.join("keep.txt"), "keep").unwrap();
        assert!(store
            .put_evicting(&target, &mut &b"short"[..], 100)
            .is_err());
        let (mut file, n) = store.get(&target).unwrap().unwrap();
        assert_eq!(n, 8);
        let mut payload = Vec::new();
        file.read_to_end(&mut payload).unwrap();
        assert_eq!(payload, b"original");
        assert_eq!(fs::read(root.join("keep.txt")).unwrap(), b"keep");
        assert!(store.capacity().unwrap().used_bytes <= 17000);
        assert!(!fs::read_dir(&root).unwrap().any(|e| e
            .unwrap()
            .path()
            .extension()
            .is_some_and(|e| e == "tmp")));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn eviction_handles_entry_limit_and_protects_recent_temporaries() {
        let root = std::env::temp_dir().join(format!("soliton-evict-{}", uuid::Uuid::new_v4()));
        let store = Store::open(&root, 2 * 1024 * 1024).unwrap();
        for i in 0u32..256 {
            let mut id = identity();
            id.prefix = hash(&i.to_le_bytes());
            store.put(&id, &mut &b"a"[..], 1).unwrap();
        }
        let recent = root.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        private_options()
            .write(true)
            .create_new(true)
            .open(&recent)
            .unwrap()
            .write_all(b"keep")
            .unwrap();
        let mut id = identity();
        id.prefix = hash(b"incoming");
        let eviction = store.put_evicting(&id, &mut &b"b"[..], 1).unwrap();
        assert_eq!(eviction.removed_entries, 1);
        assert_eq!(store.capacity().unwrap().entry_count, 256);
        assert_eq!(fs::read(&recent).unwrap(), b"keep");
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn eviction_does_not_remove_hardlinks_or_cross_symlinks() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("soliton-evict-{}", uuid::Uuid::new_v4()));
        let store = Store::open(&root, 9000).unwrap();
        let id = identity();
        store.put(&id, &mut &b"old"[..], 3).unwrap();
        fs::hard_link(
            root.join(format!("{}.cache", id.key().unwrap())),
            root.join("protected"),
        )
        .unwrap();
        let mut incoming = identity();
        incoming.prefix = hash(b"new");
        assert!(store.put_evicting(&incoming, &mut &b"new"[..], 3).is_err());
        assert!(store.get(&id).unwrap().is_some());
        fs::remove_file(root.join("protected")).unwrap();
        symlink("/tmp", root.join("unexpected")).unwrap();
        assert!(store.put_evicting(&incoming, &mut &b"new"[..], 3).is_err());
        assert!(store.get(&id).unwrap().is_some());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn capacity_trim_preserves_newer_states_unknowns_and_recent_temporary_files() {
        let root = std::env::temp_dir().join(format!("soliton-capacity-{}", uuid::Uuid::new_v4()));
        let store = Store::open(&root, 20000).unwrap();
        let mut ids = Vec::new();
        for i in 0..4 {
            let mut id = identity();
            id.prefix = hash(&[i]);
            store.put(&id, &mut &b"payload"[..], 7).unwrap();
            let file = File::open(root.join(format!("{}.cache", id.key().unwrap()))).unwrap();
            file.set_times(fs::FileTimes::new().set_modified(
                std::time::UNIX_EPOCH + std::time::Duration::from_secs(100 + i as u64),
            ))
            .unwrap();
            ids.push(id);
        }
        let old = root.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        let recent = root.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        for path in [&old, &recent] {
            let mut f = private_options()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap();
            f.write_all(b"orphan").unwrap();
        }
        File::open(&old)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH))
            .unwrap();
        fs::write(root.join("keep.txt"), b"Keep this file").unwrap();
        let foreign = root.join(format!("{}.tmp", uuid::Uuid::nil()));
        private_options()
            .write(true)
            .create_new(true)
            .open(&foreign)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH))
            .unwrap();
        let before = store.capacity().unwrap();
        assert_eq!(before.entry_count, 4);
        assert_eq!(before.temporary_count, 2);
        assert_eq!(before.unknown_count, 2);
        let result = store.trim().unwrap();
        assert!(result.target_met);
        assert_eq!(result.removed_entries, 2);
        assert!(store.get(&ids[0]).unwrap().is_none());
        assert!(store.get(&ids[3]).unwrap().is_some());
        assert!(!old.exists());
        assert!(recent.exists());
        assert!(foreign.exists());
        assert_eq!(fs::read(root.join("keep.txt")).unwrap(), b"Keep this file");
        assert!(!store.trim().unwrap().after.can_trim);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn protected_usage_can_prevent_target_and_store_lock_refuses_maintenance() {
        let root = std::env::temp_dir().join(format!("soliton-capacity-{}", uuid::Uuid::new_v4()));
        let store = Store::open(&root, 20000).unwrap();
        store.put(&identity(), &mut &b"state"[..], 5).unwrap();
        fs::write(root.join("unknown.cache"), vec![0u8; 20000]).unwrap();
        let lock = store.lock().unwrap();
        assert_eq!(
            store.capacity().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(store.trim().is_err());
        drop(lock);
        let result = store.trim().unwrap();
        assert!(!result.target_met);
        assert_eq!(result.after.unknown_bytes, 20000);
        assert_eq!(result.after.entry_count, 0);
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn symlink_inventory_refuses_before_any_deletion() {
        let root = std::env::temp_dir().join(format!("soliton-capacity-{}", uuid::Uuid::new_v4()));
        let store = Store::open(&root, 20000).unwrap();
        let id = identity();
        store.put(&id, &mut &b"state"[..], 5).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", root.join("unexpected")).unwrap();
        assert!(store.trim().is_err());
        assert!(store.get(&id).unwrap().is_some());
        fs::remove_dir_all(root).unwrap();
    }
    fn identity() -> Identity {
        Identity {
            model: hash(b"model"),
            tokenizer: hash(b"tokenizer"),
            runtime: hash(b"runtime"),
            prefix: hash(b"tokens"),
            format: "mlx-prompt-cache/0.32.0/research-v1".into(),
        }
    }
    #[test]
    fn roundtrip_identity_corruption_and_quota() {
        let root = std::env::temp_dir().join(format!("soliton-prefix-{}", uuid::Uuid::new_v4()));
        let store = Store::open(&root, 16000).unwrap();
        let id = identity();
        store.put(&id, &mut &b"state"[..], 5).unwrap();
        let (mut file, n) = store.get(&id).unwrap().unwrap();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        assert_eq!(n, 5);
        assert_eq!(data, b"state");
        for change in 0..5 {
            let mut other = id.clone();
            match change {
                0 => other.model = hash(b"other"),
                1 => other.tokenizer = hash(b"other"),
                2 => other.runtime = hash(b"other"),
                3 => other.prefix = hash(b"other"),
                _ => other.format = "v2".into(),
            };
            assert!(store.get(&other).unwrap().is_none());
        }
        assert!(store.put(&id, &mut &b"short"[..], 9).is_err());
        assert!(store.get(&id).unwrap().is_some());
        assert!(store.put(&id, &mut &vec![0u8; 14000][..], 14000).is_err());
        let staging = root.with_extension("staging");
        fs::create_dir(&staging).unwrap();
        let restored = staging.join("restored");
        assert_eq!(store.restore_to(&id, &restored).unwrap().unwrap().0, 5);
        assert_eq!(fs::read(&restored).unwrap(), b"state");
        fs::write(&restored, b"existing").unwrap();
        assert_eq!(
            store.restore_to(&id, &restored).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&restored).unwrap(), b"existing");
        fs::remove_file(&restored).unwrap();
        let path = root.join(format!("{}.cache", id.key().unwrap()));
        let mut f = OpenOptions::new().write(true).open(path).unwrap();
        f.seek(SeekFrom::Start(4104)).unwrap();
        f.write_all(b"X").unwrap();
        assert!(store.get(&id).is_err());
        assert!(store.restore_to(&id, &restored).is_err());
        assert_eq!(fs::read_dir(&staging).unwrap().count(), 0);
        fs::remove_dir(staging).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn rejects_symlinks() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("soliton-prefix-{}", uuid::Uuid::new_v4()));
        let store = Store::open(&root, 16000).unwrap();
        let id = identity();
        symlink(
            "/etc/passwd",
            root.join(format!("{}.cache", id.key().unwrap())),
        )
        .unwrap();
        assert!(store.get(&id).is_err());
        assert!(store.put(&id, &mut &b"state"[..], 5).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
