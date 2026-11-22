//! Opt-in text profile for the reviewed Inkling GGUF and llama-server build.
use axum::http::StatusCode;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

pub(crate) const BACKEND: &str = "strata-inkling-metal";
pub(crate) const CONTEXT: u64 = 4096;
pub(crate) const MIN_MEMORY_BYTES: u64 = 32 * 1024 * 1024 * 1024;
const MODEL_BYTES: u64 = 13_340_510_176;

pub(crate) async fn verify(
    executable: &str,
    model: &str,
    projector: Option<&str>,
) -> Result<(), String> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Err("Inkling requires Apple Silicon".into());
    }
    let executable = executable.to_owned();
    let model = model.to_owned();
    let projector = projector.map(str::to_owned);
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
        let mut verified_targets = std::collections::HashSet::new();
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
            // Release aliases resolve to the same file, but each name and expected digest
            // still has to match the manifest.
            if !verified_targets.contains(&(target.clone(), expected)) {
                verify_file(&target, expected, None)
                    .map_err(|e| format!("Inkling runtime {name}: {e}"))?;
                verified_targets.insert((target, expected));
            }
        }
        if let Some(projector) = projector {
            let path = Path::new(&projector);
            if !path.is_absolute() {
                return Err("Select an absolute Inkling projector path".into());
            }
            verify_file(
                path,
                "5a72e0b6e2bef13c3f455f4edb85df0c752b92ff76ab46af879eae088163efe6",
                Some(639_348_448),
            )?;
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
    vision: bool,
) -> Result<(), (StatusCode, String)> {
    let (count, output) = count_chat(client, port, key, body, vision).await?;
    if count + output > CONTEXT {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("Inkling prompt ({count}) plus output ({output}) exceeds {CONTEXT} tokens"),
        ));
    }
    Ok(())
}

pub(crate) async fn count_chat(
    client: &reqwest::Client,
    port: u16,
    key: &str,
    body: &mut Value,
    vision: bool,
) -> Result<(u64, u64), (StatusCode, String)> {
    let bad = || {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "Inkling requires text, enabled inline images, and an explicit output budget".into(),
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
                || !(m["content"].is_string() || vision && valid_image_content(m))
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
    if vision {
        let response = post("v1/chat/completions/input_tokens", body.clone()).await?;
        let count = response["input_tokens"]
            .as_u64()
            .filter(|&n| n > 0)
            .ok_or_else(unavailable)?;
        return Ok((count, output));
    }
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
    Ok((count, output))
}

const MAX_IMAGE_BYTES: u64 = 4 * 1024 * 1024;

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else {
        None
    }
}

fn valid_image_content(message: &Value) -> bool {
    message["role"] == "user"
        && message["content"].as_array().is_some_and(|parts| {
            parts.len() == 2
                && parts[0]["type"] == "text"
                && parts[0]["text"].is_string()
                && parts[1]["type"] == "image_url"
                && parts[1]["image_url"]["url"].as_str().is_some_and(|url| {
                    let Some((prefix, data)) = url.split_once(",") else {
                        return false;
                    };
                    matches!(prefix, "data:image/png;base64" | "data:image/jpeg;base64")
                        && data.len() <= (MAX_IMAGE_BYTES as usize).div_ceil(3) * 4
                        && STANDARD.decode(data).ok().is_some_and(|bytes| {
                            bytes.len() <= MAX_IMAGE_BYTES as usize
                                && image_mime(&bytes)
                                    .is_some_and(|mime| prefix == format!("data:{mime};base64"))
                        })
                })
        })
}

pub(crate) fn messages(history: &[super::Message], root: &str) -> Result<Value, String> {
    let mut rendered = Vec::with_capacity(history.len());
    let mut image_bytes = 0;
    for message in history {
        let content = match message.attachments.as_slice() {
            [] => json!(message.content),
            [attachment]
                if message.role == "user" && attachment.kind == super::omni::InputMode::Image =>
            {
                let id = uuid::Uuid::parse_str(&attachment.id)
                    .map_err(|_| "Invalid Inkling asset ID")?;
                let path = Path::new(root).join(id.to_string());
                if !std::fs::symlink_metadata(&path)
                    .map_err(|_| "Inkling image missing")?
                    .is_file()
                {
                    return Err("Invalid Inkling image location".into());
                }
                let mut bytes = Vec::new();
                std::fs::File::open(path)
                    .and_then(|file| file.take(MAX_IMAGE_BYTES + 1).read_to_end(&mut bytes))
                    .map_err(|_| "Inkling image unreadable")?;
                image_bytes += bytes.len() as u64;
                if bytes.len() as u64 > MAX_IMAGE_BYTES || image_bytes > 2 * MAX_IMAGE_BYTES {
                    return Err(
                        "Inkling images exceed the per-image or conversation byte limit".into(),
                    );
                }
                let mime = image_mime(&bytes).ok_or("Inkling accepts PNG or JPEG images")?;
                json!([{"type":"text","text":message.content},
                    {"type":"image_url","image_url":{"url":format!("data:{mime};base64,{}",STANDARD.encode(&bytes))}}])
            }
            _ => return Err("Inkling accepts one PNG or JPEG per user message".into()),
        };
        rendered.push(json!({"role":message.role,"content":content}));
    }
    Ok(json!(rendered))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vision_accepts_only_inline_png_or_jpeg_for_user_messages() {
        let inline = format!(
            "data:image/png;base64,{}",
            STANDARD.encode(b"\x89PNG\r\n\x1a\n")
        );
        let mut message = json!({"role":"user","content":[
            {"type":"text","text":"Describe the image"},
            {"type":"image_url","image_url":{"url":inline}}
        ]});
        assert!(valid_image_content(&message));

        message["content"][1]["image_url"]["url"] = json!("file://example.png");
        assert!(!valid_image_content(&message));
        message["content"][1]["image_url"]["url"] = json!("https://example.invalid/image.png");
        assert!(!valid_image_content(&message));
        message["content"][1]["image_url"]["url"] = json!(inline);
        message["role"] = json!("assistant");
        assert!(!valid_image_content(&message));
    }
}
