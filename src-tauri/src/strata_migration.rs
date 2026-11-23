use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use studio_core::{engine::Engine, Settings};

/// Stage a replacement only for the reviewed managed Q2_0 layout.
pub fn stage(settings: &Settings, data: &Path) -> Result<Option<(Settings, PathBuf)>, String> {
    let old = data.join("Strata-metal-v1");
    let config = old.join("strata-q2_0.json");
    if settings.engine != Engine::Strata
        || settings.mock
        || Path::new(&settings.strata_root) != old
        || Path::new(&settings.config) != config
    {
        return Ok(None);
    }
    let new = data.join("Strata-metal-v2");
    let executable = new.join("engine/build-metal/strata");
    let python = data.join("python-3.12.16/bin/python3");
    if !executable.is_file() || !python.is_file() || !new.join("terminal/serve/server.py").is_file()
    {
        return Ok(None);
    }
    let result = |error: std::io::Error| error.to_string();
    let mut bytes = Vec::new();
    fs::File::open(config)
        .map_err(result)?
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(result)?;
    if bytes.len() > 1_048_576 {
        return Ok(None);
    }
    let mut config: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let old_executable = old.join("build-metal/strata");
    if config["backend"] != "metal"
        || config["model_name"] != "qwen3.8-flash-next-q2_0"
        || config["exe"].as_str().map(Path::new) != Some(old_executable.as_path())
    {
        return Ok(None);
    }
    let cwd = config["cwd"].as_str().map(Path::new).unwrap_or(&old);
    if cwd != old
        || !config["tokenizer"]
            .as_str()
            .filter(|path| !path.is_empty())
            .is_some_and(|path| cwd.join(path).join("vocab.json").is_file())
    {
        return Ok(None);
    }
    let Some(args) = config["args"].as_array() else {
        return Ok(None);
    };
    for (flag, directory) in [
        ("--pack", true),
        ("--native", false),
        ("--ple-gguf", false),
        ("--expert-profile", false),
    ] {
        let Some(path) = args
            .windows(2)
            .find(|pair| pair[0] == flag)
            .and_then(|pair| pair[1].as_str())
            .filter(|path| !path.is_empty())
            .map(|path| cwd.join(path))
        else {
            return Ok(None);
        };
        if (directory && !path.is_dir()) || (!directory && !path.is_file()) {
            return Ok(None);
        }
    }
    // Keep the original working directory, which resolves relative model and
    // expert-profile references in the copied configuration.
    config["cwd"] = serde_json::json!(old);
    config["exe"] = serde_json::json!(executable);
    let id = uuid::Uuid::new_v4();
    config["log"] = serde_json::json!(new.join(format!("q2_0-migrated-{id}.log")));
    let path = new.join(format!("strata-q2_0-migrated-{id}.json"));
    let rendered = serde_json::to_vec_pretty(&config).map_err(|error| error.to_string())?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options.open(&path).map_err(result)?;
    if let Err(error) = output.write_all(&rendered).and_then(|()| output.sync_all()) {
        let _ = fs::remove_file(&path);
        return Err(error.to_string());
    }
    let mut next = settings.clone();
    next.strata_root = new.to_string_lossy().into_owned();
    next.config = path.to_string_lossy().into_owned();
    if Path::new(&next.python)
        .strip_prefix(data)
        .ok()
        .is_some_and(|relative| {
            relative == Path::new("python-3.12.15/bin/python3")
                || relative == Path::new("python-3.12.16/bin/python3")
        })
    {
        next.python = python.to_string_lossy().into_owned();
    }
    Ok(Some((next, path)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn migrates_only_reviewed_managed_config_with_existing_assets() {
        let data = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let old = data.join("Strata-metal-v1");
        let new = data.join("Strata-metal-v2");
        for dir in [
            old.join("pack"),
            old.join("tokenizer"),
            new.join("terminal/serve"),
            new.join("engine/build-metal"),
            data.join("python-3.12.16/bin"),
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        for path in [
            old.join("native.gguf"),
            old.join("ple.gguf"),
            old.join("profile.bin"),
            old.join("tokenizer/vocab.json"),
            new.join("terminal/serve/server.py"),
            new.join("engine/build-metal/strata"),
            data.join("python-3.12.16/bin/python3"),
        ] {
            fs::write(path, b"").unwrap();
        }
        let path = old.join("strata-q2_0.json");
        let legacy = json!({
            "backend": "metal", "model_name": "qwen3.8-flash-next-q2_0",
            "exe": old.join("build-metal/strata"),
            "tokenizer": "tokenizer",
            "args": ["--pack", "pack", "--native", "native.gguf", "--ple-gguf", "ple.gguf", "--expert-profile", "profile.bin"],
        });
        fs::write(&path, legacy.to_string()).unwrap();
        let settings = Settings {
            strata_root: old.to_string_lossy().into_owned(),
            config: path.to_string_lossy().into_owned(),
            python: data
                .join("python-3.12.15/bin/python3")
                .to_string_lossy()
                .into_owned(),
            ..Settings::default()
        };
        let (next, staged) = stage(&settings, &data).unwrap().unwrap();
        let migrated: Value = serde_json::from_slice(&fs::read(staged).unwrap()).unwrap();
        assert_eq!(migrated["args"], legacy["args"]);
        assert_eq!(migrated["cwd"], json!(old));
        assert_eq!(
            migrated["exe"],
            json!(new.join("engine/build-metal/strata"))
        );
        assert_eq!(fs::read_to_string(path).unwrap(), legacy.to_string());
        assert!(next.python.ends_with("python-3.12.16/bin/python3"));
        assert!(stage(&next, &data).unwrap().is_none());
        fs::remove_file(old.join("profile.bin")).unwrap();
        assert!(stage(&settings, &data).unwrap().is_none());
        fs::remove_dir_all(data).unwrap();
    }
}
