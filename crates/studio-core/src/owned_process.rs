//! Track macOS descendants by PID + birth time, including separate native process groups.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    pid: i32,
    born: (u64, u64),
}
#[cfg(target_os = "macos")]
fn info(pid: i32) -> Option<libc::proc_bsdinfo> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as i32;
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (read == size && info.pbi_status != 5).then_some(info) // Zombies hold no model allocations.
}
impl Identity {
    pub fn alive(self) -> bool {
        #[cfg(target_os = "macos")]
        {
            info(self.pid).is_some_and(|i| (i.pbi_start_tvsec, i.pbi_start_tvusec) == self.born)
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }
    pub fn kill(self) {
        #[cfg(unix)]
        if self.alive() {
            unsafe {
                libc::kill(self.pid, libc::SIGKILL);
            }
        }
    }
}
pub fn capture(root: u32) -> Result<Vec<Identity>, String> {
    #[cfg(target_os = "macos")]
    {
        let mut found = Vec::new();
        let mut pending = vec![root];
        while let Some(parent) = pending.pop() {
            let Some(i) = info(parent as i32) else {
                continue;
            };
            let identity = Identity {
                pid: parent as i32,
                born: (i.pbi_start_tvsec, i.pbi_start_tvusec),
            };
            if found.contains(&identity) {
                continue;
            }
            found.push(identity);
            if found.len() > 1024 {
                return Err("Engine process tree exceeds tracking limit".into());
            }
            let mut children = [0i32; 1024];
            let bytes = unsafe {
                libc::proc_listpids(
                    6,
                    parent,
                    children.as_mut_ptr().cast(),
                    std::mem::size_of_val(&children) as i32,
                )
            };
            if bytes < 0 || bytes as usize >= std::mem::size_of_val(&children) {
                return Err("Cannot inspect engine workers before stopping".into());
            }
            for child in children[..bytes as usize / std::mem::size_of::<i32>()]
                .iter()
                .copied()
                .filter(|p| *p > 0)
            {
                if info(child).is_some_and(|i| i.pbi_ppid == parent) {
                    pending.push(child as u32);
                }
            }
        }
        Ok(found)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = root;
        Ok(Vec::new())
    }
}
