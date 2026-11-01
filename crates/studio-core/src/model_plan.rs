//! Read-only capacity observations. Missing backend budgets never imply a model fits.
use crate::{engine::Engine, metal_policy, preparation, Settings};
use serde::Serialize;
use serde_json::Value;
use std::{
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPlan {
    pub backend: String,
    pub context: Option<u64>,
    pub weights_bytes: Option<u64>,
    pub resident_estimate_bytes: Option<u64>,
    pub kv_bytes: Option<u64>,
    pub workspace_bytes: Option<u64>,
    pub os_reserve_bytes: u64,
    pub memory_budget_bytes: Option<u64>,
    pub disk_free_bytes: Option<u64>,
    pub warnings: Vec<&'static str>,
    // Capacity observations are not a qualification or an admission decision.
    pub assessment: &'static str,
}
fn resolve(root: &Path, value: &str) -> PathBuf {
    let p = Path::new(value);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}
fn config(path: &Path) -> Option<Value> {
    let file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 1024 * 1024 {
        return None;
    }
    serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes)).ok()
}
fn context(args: &Value, flag: &str) -> Option<u64> {
    let args = args.as_array()?;
    let mut found = None;
    for (i, arg) in args.iter().enumerate() {
        let s = arg.as_str()?;
        let n = if s == flag {
            args.get(i + 1)?.as_str()?.parse().ok()?
        } else if let Some(v) = s.strip_prefix(&format!("{flag}=")) {
            v.parse().ok()?
        } else {
            continue;
        };
        if n == 0 || found.is_some() {
            return None;
        }
        found = Some(n);
    }
    found
}
pub fn inspect(settings: &Settings, memory: Option<u64>, inspection: &Value) -> ModelPlan {
    let reserve = metal_policy::automatic(memory)["keepFreeGb"]
        .as_u64()
        .unwrap_or(7)
        * 1_000_000_000;
    let mut p = ModelPlan {
        backend: settings.engine.label().into(),
        context: None,
        weights_bytes: None,
        resident_estimate_bytes: None,
        kv_bytes: None,
        workspace_bytes: None,
        os_reserve_bytes: reserve,
        memory_budget_bytes: None,
        disk_free_bytes: None,
        warnings: vec![],
        assessment: "incomplete",
    };
    let disk = match settings.engine {
        Engine::Strata => {
            let root = Path::new(&settings.strata_root);
            let path = resolve(root, &settings.config);
            let mut disk = root.parent().unwrap_or(root).join("Strata-data");
            if let Some(v) = config(&path) {
                let backend = v["backend"].as_str().unwrap_or("");
                if matches!(backend, "metal" | "strata-gemma3-metal") {
                    p.backend = backend.into();
                    p.context = context(
                        &v["args"],
                        if backend == "metal" {
                            "--max-context"
                        } else {
                            "--ctx-size"
                        },
                    );
                    if backend == "metal" {
                        if let Some(next) =
                            metal_policy::preview(&path, memory, &settings.metal_memory)
                        {
                            p.context = next["context"].as_u64();
                            if let Some(gb) = next["keepFreeGb"].as_u64() {
                                p.os_reserve_bytes = gb * 1_000_000_000;
                            }
                        }
                    } else {
                        let cwd = v["cwd"]
                            .as_str()
                            .map(|s| resolve(root, s))
                            .unwrap_or(root.to_path_buf());
                        if let Some(model) = v["model"].as_str().filter(|s| !s.is_empty()) {
                            disk = resolve(&cwd, model);
                        }
                        p.weights_bytes = v["model"]
                            .as_str()
                            .and_then(|s| std::fs::metadata(resolve(&cwd, s)).ok())
                            .filter(|m| m.is_file())
                            .map(|m| m.len());
                        if p.weights_bytes.is_none() {
                            p.warnings.push("model_missing");
                        }
                    }
                }
            } else {
                p.warnings.push("config_missing");
            }
            disk
        }
        Engine::VllmApple => {
            p.context = Some(settings.vllm.context as u64);
            // Inspection already binds settings and expires after five minutes. Its memory
            // estimate is backend-owned, not a measurement or a full capacity proof.
            p.resident_estimate_bytes = inspection["estimatedResidentBytes"].as_u64();
            if inspection["fitsMemory"] == false {
                p.warnings.push("backend_memory");
            }
            PathBuf::from(&settings.vllm.model)
        }
        Engine::Lfm3Omni => {
            p.context = Some(settings.omni.context as u64);
            PathBuf::from(&settings.omni.model)
        }
    };
    p.disk_free_bytes = preparation::free_bytes(&disk);
    p.memory_budget_bytes = memory.map(|m| m.saturating_sub(p.os_reserve_bytes));
    if memory.is_some_and(|m| p.os_reserve_bytes >= m) {
        p.warnings.push("reserve_exhausted");
    }
    if let (Some(w), Some(b)) = (p.weights_bytes, p.memory_budget_bytes) {
        if w > b {
            p.warnings.push("weights_over_budget");
        }
    }
    if p.context.is_some_and(|c| settings.max_tokens as u64 >= c) {
        p.warnings.push("output_reservation");
    }
    if p.disk_free_bytes == Some(0) {
        p.warnings.push("disk_full");
    }
    if !p.warnings.is_empty() {
        p.assessment = "attention";
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn shared_snapshot_tracks_saved_engine_settings() {
        let studio = crate::Studio::new(PathBuf::from("/tmp/soliton-plan-fixture"), 0);
        {
            let mut s = studio.settings.write().await;
            s.engine = Engine::VllmApple;
            s.vllm.context = 4096;
            s.max_tokens = 4096;
        }
        let state = studio.snapshot().await;
        assert_eq!(state["modelPlan"]["context"], 4096);
        assert_eq!(state["modelPlan"]["warnings"][0], "output_reservation");
        studio.settings.write().await.vllm.context = 8192;
        let state = studio.snapshot().await;
        assert_eq!(state["modelPlan"]["context"], 8192);
        assert_eq!(state["modelPlan"]["warnings"], serde_json::json!([]));
        assert_eq!(state["modelPlan"]["assessment"], "incomplete");
    }
    #[test]
    fn unknown_is_not_a_fit_and_output_is_bounded() {
        let mut s = Settings::default();
        s.engine = Engine::VllmApple;
        s.max_tokens = 1024;
        let p = inspect(&s, None, &Value::Null);
        assert!(p.memory_budget_bytes.is_none());
        assert!(p.resident_estimate_bytes.is_none());
        assert!(p.warnings.contains(&"output_reservation"));
        assert_ne!(p.assessment, "verified");
    }
    #[test]
    fn gemma_observations_do_not_modify_config_or_guess_runtime_budgets() {
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("model.gguf"), [0u8; 32]).unwrap();
        let bytes = br#"{"backend":"strata-gemma3-metal","model":"model.gguf","args":["--ctx-size","2048"]}"#;
        std::fs::write(root.join("config.json"), bytes).unwrap();
        let mut s = Settings::default();
        s.strata_root = root.to_string_lossy().into();
        s.config = "config.json".into();
        let p = inspect(&s, Some(32 << 30), &Value::Null);
        assert_eq!(p.context, Some(2048));
        assert_eq!(p.weights_bytes, Some(32));
        assert_eq!(p.os_reserve_bytes, 7_000_000_000);
        assert_eq!(p.memory_budget_bytes, Some((32 << 30) - 7_000_000_000));
        assert!(p.kv_bytes.is_none() && p.workspace_bytes.is_none());
        assert_eq!(p.assessment, "incomplete");
        assert_eq!(std::fs::read(root.join("config.json")).unwrap(), bytes);
        let p = inspect(&s, Some(1), &Value::Null);
        assert!(p.warnings.contains(&"reserve_exhausted"));
        assert!(p.warnings.contains(&"weights_over_budget"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn duplicate_or_invalid_context_is_unknown() {
        assert_eq!(
            context(&serde_json::json!(["--ctx-size=2048"]), "--ctx-size"),
            Some(2048)
        );
        for args in [
            serde_json::json!(["--ctx-size", "0"]),
            serde_json::json!(["--ctx-size", "2048", "--ctx-size=4096"]),
        ] {
            assert_eq!(context(&args, "--ctx-size"), None);
        }
    }
}
