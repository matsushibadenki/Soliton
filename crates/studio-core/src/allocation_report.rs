//! Scalar allocation reports from an owned native engine stderr stream.
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
    recurrent: BTreeMap<String, u64>,
    projector_max: Option<u64>,
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
    pub recurrent_bytes: Option<u64>,
    pub projector_max_buffer_bytes: Option<u64>,
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
        } else if prefix == "llama_memory_recurrent" && rest.contains(" RS buffer size =") {
            "recurrent"
        } else if prefix == "reserve_compute_meta" && rest.contains(" compute buffer size =") {
            "projector"
        } else {
            return;
        };
        let Some((device, value)) = rest.trim().split_once(match category {
            "kv" => " KV buffer size =",
            "compute" | "projector" => " compute buffer size =",
            "recurrent" => " RS buffer size =",
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
        if category == "projector" {
            // The log omits graph identity, so only one reported allocation can be compared.
            self.projector_max = Some(
                self.projector_max
                    .map_or(value, |previous| previous.max(value)),
            );
        } else if category == "kv" {
            // SWA and full-attention caches legitimately report the same device twice.
            if self.kv.len() >= 16 {
                self.invalid = true;
                return;
            }
            self.kv.push(value);
        } else {
            let map = match category {
                "compute" => &mut self.compute,
                "recurrent" => &mut self.recurrent,
                _ => &mut self.output,
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
            recurrent_bytes: (!self.invalid)
                .then(|| sum(self.recurrent.values().copied()))
                .flatten(),
            projector_max_buffer_bytes: if self.invalid {
                None
            } else {
                self.projector_max
            },
            workspace_bytes: compute.zip(output).and_then(|(c, o)| c.checked_add(o)),
        }
    }
}
fn digest(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}
#[derive(Clone)]
enum Binding {
    Strata {
        root: String,
        config: String,
        hash: Vec<u8>,
    },
    Omni(crate::omni_runtime::Settings),
}

#[derive(Clone)]
pub(crate) struct Session {
    pub collector: Arc<Mutex<Collector>>,
    id: String,
    pid: u32,
    binding: Binding,
}
impl Session {
    pub fn new(settings: &Settings, bytes: &[u8], pid: u32) -> Self {
        Self::bound(
            pid,
            Binding::Strata {
                root: settings.strata_root.clone(),
                config: settings.config.clone(),
                hash: digest(bytes),
            },
        )
    }
    pub fn omni(settings: &crate::omni_runtime::Settings, pid: u32) -> Self {
        Self::bound(pid, Binding::Omni(settings.clone()))
    }
    fn bound(pid: u32, binding: Binding) -> Self {
        Self {
            collector: Arc::new(Mutex::new(Collector::default())),
            id: uuid::Uuid::new_v4().to_string(),
            pid,
            binding,
        }
    }
    pub fn same_report(&self, report: &Report) -> bool {
        self.id == report.session && self.pid == report.pid
    }
    pub async fn snapshot(&self, settings: &Settings) -> Option<Report> {
        match &self.binding {
            Binding::Strata { root, config, hash } => {
                if settings.mock
                    || settings.engine != Engine::Strata
                    || &settings.strata_root != root
                    || &settings.config != config
                {
                    return None;
                }
                // Bind the display to the actual launched config, including in-place edits.
                if !std::fs::metadata(config).ok()?.is_file() {
                    return None;
                }
                let file = std::fs::File::open(config).ok()?;
                if !file.metadata().ok()?.is_file() {
                    return None;
                }
                let mut bytes = Vec::new();
                file.take(1024 * 1024 + 1).read_to_end(&mut bytes).ok()?;
                if bytes.len() > 1024 * 1024 || &digest(&bytes) != hash {
                    return None;
                }
            }
            Binding::Omni(config) => {
                if settings.engine != Engine::Lfm3Omni || &settings.omni != config {
                    return None;
                }
            }
        }
        let report = self
            .collector
            .lock()
            .await
            .report(self.id.clone(), self.pid);
        (report.kv_bytes.is_some()
            || report.compute_bytes.is_some()
            || report.output_bytes.is_some()
            || report.recurrent_bytes.is_some()
            || report.projector_max_buffer_bytes.is_some())
        .then_some(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn omni_report_only_matches_its_launch_configuration() {
        let mut settings = Settings {
            engine: Engine::Lfm3Omni,
            ..Settings::default()
        };
        settings.omni.context = 4096;
        let session = Session::omni(&settings.omni, 123);
        assert!(session.snapshot(&settings).await.is_none());

        session
            .collector
            .lock()
            .await
            .observe("llama_kv_cache: Metal KV buffer size = 16.00 MiB");
        let report = session.snapshot(&settings).await.unwrap();
        assert_eq!(report.kv_bytes, Some(16 * 1024 * 1024));
        assert!(session.same_report(&report));
        assert!(!Session::omni(&settings.omni, 123).same_report(&report));

        settings.omni.context = 2048;
        assert!(session.snapshot(&settings).await.is_none());
        settings.omni.context = 4096;
        settings.engine = Engine::Strata;
        assert!(session.snapshot(&settings).await.is_none());
    }
    #[test]
    fn recurrent_and_projector_reports_do_not_inflate_core_workspace() {
        let mut collector = Collector::default();
        for line in [
            "llama_memory_recurrent: MTL0 RS buffer size = 1.00 MiB",
            "reserve_compute_meta: MTL0 compute buffer size = 8.00 MiB",
            "reserve_compute_meta: CPU compute buffer size = 2.00 MiB",
            "reserve_compute_meta: MTL0 compute buffer size = 4.00 MiB",
            "sched_reserve: MTL0 compute buffer size = 16.00 MiB",
            "llama_context: CPU output buffer size = 1.00 MiB",
        ] {
            collector.observe(line);
        }
        let report = collector.report("test".into(), 1);
        assert_eq!(report.recurrent_bytes, Some(1048576));
        assert_eq!(report.projector_max_buffer_bytes, Some(8 * 1048576));
        assert_eq!(report.workspace_bytes, Some(17 * 1048576));
    }
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
