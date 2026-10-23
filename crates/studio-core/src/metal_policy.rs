//! Studio-owned Metal defaults, reapplied after setup and before engine launch.
//! Do not change the upstream installer or override its memory safety/arena policy.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetalProfile {
    pub context: u32,
    pub prompt_chunk: u32,
}

pub fn profile(memory_bytes: Option<u64>) -> MetalProfile {
    let gib = memory_bytes.unwrap_or(0) / (1024 * 1024 * 1024);
    let (context, prompt_chunk) = match gib {
        0..=32 => (8192, 64),
        33..=64 => (32768, 256),
        65..=95 => (32768, 512),
        _ => (65536, 1024),
    };
    MetalProfile {
        context,
        prompt_chunk,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct MemorySettings {
    pub automatic: bool,
    pub context: u32,
    pub prompt_chunk: u32,
    pub keep_free_gb: u32,
}
impl Default for MemorySettings {
    fn default() -> Self {
        Self {
            automatic: true,
            context: 8192,
            prompt_chunk: 64,
            keep_free_gb: 7,
        }
    }
}
impl MemorySettings {
    pub fn valid(&self, memory: Option<u64>) -> bool {
        (512..=262144).contains(&self.context)
            && (64..=4096).contains(&self.prompt_chunk)
            && self.prompt_chunk % 64 == 0
            && (4..=256).contains(&self.keep_free_gb)
            && (self.automatic
                || memory.is_none_or(|n| self.keep_free_gb as u64 * 1_000_000_000 < n))
    }
    pub fn context_for(&self, memory: Option<u64>) -> u32 {
        if self.automatic {
            profile(memory).context
        } else {
            self.context
        }
    }
}
pub fn automatic(memory: Option<u64>) -> Value {
    let mut value = json!(profile(memory));
    value["keepFreeGb"] = json!(((memory.unwrap_or(0) as f64 / 1e9) * 0.15).max(7.0).ceil() as u32);
    value
}

// Existing smaller values are deliberate limits and remain in effect. Duplicate flags
// are collapsed so a later upstream argument cannot silently undo the limit.
fn tune_with(
    config: &mut Value,
    p: MetalProfile,
    settings: &MemorySettings,
    keep: u32,
) -> Result<Option<MetalProfile>, String> {
    if config["backend"].as_str() != Some("metal") {
        return Ok(None);
    }
    let args = config["args"]
        .as_array()
        .ok_or("Metal config args must be an array")?;
    let owned = config["studio_metal_memory"].clone();
    let mut next = Vec::new();
    let mut context = p.context;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str().ok_or("Metal arguments must be strings")?;
        if arg == "--max-context" || arg.starts_with("--max-context=") {
            let value = if let Some(value) = arg.strip_prefix("--max-context=") {
                value
            } else {
                i += 1;
                args.get(i)
                    .and_then(Value::as_str)
                    .ok_or("Missing --max-context value")?
            };
            let n: u32 = value.parse().map_err(|_| "Invalid --max-context value")?;
            if n == 0 {
                return Err("Invalid --max-context value".into());
            }
            if owned["automatic"].as_bool() != Some(false)
                || owned["context"].as_u64() != Some(n as u64)
            {
                context = context.min(n);
            }
        } else {
            next.push(args[i].clone());
        }
        i += 1;
    }
    next.extend([json!("--max-context"), json!(context.to_string())]);
    let env = if config.get("env").is_none() {
        serde_json::Map::new()
    } else {
        config["env"]
            .as_object()
            .ok_or("Metal config env must be an object")?
            .clone()
    };
    let mut chunk = p.prompt_chunk;
    if let Some(value) = env.get("STRATA_METAL_PROMPT_CHUNK") {
        let n: u32 = value
            .as_str()
            .ok_or("Metal prompt chunk must be a string")?
            .parse()
            .map_err(|_| "Invalid Metal prompt chunk")?;
        if n < 64 || n % 64 != 0 {
            return Err("Metal prompt chunk must be a positive multiple of 64".into());
        }
        if owned["automatic"].as_bool() != Some(false)
            || owned["promptChunk"].as_u64() != Some(n as u64)
        {
            chunk = chunk.min(n);
        }
    }
    let mut keep_free = keep;
    if let Some(v) = env.get("STRATA_METAL_KEEP_FREE_GB") {
        let n: u32 = v
            .as_str()
            .ok_or("Metal memory reserve must be a string")?
            .parse()
            .map_err(|_| "Invalid Metal memory reserve")?;
        if owned["automatic"].as_bool() != Some(false)
            || owned["keepFreeGb"].as_u64() != Some(n as u64)
        {
            keep_free = n;
        }
    }
    if !settings.automatic {
        context = settings.context;
        chunk = settings.prompt_chunk;
        keep_free = settings.keep_free_gb;
        let last = next.len() - 1;
        next[last] = json!(context.to_string());
    }
    config["args"] = json!(next);
    config["env"] = json!(env);
    config["env"]["STRATA_METAL_PROMPT_CHUNK"] = json!(chunk.to_string());
    config["env"]["STRATA_METAL_KEEP_FREE_GB"] = json!(keep_free.to_string());
    config["studio_metal_memory"] = json!({"context":context,"promptChunk":chunk,"keepFreeGb":keep_free,"automatic":settings.automatic});
    Ok(Some(MetalProfile {
        context,
        prompt_chunk: chunk,
    }))
}

/// Small config files only; write in the same directory and rename atomically.
/// A failed tune never changes the original file. Unchanged files are not rewritten.
pub fn apply(path: &Path, memory: Option<u64>) -> Result<Option<MetalProfile>, String> {
    apply_with_settings(path, memory, &MemorySettings::default())
}
pub fn apply_with_settings(
    path: &Path,
    memory: Option<u64>,
    settings: &MemorySettings,
) -> Result<Option<MetalProfile>, String> {
    if !settings.valid(memory) {
        return Err("Invalid Metal memory settings".into());
    }
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if metadata.len() > 1024 * 1024 {
        return Err("Config exceeds 1 MiB".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut config: Value =
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
            .map_err(|e| e.to_string())?;
    let before = config.clone();
    let applied = tune_with(
        &mut config,
        profile(memory),
        settings,
        automatic(memory)["keepFreeGb"].as_u64().unwrap() as u32,
    )?;
    if before != config {
        let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> std::io::Result<()> {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            f.set_permissions(metadata.permissions())?;
            f.write_all(&serde_json::to_vec_pretty(&config)?)?;
            f.write_all(b"\n")?;
            f.sync_all()?;
            std::fs::rename(&temp, path)
        })();
        if let Err(e) = result {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("Cannot save Metal memory settings: {e}"));
        }
    }
    Ok(applied)
}

pub fn preview(path: &Path, memory: Option<u64>, settings: &MemorySettings) -> Option<Value> {
    if std::fs::metadata(path).ok()?.len() > 1024 * 1024 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let mut v: Value =
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes)).ok()?;
    tune_with(
        &mut v,
        profile(memory),
        settings,
        automatic(memory)["keepFreeGb"].as_u64()? as u32,
    )
    .ok()??;
    Some(v["studio_metal_memory"].clone())
}
#[cfg(test)]
fn tune(config: &mut Value, p: MetalProfile) -> Result<Option<MetalProfile>, String> {
    tune_with(config, p, &MemorySettings::default(), 7)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capacity_boundaries_and_unknown_memory() {
        for (gib, context, chunk) in [
            (8, 8192, 64),
            (32, 8192, 64),
            (48, 32768, 256),
            (64, 32768, 256),
            (72, 32768, 512),
            (96, 65536, 1024),
            (128, 65536, 1024),
        ] {
            assert_eq!(
                profile(Some(gib * 1024 * 1024 * 1024)),
                MetalProfile {
                    context,
                    prompt_chunk: chunk
                }
            );
        }
        assert_eq!(profile(None), profile(Some(32 * 1024 * 1024 * 1024)));
    }
    #[test]
    fn caps_duplicates_preserves_custom_settings_and_smaller_limits() {
        let mut v = json!({"backend":"metal","args":["--pack","/model","--max-context","4096","--max-context=65536"],
            "env":{"STRATA_METAL_PROMPT_CHUNK":"128","CUSTOM":"keep","STRATA_METAL_KEEP_FREE_GB":"9"},"other":true});
        let applied = tune(&mut v, profile(Some(32 << 30))).unwrap().unwrap();
        assert_eq!(
            applied,
            MetalProfile {
                context: 4096,
                prompt_chunk: 64
            }
        );
        assert_eq!(
            v["args"],
            json!(["--pack", "/model", "--max-context", "4096"])
        );
        assert_eq!(v["env"]["CUSTOM"], "keep");
        assert_eq!(v["env"]["STRATA_METAL_KEEP_FREE_GB"], "9");
        let before = v.clone();
        tune(&mut v, profile(Some(32 << 30))).unwrap();
        assert_eq!(v, before);
    }
    #[test]
    fn repeated_upstream_preparation_cannot_reset_the_policy() {
        let path =
            std::env::temp_dir().join(format!("strata-policy-{}.json", uuid::Uuid::new_v4()));
        for _ in 0..2 {
            std::fs::write(&path, br#"{"backend":"metal","args":["--max-context","32768"],"env":{"STRATA_METAL_STREAM":"1"}}"#).unwrap();
            assert_eq!(
                apply(&path, Some(32 << 30)).unwrap().unwrap(),
                profile(Some(32 << 30))
            );
            let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(v["args"], json!(["--max-context", "8192"]));
            assert_eq!(v["env"]["STRATA_METAL_PROMPT_CHUNK"], "64");
            assert_eq!(v["env"]["STRATA_METAL_STREAM"], "1");
        }
        let broken = br#"{"backend":"metal","args":["--max-context"],"env":{}}"#;
        std::fs::write(&path, broken).unwrap();
        assert!(apply(&path, Some(32 << 30)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), broken);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn other_backends_are_untouched() {
        let mut v = json!({"backend":"cuda","args":[]});
        let before = v.clone();
        assert!(tune(&mut v, profile(None)).unwrap().is_none());
        assert_eq!(v, before);
    }
    #[test]
    fn manual_limits_survive_repreparation_and_auto_restores_defaults() {
        let memory = Some(32 << 30);
        let manual = MemorySettings {
            automatic: false,
            context: 16384,
            prompt_chunk: 128,
            keep_free_gb: 8,
        };
        let mut config = Value::Null;
        for _ in 0..2 {
            config = json!({"backend":"metal","args":["--max-context","65536"],"env":{}});
            tune_with(&mut config, profile(memory), &manual, 7).unwrap();
            assert_eq!(config["studio_metal_memory"]["context"], 16384);
            assert_eq!(config["env"]["STRATA_METAL_PROMPT_CHUNK"], "128");
            assert_eq!(config["env"]["STRATA_METAL_KEEP_FREE_GB"], "8");
        }
        tune_with(&mut config, profile(memory), &MemorySettings::default(), 7).unwrap();
        assert_eq!(config["studio_metal_memory"]["context"], 8192);
        assert_eq!(config["env"]["STRATA_METAL_PROMPT_CHUNK"], "64");
        assert_eq!(config["env"]["STRATA_METAL_KEEP_FREE_GB"], "7");
        let smaller = MemorySettings {
            context: 4096,
            prompt_chunk: 64,
            ..manual
        };
        tune_with(&mut config, profile(memory), &smaller, 7).unwrap();
        tune_with(&mut config, profile(memory), &MemorySettings::default(), 7).unwrap();
        assert_eq!(config["studio_metal_memory"]["context"], 8192);
    }
    #[test]
    fn memory_settings_validate_and_old_settings_default_to_auto() {
        let old: crate::Settings = serde_json::from_value(json!({"strataRoot":"", "python":"python3","config":"","mock":false,"temperature":0.7,"maxTokens":512,"language":"en"})).unwrap();
        assert!(old.metal_memory.automatic);
        for (context, chunk, reserve) in [
            (0, 64, 7),
            (262145, 64, 7),
            (8192, 65, 7),
            (8192, 8192, 7),
            (8192, 64, 1),
            (8192, 64, 35),
        ] {
            assert!(!MemorySettings {
                automatic: false,
                context,
                prompt_chunk: chunk,
                keep_free_gb: reserve
            }
            .valid(Some(32 << 30)));
        }
    }
}
