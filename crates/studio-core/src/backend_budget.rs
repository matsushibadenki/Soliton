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
pub struct MlxAllocator {
    pub active_bytes: u64,
    pub free_cache_bytes: u64,
    pub peak_active_bytes: Option<u64>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetainedKv {
    pub bytes: u64,
    pub limit_bytes: u64,
    pub entries: u64,
}
const SPAN_NAMES: [&str; 11] = [
    "qualification.verify",
    "qualification.validate",
    "qualification.model_identity",
    "qualification.backend_identity",
    "cache.fetch",
    "cache.insert",
    "cache.trim",
    "batch.prefill",
    "batch.decode",
    "sequential.prefill_first",
    "sequential.decode",
];
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    calls: u64,
    failures: u64,
    total_ms: f64,
    last_ms: Option<f64>,
    max_ms: Option<f64>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InternalSpans {
    schema: &'static str,
    basis: &'static str,
    consistency: &'static str,
    spans: std::collections::BTreeMap<String, Span>,
}
fn duration(v: &Value) -> Option<f64> {
    v.as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0 && *n <= 1e12)
}
fn internal_spans(value: &Value) -> Option<InternalSpans> {
    if value["schema"] != "soliton-internal-wall-spans-v1"
        || value["basis"] != "host_wall_no_added_sync"
        || value["consistency"] != "atomic_ledger_not_request_atomic"
    {
        return None;
    }
    let rows = value["spans"].as_object()?;
    if rows.len() != SPAN_NAMES.len() {
        return None;
    }
    let mut spans = std::collections::BTreeMap::new();
    for name in SPAN_NAMES {
        let row = rows.get(name)?;
        let calls = number(&row["calls"])?;
        let failures = number(&row["failures"])?;
        let total_ms = duration(&row["total_ms"])?;
        if failures > calls {
            return None;
        }
        let (last_ms, max_ms) = if calls == 0 {
            if failures != 0
                || total_ms != 0.0
                || !row["last_ms"].is_null()
                || !row["max_ms"].is_null()
            {
                return None;
            }
            (None, None)
        } else {
            let last = duration(&row["last_ms"])?;
            let max = duration(&row["max_ms"])?;
            if last > max || max > total_ms {
                return None;
            }
            (Some(last), Some(max))
        };
        spans.insert(
            name.into(),
            Span {
                calls,
                failures,
                total_ms,
                last_ms,
                max_ms,
            },
        );
    }
    Some(InternalSpans {
        schema: "soliton-internal-wall-spans-v1",
        basis: "host_wall_no_added_sync",
        consistency: "atomic_ledger_not_request_atomic",
        spans,
    })
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub startup_spans: Option<InternalSpans>,
    pub backend_spans: Option<InternalSpans>,
    pub mlx_allocator: Option<MlxAllocator>,
    pub retained_kv: Option<RetainedKv>,
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
            startup_spans: internal_spans(&health["startup_internal_spans"]),
            backend_spans: None,
            mlx_allocator: None,
            retained_kv: None,
            sampled_at: None,
            allocator_bytes: allocator,
            allocator_peak_bytes: peak,
            kv_bytes: kv,
            components,
        },
    )
}
fn merge_retained_wrapper(reading: &mut Reading, value: &Value, expected_limit: u64) {
    // This endpoint describes retained LRU accounting, not active attention KV.
    if value["accounting"] != "LRU tensor bytes, not unique physical memory" {
        return;
    }
    let a = &value["allocator"];
    if a["consistency"] == "non_atomic" {
        if let (Some(active), Some(free)) = (number(&a["active_bytes"]), number(&a["cache_bytes"]))
        {
            reading.mlx_allocator = Some(MlxAllocator {
                active_bytes: active,
                free_cache_bytes: free,
                peak_active_bytes: number(&a["peak_bytes"]).filter(|n| *n >= active),
            });
        }
    }
    if let (Some(bytes), Some(limit), Some(entries)) = (
        number(&value["retained_bytes"]),
        number(&value["max_bytes"]),
        number(&value["entries"]),
    ) {
        if limit == expected_limit && limit > 0 && bytes <= limit && entries <= 100_000 {
            reading.backend_spans = internal_spans(&value["internal_spans"]);
            reading.retained_kv = Some(RetainedKv {
                bytes,
                limit_bytes: limit,
                entries,
            });
        }
    }
}
async fn bounded_json(
    mut response: reqwest::Response,
    remaining: usize,
) -> Result<(Value, usize), &'static str> {
    if !response.status().is_success() {
        return Err("Runtime diagnostic request was rejected");
    }
    if response
        .content_length()
        .is_some_and(|n| n > remaining as u64)
    {
        return Err("Runtime diagnostic response exceeds 64 KiB");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Runtime diagnostic response failed")?
    {
        if bytes.len() + chunk.len() > remaining {
            return Err("Runtime diagnostic response exceeds 64 KiB");
        }
        bytes.extend_from_slice(&chunk);
    }
    let value =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid runtime diagnostic response")?;
    Ok((value, bytes.len()))
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
        let (port, key, pid, backend_port) = {
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
            (
                p.port,
                p.key.clone(),
                pid,
                p.vllm_backend_port.filter(|_| {
                    settings.vllm.backend == "mlx_lm" && settings.vllm.kv_cache_mib > 0
                }),
            )
        };
        let mut reading = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let response = self
                .client
                .get(format!("http://127.0.0.1:{port}/v1/runtime"))
                .bearer_auth(&key)
                .send()
                .await
                .map_err(|_| "Runtime diagnostic request failed")?;
            let (value, size) = bounded_json(response, 65536).await?;
            let mut reading = parse(&value).ok_or("Runtime memory diagnostics are unavailable")?;
            if let Some(backend_port) = backend_port {
                if let Ok(response) = self
                    .client
                    .get(format!(
                        "http://127.0.0.1:{backend_port}/vllm-apple/kv-cache"
                    ))
                    .send()
                    .await
                {
                    if let Ok((value, _)) = bounded_json(response, 65536 - size).await {
                        merge_retained_wrapper(
                            &mut reading,
                            &value,
                            settings.vllm.kv_cache_mib as u64 * (1 << 20),
                        );
                    }
                }
            }
            Ok(reading)
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
                json!({"control_ready":false,"inference_ready":false,"memory_telemetry":{"allocator_source":"mlx-wrapper-v1","allocator_current_bytes":100,"allocator_peak_bytes":120,"kv_source":"mlx-wrapper-v1","kv_used_bytes":0},"startup_internal_spans":spans_fixture()}),
            )
        });
        let app = axum::Router::new()
            .route("/health", handler.clone())
            .route("/v1/runtime", handler)
            .route("/vllm-apple/kv-cache",axum::routing::get(||async{axum::Json(json!({"retained_bytes":40,"max_bytes":1073741824u64,"entries":2,"accounting":"LRU tensor bytes, not unique physical memory","allocator":{"active_bytes":200,"cache_bytes":50,"peak_bytes":250,"consistency":"non_atomic"},"internal_spans":spans_fixture()}))}));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let studio = Arc::new(Studio::new(
            std::env::temp_dir().join("soliton-budget-fixture"),
            0,
        ));
        {
            let mut settings = studio.settings.write().await;
            settings.engine = Engine::VllmApple;
            settings.vllm.kv_cache_mib = 1024;
        }
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
            p.vllm_backend_port = Some(port);
        }
        studio.refresh_backend_budget().await.unwrap();
        let state = studio.snapshot().await;
        assert_eq!(state["modelPlan"]["backendBudget"]["allocatorBytes"], 100);
        assert!(state["modelPlan"]["backendBudget"]["startupSpans"]["spans"].is_object());
        assert!(state["modelPlan"]["backendBudget"]["backendSpans"]["spans"].is_object());
        assert!(
            state["modelPlan"]["backendBudget"]["backendSpans"]["spans"]["batch.prefill"]["lastMs"]
                .is_null()
        );
        assert_eq!(state["modelPlan"]["backendBudget"]["kvBytes"], 0);
        assert_eq!(
            state["modelPlan"]["backendBudget"]["mlxAllocator"]["activeBytes"],
            200
        );
        assert_eq!(
            state["modelPlan"]["backendBudget"]["retainedKv"]["bytes"],
            40
        );
        studio.process.lock().await.vllm_backend_port = Some(0);
        studio.refresh_backend_budget().await.unwrap();
        let partial = studio.snapshot().await;
        assert_eq!(partial["modelPlan"]["backendBudget"]["allocatorBytes"], 100);
        assert!(partial["modelPlan"]["backendBudget"]["mlxAllocator"].is_null());
        assert!(partial["modelPlan"]["backendBudget"]["retainedKv"].is_null());
        assert!(partial["modelPlan"]["backendBudget"]["backendSpans"].is_null());
        studio.process.lock().await.vllm_backend_port = Some(port);
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
    fn retained_wrapper_scopes_and_missing_metadata_are_kept_distinct() {
        let mut reading=parse(&json!({"memory_budget":{"components":{"mtp":{"current_bytes":0,"source":"model_spec","accounting":"additive"}}}})).unwrap();
        let v = json!({"retained_bytes":40,"max_bytes":100,"entries":2,"accounting":"LRU tensor bytes, not unique physical memory","allocator":{"active_bytes":200,"cache_bytes":50,"peak_bytes":250,"consistency":"non_atomic"}});
        merge_retained_wrapper(&mut reading, &v, 100);
        assert_eq!(reading.mlx_allocator.as_ref().unwrap().active_bytes, 200);
        assert_eq!(reading.mlx_allocator.as_ref().unwrap().free_cache_bytes, 50);
        assert_eq!(reading.retained_kv.as_ref().unwrap().bytes, 40);
        assert!(reading.kv_bytes.is_none() && reading.allocator_bytes.is_none());
        let mut unknown = reading.clone();
        unknown.mlx_allocator = None;
        unknown.retained_kv = None;
        merge_retained_wrapper(&mut unknown, &json!({"retained_bytes":0}), 100);
        assert!(unknown.mlx_allocator.is_none() && unknown.retained_kv.is_none());
        let mut wrong = unknown.clone();
        merge_retained_wrapper(&mut wrong, &v, 999);
        assert!(wrong.retained_kv.is_none());
        assert!(wrong.mlx_allocator.is_some());
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
    fn spans_fixture() -> Value {
        let rows: serde_json::Map<String, Value> = SPAN_NAMES
            .into_iter()
            .map(|name| {
                (
                    name.into(),
                    json!({"calls":0,"failures":0,"total_ms":0.0,"last_ms":null,"max_ms":null}),
                )
            })
            .collect();
        json!({"schema":"soliton-internal-wall-spans-v1","basis":"host_wall_no_added_sync","consistency":"atomic_ledger_not_request_atomic","spans":rows})
    }
    #[test]
    fn internal_spans_keep_unknown_separate_and_reject_inconsistent_values() {
        let fixture = spans_fixture();
        let parsed = internal_spans(&fixture).unwrap();
        assert!(parsed.spans["batch.prefill"].last_ms.is_none());
        let mut measured = fixture.clone();
        measured["spans"]["cache.fetch"] =
            json!({"calls":2,"failures":1,"total_ms":4.0,"last_ms":0.0,"max_ms":4.0});
        assert_eq!(
            internal_spans(&measured).unwrap().spans["cache.fetch"].last_ms,
            Some(0.0)
        );
        for (key, value) in [
            ("calls", json!(-1)),
            ("calls", json!(true)),
            ("failures", json!(3)),
            ("total_ms", json!(-1)),
            ("total_ms", json!(3.0)),
            ("last_ms", json!(5)),
            ("max_ms", json!(1e20)),
        ] {
            let mut bad = measured.clone();
            bad["spans"]["cache.fetch"][key] = value;
            assert!(internal_spans(&bad).is_none(), "{key}");
        }
        let mut bad = fixture.clone();
        bad["schema"] = json!("unknown");
        assert!(internal_spans(&bad).is_none());
        let mut bad = fixture.clone();
        bad["spans"]["batch.prefill"]["last_ms"] = json!(0);
        assert!(internal_spans(&bad).is_none());
        let mut bad = fixture.clone();
        bad["spans"]["unbounded"] = json!({});
        assert!(internal_spans(&bad).is_none());
        assert!(internal_spans(&json!({})).is_none());
    }
    #[test]
    fn internal_spans_are_bound_to_expected_retained_profile() {
        let mut reading=parse(&json!({"memory_budget":{"components":{"weights":{"current_bytes":10,"source":"model_weight_files","accounting":"additive"}}},"startup_internal_spans":spans_fixture()})).unwrap();
        assert!(reading.startup_spans.is_some());
        assert!(reading.backend_spans.is_none());
        let value = json!({"accounting":"LRU tensor bytes, not unique physical memory","retained_bytes":0,"max_bytes":100,"entries":0,"internal_spans":spans_fixture()});
        merge_retained_wrapper(&mut reading, &value, 99);
        assert!(reading.backend_spans.is_none());
        merge_retained_wrapper(&mut reading, &value, 100);
        assert!(reading.backend_spans.is_some());
    }
}
