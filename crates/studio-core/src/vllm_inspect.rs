//! On-demand metadata/evidence inspection; never loads a model or relaxes a gate.
use crate::{engine::VllmSettings, Studio};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::AsyncReadExt;

pub(crate) struct Cached {
    pub settings: VllmSettings,
    pub result: Inspection,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub status: String,
    pub reason: String,
    pub architecture: Option<String>,
    pub fits_memory: Option<bool>,
    pub estimated_resident_bytes: Option<u64>,
    pub context: u32,
    pub checked_at: u64,
}
pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
impl Inspection {
    pub fn from_report(v: &Value, settings: &VllmSettings, success: bool) -> Self {
        let detail = v["detail"].as_str().unwrap_or("");
        let reason = if detail.contains("identity mismatch") {
            "evidence_identity_mismatch"
        } else if detail.contains("expired") {
            "evidence_expired"
        } else if detail.contains("workload exceeds") {
            "evidence_limit_exceeded"
        } else if detail.contains("report digest mismatch") {
            "evidence_corrupt"
        } else if detail.contains("owner-only") {
            "evidence_permissions"
        } else if detail.contains("fingerprint probe failed") {
            "backend_fingerprint_failed"
        } else if v["fits_memory"] == false {
            "model_memory_budget"
        } else if (!success && !(v["schema_version"] == 2 && v["qualification"] == "unverified"))
            || v["error_code"].is_string()
        {
            "inspection_failed"
        } else if success
            && v["runnable"] == true
            && v["qualification"] == "text_smoke_30min"
            && !settings.evidence.is_empty()
        {
            "identity_bound_text_smoke"
        } else {
            "unverified"
        };
        let status = match reason {
            "identity_bound_text_smoke" => "verified",
            "unverified" => "unverified",
            _ => "blocked",
        };
        Self {
            status: status.into(),
            reason: reason.into(),
            architecture: v["architecture"]
                .as_str()
                .filter(|s| {
                    s.len() <= 128
                        && s.chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                })
                .map(str::to_string),
            fits_memory: v["fits_memory"].as_bool(),
            estimated_resident_bytes: v["estimated_resident_bytes"].as_u64(),
            context: settings.context,
            checked_at: now(),
        }
    }
}
impl Studio {
    pub async fn inspect_vllm(&self) -> Result<(), String> {
        let _lifecycle = self.lifecycle.lock().await;
        if self.preparation.lock().await.busy()
            || self.document.read().await.generating
            || self
                .active_requests
                .load(std::sync::atomic::Ordering::SeqCst)
                != 0
        {
            return Err("Wait for active requests and preparation to finish".into());
        }
        {
            let mut process = self.process.lock().await;
            if process
                .child
                .as_mut()
                .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
            {
                return Err("Stop the engine before inspecting its environment".into());
            }
        }
        *self.vllm_inspection.lock().await = None;
        let settings = self.settings.read().await.vllm.clone();
        let mut cmd = settings.inspection_command()?;
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd
            .spawn()
            .map_err(|_| "Cannot launch vLLM-Apple inspection")?;
        let group = child.id();
        let mut stdout = child.stdout.take().unwrap().take(262145);
        let mut stderr = child.stderr.take().unwrap().take(16385);
        let mut out = Vec::new();
        let mut err = Vec::new();
        let result = tokio::time::timeout(Duration::from_secs(150), async {
            tokio::try_join!(
                stdout.read_to_end(&mut out),
                stderr.read_to_end(&mut err),
                child.wait()
            )
        })
        .await;
        let status = match result {
            Ok(Ok((_, _, status))) if out.len() <= 262144 && err.len() <= 16384 => status,
            _ => {
                #[cfg(unix)]
                if let Some(id) = group {
                    unsafe {
                        libc::kill(-(id as i32), libc::SIGKILL);
                    }
                }
                let _ = child.kill().await;
                return Err("vLLM-Apple inspection timed out or exceeded its output limit".into());
            }
        };
        let report: Value =
            serde_json::from_slice(&out).map_err(|_| "Invalid vLLM-Apple inspection response")?;
        let result = Inspection::from_report(&report, &settings, status.success());
        *self.vllm_inspection.lock().await = Some(Cached { settings, result });
        Ok(())
    }
    pub(crate) async fn inspection_snapshot(&self, settings: &VllmSettings) -> Value {
        self.vllm_inspection
            .lock()
            .await
            .as_ref()
            .filter(|cached| {
                &cached.settings == settings
                    && now().saturating_sub(cached.result.checked_at) <= 300_000
            })
            .map(|cached| json!(cached.result))
            .unwrap_or(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_requires_bound_evidence_and_never_exposes_process_details() {
        let mut settings = VllmSettings::default();
        let report = json!({"runnable":true,"qualification":"text_smoke_30min","fits_memory":true,"architecture":"Gemma2ForCausalLM","estimated_resident_bytes":123});
        assert_eq!(
            Inspection::from_report(&report, &settings, true).status,
            "unverified"
        );
        assert_eq!(Inspection::from_report(&json!({"schema_version":2,"qualification":"unverified","runnable":false,"fits_memory":true}), &settings, false).status, "unverified");
        settings.evidence = "/private/evidence.json".into();
        assert_eq!(
            Inspection::from_report(&report, &settings, true).status,
            "verified"
        );
        let failure = Inspection::from_report(
            &json!({"error_code":"model_inspection_failed","detail":"architecture evidence identity mismatch PRIVATE"}),
            &settings,
            false,
        );
        assert_eq!(failure.reason, "evidence_identity_mismatch");
        assert!(!serde_json::to_string(&failure).unwrap().contains("PRIVATE"));
        assert_eq!(
            Inspection::from_report(&json!({"fits_memory":false}), &settings, true).reason,
            "model_memory_budget"
        );
        assert_eq!(
            Inspection::from_report(
                &json!({"detail":"architecture evidence expired"}),
                &settings,
                false
            )
            .reason,
            "evidence_expired"
        );
    }
    #[tokio::test]
    async fn shared_diagnostic_disappears_after_settings_change_or_expiry() {
        let studio = Studio::new(std::path::PathBuf::from("/tmp/Strata"), 0);
        let settings = studio.settings.read().await.vllm.clone();
        let mut result = Inspection::from_report(&json!({}), &settings, true);
        *studio.vllm_inspection.lock().await = Some(Cached {
            settings: settings.clone(),
            result: result.clone(),
        });
        assert!(!studio.inspection_snapshot(&settings).await.is_null());
        let mut changed = settings.clone();
        changed.context += 1;
        assert!(studio.inspection_snapshot(&changed).await.is_null());
        result.checked_at = 0;
        *studio.vllm_inspection.lock().await = Some(Cached {
            settings: settings.clone(),
            result,
        });
        assert!(studio.inspection_snapshot(&settings).await.is_null());
    }
}
