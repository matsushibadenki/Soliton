//! Opt-in text profile for the reviewed Inkling GGUF and llama-server build.
use axum::http::StatusCode;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

pub(crate) const BACKEND: &str = "strata-inkling-metal";
pub(crate) const CONTEXT: u64 = 4096;
const MODEL_BYTES: u64 = 13_340_510_176;

pub(crate) async fn verify(executable: &str, model: &str) -> Result<(), String> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Err("Inkling requires Apple Silicon".into());
    }
    let executable = executable.to_owned();
    let model = model.to_owned();
    tokio::task::spawn_blocking(move || {
        let executable = Path::new(&executable);
        let model = Path::new(&model);
        if !executable.is_absolute()
            || !model.is_absolute()
            || executable.file_name() != Some(std::ffi::OsStr::new("llama-server"))
        {
            return Err("Select absolute Inkling llama-server and model paths".into());
        }
        let root = executable
            .parent()
            .unwrap()
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let manifest = include_str!("inkling-b12600.sha256");
        let files: std::collections::BTreeMap<_, _> = manifest
            .lines()
            .map(|line| {
                let (hash, name) = line
                    .split_once("  ")
                    .expect("valid built-in runtime manifest");
                (name, hash)
            })
            .collect();
        for entry in root.read_dir().map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            if entry.path().extension().is_some_and(|ext| ext == "dylib")
                && !name.to_str().is_some_and(|name| files.contains_key(name))
            {
                return Err(format!(
                    "Unexpected Inkling library: {}",
                    name.to_string_lossy()
                ));
            }
        }
        for (name, expected) in files {
            let target = root
                .join(name)
                .canonicalize()
                .map_err(|e| format!("Inkling runtime {name}: {e}"))?;
            if target.parent() != Some(root.as_path()) {
                return Err(format!(
                    "Inkling runtime file escapes its directory: {name}"
                ));
            }
            verify_file(&target, expected, None)
                .map_err(|e| format!("Inkling runtime {name}: {e}"))?;
        }
        verify_file(
            model,
            "23a8e6792481b6f1d0ebad51399e9ee2a2c2be8525b09ed1ae6737811159f892",
            Some(MODEL_BYTES),
        )?;
        Ok(())
    })
    .await
    .map_err(|_| "Inkling verification failed".to_owned())?
}

fn verify_file(path: &Path, expected: &str, size: Option<u64>) -> Result<(), String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || size.is_some_and(|n| metadata.len() != n) {
        return Err("Unexpected file type or size".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let bytes = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if bytes == 0 {
            break;
        }
        hash.update(&buffer[..bytes]);
    }
    let actual: String = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if actual != expected {
        return Err("SHA-256 mismatch".into());
    }
    Ok(())
}

pub(crate) async fn admit(
    client: &reqwest::Client,
    port: u16,
    key: &str,
    body: &mut Value,
) -> Result<(), (StatusCode, String)> {
    let bad = || {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "Inkling accepts plain text chat and an explicit output budget".into(),
        )
    };
    let fields = body.as_object_mut().ok_or_else(bad)?;
    if fields.keys().any(|key| {
        !matches!(
            key.as_str(),
            "model"
                | "messages"
                | "max_tokens"
                | "temperature"
                | "seed"
                | "stream"
                | "stream_options"
        )
    }) {
        return Err(bad());
    }
    let messages = fields
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(bad)?;
    if messages.is_empty()
        || messages.iter().any(|m| {
            m.as_object().is_none_or(|o| o.len() != 2)
                || !matches!(m["role"].as_str(), Some("system" | "user" | "assistant"))
                || !m["content"].is_string()
        })
    {
        return Err(bad());
    }
    let output = fields
        .get("max_tokens")
        .and_then(Value::as_u64)
        .filter(|n| *n > 0 && *n < CONTEXT)
        .ok_or_else(bad)?;
    fields.insert(
        "chat_template_kwargs".into(),
        json!({"reasoning_effort":"none"}),
    );
    let unavailable = || {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Inkling native token count unavailable".into(),
        )
    };
    let post = |path, value: Value| async move {
        let response = client
            .post(format!("http://127.0.0.1:{port}/{path}"))
            .bearer_auth(key)
            .timeout(std::time::Duration::from_secs(15))
            .json(&value)
            .send()
            .await
            .map_err(|_| unavailable())?;
        super::backend_budget::bounded_json(response, 20 * 1024 * 1024)
            .await
            .map(|(v, _)| v)
            .map_err(|_| unavailable())
    };
    let rendered = post("apply-template", body.clone()).await?;
    let prompt = rendered["prompt"].as_str().ok_or_else(unavailable)?;
    let tokens = post(
        "tokenize",
        json!({"content":prompt,"add_special":true,"parse_special":true}),
    )
    .await?;
    let count = tokens["tokens"]
        .as_array()
        .filter(|ids| !ids.is_empty() && ids.iter().all(|id| id.as_u64().is_some()))
        .ok_or_else(unavailable)?
        .len() as u64;
    if count + output > CONTEXT {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("Inkling prompt ({count}) plus output ({output}) exceeds {CONTEXT} tokens"),
        ));
    }
    Ok(())
}
