//! System-wide macOS memory, using the same reclaimable-memory estimate as Strata.
//! No subprocess or weight reads on the polling path.
use serde::Serialize;
use std::collections::VecDeque;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub sampled_at: u64,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub free_bytes: u64,
}

fn calculate(
    total: u64,
    page: u64,
    free: u64,
    purgeable: u64,
    inactive: u64,
    speculative: u64,
    file: u64,
) -> Sample {
    let free_bytes = free.saturating_mul(page).min(total);
    let available_bytes = free
        .saturating_add(purgeable)
        .saturating_add(inactive.saturating_add(speculative).min(file))
        .saturating_mul(page)
        .min(total);
    Sample {
        sampled_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        total_bytes: total,
        used_bytes: total - available_bytes,
        available_bytes,
        free_bytes,
    }
}

pub fn read(total: Option<u64>) -> Option<Sample> {
    #[cfg(target_os = "macos")]
    {
        // Retain one host send right for the lifetime of the process.
        static HOST: std::sync::OnceLock<libc::mach_port_t> = std::sync::OnceLock::new();
        let total = total.filter(|n| *n > 0)?;
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if page <= 0 {
            return None;
        }
        let mut stats: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
        let mut count = libc::HOST_VM_INFO64_COUNT;
        // libc deprecates this binding in favor of mach2; the macOS API itself is supported.
        #[allow(deprecated)]
        let host = *HOST.get_or_init(|| unsafe { libc::mach_host_self() });
        let status = unsafe {
            libc::host_statistics64(
                host,
                libc::HOST_VM_INFO64,
                &mut stats as *mut _ as libc::host_info64_t,
                &mut count,
            )
        };
        let required = (std::mem::offset_of!(libc::vm_statistics64, external_page_count)
            + std::mem::size_of::<libc::natural_t>())
            / std::mem::size_of::<libc::integer_t>();
        if status != libc::KERN_SUCCESS || (count as usize) < required {
            return None;
        }
        return Some(calculate(
            total,
            page as u64,
            stats.free_count as u64,
            stats.purgeable_count as u64,
            stats.inactive_count as u64,
            stats.speculative_count as u64,
            stats.external_page_count as u64,
        ));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = total;
        None
    }
}

/// Summed physical footprints in the owned server process tree (not the Studio GUI).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSample {
    pub sampled_at: u64,
    pub pid: u32,
    pub used_bytes: u64,
    pub process_count: u32,
    pub root_bytes: u64,
}
pub fn read_engine(pid: Option<u32>) -> Option<EngineSample> {
    #[cfg(target_os = "macos")]
    {
        let pid = pid.filter(|p| *p > 0 && *p <= i32::MAX as u32)?;
        fn footprint(pid: i32) -> Option<u64> {
            let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
            let status = unsafe {
                libc::proc_pid_rusage(
                    pid,
                    libc::RUSAGE_INFO_V2,
                    &mut info as *mut _ as *mut libc::rusage_info_t,
                )
            };
            (status == 0).then_some(info.ri_phys_footprint)
        }
        let root = pid as i32;
        let root_bytes = footprint(root)?;
        let mut used_bytes = root_bytes;
        let mut process_count = 1;
        fn parent_pid(pid: i32) -> Option<u32> {
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
            (read == size).then_some(info.pbi_ppid)
        }
        // Strata 0.2.x isolates the Metal child in its own group. Follow parent
        // identities instead of process groups, including the lifeline watcher.
        let mut pending = vec![pid];
        let mut seen = std::collections::HashSet::from([pid]);
        while let Some(parent) = pending.pop() {
            let mut children = [0i32; 256];
            // PROC_PPID_ONLY from the macOS SDK sys/proc_info.h; returns bytes.
            let bytes = unsafe {
                libc::proc_listpids(
                    6,
                    parent,
                    children.as_mut_ptr().cast(),
                    std::mem::size_of_val(&children) as i32,
                )
            };
            if bytes < 0 || bytes as usize >= std::mem::size_of_val(&children) {
                return None;
            }
            let count = bytes as usize / std::mem::size_of::<i32>();
            for child in children[..count].iter().copied().filter(|&p| p > 0) {
                if parent_pid(child) != Some(parent) {
                    continue;
                }
                if !seen.insert(child as u32) {
                    continue;
                }
                if seen.len() > 256 {
                    return None;
                }
                if let Some(used) = footprint(child) {
                    used_bytes = used_bytes.saturating_add(used);
                    process_count += 1;
                    pending.push(child as u32);
                } else if parent_pid(child) == Some(parent) {
                    return None; // Never show a partial total for a live child.
                }
            }
        }
        footprint(root)?; // The owned server must still be alive.
        Some(EngineSample {
            sampled_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            pid,
            used_bytes,
            process_count,
            root_bytes,
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

const HISTORY_MS: u64 = 10 * 60 * 1000;
const MAX_POINTS: usize = 620;
const MAX_EVENTS: usize = 120;
fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Point {
    at: u64,
    mac_used_bytes: Option<u64>,
    engine_used_bytes: Option<u64>,
    pid: Option<u32>,
    inferring: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Event {
    at: u64,
    kind: &'static str,
    context: Option<u32>,
}

#[derive(Default)]
pub struct Monitor {
    current: Option<Sample>,
    baseline: Option<Sample>,
    refreshed: Option<Instant>,
    engine: Option<EngineSample>,
    engine_baseline: Option<EngineSample>,
    pid: Option<u32>,
    history: VecDeque<Point>,
    events: VecDeque<Event>,
    inferences: usize,
}
impl Monitor {
    fn prune(&mut self, now: u64) {
        while self
            .history
            .front()
            .is_some_and(|p| p.at < now.saturating_sub(HISTORY_MS))
            || self.history.len() > MAX_POINTS
        {
            self.history.pop_front();
        }
        while self
            .events
            .front()
            .is_some_and(|p| p.at < now.saturating_sub(HISTORY_MS))
            || self.events.len() > MAX_EVENTS
        {
            self.events.pop_front();
        }
    }
    fn record(&mut self) {
        let now = timestamp();
        self.history.push_back(Point {
            at: now,
            mac_used_bytes: self.current.as_ref().map(|s| s.used_bytes),
            engine_used_bytes: self.engine.as_ref().map(|s| s.used_bytes),
            pid: self.pid,
            inferring: self.inferences > 0,
        });
        self.prune(now);
    }
    pub fn event(&mut self, kind: &'static str, context: Option<u32>) {
        let now = timestamp();
        self.events.push_back(Event {
            at: now,
            kind,
            context,
        });
        self.prune(now);
    }
    pub fn inference(&mut self, started: bool) {
        if started {
            self.inferences += 1;
            self.event("inferenceStart", None);
        } else {
            self.inferences = self.inferences.saturating_sub(1);
            self.event("inferenceEnd", None);
        }
        // Sample on the next tick even for very short replies.
        self.refreshed = None;
    }
    pub fn refresh(&mut self, total: Option<u64>, pid: Option<u32>) {
        if self.pid != pid
            || self
                .refreshed
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(1))
        {
            if self.pid != pid {
                self.event(
                    if pid.is_some() {
                        "engineStart"
                    } else {
                        "engineStop"
                    },
                    None,
                );
            }
            self.current = read(total);
            self.engine = read_engine(pid);
            self.pid = pid;
            self.refreshed = Some(Instant::now());
            self.record();
        }
    }

    pub fn capture(&mut self, total: Option<u64>, pid: Option<u32>) {
        self.current = read(total);
        self.baseline = self.current.clone();
        self.engine = read_engine(pid);
        self.engine_baseline = self.engine.clone();
        self.pid = pid;
        self.refreshed = Some(Instant::now());
        self.record();
    }
    pub fn snapshot(&mut self, total: Option<u64>, pid: Option<u32>) -> serde_json::Value {
        self.refresh(total, pid);
        serde_json::json!({"current":self.current,"beforeChange":self.baseline,"engine":{"current":self.engine,"beforeChange":self.engine_baseline},"history":self.history,"events":self.events})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn available_only_counts_reclaimable_file_pages_and_stays_bounded() {
        let s = calculate(1000, 10, 5, 2, 40, 4, 10);
        assert_eq!(s.free_bytes, 50);
        assert_eq!(s.available_bytes, 170);
        assert_eq!(s.used_bytes, 830);
        let s = calculate(100, u64::MAX, u64::MAX, u64::MAX, 1, 1, 1);
        assert_eq!(s.available_bytes, 100);
        assert_eq!(s.used_bytes, 0);
        assert_eq!(s.free_bytes, 100);
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn engine_sample_tracks_owned_pid_and_preserves_baseline_on_stop() {
        let pid = std::process::id();
        let mut m = Monitor::default();
        m.capture(Some(32 << 30), Some(pid));
        let before = m.snapshot(Some(32 << 30), Some(pid));
        assert!(before["engine"]["current"]["usedBytes"].as_u64().unwrap() > 0);
        let stopped = m.snapshot(Some(32 << 30), None);
        assert!(stopped["engine"]["current"].is_null());
        assert_eq!(
            before["engine"]["current"],
            stopped["engine"]["beforeChange"]
        );
        assert!(read_engine(Some(u32::MAX)).is_none());
        assert!(read_engine(None).is_none());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn engine_sample_includes_native_children_but_not_other_trees() {
        use std::os::unix::process::CommandExt;
        struct Group(std::process::Child);
        impl Drop for Group {
            fn drop(&mut self) {
                unsafe {
                    libc::kill(-(self.0.id() as i32), libc::SIGKILL);
                }
                let _ = self.0.wait();
            }
        }
        let owned = Group(
            std::process::Command::new("/bin/sh")
                .args(["-c", "sleep 30 & wait"])
                .process_group(0)
                .spawn()
                .unwrap(),
        );
        let unrelated = Group(
            std::process::Command::new("/bin/sleep")
                .arg("30")
                .process_group(0)
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        let sample = loop {
            if let Some(sample) = read_engine(Some(owned.0.id())) {
                if sample.process_count == 2 {
                    break sample;
                }
            }
            assert!(Instant::now() < deadline, "native child was not counted");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(sample.used_bytes > sample.root_bytes);
        assert_eq!(sample.pid, owned.0.id());
        assert_eq!(
            read_engine(Some(unrelated.0.id())).unwrap().process_count,
            1
        );
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn engine_sample_includes_child_in_a_separate_group() {
        use std::os::unix::process::CommandExt;
        // Python mirrors serve.lifeline: its engine child has a private group.
        let mut child = std::process::Command::new("/usr/bin/python3")
            .args(["-c", "import subprocess,time,signal; p=subprocess.Popen(['/bin/sleep','10'],start_new_session=True); signal.signal(signal.SIGTERM,lambda *a:(p.terminate(),p.wait(),exit(0))); time.sleep(10)"])
            .process_group(0).spawn().unwrap();
        let pid = child.id();
        let deadline = Instant::now() + Duration::from_secs(3);
        let found = loop {
            if read_engine(Some(pid))
                .is_some_and(|s| s.process_count >= 2 && s.used_bytes > s.root_bytes)
            {
                break true;
            }
            if Instant::now() > deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        // Let Python terminate/reap its isolated child on orderly shutdown.
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
        let _ = child.wait();
        assert!(found, "isolated native child was not included");
    }
    #[test]
    fn history_is_bounded_and_keeps_missing_engine_as_a_gap() {
        let mut m = Monitor::default();
        for _ in 0..700 {
            m.record();
            m.event("settings", Some(4096));
        }
        assert_eq!(m.history.len(), MAX_POINTS);
        assert_eq!(m.events.len(), MAX_EVENTS);
        assert!(m.history.back().unwrap().engine_used_bytes.is_none());
        m.prune(timestamp() + HISTORY_MS + 1);
        assert!(m.history.is_empty() && m.events.is_empty());
    }
    #[test]
    fn overlapping_inferences_remain_active_until_last_request_ends() {
        let mut m = Monitor::default();
        m.inference(true);
        m.inference(true);
        m.inference(false);
        m.record();
        assert!(m.history.back().unwrap().inferring);
        m.inference(false);
        m.record();
        assert!(!m.history.back().unwrap().inferring);
        assert_eq!(m.events.len(), 4);
    }
    #[test]
    fn unknown_capacity_never_reports_a_fake_zero() {
        assert!(read(None).is_none());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn native_sample_is_nonzero_and_cached_without_moving_baseline() {
        let mut m = Monitor::default();
        m.capture(Some(32 << 30), None);
        let a = m.snapshot(Some(32 << 30), None);
        let b = m.snapshot(Some(32 << 30), None);
        assert!(a["current"]["sampledAt"].as_u64().unwrap() > 0);
        assert_eq!(a, b);
        assert_eq!(a["current"], a["beforeChange"]);
    }
}
