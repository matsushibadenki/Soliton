//! Normalize existing vLLM runtime ledgers without inferring additive physical memory.
use serde::Serialize;
use serde_json::Value;
const LIMIT: u64 = 1 << 50;
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    pub name: &'static str,
    pub bytes: u64,
    pub basis: &'static str,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub sampled_at: Option<u64>,
    pub allocator_bytes: Option<u64>,
    pub allocator_peak_bytes: Option<u64>,
    pub kv_bytes: Option<u64>,
    pub components: Vec<Component>,
}
fn number(v: &Value) -> Option<u64> {
    v.as_u64().filter(|n| *n <= LIMIT)
}
fn source(v: &Value) -> Option<&str> {
    v.as_str().filter(|s| {
        !s.is_empty()
            && s.len() <= 80
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | ':'))
    })
}
pub fn parse(health: &Value) -> Option<Reading> {
    let telemetry = &health["memory_telemetry"];
    let allocator_source = source(&telemetry["allocator_source"]);
    let allocator = allocator_source.and_then(|_| number(&telemetry["allocator_current_bytes"]));
    let peak = allocator_source
        .and_then(|_| number(&telemetry["allocator_peak_bytes"]))
        .filter(|peak| allocator.is_none_or(|current| *peak >= current));
    let kv = source(&telemetry["kv_source"]).and_then(|_| number(&telemetry["kv_used_bytes"]));
    let mut components = Vec::new();
    for name in [
        "weights",
        "recurrent",
        "prefix",
        "window",
        "experts",
        "ngram",
        "mtp",
        "scratch",
        "coreml",
    ] {
        let value = &health["memory_budget"]["components"][name];
        let Some(bytes) = number(&value["current_bytes"]) else {
            continue;
        };
        let Some(src) = source(&value["source"]) else {
            continue;
        };
        if value["accounting"] != "additive" {
            continue;
        }
        let basis = match src {
            "model_spec" => "model_spec",
            "model_weight_files" => "artifact",
            "request_projection" => "projection",
            "state_spec+semantic_state"
            | "semantic_state"
            | "state_spec+scheduler_reservations"
            | "scheduler_reservations" => "ledger",
            _ => continue,
        };
        components.push(Component { name, bytes, basis });
    }
    // Deliberately omit kv/metal_heap from components: they overlap telemetry.
    // Never add allocator + KV + components or subtract them to invent workspace.
    (allocator.is_some() || peak.is_some() || kv.is_some() || !components.is_empty()).then_some(
        Reading {
            sampled_at: None,
            allocator_bytes: allocator,
            allocator_peak_bytes: peak,
            kv_bytes: kv,
            components,
        },
    )
}
pub(crate) struct Sample {
    pub reading: Reading,
    pub at: std::time::Instant,
}
impl Sample {
    pub fn snapshot(&self) -> Option<Reading> {
        (self.at.elapsed() < std::time::Duration::from_secs(30)).then(|| self.reading.clone())
    }
}
impl crate::Studio {
    pub async fn refresh_backend_budget(&self) -> Result<(), String> {
        let settings = self.settings.read().await.clone();
        if settings.engine != crate::engine::Engine::VllmApple {
            return Err("Select vLLM-Apple for runtime diagnostics".into());
        }
        let request = uuid::Uuid::new_v4().to_string();
        let (port, key, pid) = {
            let mut p = self.process.lock().await;
            p.backend_budget = None;
            p.backend_budget_request = Some(request.clone());
            if p.vllm_memory_binding.as_ref() != Some(&settings.vllm) {
                return Err(
                    "Restart the engine with the saved settings before refreshing diagnostics"
                        .into(),
                );
            }
            let pid = p
                .child
                .as_mut()
                .and_then(|c| {
                    if matches!(c.try_wait(), Ok(None)) {
                        c.id()
                    } else {
                        None
                    }
                })
                .ok_or("Start the selected engine before refreshing diagnostics")?;
            (p.port, p.key.clone(), pid)
        };
        let mut reading = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let mut response = self
                .client
                .get(format!("http://127.0.0.1:{port}/v1/runtime"))
                .bearer_auth(&key)
                .send()
                .await
                .map_err(|_| "Runtime diagnostic request failed")?;
            if !response.status().is_success() {
                return Err("Runtime diagnostic request was rejected");
            }
            if response.content_length().is_some_and(|n| n > 65536) {
                return Err("Runtime diagnostic response exceeds 64 KiB");
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| "Runtime diagnostic response failed")?
            {
                if bytes.len() + chunk.len() > 65536 {
                    return Err("Runtime diagnostic response exceeds 64 KiB");
                }
                bytes.extend_from_slice(&chunk);
            }
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|_| "Invalid runtime diagnostic response")?;
            parse(&value).ok_or("Runtime memory diagnostics are unavailable")
        })
        .await
        .map_err(|_| "Runtime diagnostic request timed out")?
        .map_err(str::to_string)?;
        reading.sampled_at = Some(crate::vllm_inspect::now());
        let current = self.settings.read().await.clone();
        let mut p = self.process.lock().await;
        let alive = p
            .child
            .as_mut()
            .is_some_and(|c| c.id() == Some(pid) && matches!(c.try_wait(), Ok(None)));
        if current.engine != settings.engine
            || current.vllm != settings.vllm
            || p.port != port
            || p.key != key
            || !alive
            || p.backend_budget_request.as_deref() != Some(request.as_str())
        {
            return Err("Engine session changed; refresh diagnostics again".into());
        }
        p.backend_budget_request = None;
        p.backend_budget = Some(Sample {
            reading,
            at: std::time::Instant::now(),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[cfg(unix)]
    #[tokio::test]
    async fn shared_snapshot_requires_live_bound_process_and_clears_on_stop() {
        use crate::{engine::Engine, Studio};
        use std::sync::Arc;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handler = axum::routing::get(|| async {
            axum::Json(
                json!({"control_ready":false,"inference_ready":false,"memory_telemetry":{"allocator_source":"mlx-wrapper-v1","allocator_current_bytes":100,"allocator_peak_bytes":120,"kv_source":"mlx-wrapper-v1","kv_used_bytes":0}}),
            )
        });
        let app = axum::Router::new()
            .route("/health", handler.clone())
            .route("/v1/runtime", handler);
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let studio = Arc::new(Studio::new(
            std::env::temp_dir().join("soliton-budget-fixture"),
            0,
        ));
        studio.settings.write().await.engine = Engine::VllmApple;
        let binding = studio.settings.read().await.vllm.clone();
        let mut cmd = tokio::process::Command::new("/bin/sleep");
        cmd.arg("30").kill_on_drop(true).process_group(0);
        let child = cmd.spawn().unwrap();
        {
            let mut p = studio.process.lock().await;
            p.group = child.id();
            p.child = Some(child);
            p.port = port;
            p.key = "fixture".into();
            p.vllm_memory_binding = Some(binding.clone());
        }
        studio.refresh_backend_budget().await.unwrap();
        let state = studio.snapshot().await;
        assert_eq!(state["modelPlan"]["backendBudget"]["allocatorBytes"], 100);
        assert_eq!(state["modelPlan"]["backendBudget"]["kvBytes"], 0);
        {
            let mut p = studio.process.lock().await;
            p.backend_budget.as_mut().unwrap().at =
                std::time::Instant::now() - std::time::Duration::from_secs(31);
        }
        assert!(studio.snapshot().await["modelPlan"]["backendBudget"].is_null());
        studio.refresh_backend_budget().await.unwrap();
        studio.settings.write().await.vllm.context += 1;
        assert!(studio.snapshot().await["modelPlan"]["backendBudget"].is_null());
        studio.settings.write().await.vllm = binding;
        studio.process.lock().await.port = 0;
        assert!(studio.refresh_backend_budget().await.is_err());
        assert!(studio.process.lock().await.backend_budget.is_none());
        studio.stop().await.unwrap();
        assert!(studio.snapshot().await["modelPlan"]["backendBudget"].is_null());
        server.abort();
    }
    #[test]
    fn preserves_zero_and_provenance_without_adding_overlap() {
        let h = json!({"memory_telemetry":{"allocator_source":"mlx-wrapper-v1","allocator_current_bytes":100,"allocator_peak_bytes":120,"kv_source":"mlx-wrapper-v1","kv_used_bytes":10},"memory_budget":{"components":{"mtp":{"current_bytes":0,"source":"model_spec","accounting":"additive"},"scratch":{"current_bytes":20,"source":"state_spec+scheduler_reservations","accounting":"additive"},"metal_heap":{"current_bytes":100,"source":"mlx-wrapper-v1","accounting":"overlap_envelope"}}}});
        let r = parse(&h).unwrap();
        assert_eq!(r.allocator_bytes, Some(100));
        assert_eq!(r.kv_bytes, Some(10));
        assert_eq!(r.components.len(), 2);
        assert_eq!(r.components[0].bytes, 0);
        assert_eq!(r.components[0].basis, "model_spec");
        assert_eq!(r.components[1].basis, "ledger");
    }
    #[test]
    fn invalid_values_missing_sources_and_unknown_schemas_are_unknown() {
        assert!(parse(&json!({})).is_none());
        let artifact=parse(&json!({"memory_budget":{"components":{"weights":{"current_bytes":42,"source":"model_weight_files","accounting":"additive"}}}})).unwrap();
        assert_eq!(artifact.components[0].basis, "artifact");
        for bad in [json!(-1), json!(true), json!(1.5), json!(u64::MAX)] {
            assert!(parse(&json!({"memory_telemetry":{"allocator_source":"mlx-wrapper-v1","allocator_current_bytes":bad}})).is_none());
        }
        assert!(parse(&json!({"memory_telemetry":{"allocator_current_bytes":10}})).is_none());
        let r=parse(&json!({"memory_telemetry":{"allocator_source":"mlx-wrapper-v1","allocator_current_bytes":10,"allocator_peak_bytes":5}})).unwrap();
        assert_eq!(r.allocator_peak_bytes, None);
        assert!(parse(&json!({"memory_budget":{"components":{"scratch":{"current_bytes":1,"source":"unexpected","accounting":"additive"}}}})).is_none());
    }
}
