//! Reviewed execution profiles for discovery. Launch-time qualification remains authoritative.
use super::Settings;
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path};

pub(crate) fn inventory(s: &Settings) -> Value {
    let strata = crate::strata_terminal_root(Path::new(&s.strata_root))
        .join("serve/server.py")
        .is_file()
        && Path::new(&s.python).is_file();
    let mlx = s.vllm.backend == "mlx_lm" && s.vllm.validate_paths().is_ok();
    let omni = Path::new(&s.omni.runtime_dir)
        .join("llama-server")
        .is_file();
    let prefix = Path::new(&s.prefix_cache.store_executable).is_file()
        && Path::new(&s.prefix_cache.python).is_file()
        && Path::new(&s.prefix_cache.runtime_dir).is_dir();
    let media_evidence = Path::new(&s.omni.evidence).is_file();
    let profile = |model: &str,
                   task,
                   engine,
                   setup,
                   context: Option<u32>,
                   output: Option<u32>,
                   runtime| {
        // File presence is inexpensive to inspect. Identity and expiry are checked on launch.
        let evidence = match setup {
            "mlx" => Some(Path::new(&s.vllm.evidence).is_file()),
            "ssd" => Some(Path::new(&s.prefix_cache.evidence).is_file()),
            "media" => Some(media_evidence),
            "voice" => Some(media_evidence && Path::new(&s.omni.realtime_evidence).is_file()),
            _ => None,
        };
        let version = match (setup, model) {
            ("mlx", "Gemma 3 1B IT") => Some("MLX-LM 0.32.0"),
            ("mlx", "Gemma 2 2B") => Some("MLX-LM 0.32.2"),
            _ => None,
        };
        json!({ "model": model, "task": task, "engine": engine, "setup": setup, "context": context,
            "output": output, "runtime": runtime, "evidence": evidence, "version": version })
    };
    json!([
        profile(
            "Gemma 3 1B IT",
            "text",
            "Strata",
            "strata",
            Some(2048),
            None,
            strata
        ),
        profile(
            "Gemma 3 1B IT",
            "text",
            "vLLM-Apple / MLX LM",
            "mlx",
            Some(512),
            None,
            mlx
        ),
        profile(
            "Gemma 2 2B",
            "text",
            "vLLM-Apple / MLX LM",
            "mlx",
            Some(4096),
            None,
            mlx
        ),
        profile(
            "Gemma 2 2B",
            "text",
            "MLX SSD",
            "ssd",
            Some(512),
            Some(128),
            prefix
        ),
        profile(
            "LFM3-Omni-2B",
            "text",
            "LFM3-Omni",
            "media",
            Some(4096),
            None,
            omni
        ),
        profile(
            "LFM3-Omni-2B",
            "media",
            "LFM3-Omni",
            "media",
            Some(4096),
            None,
            omni
        ),
        profile(
            "LFM3-Omni-2B",
            "voice",
            "LFM3-Omni",
            "voice",
            Some(4096),
            None,
            omni
        ),
        profile(
            "Qwen3.8-Flash-Next",
            "text",
            "Strata",
            "shards",
            None,
            None,
            strata
        )
    ])
}

pub(crate) fn library_model(path: &Path, inspection: &Value) -> Option<&'static str> {
    if inspection["model"] == "GEMMA3-1B" || inspection["backend"] == "strata-gemma3-metal" {
        return Some("Gemma 3 1B IT");
    }
    if inspection["model"]
        .as_str()
        .is_some_and(|id| super::preparation::MODELS.iter().any(|m| m.id == id))
    {
        return Some("Qwen3.8-Flash-Next");
    }
    let name = path.file_name()?.to_str()?;
    if name == "LFM3-Omni-2B-Q4_K_M.gguf" {
        return Some("LFM3-Omni-2B");
    }
    if name.starts_with("Qwen3.8-Flash-Next-") && inspection["kind"] == "configured" {
        return Some("Qwen3.8-Flash-Next");
    }
    if path.extension()?.to_str()? != "safetensors" {
        return None;
    }
    let config = path.parent()?.join("config.json");
    if !fs::metadata(&config).ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(config)
        .ok()?
        .take(65537)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 65536 {
        return None;
    }
    let config: Value = serde_json::from_slice(&bytes).ok()?;
    match (
        config["model_type"].as_str()?,
        config["hidden_size"].as_u64()?,
        config["num_hidden_layers"].as_u64()?,
    ) {
        ("gemma2", 2304, 26) => Some("Gemma 2 2B"),
        ("gemma3_text", 1152, 26) => Some("Gemma 3 1B IT"),
        _ => None,
    }
}
