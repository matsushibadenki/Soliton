//! Versioned, offline knowledge-editing experiments. Rust owns the document,
//! job lifecycle, acceptance gates and reports; MLX owns model/adapter buffers.
use crate::{authorized, error, Studio};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::watch,
};
pub const ASSESSMENT_VERSION: u32 = 1;
fn legacy_match_mode() -> String {
    "contains".into()
}
fn unknown_language() -> String {
    "und".into()
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Workspace {
    pub plan: Option<crate::knowledge_planner::Plan>,
    pub view: String,
    pub revision: u64,
    pub draft: Draft,
    pub selected: Option<String>,
    pub experiments: Vec<Experiment>,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            plan: None,
            view: "edit".into(),
            revision: 0,
            draft: Draft::default(),
            selected: None,
            experiments: Vec::new(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Draft {
    pub planning: Option<crate::knowledge_planner::Plan>,
    pub name: String,
    pub model: String,
    pub python: String,
    pub operation: String,
    pub source: String,
    pub training: Vec<Example>,
    pub evaluation: Vec<Case>,
    pub steps: u32,
    pub layers: u32,
    pub learning_rate: f64,
    pub max_tokens: u32,
}
impl Default for Draft {
    fn default() -> Self {
        Self {
            planning: None,
            name: String::new(),
            model: String::new(),
            python: String::new(),
            operation: "add".into(),
            source: String::new(),
            training: vec![Example::default()],
            evaluation: ["target", "paraphrase", "indirect", "locality"]
                .into_iter()
                .map(|kind| Case {
                    kind: kind.into(),
                    ..Case::default()
                })
                .collect(),
            steps: 20,
            layers: 4,
            learning_rate: 0.0001,
            max_tokens: 128,
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Example {
    pub prompt: String,
    pub response: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub kind: String,
    pub prompt: String,
    pub expected: String,
    pub forbidden: String,
    #[serde(rename = "matchMode", default = "legacy_match_mode")]
    pub match_mode: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default = "unknown_language")]
    pub language: String,
}
impl Default for Case {
    fn default() -> Self {
        Self {
            kind: String::new(),
            prompt: String::new(),
            expected: String::new(),
            forbidden: String::new(),
            match_mode: "exact".into(),
            aliases: Vec::new(),
            language: unknown_language(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Experiment {
    #[serde(default)]
    pub assessment_version: u32,
    pub id: String,
    pub created_at: u64,
    pub draft: Draft,
    pub status: String,
    pub directory: String,
    pub results: Vec<Comparison>,
    pub logs: Vec<String>,
    pub error: Option<String>,
    pub provenance: Option<Value>,
    #[serde(default)]
    pub progress: Option<Value>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Comparison {
    pub baseline: Option<Answer>,
    pub candidate: Option<Answer>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub text: String,
    pub tokens: u32,
    pub elapsed_ms: u64,
    pub truncated: bool,
    pub passed: bool,
    #[serde(default)]
    pub reasons: Vec<String>,
}
fn terminal(status: &str) -> bool {
    matches!(
        status,
        "passed" | "rejected" | "failed" | "cancelled" | "interrupted" | "retained" | "archived"
    )
}
impl Workspace {
    pub fn restore(&mut self) {
        for experiment in &mut self.experiments {
            if !terminal(&experiment.status) {
                experiment.status = "interrupted".into();
                experiment.error =
                    Some("Application closed before the experiment completed".into());
            }
        }
    }
}
fn normalized(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn answer_key(value: &str) -> String {
    value.trim().to_lowercase()
}
fn bounded(s: &str, max: usize) -> Result<(), String> {
    if s.len() > max || s.contains('\0') {
        Err("Text exceeds the allowed size or contains NUL".into())
    } else {
        Ok(())
    }
}
impl Draft {
    pub(crate) fn validate(&self, run: bool) -> Result<(), String> {
        if !["add", "suppress"].contains(&self.operation.as_str()) {
            return Err("Unknown knowledge operation".into());
        }
        for text in [&self.name, &self.model, &self.python, &self.source] {
            bounded(text, 4096)?;
        }
        if self.training.is_empty()
            || self.training.len() > 64
            || self.evaluation.len() < if run { 4 } else { 3 }
            || self.evaluation.len() > 32
        {
            return Err("Use 1–64 training examples and 4–32 evaluation cases (saved legacy drafts may have 3)".into());
        }
        if !(1..=500).contains(&self.steps)
            || !(1..=32).contains(&self.layers)
            || !(16..=512).contains(&self.max_tokens)
            || !self.learning_rate.is_finite()
            || !(0.000001..=0.001).contains(&self.learning_rate)
        {
            return Err("Training settings are outside supported bounds".into());
        }
        let mut train_prompts = BTreeSet::new();
        for row in &self.training {
            bounded(&row.prompt, 4096)?;
            bounded(&row.response, 4096)?;
            if run && (row.prompt.trim().is_empty() || row.response.trim().is_empty()) {
                return Err("Complete every training question and answer".into());
            }
            train_prompts.insert(normalized(&row.prompt));
        }
        let mut prompts = BTreeSet::new();
        for case in &self.evaluation {
            if !["target", "paraphrase", "indirect", "locality"].contains(&case.kind.as_str()) {
                return Err("Unknown evaluation category".into());
            }
            if !["contains", "exact"].contains(&case.match_mode.as_str())
                || !["und", "en", "ja", "zh"].contains(&case.language.as_str())
                || case.aliases.len() > 8
            {
                return Err("Invalid answer rule, language or alias count (maximum 8)".into());
            }
            let mut variants = BTreeSet::new();
            variants.insert(answer_key(&case.expected));
            for alias in &case.aliases {
                bounded(alias, 4096)?;
                if alias.trim().is_empty() || !variants.insert(answer_key(alias)) {
                    return Err("Answer aliases must be nonempty and unique".into());
                }
            }
            for text in [&case.prompt, &case.expected, &case.forbidden] {
                bounded(text, 4096)?;
            }
            if run {
                if case.prompt.trim().is_empty() || case.expected.trim().is_empty() {
                    return Err(
                        "Every evaluation needs a question and required answer phrase".into(),
                    );
                }
                let key = normalized(&case.prompt);
                if train_prompts.contains(&key) || !prompts.insert(key) {
                    return Err(
                        "Evaluation questions must be unique and held out from training".into(),
                    );
                }
                if self.operation == "suppress"
                    && case.kind != "locality"
                    && case.forbidden.trim().is_empty()
                {
                    return Err("Suppression tests need both a required refusal phrase and a forbidden knowledge phrase".into());
                }
                if !case.forbidden.trim().is_empty()
                    && std::iter::once(&case.expected)
                        .chain(case.aliases.iter())
                        .any(|s| answer_key(s).contains(&answer_key(&case.forbidden)))
                {
                    return Err("Required and forbidden phrases conflict".into());
                }
            }
        }
        if run {
            if self.name.trim().is_empty() || self.source.trim().is_empty() {
                return Err(
                    "Give the experiment a name and record the source or reason for editing".into(),
                );
            }
            for kind in ["target", "paraphrase", "indirect", "locality"] {
                if !self.evaluation.iter().any(|c| c.kind == kind) {
                    return Err(
                        "Include target, paraphrase, indirect and locality evaluation cases".into(),
                    );
                }
            }
        }
        Ok(())
    }
}
fn score(case: &Case, text: &str, truncated: bool) -> bool {
    assessment(case, text, truncated).is_empty()
}
fn assessment(case: &Case, text: &str, truncated: bool) -> Vec<String> {
    // Answer matching preserves punctuation and internal whitespace in exact mode.
    let answer = answer_key(text);
    let mut reasons = Vec::new();
    if truncated {
        reasons.push("truncated".into());
    }
    if answer.is_empty() {
        reasons.push("empty".into());
    }
    let matched = std::iter::once(&case.expected)
        .chain(case.aliases.iter())
        .any(|expected| {
            let expected = answer_key(expected);
            !expected.is_empty()
                && match case.match_mode.as_str() {
                    "exact" => answer == expected,
                    "contains" => answer.contains(&expected),
                    _ => false,
                }
        });
    if !matched {
        reasons.push(
            if case.match_mode == "exact" {
                "notExact"
            } else {
                "missingExpected"
            }
            .into(),
        );
    }
    if !case.forbidden.trim().is_empty() && answer.contains(&answer_key(&case.forbidden)) {
        reasons.push("forbiddenPresent".into());
    }
    reasons
}
fn quality_passed(experiment: &Experiment) -> bool {
    experiment.assessment_version == ASSESSMENT_VERSION
        && experiment.draft.validate(true).is_ok()
        && experiment.provenance.is_some()
        && experiment.results.len() == experiment.draft.evaluation.len()
        && experiment
            .results
            .iter()
            .zip(&experiment.draft.evaluation)
            .all(|(row, case)| {
                row.baseline.is_some()
                    && row
                        .candidate
                        .as_ref()
                        .is_some_and(|a| a.passed && score(case, &a.text, a.truncated))
                    && (case.kind != "locality"
                        || row
                            .baseline
                            .as_ref()
                            .is_some_and(|a| a.passed && score(case, &a.text, a.truncated)))
            })
}

pub async fn snapshot(State(s): State<Arc<Studio>>, h: HeaderMap) -> Response {
    if !authorized(&h, &s) {
        return error(StatusCode::UNAUTHORIZED, "Studio token required");
    }
    let settings = s.settings.read().await;
    let candidate = Path::new(&settings.vllm.executable)
        .parent()
        .map(|p| p.join("python"));
    let python = candidate
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| settings.vllm.python.clone());
    let model = settings.vllm.model.clone();
    drop(settings);
    let busy = s.knowledge_job.lock().await.is_some();
    Json(
        json!({"workspace":s.document.read().await.knowledge,"busy":busy,
        "suggestedModel":model,"suggestedPython":python}),
    )
    .into_response()
}

pub async fn action(State(s): State<Arc<Studio>>, h: HeaderMap, Json(v): Json<Value>) -> Response {
    if !authorized(&h, &s) {
        return error(StatusCode::UNAUTHORIZED, "Studio token required");
    }
    let result = if matches!(v["action"].as_str(), Some("generatePlan" | "importPlan")) {
        crate::knowledge_planner::apply(s, v).await
    } else {
        apply(s, v).await
    };
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(StatusCode::BAD_REQUEST, e),
    }
}
async fn apply(s: Arc<Studio>, v: Value) -> Result<Value, String> {
    match v["action"].as_str().unwrap_or("") {
        "view" => {
            let view = v["view"].as_str().ok_or("Missing view")?;
            if !["edit", "results", "history"].contains(&view) {
                return Err("Unknown knowledge view".into());
            }
            s.document.write().await.knowledge.view = view.into();
        }
        "cancel" => {
            s.cancel_knowledge().await;
        }
        "select" | "retain" | "archive" => {
            if v["action"] != "select" && s.knowledge_job.lock().await.is_some() {
                return Err("Wait for the experiment report to finish saving".into());
            }
            let id = v["id"].as_str().ok_or("Missing experiment ID")?;
            let mut doc = s.document.write().await;
            let item = doc
                .knowledge
                .experiments
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or("Unknown experiment")?;
            match v["action"].as_str().unwrap() {
                "retain" if item.status == "passed" && quality_passed(item) => {
                    item.status = "retained".into()
                }
                "archive" if terminal(&item.status) => item.status = "archived".into(),
                "select" => (),
                _ => return Err("Only a completed, passing experiment can be retained".into()),
            }
            doc.knowledge.selected = Some(id.into());
            if v["action"] == "select" {
                doc.knowledge.view = "results".into();
            }
        }
        "save" | "run" => {
            let run = v["action"] == "run";
            let mut draft: Draft =
                serde_json::from_value(v["draft"].clone()).map_err(|e| e.to_string())?;
            draft.validate(run)?;
            let revision = v["revision"].as_u64().ok_or("Missing draft revision")?;
            // Reserve the engine lifecycle before the document: no training/inference overlap.
            let lifecycle = s
                .lifecycle
                .try_lock()
                .map_err(|_| "Another engine operation is running")?;
            let mut job = s.knowledge_job.lock().await;
            if job.is_some() {
                return Err("A knowledge experiment is running".into());
            }
            if run {
                if s.preparation.lock().await.busy() {
                    return Err("Wait for model preparation to finish".into());
                }
                if s.process
                    .lock()
                    .await
                    .child
                    .as_mut()
                    .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
                {
                    return Err("Stop the inference engine before running an experiment".into());
                }
                let model = PathBuf::from(draft.model.trim());
                if !model.is_absolute() || !model.join("config.json").is_file() {
                    return Err("Select an absolute local MLX model folder with config.json".into());
                }
                draft.model = std::fs::canonicalize(model)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .into();
                let python = Path::new(draft.python.trim());
                if !python.is_absolute() || !python.is_file() {
                    return Err("Choose an absolute Python executable with mlx-lm installed".into());
                }
                // Preserve venv symlinks: canonicalizing Python would discard its environment.
                draft.python = draft.python.trim().into();
            }
            let storage = s.storage.lock().await.clone();
            let mut doc = s.document.write().await;
            if doc.knowledge.revision != revision {
                return Err(
                    "Draft changed in another window. Reload the draft before saving".into(),
                );
            }
            if run && doc.knowledge.experiments.len() >= 40 {
                return Err("Experiment history is full (40). Export and manage the stored document before adding experiments".into());
            }
            doc.knowledge.draft = draft.clone();
            doc.knowledge.revision += 1;
            if run {
                let id = uuid::Uuid::new_v4().to_string();
                let parent = storage.as_deref().and_then(Path::parent).unwrap_or(&s.root);
                let directory = std::path::absolute(parent.join(".soliton-knowledge").join(&id))
                    .map_err(|e| e.to_string())?;
                let experiment = Experiment {
                    assessment_version: ASSESSMENT_VERSION,
                    id: id.clone(),
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    results: vec![Comparison::default(); draft.evaluation.len()],
                    draft,
                    status: "checking".into(),
                    directory: directory.to_string_lossy().into(),
                    logs: vec![],
                    error: None,
                    provenance: None,
                    progress: None,
                };
                doc.knowledge.experiments.insert(0, experiment.clone());
                doc.knowledge.selected = Some(id.clone());
                doc.knowledge.view = "results".into();
                let (cancel, receiver) = watch::channel(false);
                *job = Some(cancel);
                drop(doc);
                drop(job);
                drop(lifecycle);
                // Persist before spawning: a failed save must not leave an untracked worker.
                if let Err(e) = s.persist().await {
                    s.knowledge_job.lock().await.take();
                    s.update_knowledge(&id, |e| {
                        e.status = "failed".into();
                        e.error = Some("Could not persist experiment".into());
                    })
                    .await;
                    return Err(e);
                }
                let worker = s.clone();
                tokio::spawn(async move {
                    worker.run_knowledge(experiment, receiver).await;
                });
                return Ok(json!({"ok":true,"id":id}));
            }
        }
        _ => return Err("Unknown knowledge action".into()),
    }
    s.persist().await?;
    Ok(json!({"ok":true}))
}
impl Studio {
    pub async fn cancel_knowledge(&self) {
        if let Some(cancel) = self.knowledge_job.lock().await.as_ref() {
            let _ = cancel.send(true);
        }
    }
    pub async fn shutdown_knowledge(&self) {
        let receiver = {
            let job = self.knowledge_job.lock().await;
            job.as_ref().map(|cancel| {
                let _ = cancel.send(true);
                cancel.subscribe()
            })
        };
        // The supervisor drops its only sender after killing and reaping the worker.
        if let Some(mut receiver) = receiver {
            while receiver.changed().await.is_ok() {}
        }
    }
    async fn update_knowledge(&self, id: &str, update: impl FnOnce(&mut Experiment)) {
        let mut doc = self.document.write().await;
        if let Some(e) = doc.knowledge.experiments.iter_mut().find(|e| e.id == id) {
            update(e);
        }
    }
    async fn run_knowledge(
        self: Arc<Self>,
        experiment: Experiment,
        mut cancel: watch::Receiver<bool>,
    ) {
        let id = experiment.id.clone();
        let result = self.knowledge_worker(&experiment, &mut cancel).await;
        self.update_knowledge(&id, |e| {
            if *cancel.borrow() {
                e.status = "cancelled".into();
            } else if let Err(error) = result {
                e.status = "failed".into();
                e.error = Some(error);
            } else {
                e.status = if quality_passed(e) {
                    "passed"
                } else {
                    "rejected"
                }
                .into();
            }
        })
        .await;
        let report = self
            .document
            .read()
            .await
            .knowledge
            .experiments
            .iter()
            .find(|e| e.id == id)
            .cloned();
        if let Some(report) = report {
            let path = Path::new(&report.directory).join("report.json");
            let saved = match serde_json::to_vec_pretty(&report) {
                Ok(data) => tokio::fs::write(path, data)
                    .await
                    .map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            };
            if let Err(error) = saved {
                self.update_knowledge(&id, |e| {
                    e.status = "failed".into();
                    e.error = Some(format!("Could not save report: {error}"));
                })
                .await;
            }
        }
        if let Err(error) = self.persist().await {
            self.update_knowledge(&id, |e| {
                e.status = "failed".into();
                e.error = Some(format!("Could not persist result: {error}"));
            })
            .await;
        }
        self.knowledge_job.lock().await.take();
    }
    async fn knowledge_worker(
        &self,
        experiment: &Experiment,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<(), String> {
        let directory = Path::new(&experiment.directory);
        tokio::fs::create_dir_all(directory)
            .await
            .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
                .await
                .map_err(|e| e.to_string())?;
        }
        tokio::fs::write(
            directory.join("manifest.json"),
            serde_json::to_vec_pretty(experiment).map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
        let script = directory.join("worker.py");
        tokio::fs::write(&script, include_str!("knowledge_worker.py"))
            .await
            .map_err(|e| e.to_string())?;
        if *cancel.borrow() {
            return Err("Cancelled".into());
        }
        let mut command = Command::new(&experiment.draft.python);
        command
            .args(["-u", "-I"])
            .arg(&script)
            .arg(directory)
            .current_dir(directory)
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .env("HF_DATASETS_OFFLINE", "1")
            .env("TOKENIZERS_PARALLELISM", "false")
            .env("WANDB_DISABLED", "true")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .spawn()
            .map_err(|e| format!("Could not start MLX Python: {e}"))?;
        let mut stdout = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut stderr = BufReader::new(child.stderr.take().unwrap()).lines();
        let timeout = tokio::time::sleep(Duration::from_secs(3600));
        tokio::pin!(timeout);
        let (mut out, mut err, mut complete) = (true, true, false);
        let result: Result<(), String> = async {
            while out || err {
                tokio::select! {
                    biased;
                    _ = cancel.changed() => { return Err("Cancelled".into()); }
                    _ = &mut timeout => { return Err("Experiment exceeded the one-hour limit".into()); }
                    line = stdout.next_line(), if out => match line.map_err(|e| e.to_string())? {
                        None => out = false,
                        Some(line) => {
                            if line.len() > 128 * 1024 { return Err("Worker response exceeded the protocol limit".into()); }
                            if let Ok(event) = serde_json::from_str::<Value>(&line) {
                                match event["kind"].as_str() {
                                    Some("phase") => {
                                        let phase = event["phase"].as_str().unwrap_or("");
                                        if !["checking", "baseline", "training", "candidate", "verifying"].contains(&phase) { return Err("Unknown worker phase".into()); }
                                        self.update_knowledge(&experiment.id, |e| e.status = phase.into()).await;
                                    }
                                    Some("answer") => {
                                        let index = event["index"].as_u64().ok_or("Missing answer index")? as usize;
                                        let case = experiment.draft.evaluation.get(index).ok_or("Invalid answer index")?;
                                        let phase = event["phase"].as_str().ok_or("Missing answer phase")?;
                                        if !["baseline", "candidate"].contains(&phase) { return Err("Invalid answer phase".into()); }
                                        let text = event["text"].as_str().ok_or("Missing answer text")?.to_string();
                                        let truncated = event["truncated"].as_bool().ok_or("Missing truncation state")?;
                                        let reasons=assessment(case,&text,truncated);
                                        let answer = Answer { passed: reasons.is_empty(), text, truncated, reasons,
                                            tokens: event["tokens"].as_u64().unwrap_or(0) as u32, elapsed_ms: event["elapsedMs"].as_u64().unwrap_or(0) };
                                        self.update_knowledge(&experiment.id, |e| {
                                            if phase == "baseline" { e.results[index].baseline = Some(answer); }
                                            else { e.results[index].candidate = Some(answer); }
                                        }).await;
                                    }
                                    Some("progress") => {
                                        if event["iteration"].as_u64().is_none_or(|n| n > experiment.draft.steps as u64)
                                            || event["loss"].as_f64().is_none_or(|n| !n.is_finite()) {
                                            return Err("Invalid training metrics".into());
                                        }
                                        self.update_knowledge(&experiment.id, |e| e.progress = Some(event)).await;
                                    }
                                    Some("complete") => {
                                        for key in ["sourceSha256", "adapterSha256"] {
                                            let hash = event[key].as_str().unwrap_or("");
                                            if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) { return Err("Missing artifact fingerprint".into()); }
                                        }
                                        self.update_knowledge(&experiment.id, |e| e.provenance = Some(event)).await;
                                        complete = true;
                                    }
                                    Some("error") => return Err(event["message"].as_str().unwrap_or("MLX worker failed").into()),
                                    _ => (),
                                }
                            }
                        }
                    },
                    line = stderr.next_line(), if err => match line.map_err(|e| e.to_string())? {
                        None => err = false,
                        Some(line) => self.update_knowledge(&experiment.id, |e| {
                            if e.logs.len() >= 60 { e.logs.remove(0); }
                            e.logs.push(line.chars().take(1000).collect());
                        }).await,
                    }
                }
            }
            tokio::select! {
                _ = cancel.changed() => Err("Cancelled".into()),
                _ = &mut timeout => Err("Experiment timed out".into()),
                status = child.wait() => {
                    let status = status.map_err(|e| e.to_string())?;
                    if status.success() && complete { Ok(()) } else { Err(format!("MLX worker did not finish successfully ({status})")) }
                }
            }
        }.await;
        if result.is_err() {
            #[cfg(unix)]
            if let Some(pid) = child.id() {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_rules_reject_substrings_negations_and_extra_units_with_reviewed_aliases() {
        let mut case = Case {
            expected: "4".into(),
            ..Case::default()
        };
        assert!(score(&case, " 4\n", false));
        for text in ["14", "4個", "4, but the answer is 5", "The answer is not 4"] {
            assert!(!score(&case, text, false));
        }
        assert_eq!(assessment(&case, "14", false), vec!["notExact"]);
        case.expected = "Amber".into();
        case.aliases = vec!["琥珀色".into(), "Amber.".into()];
        assert!(score(&case, "AMBER.", false));
        assert!(score(&case, "琥珀色", false));
        assert!(!score(&case, "It is not amber", false));
        assert!(!score(&case, "琥珀色ではありません", false));
        case.forbidden = "secret".into();
        case.aliases.push("secret amber".into());
        assert!(!score(&case, "secret amber", false));
        assert!(assessment(&case, "secret amber", false).contains(&"forbiddenPresent".into()));
        assert!(!score(&case, "Amber", true));
        assert!(!score(&case, " ", false));
    }
    #[test]
    fn legacy_cases_preserve_contains_but_new_defaults_are_exact() {
        let case: Case = serde_json::from_value(
            json!({"kind":"target","prompt":"q","expected":"4","forbidden":""}),
        )
        .unwrap();
        assert_eq!(case.match_mode, "contains");
        assert_eq!(case.language, "und");
        assert!(case.aliases.is_empty());
        assert!(score(&case, "14", false));
        assert_eq!(Case::default().match_mode, "exact");
        assert_eq!(Draft::default().evaluation.len(), 4);
    }
    #[test]
    fn alias_conflicts_bounds_and_indirect_coverage_are_enforced() {
        let mut d = draft();
        d.evaluation.retain(|c| c.kind != "indirect");
        assert!(d.validate(false).is_ok());
        assert!(d.validate(true).is_err());
        d = draft();
        d.evaluation[0].aliases = vec!["other".into(), " OTHER ".into()];
        assert!(d.validate(true).is_err());
        d.evaluation[0].aliases = vec![" ".into()];
        assert!(d.validate(false).is_err());
        d.evaluation[0].aliases = vec!["leak".into()];
        d.evaluation[0].forbidden = "LEAK".into();
        assert!(d.validate(true).is_err());
        d = draft();
        d.evaluation[0].prompt = "Training\n  question".into();
        assert!(d.validate(true).is_err());
        d = draft();
        d.evaluation[0].language = "unknown".into();
        assert!(d.validate(false).is_err());
    }
    fn draft() -> Draft {
        Draft {
            name: "Local fact".into(),
            source: "Approved test fixture".into(),
            training: vec![Example {
                prompt: "Training question".into(),
                response: "Answer".into(),
            }],
            evaluation: ["target", "paraphrase", "indirect", "locality"]
                .into_iter()
                .map(|kind| Case {
                    kind: kind.into(),
                    prompt: format!("Held-out {kind}"),
                    expected: "answer".into(),
                    forbidden: String::new(),
                    match_mode: "contains".into(),
                    ..Case::default()
                })
                .collect(),
            ..Draft::default()
        }
    }
    #[test]
    fn evaluation_is_held_out_and_requires_locality() {
        let mut d = draft();
        assert!(d.validate(true).is_ok());
        d.evaluation[0].prompt = " TRAINING QUESTION ".into();
        assert!(d.validate(true).is_err());
        d = draft();
        d.evaluation[2].kind = "target".into();
        assert!(d.validate(true).is_err());
        d = draft();
        d.evaluation[1].prompt = d.evaluation[0].prompt.clone();
        assert!(d.validate(true).is_err());
    }
    #[test]
    fn suppression_requires_positive_and_negative_checks() {
        let mut d = draft();
        d.operation = "suppress".into();
        assert!(d.validate(true).is_err());
        for c in &mut d.evaluation {
            c.forbidden = "secret".into();
        }
        assert!(d.validate(true).is_ok());
        let c = &d.evaluation[0];
        assert!(score(c, "An ANSWER", false));
        assert!(!score(c, "An answer with SECRET", false));
        assert!(!score(c, "", false));
        assert!(!score(c, "answer", true));
        d.evaluation[0].expected = "secret".into();
        assert!(d.validate(true).is_err());
    }
    #[test]
    fn runtime_bounds_and_incomplete_drafts() {
        assert!(Draft::default().validate(false).is_ok());
        assert!(Draft::default().validate(true).is_err());
        let mut d = draft();
        d.steps = 0;
        assert!(d.validate(true).is_err());
        d.steps = 20;
        d.learning_rate = f64::NAN;
        assert!(d.validate(true).is_err());
    }
    #[test]
    fn acceptance_needs_complete_results_and_passing_baseline_locality() {
        let mut e = Experiment {
            assessment_version: ASSESSMENT_VERSION,
            id: "id".into(),
            created_at: 0,
            draft: draft(),
            status: "verifying".into(),
            directory: String::new(),
            results: vec![Comparison::default(); 4],
            logs: vec![],
            error: None,
            provenance: Some(json!({})),
            progress: None,
        };
        assert!(!quality_passed(&e));
        let a = Answer {
            text: "answer".into(),
            passed: true,
            tokens: 1,
            elapsed_ms: 1,
            truncated: false,
            reasons: vec![],
        };
        e.results = vec![
            Comparison {
                baseline: Some(a.clone()),
                candidate: Some(a)
            };
            4
        ];
        assert!(quality_passed(&e));
        let mut forged = e.clone();
        forged.results[0].candidate.as_mut().unwrap().text = "wrong".into();
        assert!(!quality_passed(&forged));
        let mut historical = e.clone();
        historical.assessment_version = 0;
        assert!(!quality_passed(&historical));
        let mut serialized = serde_json::to_value(&e).unwrap();
        serialized
            .as_object_mut()
            .unwrap()
            .remove("assessmentVersion");
        assert_eq!(
            serde_json::from_value::<Experiment>(serialized)
                .unwrap()
                .assessment_version,
            0
        );
        e.results[3].baseline.as_mut().unwrap().passed = false;
        assert!(!quality_passed(&e));
        let mut w = Workspace {
            experiments: vec![e],
            ..Workspace::default()
        };
        w.restore();
        assert_eq!(w.experiments[0].status, "interrupted");
    }
    #[tokio::test]
    async fn stale_drafts_and_invalid_views_cannot_overwrite_shared_state() {
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let s = Arc::new(Studio::new(root, 0));
        let first = draft();
        apply(
            s.clone(),
            json!({"action":"save", "revision":0, "draft":first}),
        )
        .await
        .unwrap();
        assert!(apply(
            s.clone(),
            json!({"action":"save", "revision":0, "draft":Draft::default()})
        )
        .await
        .is_err());
        assert!(apply(s.clone(), json!({"action":"view", "view":"unknown"}))
            .await
            .is_err());
        let doc = s.document.read().await;
        assert_eq!(doc.knowledge.revision, 1);
        assert_eq!(doc.knowledge.draft.name, "Local fact");
        assert_eq!(doc.knowledge.view, "edit");
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn cancellation_reaps_worker_releases_reservation_and_survives_restart() {
        use std::os::unix::fs::PermissionsExt;
        let root =
            std::env::temp_dir().join(format!("soliton-knowledge-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("model")).unwrap();
        std::fs::write(root.join("model/config.json"), "{}").unwrap();
        let python = root.join("test-python");
        std::fs::write(
            &python,
            "#!/bin/sh\nprintf '{\"kind\":\"phase\",\"phase\":\"training\"}\\n'\nsleep 30\n",
        )
        .unwrap();
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o700)).unwrap();
        let storage = root.join("state.json");
        let s = Arc::new(Studio::new(root.clone(), 0).with_storage(storage.clone()));
        let mut d = draft();
        d.python = python.to_string_lossy().into();
        d.model = root.join("model").to_string_lossy().into();
        apply(s.clone(), json!({"action":"run", "revision":0, "draft":d}))
            .await
            .unwrap();
        assert!(s.knowledge_job.lock().await.is_some());
        assert!(s.start().await.is_err());
        assert!(
            apply(s.clone(), json!({"action":"run", "revision":1, "draft":d}))
                .await
                .is_err()
        );
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if s.document.read().await.knowledge.experiments[0].status == "training" {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(3), s.shutdown_knowledge())
            .await
            .unwrap();
        assert!(s.knowledge_job.lock().await.is_none());
        let id = s.document.read().await.knowledge.experiments[0].id.clone();
        assert!(apply(s.clone(), json!({"action":"retain", "id":id}))
            .await
            .is_err());
        let restored = Studio::new(root.clone(), 0).with_storage(storage);
        assert_eq!(
            restored.document.read().await.knowledge.experiments[0].status,
            "cancelled"
        );
        // Cancel before the spawned task has a chance to create its artifacts.
        apply(s.clone(), json!({"action":"run", "revision":1, "draft":d}))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), s.shutdown_knowledge())
            .await
            .unwrap();
        let doc = s.document.read().await;
        assert_eq!(doc.knowledge.experiments[0].status, "cancelled");
        assert!(Path::new(&doc.knowledge.experiments[0].directory)
            .join("report.json")
            .is_file());
        drop(doc);
        std::fs::remove_dir_all(root).unwrap();
    }
}
