//! Qualified, exclusive MLX SSD runtime. Separate evidence from vLLM-Apple.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};
use tokio::process::Command;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub python: String,
    pub model: String,
    pub runtime_dir: String,
    pub store_executable: String,
    pub cache_root: String,
    #[serde(rename = "quotaMiB", alias = "quotaMib")]
    pub quota_mib: u32,
    pub evidence: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            python: String::new(),
            model: String::new(),
            runtime_dir: String::new(),
            store_executable: String::new(),
            cache_root: String::new(),
            quota_mib: 256,
            evidence: String::new(),
        }
    }
}
impl Settings {
    /// Control-plane inspection/maintenance; the qualified worker and its executable are unchanged.
    pub fn storage(&self, trim: bool) -> std::io::Result<Value> {
        use std::{fs, io};
        let bad = |text| io::Error::new(io::ErrorKind::InvalidInput, text);
        let root = Path::new(&self.cache_root);
        if !self.valid() || !root.is_absolute() {
            return Err(bad("Select an absolute private SSD cache folder"));
        }
        let quota = self.quota_mib as u64 * 1024 * 1024;
        let empty = crate::prefix_store::Capacity {
            quota_bytes: quota,
            target_bytes: quota * 3 / 4,
            ..Default::default()
        };
        let missing = || json!({"exists":false,"capacity":empty,"staging":crate::prefix_staging::Staging::default(),"diskFreeBytes":crate::preparation::free_bytes(root)});
        let metadata = match fs::symlink_metadata(root) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(missing()),
            Err(e) => return Err(e),
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(bad("Cache root must be a real private directory"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.mode() & 0o077 != 0 || metadata.uid() != unsafe { libc::geteuid() } {
                return Err(bad("Cache root must be owner-only"));
            }
        }
        let staging = crate::prefix_staging::inspect(root)?;
        let path = root.join("store");
        match fs::symlink_metadata(&path) {
            Ok(_) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let mut report = missing();
                report["staging"] = serde_json::to_value(staging)?;
                return Ok(report);
            }
            Err(e) => return Err(e),
        };
        let store = crate::prefix_store::Store::existing(&path, quota)?;
        if trim {
            let result = store.trim()?;
            Ok(
                json!({"exists":true,"capacity":result.after,"trim":result,"staging":staging,"diskFreeBytes":crate::preparation::free_bytes(root)}),
            )
        } else {
            Ok(
                json!({"exists":true,"capacity":store.capacity()?,"staging":staging,"diskFreeBytes":crate::preparation::free_bytes(root)}),
            )
        }
    }
    /// Separate action: never trims saved prefixes as a side effect of staging cleanup.
    pub fn cleanup_staging(&self) -> std::io::Result<Value> {
        // Preflight the entire capacity report before any deletion, including store validation.
        let mut report = self.storage(false)?;
        if !Path::new(&self.cache_root).exists() {
            return Ok(report);
        }
        let cleanup = crate::prefix_staging::cleanup(Path::new(&self.cache_root))?;
        report = self.storage(false)?;
        report["stagingCleanup"] = serde_json::to_value(cleanup)?;
        Ok(report)
    }
    pub fn valid(&self) -> bool {
        (64..=4096).contains(&self.quota_mib)
            && [
                &self.python,
                &self.model,
                &self.runtime_dir,
                &self.store_executable,
                &self.cache_root,
                &self.evidence,
            ]
            .iter()
            .all(|s| s.len() <= 4096 && !s.contains('\0'))
    }
    fn base(&self, port: u16, key: &str, evidence: bool) -> Result<Command, String> {
        if !self.valid() {
            return Err("Invalid MLX SSD settings".into());
        }
        for value in [
            &self.python,
            &self.model,
            &self.runtime_dir,
            &self.store_executable,
            &self.cache_root,
        ]
        .into_iter()
        .chain(evidence.then_some(&self.evidence))
        {
            if value.is_empty() || !Path::new(value).is_absolute() {
                return Err("MLX SSD requires absolute model, Python, runtime, store, cache and evidence paths".into());
            }
        }
        for file in [
            Path::new(&self.python).to_path_buf(),
            Path::new(&self.store_executable).to_path_buf(),
            Path::new(&self.runtime_dir).join("prefix-cache-http.py"),
            Path::new(&self.runtime_dir).join("prefix_cache_contract.py"),
            Path::new(&self.runtime_dir).join("prefix_cache_roundtrip_support.py"),
            Path::new(&self.runtime_dir).join("prefix_cache_writes.py"),
        ] {
            if !file.is_file() {
                return Err(format!(
                    "MLX SSD required file not found: {}",
                    file.display()
                ));
            }
        }
        if evidence && !Path::new(&self.evidence).is_file() {
            return Err("MLX SSD qualification evidence file not found".into());
        }
        let mut command = Command::new(&self.python);
        command
            .arg("-u")
            .arg(Path::new(&self.runtime_dir).join("prefix-cache-http.py"))
            .args([
                "--model",
                &self.model,
                "--cache-root",
                &self.cache_root,
                "--quota-mib",
                &self.quota_mib.to_string(),
                "--port",
                &port.to_string(),
                "--store-executable",
                &self.store_executable,
            ])
            .env("SOLITON_PREFIX_HTTP_TOKEN", key)
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .env_remove("PYTHONPATH")
            .env_remove("PYTHONHOME")
            .env_remove("__PYVENV_LAUNCHER__")
            .env_remove("PYTHONEXECUTABLE")
            .stdin(std::process::Stdio::null());
        // A GUI process must not silently inject unqualified numerical/runtime flags.
        for (name, _) in std::env::vars() {
            if ["MLX_", "METAL_", "VECLIB_", "OMP_", "TOKENIZERS_"]
                .iter()
                .any(|prefix| name.starts_with(prefix))
                || name == "PYTORCH_ENABLE_MPS_FALLBACK"
            {
                command.env_remove(name);
            }
        }
        if evidence {
            command.args(["--evidence", &self.evidence]);
        }
        Ok(command)
    }
    /// Computes configuration identity without loading model weights or approving startup.
    pub async fn inspect(&self) -> Result<Value, String> {
        let key = uuid::Uuid::new_v4().to_string();
        let mut command = self.base(18082, &key, false)?;
        command
            .args(["--qualification-mode", "--verify-only"])
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(60), command.output())
            .await
            .map_err(|_| "MLX SSD inspection timed out")?
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(3000)
                .collect());
        }
        if output.stdout.len() > 16384 {
            return Err("MLX SSD inspection exceeded bounds".into());
        }
        let value: Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid inspection response")?;
        if value["verified"] != false
            || value["identity"]
                .as_str()
                .is_none_or(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("Invalid configuration identity".into());
        }
        Ok(value)
    }
    pub async fn command(&self, port: u16, key: &str) -> Result<(Command, String), String> {
        let mut verify = self.base(port, key, true)?;
        verify.arg("--verify-only").kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(60), verify.output())
            .await
            .map_err(|_| "MLX SSD evidence verification timed out")?
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "MLX SSD qualification refused: {}",
                String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(3000)
                    .collect::<String>()
            ));
        }
        if output.stdout.len() > 16384 {
            return Err("Invalid MLX SSD verification response".into());
        }
        let response: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| "Invalid MLX SSD evidence response")?;
        let identity = response["identity"]
            .as_str()
            .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or("Invalid qualification identity")?
            .to_string();
        if response["verified"] != true {
            return Err("MLX SSD evidence is unverified".into());
        }
        Ok((self.base(port, key, true)?, identity))
    }
}
/// Reject unsupported API options before requesting a count or mutating history.
pub fn validate_body(body: &Value) -> Result<(), String> {
    if serde_json::to_vec(body).map_err(|_| "Invalid JSON")?.len() > 65536 {
        return Err("MLX SSD requests must be at most 64 KiB".into());
    }
    let object = body.as_object().ok_or("Expected a JSON request")?;
    if object.keys().any(|k| {
        ![
            "model",
            "messages",
            "stream",
            "max_tokens",
            "temperature",
            "seed",
            "stream_options",
        ]
        .contains(&k.as_str())
    }) {
        return Err("MLX SSD supports text chat without tools or custom sampling options".into());
    }
    if body["model"] != "default_model"
        || body
            .get("temperature")
            .is_some_and(|v| v.as_f64() != Some(0.0))
        || body.get("seed").is_some_and(|v| v.as_u64() != Some(0))
        || body.get("stream").is_some_and(|v| !v.is_boolean())
        || body["max_tokens"]
            .as_u64()
            .is_none_or(|n| !(1..=128).contains(&n))
    {
        return Err(
            "MLX SSD requires default_model, temperature 0, seed 0 and 1–128 output tokens".into(),
        );
    }
    if body.get("stream_options").is_some_and(|v| {
        *v != json!({"include_usage":true}) && *v != json!({"include_usage":false})
    }) {
        return Err("Unsupported stream options".into());
    }
    let messages = body["messages"]
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 32)
        .ok_or("Use 1–32 text messages")?;
    for message in messages {
        let m = message.as_object().ok_or("Invalid message")?;
        if m.len() != 2
            || !m.contains_key("role")
            || !m.contains_key("content")
            || !matches!(
                message["role"].as_str(),
                Some("system" | "user" | "assistant")
            )
            || !message["content"].is_string()
        {
            return Err("MLX SSD supports text-only system/user/assistant messages".into());
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_migrate_and_request_bounds() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.quota_mib, 256);
        let configured: Settings = serde_json::from_str(r#"{"quotaMiB":512}"#).unwrap();
        assert_eq!(configured.quota_mib, 512);
        assert_eq!(serde_json::to_value(&configured).unwrap()["quotaMiB"], 512);
        assert!(serde_json::to_value(&configured)
            .unwrap()
            .get("quotaMib")
            .is_none());
        assert_eq!(
            serde_json::from_str::<Settings>(r#"{"quotaMib":128}"#)
                .unwrap()
                .quota_mib,
            128
        );
        let body = json!({"model":"default_model","messages":[{"role":"user","content":"hello"}],"max_tokens":32,"temperature":0,"seed":0});
        assert!(validate_body(&body).is_ok());
        for bad in [
            json!({"max_tokens":129}),
            json!({"temperature":0.7}),
            json!({"tools":[]}),
            json!({"seed":true}),
        ] {
            let mut b = body.clone();
            for (k, v) in bad.as_object().unwrap() {
                b[k] = v.clone();
            }
            assert!(validate_body(&b).is_err());
        }
    }
}
