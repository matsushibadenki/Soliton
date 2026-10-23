//! Read-only, bounded model inventory. Weight bytes never enter the WebView.
use super::*;
use axum::extract::Query;
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::Path,
    time::Instant,
};

const ENTRY_LIMIT: usize = 100_000;
const MODEL_LIMIT: usize = 2_000;
const DEPTH_LIMIT: usize = 16;
const SCAN_SECONDS: u64 = 15;
pub(crate) type CachedScan = (String, Instant, Value);
#[derive(Default, Deserialize)]
pub(crate) struct ScanQuery {
    #[serde(default)]
    refresh: bool,
}
pub(crate) async fn scan_handler(
    State(studio): State<Arc<Studio>>,
    h: HeaderMap,
    Query(query): Query<ScanQuery>,
) -> Response {
    if !authorized(&h, &studio) {
        return error(StatusCode::UNAUTHORIZED, "Studio token required");
    }
    let settings = studio.settings.read().await.clone();
    let key = serde_json::to_string(&settings).unwrap_or_default();
    let mut cache = studio.catalog.lock().await;
    if let Some((stored_key, when, value)) = cache.as_ref() {
        if !query.refresh && stored_key == &key && when.elapsed() < Duration::from_secs(30) {
            return Json(value.clone()).into_response();
        }
    }
    match tokio::task::spawn_blocking(move || scan(&settings, &Environment::system())).await {
        Ok(value) => {
            *cache = Some((key, Instant::now(), value.clone()));
            Json(value).into_response()
        }
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}
pub fn valid_directories(paths: &[String]) -> bool {
    paths.len() <= 32
        && paths.iter().all(|p| {
            !p.trim().is_empty()
                && p.len() <= 4096
                && !p.contains('\0')
                && (Path::new(p).is_absolute() || p.starts_with("~/"))
        })
}
struct Environment {
    home: PathBuf,
    vars: HashMap<String, String>,
}
impl Environment {
    fn system() -> Self {
        Self {
            home: std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(PathBuf::from)
                .unwrap_or_default(),
            vars: std::env::vars_os()
                .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
                .collect(),
        }
    }
    fn expand(&self, value: &str) -> PathBuf {
        if value == "~" {
            self.home.clone()
        } else if let Some(p) = value.strip_prefix("~/") {
            self.home.join(p)
        } else {
            PathBuf::from(value)
        }
    }
    fn var(&self, name: &str) -> Option<PathBuf> {
        self.vars
            .get(name)
            .filter(|p| !p.trim().is_empty())
            .map(|p| self.expand(p))
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanRoot {
    source: String,
    path: String,
    status: String,
    files: usize,
    errors: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelFile {
    name: String,
    path: String,
    selection_path: String,
    bytes: u64,
    format: String,
    status: String,
    sources: Vec<String>,
}
fn json_file(path: &Path) -> Option<Value> {
    if fs::metadata(path).ok()?.len() > 1024 * 1024 {
        return None;
    }
    let b = fs::read(path).ok()?;
    serde_json::from_slice(b.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&b)).ok()
}
fn add_root(roots: &mut Vec<(String, PathBuf)>, source: &str, path: PathBuf) {
    if !path.is_absolute() {
        return;
    }
    let canonical = path.canonicalize().unwrap_or(path);
    if !roots.iter().any(|(s, p)| s == source && p == &canonical) {
        roots.push((source.into(), canonical));
    }
}
fn roots(settings: &Settings, env: &Environment) -> Vec<(String, PathBuf)> {
    let mut roots = Vec::new();
    let root = env.expand(&settings.strata_root);
    add_root(&mut roots, "strata", root.clone());
    if let Some(parent) = root.parent() {
        add_root(&mut roots, "strata", parent.join("Strata-data"));
    }
    let config_home = env
        .var("XDG_CONFIG_HOME")
        .unwrap_or_else(|| env.home.join(".config"));
    let strata_settings = if cfg!(windows) {
        env.var("APPDATA")
            .unwrap_or_else(|| env.home.join("AppData/Roaming"))
            .join("Strata/settings.json")
    } else {
        config_home.join("strata/settings.json")
    };
    if let Some(v) = json_file(&strata_settings) {
        if let Some(p) = v["data_dir"].as_str() {
            add_root(&mut roots, "strata", env.expand(p));
        }
    }
    // Run configs may point to model stores on another volume.
    if !settings.config.is_empty() {
        let cfg = env.expand(&settings.config);
        let cfg = if cfg.is_absolute() {
            cfg
        } else {
            root.join(cfg)
        };
        if let Some(v) = json_file(&cfg) {
            let cwd = v["cwd"]
                .as_str()
                .map(|p| env.expand(p))
                .map(|p| if p.is_absolute() { p } else { root.join(p) })
                .unwrap_or(root.clone());
            if let Some(args) = v["args"].as_array() {
                for pair in args.windows(2) {
                    if [
                        "--pack",
                        "--native",
                        "--ple-gguf",
                        "--native-dense-gguf",
                        "--mtp",
                    ]
                    .contains(&pair[0].as_str().unwrap_or(""))
                    {
                        if let Some(p) = pair[1].as_str() {
                            let p = env.expand(p);
                            let p = if p.is_absolute() { p } else { cwd.join(p) };
                            let folder = if p.is_dir() {
                                Some(p)
                            } else {
                                p.parent().map(Path::to_path_buf)
                            };
                            if let Some(p) = folder {
                                add_root(&mut roots, "strata", p);
                            }
                        }
                    }
                }
            }
        }
    }
    for base in [
        env.home.join(".lmstudio"),
        env.home.join(".cache/lm-studio"),
    ] {
        if let Some(v) = json_file(&base.join("settings.json")) {
            for key in ["downloadsFolder", "modelsDirectory"] {
                if let Some(p) = v[key].as_str() {
                    add_root(&mut roots, "lmstudio", env.expand(p));
                }
            }
        }
        add_root(&mut roots, "lmstudio", base.join("models"));
    }
    let master = env
        .var("UNSLOTH_HOME")
        .unwrap_or_else(|| env.home.join(".unsloth"));
    let studio = env
        .var("UNSLOTH_STUDIO_HOME")
        .or_else(|| env.var("STUDIO_HOME"))
        .unwrap_or_else(|| master.join("studio"));
    // Current Unsloth saves just these two location settings in SQLite. Open read-only;
    // never import its Python package (which creates/migrates the database).
    if let Ok(db) = Connection::open_with_flags(
        studio.join("studio.db"),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        let _ = db.busy_timeout(Duration::from_millis(200));
        let sql = "SELECT value_json FROM app_settings \
                   WHERE key IN ('hugging_face_cache_home','hugging_face_cache_history') \
                   AND length(value_json)<65536 LIMIT 2";
        if let Ok(mut query) = db.prepare(sql) {
            if let Ok(rows) = query.query_map([], |r| r.get::<_, String>(0)) {
                for row in rows.flatten() {
                    let Ok(value) = serde_json::from_str::<Value>(&row) else {
                        continue;
                    };
                    let paths = if let Some(path) = value.as_str() {
                        vec![path]
                    } else {
                        value
                            .as_array()
                            .map(|a| a.iter().filter_map(Value::as_str).take(16).collect())
                            .unwrap_or_default()
                    };
                    for path in paths {
                        add_root(&mut roots, "unsloth", env.expand(path).join("hub"));
                    }
                }
            }
        }
    }
    for p in [
        studio.join("cache/huggingface/hub"),
        studio.join("models"),
        studio.join("exports"),
        master.join(".cache"),
        env.home.join(".cache/huggingface/hub"),
    ] {
        add_root(&mut roots, "unsloth", p);
    }
    for key in ["HF_HUB_CACHE", "HUGGINGFACE_HUB_CACHE"] {
        if let Some(p) = env.var(key) {
            add_root(&mut roots, "unsloth", p);
        }
    }
    if let Some(p) = env.var("HF_HOME") {
        add_root(&mut roots, "unsloth", p.join("hub"));
    }
    if let Some(p) = env.var("XDG_CACHE_HOME") {
        add_root(&mut roots, "unsloth", p.join("huggingface/hub"));
    }
    if let Some(p) = env.var("OLLAMA_MODELS") {
        add_root(&mut roots, "ollama", p);
    }
    add_root(&mut roots, "ollama", env.home.join(".ollama/models"));
    if cfg!(target_os = "linux") {
        add_root(
            &mut roots,
            "ollama",
            PathBuf::from("/usr/share/ollama/.ollama/models"),
        );
    }
    for p in &settings.model_directories {
        add_root(&mut roots, "custom", env.expand(p));
    }
    roots
}
fn skipped(name: &str) -> bool {
    [
        ".git",
        ".venv",
        "venv",
        "node_modules",
        "target",
        "__pycache__",
        ".locks",
        "logs",
    ]
    .contains(&name)
}
fn identity(_path: &Path, meta: &fs::Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}", meta.dev(), meta.ino())
    }
    #[cfg(not(unix))]
    {
        _path.to_string_lossy().to_lowercase()
    }
}
fn model_file(path: &Path, source: &str, meta: &fs::Metadata) -> Option<ModelFile> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let blob = source == "ollama"
        && path.parent()?.file_name()?.to_str() == Some("blobs")
        && name.starts_with("sha256-");
    let format = if extension == "gguf" || blob {
        "GGUF"
    } else if extension == "safetensors" {
        "SafeTensors"
    } else if ["pytorch_model.bin", "model.bin"].contains(&name.as_str()) {
        "PyTorch"
    } else if name == "experts.bin" && path.parent()?.join("native_experts.txt").is_file() {
        "Strata pack"
    } else {
        return None;
    };
    let mut header = [0u8; 24];
    let read = fs::File::open(path).and_then(|mut f| f.read(&mut header));
    let status = if format == "GGUF" {
        match read {
            Ok(n)
                if n >= 24
                    && &header[..4] == b"GGUF"
                    && [2, 3].contains(&u32::from_le_bytes(header[4..8].try_into().ok()?)) =>
            {
                "configurationRequired"
            }
            Ok(_) if blob => return None,
            Ok(_) => "invalidFile",
            Err(_) => "unreadable",
        }
    } else if read.is_err() {
        "unreadable"
    } else if format == "Strata pack" {
        "configurationRequired"
    } else {
        "unsupportedFormat"
    };
    let canonical = path.canonicalize().ok()?;
    Some(ModelFile {
        name,
        path: canonical.to_string_lossy().into_owned(),
        selection_path: path.to_string_lossy().into_owned(),
        bytes: meta.len(),
        format: format.into(),
        status: status.into(),
        sources: vec![source.into()],
    })
}
fn ollama_names(root: &Path, started: Instant) -> HashMap<String, String> {
    let base = root.join("manifests");
    let mut queue = VecDeque::from([(base.clone(), 0)]);
    let mut names: HashMap<String, String> = HashMap::new();
    let mut entries = 0;
    while let Some((dir, depth)) = queue.pop_front() {
        if depth > 6 || entries >= 10_000 || started.elapsed() > Duration::from_secs(SCAN_SECONDS) {
            break;
        }
        let Ok(files) = fs::read_dir(dir) else {
            continue;
        };
        for f in files.flatten() {
            entries += 1;
            if entries >= 10_000
                || names.len() >= MODEL_LIMIT * 2
                || started.elapsed() > Duration::from_secs(SCAN_SECONDS)
            {
                break;
            }
            let p = f.path();
            let Ok(t) = f.file_type() else { continue };
            if t.is_dir() {
                queue.push_back((p, depth + 1));
                continue;
            }
            if !t.is_file() {
                continue;
            }
            let Some(v) = json_file(&p) else { continue };
            let Some(layers) = v["layers"].as_array() else {
                continue;
            };
            for layer in layers.iter().take(1024) {
                if layer["mediaType"].as_str() != Some("application/vnd.ollama.image.model") {
                    continue;
                }
                let Some(digest) = layer["digest"].as_str() else {
                    continue;
                };
                let digest = digest.replace(':', "-");
                if digest.len() != 71
                    || !digest.starts_with("sha256-")
                    || !digest[7..].bytes().all(|c| c.is_ascii_hexdigit())
                {
                    continue;
                }
                let label = p
                    .strip_prefix(&base)
                    .ok()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                names
                    .entry(digest)
                    .and_modify(|s| {
                        s.push_str(", ");
                        s.push_str(&label)
                    })
                    .or_insert(label);
            }
        }
    }
    names
}

fn scan(settings: &Settings, env: &Environment) -> Value {
    let started = Instant::now();
    let mut report = Vec::new();
    let mut models: Vec<ModelFile> = Vec::new();
    let mut seen = HashMap::new();
    let mut entries = 0;
    for (source, root) in roots(settings, env) {
        let mut result = ScanRoot {
            source: source.clone(),
            path: root.to_string_lossy().into_owned(),
            status: "scanned".into(),
            files: 0,
            errors: 0,
        };
        match fs::metadata(&root) {
            Err(e) => {
                result.status = if e.kind() == std::io::ErrorKind::NotFound {
                    "missing"
                } else {
                    "unreadable"
                }
                .into();
                report.push(result);
                continue;
            }
            Ok(m) if !m.is_dir() => {
                result.status = "unreadable".into();
                report.push(result);
                continue;
            }
            _ => {}
        }
        let aliases = if source == "ollama" {
            ollama_names(&root, started)
        } else {
            HashMap::new()
        };
        let mut queue = VecDeque::from([(root, 0)]);
        let mut visited = HashSet::new();
        while let Some((dir, depth)) = queue.pop_front() {
            if entries >= ENTRY_LIMIT
                || models.len() >= MODEL_LIMIT
                || started.elapsed() > Duration::from_secs(SCAN_SECONDS)
            {
                result.status = "limited".into();
                break;
            }
            let Ok(canonical) = dir.canonicalize() else {
                result.errors += 1;
                continue;
            };
            if !visited.insert(canonical) {
                continue;
            }
            let files = match fs::read_dir(&dir) {
                Ok(f) => f,
                Err(_) => {
                    result.errors += 1;
                    continue;
                }
            };
            for f in files {
                entries += 1;
                if entries >= ENTRY_LIMIT
                    || models.len() >= MODEL_LIMIT
                    || started.elapsed() > Duration::from_secs(SCAN_SECONDS)
                {
                    result.status = "limited".into();
                    break;
                }
                let f = match f {
                    Ok(f) => f,
                    Err(_) => {
                        result.errors += 1;
                        continue;
                    }
                };
                let p = f.path();
                let name = f.file_name();
                let name = name.to_string_lossy();
                if skipped(&name) {
                    continue;
                }
                let meta = match fs::metadata(&p) {
                    Ok(m) => m,
                    Err(_) => {
                        result.errors += 1;
                        continue;
                    }
                };
                if meta.is_dir() {
                    if depth < DEPTH_LIMIT {
                        queue.push_back((p, depth + 1));
                    } else {
                        result.status = "limited".into();
                    }
                } else if meta.is_file() {
                    if let Some(mut model) = model_file(&p, &source, &meta) {
                        result.files += 1;
                        if let Some(label) = aliases.get(name.as_ref()) {
                            model.name = label.clone();
                        }
                        let key = identity(&p, &meta);
                        if let Some(&index) = seen.get(&key) {
                            let old: &mut ModelFile = &mut models[index];
                            if !old.sources.contains(&source) {
                                old.sources.push(source.clone());
                            }
                        } else {
                            seen.insert(key, models.len());
                            models.push(model);
                        }
                    }
                }
            }
        }
        if result.errors > 0 && result.files == 0 && result.status == "scanned" {
            result.status = "unreadable".into();
        }
        report.push(result);
    }
    models.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.path.cmp(&b.path))
    });
    json!({"roots":report,"files":models,"limited":report.iter().any(|r|r.status=="limited"),"entries":entries,"elapsedMs":started.elapsed().as_millis()})
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        path: PathBuf,
        settings: Settings,
        env: Environment,
    }
    impl Fixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("strata-model-scan-{}", uuid::Uuid::new_v4()));
            let source = path.join("source");
            let home = path.join("home");
            fs::create_dir_all(&source).unwrap();
            fs::create_dir_all(&home).unwrap();
            Self {
                settings: Settings {
                    strata_root: source.to_string_lossy().into(),
                    ..Default::default()
                },
                env: Environment {
                    home,
                    vars: HashMap::new(),
                },
                path,
            }
        }
        fn gguf(&self, path: &Path) {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut b = b"GGUF".to_vec();
            b.extend_from_slice(&3u32.to_le_bytes());
            b.extend_from_slice(&[0u8; 16]);
            fs::write(path, b).unwrap();
        }
        fn inventory(&self) -> Value {
            scan(&self.settings, &self.env)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
    #[test]
    fn scans_all_providers_and_configured_locations_without_writing() {
        let mut f = Fixture::new();
        f.gguf(&f.path.join("source/models/original.gguf"));
        let lm = f.path.join("lm-external");
        f.gguf(&lm.join("publisher/model/lm.gguf"));
        fs::create_dir_all(f.env.home.join(".lmstudio")).unwrap();
        fs::write(
            f.env.home.join(".lmstudio/settings.json"),
            json!({"downloadsFolder":lm}).to_string(),
        )
        .unwrap();
        let hf = f.path.join("hf-external");
        f.gguf(&hf.join("hub/models--owner--model/snapshots/main/unsloth.gguf"));
        let dbpath = f.env.home.join(".unsloth/studio/studio.db");
        fs::create_dir_all(dbpath.parent().unwrap()).unwrap();
        let db = Connection::open(&dbpath).unwrap();
        db.execute("CREATE TABLE app_settings (key TEXT, value_json TEXT)", [])
            .unwrap();
        db.execute(
            "INSERT INTO app_settings VALUES ('hugging_face_cache_home', ?)",
            [json!(hf).to_string()],
        )
        .unwrap();
        drop(db);
        let original_db = fs::read(&dbpath).unwrap();
        let ollama = f.path.join("ollama-external");
        let hash = "a".repeat(64);
        let digest = format!("sha256:{hash}");
        f.gguf(&ollama.join(format!("blobs/sha256-{hash}")));
        fs::write(ollama.join("blobs/sha256-non-model"), b"not model metadata").unwrap();
        let manifest = ollama.join("manifests/registry.ollama.ai/library/qwen/latest");
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(
            manifest,
            json!({"layers":[{"mediaType":"application/vnd.ollama.image.model","digest":digest}]})
                .to_string(),
        )
        .unwrap();
        f.env
            .vars
            .insert("OLLAMA_MODELS".into(), ollama.to_string_lossy().into());
        f.gguf(&f.env.home.join("chosen/custom.gguf"));
        f.settings.model_directories = vec!["~/chosen".into()];
        let v = f.inventory();
        let files = v["files"].as_array().unwrap();
        assert_eq!(files.len(), 5);
        for source in ["strata", "lmstudio", "unsloth", "ollama", "custom"] {
            assert!(
                files
                    .iter()
                    .any(|m| m["sources"].as_array().unwrap().contains(&json!(source))),
                "missing {source}"
            );
        }
        let ollama = files
            .iter()
            .find(|m| m["sources"] == json!(["ollama"]))
            .unwrap();
        assert!(ollama["name"].as_str().unwrap().ends_with("qwen/latest"));
        assert_eq!(fs::read(&dbpath).unwrap(), original_db);
        assert!(v["roots"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["status"] == "missing"));
    }
    #[test]
    fn distinguishes_invalid_and_unsupported_weights_and_ignores_partial_downloads() {
        let f = Fixture::new();
        let root = PathBuf::from(&f.settings.strata_root);
        fs::write(root.join("broken.gguf"), b"GGUF").unwrap();
        fs::write(root.join("model.safetensors"), b"tensor weights").unwrap();
        fs::write(root.join("download.gguf.part"), b"GGUF").unwrap();
        let v = f.inventory();
        let files = v["files"].as_array().unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|m| m["status"] == "invalidFile"));
        assert!(files.iter().any(|m| m["status"] == "unsupportedFormat"));
    }
    #[test]
    #[cfg(unix)]
    fn deduplicates_hf_symlinks_hardlinks_and_cycles_but_preserves_sources() {
        use std::os::unix::fs::symlink;
        let mut f = Fixture::new();
        let hf = f
            .env
            .home
            .join(".cache/huggingface/hub/models--owner--model");
        let blob = hf.join("blobs/hash");
        f.gguf(&blob);
        let snap = hf.join("snapshots/main");
        fs::create_dir_all(&snap).unwrap();
        symlink("../../blobs/hash", snap.join("model.gguf")).unwrap();
        let custom = f.path.join("custom");
        fs::create_dir_all(&custom).unwrap();
        fs::hard_link(&blob, custom.join("model.gguf")).unwrap();
        symlink(&custom, custom.join("cycle")).unwrap();
        f.settings.model_directories = vec![
            custom.to_string_lossy().into(),
            custom.to_string_lossy().into(),
        ];
        let v = f.inventory();
        let files = v["files"].as_array().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["sources"], json!(["unsloth", "custom"]));
        assert_eq!(v["limited"], false);
    }
    #[test]
    fn scans_strata_relocated_data_and_limits_deep_trees() {
        let f = Fixture::new();
        let config = f.env.home.join(".config/strata/settings.json");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        let data = f.path.join("relocated");
        f.gguf(&data.join("models/ready.gguf"));
        fs::write(config, json!({"data_dir":data}).to_string()).unwrap();
        let mut deep = PathBuf::from(&f.settings.strata_root);
        for _ in 0..19 {
            deep = deep.join("nested");
        }
        f.gguf(&deep.join("too-deep.gguf"));
        let v = f.inventory();
        assert_eq!(v["files"].as_array().unwrap().len(), 1);
        assert_eq!(v["limited"], true);
    }
    #[test]
    fn selected_config_discovers_external_native_and_pack_paths() {
        let mut f = Fixture::new();
        let external = f.path.join("external-models");
        f.gguf(&external.join("native/model.gguf"));
        fs::create_dir_all(external.join("pack")).unwrap();
        fs::write(
            external.join("pack/native_experts.txt"),
            "# strata native experts",
        )
        .unwrap();
        fs::write(external.join("pack/experts.bin"), [0u8; 32]).unwrap();
        let config = f.path.join("source/strata-selected.json");
        fs::write(
            &config,
            json!({"cwd": external, "args": ["--pack", "pack", "--native", "native/model.gguf"]})
                .to_string(),
        )
        .unwrap();
        f.settings.config = config.to_string_lossy().into();
        let inventory = f.inventory();
        let files = inventory["files"].as_array().unwrap();
        assert_eq!(files.len(), 2);
        assert!(files
            .iter()
            .any(|m| m["format"] == "Strata pack" && m["bytes"] == 32));
        assert!(files.iter().any(|m| m["format"] == "GGUF"));
    }
    #[test]
    fn old_settings_remain_readable_and_custom_locations_are_bounded() {
        let mut v = serde_json::to_value(Settings::default()).unwrap();
        v.as_object_mut().unwrap().remove("modelDirectories");
        assert!(serde_json::from_value::<Settings>(v)
            .unwrap()
            .model_directories
            .is_empty());
        assert!(valid_directories(&[
            "~/models".into(),
            "/Volumes/Models".into()
        ]));
        assert!(!valid_directories(&["relative/path".into()]));
        assert!(!valid_directories(&vec!["/models".into(); 33]));
    }
}
