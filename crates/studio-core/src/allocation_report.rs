//! Scalar allocation reports from one owned Gemma stderr stream. Not RSS/peak usage.
use crate::{engine::Engine, Settings};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read, sync::Arc};
use tokio::sync::Mutex;

#[derive(Clone, Default, Debug)]
pub(crate) struct Collector {
    kv: Vec<u64>,
    compute: BTreeMap<String, u64>,
    output: BTreeMap<String, u64>,
    invalid: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub session: String,
    pub pid: u32,
    pub kv_bytes: Option<u64>,
    pub compute_bytes: Option<u64>,
    pub output_bytes: Option<u64>,
    pub workspace_bytes: Option<u64>,
}
fn mib(value: &str) -> Option<u64> {
    if value.len() > 16
        || value.is_empty()
        || !value.chars().all(|c| c.is_ascii_digit() || c == '.')
    {
        return None;
    }
    let n = value.parse::<f64>().ok()?;
    (n.is_finite() && (0.0..=1048576.0).contains(&n)).then(|| (n * 1048576.0).round() as u64)
}
fn sum(values: impl Iterator<Item = u64>) -> Option<u64> {
    let mut values = values.peekable();
    values.peek()?;
    values.try_fold(0u64, |a, b| a.checked_add(b))
}
impl Collector {
    pub fn observe(&mut self, line: &str) {
        if line.len() > 4096 {
            return;
        }
        // common/log.cpp prefixes native INFO messages with an optional timestamp.
        let mut text = line.trim_start();
        if let Some((first, rest)) = text.split_once(' ') {
            if first.len() <= 32
                && first.split('.').count() == 4
                && first
                    .split('.')
                    .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
            {
                text = rest.trim_start();
            }
        }
        if let Some(rest) = text.strip_prefix("I ") {
            text = rest.trim_start();
        }
        let Some((prefix, rest)) = text.split_once(':') else {
            return;
        };
        let category = if prefix == "llama_kv_cache" && rest.contains(" KV buffer size =") {
            "kv"
        } else if prefix == "sched_reserve" && rest.contains(" compute buffer size =") {
            "compute"
        } else if prefix == "llama_context" && rest.contains("output buffer size =") {
            "output"
        } else {
            return;
        };
        let Some((device, value)) = rest.trim().split_once(match category {
            "kv" => " KV buffer size =",
            "compute" => " compute buffer size =",
            _ => "output buffer size =",
        }) else {
            return;
        };
        let device = device.trim();
        if device.is_empty()
            || device.len() > 32
            || !device
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            self.invalid = true;
            return;
        }
        let Some(value) = value
            .trim()
            .strip_suffix(" MiB")
            .and_then(|v| mib(v.trim()))
        else {
            self.invalid = true;
            return;
        };
        if category == "kv" {
            // SWA and full-attention caches legitimately report the same device twice.
            if self.kv.len() >= 16 {
                self.invalid = true;
                return;
            }
            self.kv.push(value);
        } else {
            let map = if category == "compute" {
                &mut self.compute
            } else {
                &mut self.output
            };
            if !map.contains_key(device) && map.len() >= 16 {
                self.invalid = true;
                return;
            }
            // Graph reallocation reports replace that backend's previous allocation.
            map.insert(device.into(), value);
        }
    }
    pub fn report(&self, session: String, pid: u32) -> Report {
        let (kv, compute, output) = if self.invalid {
            (None, None, None)
        } else {
            (
                sum(self.kv.iter().copied()),
                sum(self.compute.values().copied()),
                sum(self.output.values().copied()),
            )
        };
        Report {
            session,
            pid,
            kv_bytes: kv,
            compute_bytes: compute,
            output_bytes: output,
            workspace_bytes: compute.zip(output).and_then(|(c, o)| c.checked_add(o)),
        }
    }
}
fn digest(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}
#[derive(Clone)]
pub(crate) struct Session {
    pub collector: Arc<Mutex<Collector>>,
    id: String,
    pid: u32,
    root: String,
    config: String,
    hash: Vec<u8>,
}
impl Session {
    pub fn new(settings: &Settings, bytes: &[u8], pid: u32) -> Self {
        Self {
            collector: Arc::new(Mutex::new(Collector::default())),
            id: uuid::Uuid::new_v4().to_string(),
            pid,
            root: settings.strata_root.clone(),
            config: settings.config.clone(),
            hash: digest(bytes),
        }
    }
    pub fn same_report(&self, report: &Report) -> bool {
        self.id == report.session && self.pid == report.pid
    }
    pub async fn snapshot(&self, settings: &Settings) -> Option<Report> {
        if settings.mock
            || settings.engine != Engine::Strata
            || settings.strata_root != self.root
            || settings.config != self.config
        {
            return None;
        }
        // Bind the display to the actual launched config, including in-place edits.
        let mut bytes = Vec::new();
        if !std::fs::metadata(&self.config).ok()?.is_file() {
            return None;
        }
        let f = std::fs::File::open(&self.config).ok()?;
        if !f.metadata().ok()?.is_file() {
            return None;
        }
        f.take(1024 * 1024 + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() > 1024 * 1024 || digest(&bytes) != self.hash {
            return None;
        }
        let report = self
            .collector
            .lock()
            .await
            .report(self.id.clone(), self.pid);
        (report.kv_bytes.is_some()
            || report.compute_bytes.is_some()
            || report.output_bytes.is_some())
        .then_some(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn swa_and_backends_sum_but_compute_replacements_do_not_accumulate() {
        let mut c = Collector::default();
        c.observe("llama_kv_cache:      Metal KV buffer size =    8.00 MiB");
        c.observe("llama_kv_cache:      Metal KV buffer size =   22.00 MiB");
        c.observe("sched_reserve:        CPU compute buffer size =    2.00 MiB");
        c.observe("sched_reserve:      Metal compute buffer size =  100.00 MiB");
        c.observe("0.00.111.234 I sched_reserve:      Metal compute buffer size =  120.00 MiB");
        c.observe("llama_context: Metal_Host  output buffer size = 1.00 MiB");
        let r = c.report("fixture".into(), 1);
        assert_eq!(r.kv_bytes, Some(30 << 20));
        assert_eq!(r.compute_bytes, Some(122 << 20));
        assert_eq!(r.workspace_bytes, Some(123 << 20));
    }
    #[test]
    fn missing_malformed_and_unrelated_lines_never_become_zero() {
        let mut c = Collector::default();
        c.observe("assistant: Metal compute buffer size = 999 MiB");
        assert!(c.report("x".into(), 1).compute_bytes.is_none());
        c.observe("sched_reserve: CPU compute buffer size = 0.00 MiB");
        assert_eq!(c.report("x".into(), 1).compute_bytes, Some(0));
        assert!(c.report("x".into(), 1).workspace_bytes.is_none());
        c.observe("sched_reserve: Metal compute buffer size = NaN MiB");
        assert!(c.report("x".into(), 1).compute_bytes.is_none());
        for bad in ["-1", "inf", "1e12", "99999999999999999999"] {
            assert!(mib(bad).is_none());
        }
    }
    #[tokio::test]
    async fn sessions_are_isolated_and_config_edits_invalidate() {
        let path = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::write(&path, b"{}").unwrap();
        let mut settings = Settings::default();
        settings.config = path.to_string_lossy().into();
        let a = Session::new(&settings, b"{}", 123);
        let b = Session::new(&settings, b"{}", 124);
        a.collector
            .lock()
            .await
            .observe("sched_reserve: CPU compute buffer size = 1.00 MiB");
        assert!(a.snapshot(&settings).await.is_some());
        assert!(b.snapshot(&settings).await.is_none());
        std::fs::write(&path, b"{\"changed\":true}").unwrap();
        assert!(a.snapshot(&settings).await.is_none());
        std::fs::remove_file(path).unwrap();
    }
}
