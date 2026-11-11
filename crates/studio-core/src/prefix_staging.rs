//! Bounded staging inventory and explicit stopped-engine orphan maintenance.
use serde::Serialize;
use std::{fs, io, path::Path};

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Staging {
    pub bytes: u64,
    pub directories: u64,
    pub stale_bytes: u64,
    pub stale_directories: u64,
    pub other_bytes: u64,
    pub other_entries: u64,
    pub cleanable_bytes: u64,
    pub cleanable_directories: u64,
}
fn invalid(text: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, text)
}
fn private_directory(meta: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.is_dir()
            && !meta.file_type().is_symlink()
            && meta.mode() & 0o077 == 0
            && meta.uid() == unsafe { libc::geteuid() }
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}
fn old(meta: &fs::Metadata) -> bool {
    meta.modified()
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() >= 86400)
}
fn add(total: &mut u64, bytes: u64) -> io::Result<()> {
    *total = total
        .checked_add(bytes)
        .ok_or_else(|| invalid("Staging size overflow"))?;
    Ok(())
}
/// Scan only direct children and one level of recognized Python TemporaryDirectory names.
/// Unknown directories and symlinks fail closed instead of reporting misleading partial totals.
/// File lengths are logical bytes, not allocated blocks. Stale is a candidate, never deletion consent.
pub fn inspect(root: &Path) -> io::Result<Staging> {
    Ok(inventory(root)?.0)
}
struct Candidate {
    path: std::path::PathBuf,
    metadata: fs::Metadata,
    children: Vec<(std::ffi::OsString, fs::Metadata)>,
}
fn inventory(root: &Path) -> io::Result<(Staging, Vec<Candidate>)> {
    if !private_directory(&fs::symlink_metadata(root)?) {
        return Err(invalid("Staging root must be a real owner-only directory"));
    }
    let mut result = Staging::default();
    let mut candidates = Vec::new();
    let mut scanned = 0;
    for item in fs::read_dir(root)? {
        scanned += 1;
        if scanned > 4096 {
            return Err(invalid("Staging inventory exceeds 4096 entries"));
        }
        let item = item?;
        let name = item.file_name();
        let meta = fs::symlink_metadata(item.path())?;
        if name == "store" {
            if !private_directory(&meta) {
                return Err(invalid("Invalid store directory"));
            }
            continue;
        }
        if meta.file_type().is_symlink() {
            return Err(invalid("Symlink in staging inventory"));
        }
        let name = name.to_string_lossy();
        let stage = name.strip_prefix("stage-").is_some_and(|suffix| {
            suffix.len() == 8
                && suffix
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        });
        if !stage || !private_directory(&meta) {
            if !meta.is_file() {
                return Err(invalid("Unexpected staging directory entry"));
            }
            add(&mut result.other_bytes, meta.len())?;
            result.other_entries += 1;
            continue;
        }
        let mut bytes = 0;
        let mut stale = old(&meta);
        let mut recognized = true;
        let mut restore_temps = 0;
        let mut children = Vec::new();
        for child in fs::read_dir(item.path())? {
            scanned += 1;
            if scanned > 4096 {
                return Err(invalid("Staging inventory exceeds 4096 entries"));
            }
            let child = child?;
            let meta = fs::symlink_metadata(child.path())?;
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err(invalid("Unexpected staging child"));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                recognized &= meta.uid() == unsafe { libc::geteuid() } && meta.nlink() == 1;
            }
            let name = child.file_name();
            let restore_temp = name.to_str().is_some_and(|name| {
                name.strip_prefix('.')
                    .and_then(|name| name.strip_suffix(".tmp"))
                    .and_then(|id| uuid::Uuid::parse_str(id).ok().map(|uuid| (id, uuid)))
                    .is_some_and(|(id, uuid)| uuid.get_version_num() == 4 && uuid.to_string() == id)
            });
            if restore_temp {
                restore_temps += 1;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    recognized &= meta.mode() & 0o077 == 0;
                }
            }
            recognized &= name == "identity.json" || name == "state.safetensors" || restore_temp;
            stale &= old(&meta);
            add(&mut bytes, meta.len())?;
            children.push((child.file_name(), meta));
        }
        add(&mut result.bytes, bytes)?;
        result.directories += 1;
        let complete_payload = children.iter().any(|(name, _)| name == "state.safetensors");
        if stale && recognized && (restore_temps == 0 || (restore_temps == 1 && !complete_payload))
        {
            add(&mut result.stale_bytes, bytes)?;
            result.stale_directories += 1;
            // Age and filenames alone are insufficient for deletion. Require the worker identity.
            if children.iter().any(|(name, _)| name == "identity.json")
                && valid_identity(&item.path().join("identity.json"))
                && same(&meta, &fs::symlink_metadata(item.path())?)
            {
                result.cleanable_bytes += bytes;
                result.cleanable_directories += 1;
                candidates.push(Candidate {
                    path: item.path(),
                    metadata: meta,
                    children,
                });
            }
        }
    }
    Ok((result, candidates))
}

fn same(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev()
            && a.ino() == b.ino()
            && a.mode() == b.mode()
            && a.uid() == b.uid()
            && a.nlink() == b.nlink()
            && a.len() == b.len()
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
fn valid_identity(path: &Path) -> bool {
    use std::io::Read;
    let check = || -> io::Result<bool> {
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let mut file = options.open(path)?;
        if !file.metadata()?.is_file() || file.metadata()?.len() > 4096 {
            return Ok(false);
        }
        let mut bytes = Vec::new();
        (&mut file).take(4097).read_to_end(&mut bytes)?;
        if bytes.len() > 4096 {
            return Ok(false);
        }
        let identity: crate::prefix_store::Identity = serde_json::from_slice(&bytes)?;
        Ok(identity.format == "mlx-lm/0.32.0/gemma2-2b/http-prefix-v1" && identity.key().is_ok())
    };
    check().unwrap_or(false)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cleanup {
    pub before: Staging,
    pub after: Staging,
    pub removed_bytes: u64,
    pub removed_files: u64,
    pub removed_directories: u64,
    pub failed_directories: u64,
}
/// The caller must hold Soliton's engine lifecycle lock and establish a fully stopped engine.
/// No recursive deletion; relative no-follow directory descriptors pin all unlink operations.
#[cfg(unix)]
pub fn cleanup(root: &Path) -> io::Result<Cleanup> {
    use std::os::fd::AsRawFd;
    let directory = unix::open_root(root)?;
    if unsafe { libc::flock(directory.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::last_os_error());
    }
    struct Unlock(fs::File);
    impl Drop for Unlock {
        fn drop(&mut self) {
            unsafe {
                libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
    let locked = Unlock(directory);
    let (before, candidates) = inventory(root)?;
    let mut result = Cleanup {
        before,
        after: Staging::default(),
        removed_bytes: 0,
        removed_files: 0,
        removed_directories: 0,
        failed_directories: 0,
    };
    for candidate in candidates {
        if remove_candidate(root, &locked.0, &candidate, &mut result).is_err() {
            result.failed_directories += 1;
        }
    }
    result.after = inspect(root)?;
    Ok(result)
}
#[cfg(not(unix))]
pub fn cleanup(_root: &Path) -> io::Result<Cleanup> {
    Err(invalid(
        "Staging cleanup requires Unix directory descriptors",
    ))
}
#[cfg(unix)]
fn remove_candidate(
    root: &Path,
    parent: &fs::File,
    candidate: &Candidate,
    result: &mut Cleanup,
) -> io::Result<()> {
    if !same(&parent.metadata()?, &fs::symlink_metadata(root)?) {
        return Err(invalid("Staging root changed"));
    }
    let name = candidate
        .path
        .file_name()
        .ok_or_else(|| invalid("Missing stage name"))?;
    let directory = unix::open_at(parent, name, true)?;
    if !same(&candidate.metadata, &directory.metadata()?) {
        return Err(invalid("Staging folder changed"));
    }
    // Validate every child before deleting any file, including payloads, ownership and link count.
    for (name, expected) in &candidate.children {
        if !same(
            expected,
            &unix::open_at(&directory, name, false)?.metadata()?,
        ) {
            return Err(invalid("Staging child changed"));
        }
    }
    if !same(&candidate.metadata, &directory.metadata()?) {
        return Err(invalid("Staging folder changed"));
    }
    // Remove identity last, so a partially failed payload deletion remains retryable.
    let mut children: Vec<_> = candidate.children.iter().collect();
    children.sort_by_key(|(name, _)| name == "identity.json");
    for (name, expected) in children {
        if !same(
            expected,
            &unix::open_at(&directory, name, false)?.metadata()?,
        ) {
            return Err(invalid("Staging child changed"));
        }
        unix::unlink(&directory, name, false)?;
        result.removed_files += 1;
        result.removed_bytes += expected.len();
    }
    // Refuse a swapped name before rmdir; an unexpected new child makes nonrecursive rmdir fail.
    let current = unix::open_at(parent, name, true)?;
    if !same(&directory.metadata()?, &current.metadata()?) {
        return Err(invalid("Staging folder changed"));
    }
    unix::unlink(parent, name, true)?;
    result.removed_directories += 1;
    directory.sync_all()?;
    parent.sync_all()?;
    Ok(())
}
#[cfg(unix)]
mod unix {
    use super::*;
    use std::{
        ffi::{CString, OsStr},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{ffi::OsStrExt, fs::OpenOptionsExt},
        },
    };
    pub fn open_root(path: &Path) -> io::Result<fs::File> {
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
    }
    fn name(value: &OsStr) -> io::Result<CString> {
        CString::new(value.as_bytes()).map_err(|_| invalid("Invalid staging name"))
    }
    pub fn open_at(parent: &fs::File, value: &OsStr, directory: bool) -> io::Result<fs::File> {
        let value = name(value)?;
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                value.as_ptr(),
                libc::O_RDONLY
                    | libc::O_NOFOLLOW
                    | libc::O_NONBLOCK
                    | libc::O_CLOEXEC
                    | if directory { libc::O_DIRECTORY } else { 0 },
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { fs::File::from_raw_fd(fd) })
    }
    pub fn unlink(parent: &fs::File, value: &OsStr, directory: bool) -> io::Result<()> {
        let value = name(value)?;
        if unsafe {
            libc::unlinkat(
                parent.as_raw_fd(),
                value.as_ptr(),
                if directory { libc::AT_REMOVEDIR } else { 0 },
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        fs::File,
        os::unix::fs::{symlink, PermissionsExt},
        time::{Duration, SystemTime},
    };
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("soliton-staging-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            Self(root)
        }
        fn stage(&self, name: &str) -> std::path::PathBuf {
            let path = self.0.join(name);
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn age(path: &Path) {
        File::open(path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(90000))
            .unwrap();
    }
    fn identity() -> Vec<u8> {
        serde_json::to_vec(&crate::prefix_store::Identity {
            model: "a".repeat(64),
            tokenizer: "b".repeat(64),
            runtime: "c".repeat(64),
            prefix: "d".repeat(64),
            format: "mlx-lm/0.32.0/gemma2-2b/http-prefix-v1".into(),
        })
        .unwrap()
    }
    fn orphan(f: &Fixture, name: &str) -> std::path::PathBuf {
        let path = f.stage(name);
        fs::write(path.join("identity.json"), identity()).unwrap();
        fs::write(path.join("state.safetensors"), [2; 20]).unwrap();
        age(&path.join("identity.json"));
        age(&path.join("state.safetensors"));
        age(&path);
        path
    }
    #[test]
    fn cleanup_recovers_interrupted_restore_but_preserves_ambiguous_files() {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new();
        let interrupted = orphan(&f, "stage-abcdefgh");
        let ambiguous = orphan(&f, "stage-ijklmnop");
        let unknown = orphan(&f, "stage-qrstuvwx");
        for (folder, name, keep_payload) in [
            (
                &interrupted,
                format!(".{}.tmp", uuid::Uuid::new_v4()),
                false,
            ),
            (&ambiguous, format!(".{}.tmp", uuid::Uuid::new_v4()), true),
            (&unknown, ".user.tmp".to_owned(), false),
        ] {
            if !keep_payload {
                fs::remove_file(folder.join("state.safetensors")).unwrap();
            }
            let temp = folder.join(name);
            fs::write(&temp, b"partial restore").unwrap();
            fs::set_permissions(&temp, fs::Permissions::from_mode(0o600)).unwrap();
            age(&temp);
            age(folder);
        }
        assert_eq!(inspect(&f.0).unwrap().cleanable_directories, 1);
        let result = cleanup(&f.0).unwrap();
        assert_eq!(result.removed_directories, 1);
        assert_eq!(result.removed_files, 2);
        assert!(!interrupted.exists());
        assert!(ambiguous.exists());
        assert!(unknown.exists());
    }

    #[test]
    fn cleanup_only_removes_old_identified_orphans_and_is_idempotent() {
        let f = Fixture::new();
        let good = orphan(&f, "stage-abcdefgh");
        let bad = orphan(&f, "stage-1234_abc");
        fs::write(bad.join("identity.json"), "{}").unwrap();
        age(&bad.join("identity.json"));
        age(&bad);
        let unknown = orphan(&f, "stage-ijklmnop");
        fs::write(unknown.join("notes.txt"), "keep").unwrap();
        age(&unknown.join("notes.txt"));
        age(&unknown);
        let recent = orphan(&f, "stage-qrstuvwx");
        fs::write(recent.join("state.safetensors"), "new").unwrap();
        let store = f.stage("store");
        fs::write(store.join("saved.cache"), "keep").unwrap();
        let before = inspect(&f.0).unwrap();
        assert_eq!(before.cleanable_directories, 1);
        let result = cleanup(&f.0).unwrap();
        assert_eq!(
            (
                result.removed_directories,
                result.removed_files,
                result.failed_directories
            ),
            (1, 2, 0)
        );
        assert_eq!(result.removed_bytes, identity().len() as u64 + 20);
        assert!(!good.exists());
        assert!(bad.exists());
        assert!(unknown.exists());
        assert!(recent.exists());
        assert_eq!(
            fs::read_to_string(store.join("saved.cache")).unwrap(),
            "keep"
        );
        assert_eq!(cleanup(&f.0).unwrap().removed_bytes, 0);
    }
    #[test]
    fn cleanup_bounds_identity_and_supports_interrupted_restore_without_payload() {
        let f = Fixture::new();
        let folder = orphan(&f, "stage-abcdefgh");
        let mut wrong_format: serde_json::Value = serde_json::from_slice(&identity()).unwrap();
        wrong_format["format"] = "another-worker".into();
        let mut wrong_hash: serde_json::Value = serde_json::from_slice(&identity()).unwrap();
        wrong_hash["model"] = "not-a-content-hash".into();
        for manifest in [
            vec![b' '; 4097],
            serde_json::to_vec(&wrong_format).unwrap(),
            serde_json::to_vec(&wrong_hash).unwrap(),
        ] {
            fs::write(folder.join("identity.json"), manifest).unwrap();
            age(&folder.join("identity.json"));
            age(&folder);
            assert_eq!(cleanup(&f.0).unwrap().removed_files, 0);
            assert!(folder.join("state.safetensors").exists());
        }
        fs::write(folder.join("identity.json"), identity()).unwrap();
        fs::remove_file(folder.join("state.safetensors")).unwrap();
        age(&folder.join("identity.json"));
        age(&folder);
        let result = cleanup(&f.0).unwrap();
        assert_eq!((result.removed_directories, result.removed_files), (1, 1));
    }
    #[test]
    fn cleanup_preflight_refuses_before_any_deletion_and_rejects_busy_root() {
        use std::os::fd::AsRawFd;
        let f = Fixture::new();
        let good = orphan(&f, "stage-abcdefgh");
        symlink("/tmp", f.0.join("unexpected-link")).unwrap();
        assert!(cleanup(&f.0).is_err());
        assert!(good.join("state.safetensors").exists());
        fs::remove_file(f.0.join("unexpected-link")).unwrap();
        let held = unix::open_root(&f.0).unwrap();
        assert_eq!(
            unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        assert_eq!(
            cleanup(&f.0).err().unwrap().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(good.join("state.safetensors").exists());
        drop(held);
        assert_eq!(cleanup(&f.0).unwrap().removed_directories, 1);
    }
    #[test]
    fn cleanup_revalidates_modified_files_swapped_directories_and_hardlinks() {
        let f = Fixture::new();
        let good = orphan(&f, "stage-abcdefgh");
        let (before, candidates) = inventory(&f.0).unwrap();
        let parent = unix::open_root(&f.0).unwrap();
        let mut result = Cleanup {
            before,
            after: Staging::default(),
            removed_bytes: 0,
            removed_files: 0,
            removed_directories: 0,
            failed_directories: 0,
        };
        fs::write(good.join("state.safetensors"), "modified").unwrap();
        assert!(remove_candidate(&f.0, &parent, &candidates[0], &mut result).is_err());
        assert_eq!(result.removed_files, 0);
        assert!(good.join("identity.json").exists());
        fs::rename(&good, f.0.join("previous")).unwrap();
        orphan(&f, "stage-abcdefgh");
        assert!(remove_candidate(&f.0, &parent, &candidates[0], &mut result).is_err());
        assert_eq!(result.removed_files, 0);
        fs::remove_dir_all(f.0.join("previous")).unwrap();
        fs::hard_link(good.join("state.safetensors"), f.0.join("protected")).unwrap();
        assert_eq!(inspect(&f.0).unwrap().cleanable_directories, 0);
        assert_eq!(cleanup(&f.0).unwrap().removed_files, 0);
    }
    #[test]
    fn staging_separates_store_recent_stale_and_unknown_without_mutation() {
        let f = Fixture::new();
        let store = f.stage("store");
        fs::write(store.join("saved.cache"), [1; 100]).unwrap();
        let stale = f.stage("stage-abcdefgh");
        fs::write(stale.join("state.safetensors"), [2; 20]).unwrap();
        fs::write(stale.join("identity.json"), b"{}").unwrap();
        age(&stale.join("state.safetensors"));
        age(&stale.join("identity.json"));
        age(&stale);
        let recent = f.stage("stage-1234_abc");
        fs::write(recent.join("state.safetensors"), [3; 10]).unwrap();
        age(&recent); // A recent child prevents a stale classification even with an old directory.
        let unknown = f.stage("stage-ijklmnop");
        fs::write(unknown.join("user.txt"), b"keep").unwrap();
        age(&unknown.join("user.txt"));
        age(&unknown);
        fs::write(f.0.join("notes.txt"), b"safe").unwrap();
        let r = inspect(&f.0).unwrap();
        assert_eq!(
            (r.bytes, r.directories, r.stale_bytes, r.stale_directories),
            (36, 3, 22, 1)
        );
        assert_eq!((r.other_bytes, r.other_entries), (4, 1));
        assert_eq!(fs::read(stale.join("state.safetensors")).unwrap(), [2; 20]);
        assert!(store.join("saved.cache").exists());
    }
    #[test]
    fn staging_rejects_links_nested_unknown_and_unbounded_inventories() {
        let f = Fixture::new();
        let stage = f.stage("stage-abcdefgh");
        symlink("/tmp", stage.join("state.safetensors")).unwrap();
        assert!(inspect(&f.0).is_err());
        fs::remove_file(stage.join("state.safetensors")).unwrap();
        fs::create_dir(stage.join("nested")).unwrap();
        assert!(inspect(&f.0).is_err());
        fs::remove_dir(stage.join("nested")).unwrap();
        for i in 0..4096 {
            File::create(stage.join(i.to_string())).unwrap();
        }
        assert!(inspect(&f.0).unwrap_err().to_string().contains("4096"));
    }
    #[test]
    fn staging_rejects_nonprivate_root_and_stage() {
        let f = Fixture::new();
        let stage = f.stage("stage-abcdefgh");
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(inspect(&f.0).is_err());
        fs::remove_dir(stage).unwrap();
        fs::set_permissions(&f.0, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(inspect(&f.0).is_err());
    }
}
