//! Owned llama runtime configuration and identity-bound, fail-closed admission.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::process::Command;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub runtime_dir: String,
    pub model: String,
    pub projector: String,
    pub media_root: String,
    pub evidence: String,
    pub context: u32,
}
fn digest_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn hash_file(path: &Path) -> Result<String, String> {
    let mut f = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut b = [0u8; 1048576];
    loop {
        let n = f.read(&mut b).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(digest_hex(&h.finalize()))
}
fn tree(root: &Path, path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for e in fs::read_dir(path).map_err(|e| e.to_string())? {
        let p = e.map_err(|e| e.to_string())?.path();
        let target = p.canonicalize().map_err(|e| e.to_string())?;
        if !target.starts_with(root) {
            return Err("Omni runtime link escapes root".into());
        }
        if p.is_dir() {
            if fs::symlink_metadata(&p)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
            {
                return Err("Omni runtime directory symlink refused".into());
            }
            tree(root, &p, files)?
        } else if p.is_file() {
            files.push(p);
        } else {
            return Err("Omni runtime contains non-file".into());
        }
        if files.len() > 1024 {
            return Err("Omni runtime inventory too large".into());
        }
    }
    Ok(())
}
fn hardware() -> Result<String, String> {
    let out = std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", "hw.model", "hw.memsize", "machdep.cpu.brand_string"])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("Cannot identify Omni hardware".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
fn private_directory(root: &Path) -> Result<(), String> {
    let existed = root.exists();
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !existed {
            fs::set_permissions(root, fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        } else if fs::metadata(root)
            .map_err(|e| e.to_string())?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err("Use a private Omni media storage directory".into());
        }
    }
    Ok(())
}
impl Settings {
    pub fn valid(&self) -> bool {
        (1..=4096).contains(&self.context)
            && [
                &self.runtime_dir,
                &self.model,
                &self.projector,
                &self.media_root,
                &self.evidence,
            ]
            .iter()
            .all(|s| {
                !s.is_empty() && s.len() <= 4096 && !s.contains('\0') && Path::new(s).is_absolute()
            })
    }
    pub fn identity(&self) -> Result<Value, String> {
        if !self.valid() {
            return Err("Select local Omni runtime, model, projector, media directory and qualification evidence".into());
        }
        let root = Path::new(&self.runtime_dir)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let mut files = vec![];
        tree(&root, &root, &mut files)?;
        files.sort();
        let mut h = Sha256::new();
        for p in files {
            h.update(p.strip_prefix(&root).unwrap().to_string_lossy().as_bytes());
            h.update(b"\0");
            h.update(hash_file(&p)?.as_bytes());
            h.update(b"\n");
        }
        let runtime_hash = digest_hex(&h.finalize());
        let model_hash = hash_file(Path::new(&self.model))?;
        let projector_hash = hash_file(Path::new(&self.projector))?;
        if runtime_hash != "3d573493c4ff356474c6a6e874fb5e38799af62f4c05682e07a7c78d8e60ea2e"
            || model_hash != "9f8d30e9d4fd7a9349d586e077988229b3ca0a0e4ad730897563d1eccc9893e2"
            || projector_hash != "720ff4fc09df0e026c4ec641d0c698bf27b5bcd50561944c88eda5e3dffe1839"
        {
            return Err("Unreviewed Omni runtime or model assets".into());
        }
        let os = std::process::Command::new("/usr/bin/sw_vers")
            .arg("-productVersion")
            .output()
            .map_err(|e| e.to_string())?;
        if !os.status.success() {
            return Err("Cannot identify Omni host OS".into());
        }
        Ok(
            json!({"runtimeSha256":runtime_hash,"modelSha256":model_hash,"projectorSha256":projector_hash,"platform":format!("{}-{}",std::env::consts::OS,std::env::consts::ARCH),"osVersion":String::from_utf8_lossy(&os.stdout).trim(),"hardware":hardware()?,"ffmpegSha256":hash_file(Path::new("/opt/homebrew/bin/ffmpeg"))?,"ffprobeSha256":hash_file(Path::new("/opt/homebrew/bin/ffprobe"))?,"context":self.context,"parallel":1,"reasoning":false,"admissionVersion":1}),
        )
    }
    pub fn verify(&self) -> Result<(), String> {
        let meta = fs::metadata(&self.evidence).map_err(|e| e.to_string())?;
        if meta.len() > 1048576 {
            return Err("Omni evidence too large".into());
        }
        let v: Value =
            serde_json::from_slice(&fs::read(&self.evidence).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        validate_evidence(&v, &self.identity()?, now)
    }
    pub fn command(&self, port: u16, key: &str) -> Result<Command, String> {
        self.verify()?;
        private_directory(Path::new(&self.media_root))?;
        let mut c = Command::new(Path::new(&self.runtime_dir).join("llama"));
        for (k, _) in std::env::vars_os() {
            let s = k.to_string_lossy();
            if s.starts_with("LLAMA_") || s.starts_with("GGML_") {
                c.env_remove(k);
            }
        }
        c.args([
            "serve",
            "--alias",
            crate::omni::MODEL_ID,
            "-m",
            &self.model,
            "-mm",
            &self.projector,
            "--video-ffmpeg-dir",
            "/opt/homebrew/bin",
            "--media-path",
            &self.media_root,
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--parallel",
            "1",
            "-c",
            &self.context.to_string(),
            "--no-webui",
            "--reasoning",
            "off",
            "--temp",
            "0",
        ])
        .env("LLAMA_API_KEY", key);
        Ok(c)
    }
}
pub fn validate_evidence(v: &Value, identity: &Value, now: u64) -> Result<(), String> {
    if v["schema"] != 1
        || v["scope"] != "omni_media_text_30min"
        || v["passed"] != true
        || v["shutdownClean"] != true
        || v["identity"] != *identity
    {
        return Err("Omni qualification missing or identity mismatch".into());
    }
    if v["elapsedSeconds"]
        .as_f64()
        .is_none_or(|n| !n.is_finite() || n < 1800.)
        || v["failures"] != 0
        || v["successes"].as_u64().is_none_or(|n| n == 0)
    {
        return Err("Omni full qualification incomplete".into());
    }
    let created = v["createdAt"]
        .as_u64()
        .ok_or("Omni evidence timestamp missing")?;
    if created > now || now - created > 604800 {
        return Err("Omni qualification expired or future dated".into());
    }
    for mode in ["text", "image", "audio", "video"] {
        if v["exactCountParity"][mode] != true {
            return Err("Omni processor count parity incomplete".into());
        }
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attachment {
    pub id: String,
    pub kind: crate::omni::InputMode,
}
pub fn message(
    role: &str,
    text: &str,
    attachments: &[Attachment],
    root: &str,
) -> Result<Value, String> {
    if attachments.is_empty() {
        return Ok(json!({"role":role,"content":text}));
    }
    if attachments.len() > 16 {
        return Err("Too many Omni assets".into());
    }
    let mut parts = vec![json!({"type":"text","text":text})];
    for a in attachments {
        let id = uuid::Uuid::parse_str(&a.id).map_err(|_| "Invalid Omni asset ID")?;
        let p = Path::new(root).join(id.to_string());
        let canonical = p.canonicalize().map_err(|_| "Omni asset missing")?;
        let base = Path::new(root)
            .canonicalize()
            .map_err(|_| "Omni media storage missing")?;
        if !canonical.starts_with(&base)
            || fs::symlink_metadata(&p)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
        {
            return Err("Invalid Omni asset location".into());
        }
        let url = format!("file://{id}");
        parts.push(match a.kind {
            crate::omni::InputMode::Image => json!({"type":"image_url","image_url":{"url":url}}),
            crate::omni::InputMode::Audio => {
                json!({"type":"input_audio","input_audio":{"url":url}})
            }
            crate::omni::InputMode::Video => {
                json!({"type":"input_video","input_video":{"url":url}})
            }
            _ => return Err("Text is not a media asset".into()),
        });
    }
    Ok(json!({"role":role,"content":parts}))
}
#[derive(Debug)]
pub enum AdmissionError {
    Request(&'static str),
    Counter(&'static str),
}
impl std::fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Request(s) | Self::Counter(s) => f.write_str(s),
        }
    }
}
impl AdmissionError {
    pub fn status(&self) -> axum::http::StatusCode {
        match self {
            Self::Request(_) => axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            Self::Counter(_) => axum::http::StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}
pub async fn check(
    client: &reqwest::Client,
    port: u16,
    key: &str,
    body: &Value,
    context: u32,
) -> Result<u64, AdmissionError> {
    if body.as_object().is_none_or(|o| {
        o.keys().any(|k| {
            !matches!(
                k.as_str(),
                "model"
                    | "messages"
                    | "max_tokens"
                    | "stream"
                    | "stream_options"
                    | "temperature"
                    | "conversationId"
            )
        })
    }) {
        return Err(AdmissionError::Request(
            "Unqualified Omni output or template mode",
        ));
    }
    if body
        .get("temperature")
        .is_some_and(|v| v.as_f64() != Some(0.))
    {
        return Err(AdmissionError::Request(
            "Omni qualification requires greedy text output",
        ));
    }

    let out = body["max_tokens"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or(AdmissionError::Request(
            "Explicit positive Omni max_tokens required",
        ))?;
    let mut r = client
        .post(format!(
            "http://127.0.0.1:{port}/v1/chat/completions/input_tokens"
        ))
        .bearer_auth(key)
        .json(body)
        .timeout(std::time::Duration::from_secs(180))
        .send()
        .await
        .map_err(|_| AdmissionError::Counter("Omni exact processor counting unavailable"))?;
    if matches!(r.status().as_u16(), 400 | 422) {
        return Err(AdmissionError::Request(
            "Omni processor refused invalid media",
        ));
    }
    if !r.status().is_success() || r.content_length().is_some_and(|n| n > 65536) {
        return Err(AdmissionError::Counter(
            "Omni processor unavailable or oversized",
        ));
    }
    let mut b = Vec::new();
    while let Some(chunk) = r
        .chunk()
        .await
        .map_err(|_| AdmissionError::Counter("Omni processor response failed"))?
    {
        if b.len() + chunk.len() > 65536 {
            return Err(AdmissionError::Counter("Omni processor response too large"));
        }
        b.extend_from_slice(&chunk);
    }
    let v: Value = serde_json::from_slice(&b)
        .map_err(|_| AdmissionError::Counter("Omni processor response invalid"))?;
    let count = v["input_tokens"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or(AdmissionError::Counter(
            "Omni exact processor count missing",
        ))?;
    if count.checked_add(out).is_none_or(|n| n > context as u64) {
        return Err(AdmissionError::Request("Omni context exceeded"));
    }
    Ok(count)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sha256_identity_encoding_remains_compatible() {
        assert_eq!(digest_hex(&[0, 1, 15, 16, 255]), "00010f10ff");
        assert_eq!(
            digest_hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn no_unqualified_launch() {
        assert!(Settings::default().command(8080, "secret").is_err());
    }
    #[test]
    fn evidence_cannot_skip_parity() {
        let identity = json!({"test":1});
        let mut v = json!({"schema":1,"scope":"omni_media_text_30min","passed":true,"shutdownClean":true,"identity":identity,"elapsedSeconds":1800.,"successes":1,"failures":0,"createdAt":100,"exactCountParity":{"text":true,"image":true,"audio":true,"video":true}});
        assert!(validate_evidence(&v, &identity, 101).is_ok());
        v["exactCountParity"]["video"] = json!(false);
        assert!(validate_evidence(&v, &identity, 101).is_err());
    }
    #[test]
    fn expired_or_changed_identity_refused() {
        assert!(validate_evidence(&json!({}), &json!({}), 0).is_err());
    }
}

pub fn import_asset(
    root: &str,
    source: &str,
    kind: crate::omni::InputMode,
) -> Result<Attachment, String> {
    if kind == crate::omni::InputMode::Text {
        return Err("Text is not a media asset".into());
    }
    let source = Path::new(source);
    if !source.is_absolute()
        || fs::symlink_metadata(source)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
    {
        return Err("Select an absolute regular media file".into());
    }
    let mut input = fs::File::open(source).map_err(|e| e.to_string())?;
    let meta = input.metadata().map_err(|e| e.to_string())?;
    const LIMIT: u64 = 128 * 1024 * 1024;
    if !meta.is_file() || meta.len() == 0 || meta.len() > LIMIT {
        return Err("Media file must be between1 byte and128 MiB".into());
    }
    let root = Path::new(root);
    if !root.is_absolute() {
        return Err("Select absolute Omni media storage".into());
    }
    private_directory(root)?;
    let mut total = 0u64;
    let mut entries = 0;
    for e in fs::read_dir(root).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let m = e.metadata().map_err(|e| e.to_string())?;
        total = total.checked_add(m.len()).ok_or("Media storage overflow")?;
        entries += 1;
        if entries > 10000 {
            return Err("Omni media storage inventory too large".into());
        }
    }
    if total + meta.len() > 2 * 1024 * 1024 * 1024 {
        return Err("Omni media storage quota exceeded".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let dest = root.join(&id);
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&dest)
        .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        out.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    let result = std::io::copy(&mut (&mut input).take(LIMIT + 1), &mut out)
        .map_err(|e| e.to_string())
        .and_then(|n| {
            if n != meta.len() {
                Err("Media changed during import".into())
            } else {
                out.sync_all().map_err(|e| e.to_string())
            }
        });
    if let Err(e) = result {
        let _ = fs::remove_file(dest);
        return Err(e);
    }
    Ok(Attachment { id, kind })
}

pub fn messages(messages: &[crate::Message], settings: &Settings) -> Result<Vec<Value>, String> {
    if messages.is_empty() || messages.len() > 1000 {
        return Err("Invalid Omni conversation".into());
    }
    let mut rendered = messages
        .iter()
        .map(|m| {
            if !matches!(m.role.as_str(), "system" | "user" | "assistant")
                || m.content.len() > 1_000_000
            {
                return Err("Invalid Omni message".into());
            }
            message(&m.role, &m.content, &m.attachments, &settings.media_root)
        })
        .collect::<Result<Vec<_>, String>>()?;
    if rendered[0]["role"] == "system" {
        if !messages[0].attachments.is_empty() {
            return Err("Omni system messages cannot contain media".into());
        }
        rendered[0]["content"] = json!(format!("{}\nRespond in text.", messages[0].content));
    } else {
        rendered.insert(0, json!({"role":"system","content":"Respond in text."}));
    }
    Ok(rendered)
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    #[tokio::test]
    async fn native_processor_boundary_and_failure_refusal() {
        async fn count(axum::Json(v): axum::Json<Value>) -> axum::Json<Value> {
            assert_eq!(v["messages"][0]["content"], "fixture");
            axum::Json(json!({"input_tokens":10}))
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                axum::Router::new().route(
                    "/v1/chat/completions/input_tokens",
                    axum::routing::post(count),
                ),
            )
            .await
            .unwrap()
        });
        let client = reqwest::Client::new();
        let mut b = json!({"messages":[{"role":"user","content":"fixture"}],"max_tokens":2,"temperature":0});
        assert_eq!(check(&client, port, "key", &b, 12).await.unwrap(), 10);
        assert!(check(&client, port, "key", &b, 11).await.is_err());
        b["modalities"] = json!(["audio"]);
        assert!(check(&client, port, "key", &b, 12).await.is_err());
        task.abort();
        assert!(check(
            &client,
            port,
            "key",
            &json!({"messages":[],"max_tokens":1}),
            12
        )
        .await
        .is_err());
    }
    #[test]
    fn imported_asset_survives_source_change_and_archive() {
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        let src = root.join("source.wav");
        fs::write(&src, b"synthetic media").unwrap();
        let assets = root.join("assets");
        let a = import_asset(
            assets.to_str().unwrap(),
            src.to_str().unwrap(),
            crate::omni::InputMode::Audio,
        )
        .unwrap();
        fs::write(&src, b"changed original").unwrap();
        assert_eq!(fs::read(assets.join(&a.id)).unwrap(), b"synthetic media");
        let mut d = crate::Document::default();
        d.messages.push(crate::Message {
            role: "user".into(),
            content: "audio fixture".into(),
            attachments: vec![a.clone()],
            inference: None,
        });
        let old = d.conversations.active_id.clone();
        crate::conversations::change(&mut d, "newConversation", "").unwrap();
        crate::conversations::change(&mut d, "switchConversation", &old).unwrap();
        let bytes = serde_json::to_vec(&d).unwrap();
        let restored: crate::Document = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.messages[0].attachments[0].id, a.id);
        assert!(message(
            "user",
            "",
            &restored.messages[0].attachments,
            assets.to_str().unwrap()
        )
        .unwrap()["content"][1]["input_audio"]["url"]
            .as_str()
            .unwrap()
            .starts_with("file://"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn missing_and_traversal_assets_refused() {
        let a = Attachment {
            id: "../secret".into(),
            kind: crate::omni::InputMode::Video,
        };
        assert!(message("user", "", &[a], "/tmp").is_err());
    }
}
