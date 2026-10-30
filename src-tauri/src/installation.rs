use std::path::Path;

pub fn failure(path: &Path, code: &str, detail: &str) -> serde_json::Value {
    serde_json::json!({"code":code,"path":path.to_string_lossy(),"detail":detail,"requiredBytes":2_u64*1024*1024*1024})
}
pub fn io_failure(path: &Path, error: &std::io::Error) -> serde_json::Value {
    let code = match error.raw_os_error() {
        Some(libc::ENOSPC) => "space",
        Some(libc::EACCES) | Some(libc::EPERM) | Some(libc::EROFS) => "permission",
        _ => "installation",
    };
    failure(path, code, &error.to_string())
}
pub fn preflight(path: &Path) -> Result<(), serde_json::Value> {
    std::fs::create_dir_all(path).map_err(|e| io_failure(path, &e))?;
    let probe = path.join(format!(".write-probe-{}", uuid::Uuid::new_v4()));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|e| io_failure(path, &e))?;
    drop(file);
    std::fs::remove_file(probe).map_err(|e| io_failure(path, &e))?;
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| failure(path, "installation", "Invalid path"))?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(name.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Err(io_failure(path, &std::io::Error::last_os_error()));
    }
    let stats = unsafe { stats.assume_init() };
    let available = (stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64);
    check_space(path, available)
}
fn check_space(path: &Path, available: u64) -> Result<(), serde_json::Value> {
    if available < 2 * 1024 * 1024 * 1024 {
        let mut error = failure(
            path,
            "space",
            "Insufficient free space for runtime installation",
        );
        error["availableBytes"] = available.into();
        return Err(error);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn classifies_space_and_permissions() {
        let path = std::path::Path::new("/example");
        for (errno, code) in [
            (libc::ENOSPC, "space"),
            (libc::EACCES, "permission"),
            (libc::EROFS, "permission"),
        ] {
            assert_eq!(
                super::io_failure(path, &std::io::Error::from_raw_os_error(errno))["code"],
                code
            );
        }
    }
    #[test]
    fn refuses_low_space_before_extraction() {
        let path = std::path::Path::new("/example");
        let error = super::check_space(path, 1024).unwrap_err();
        assert_eq!(error["code"], "space");
        assert_eq!(error["availableBytes"], 1024);
        assert!(super::check_space(path, 3 * 1024 * 1024 * 1024).is_ok());
    }
    #[test]
    fn rejects_unwritable_target() {
        let result = super::preflight(std::path::Path::new("/System/Library/Soliton-install-test"));
        assert_eq!(result.unwrap_err()["code"], "permission");
    }
}
