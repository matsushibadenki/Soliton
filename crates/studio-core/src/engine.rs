//! Engine-specific configuration. Documents, API streaming and telemetry stay in Studio.
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::process::Command;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Engine {
    #[default]
    Strata,
    VllmApple,
}
impl Engine {
    pub fn label(self) -> &'static str {
        match self {
            Self::Strata => "Strata",
            Self::VllmApple => "vLLM-Apple",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VllmSettings {
    pub root: String,
    pub python: String,
    pub model: String,
    pub backend: String,
    pub executable: String,
    pub evidence: String,
    pub context: u32,
}
impl Default for VllmSettings {
    fn default() -> Self {
        Self {
            root: String::new(),
            python: "python3".into(),
            model: String::new(),
            backend: "mlx_lm".into(),
            executable: String::new(),
            evidence: String::new(),
            context: 1024,
        }
    }
}
impl VllmSettings {
    pub fn valid(&self) -> bool {
        matches!(self.backend.as_str(), "mlx_lm" | "vllm_metal")
            && (1..=131072).contains(&self.context)
            && [
                &self.root,
                &self.python,
                &self.model,
                &self.executable,
                &self.evidence,
            ]
            .iter()
            .all(|s| s.len() <= 4096 && !s.contains('\0'))
    }
    pub fn validate_paths(&self) -> Result<(), String> {
        if !self.valid() || !Path::new(&self.root).join("vllm_apple/cli.py").is_file() {
            return Err("Select a vLLM-Apple source directory containing vllm_apple/cli.py".into());
        }
        // Local artifacts only: never implicitly download a model from a Hub ID.
        if !Path::new(&self.model).is_absolute() || !Path::new(&self.model).is_dir() {
            return Err("Select an existing absolute local model directory for vLLM-Apple".into());
        }
        if !Path::new(&self.python).is_absolute()
            || !Path::new(&self.python).is_file()
            || !Path::new(&self.executable).is_absolute()
            || !Path::new(&self.executable).is_file()
        {
            return Err(
                "Select absolute Python and backend executable paths for vLLM-Apple".into(),
            );
        }
        if !self.evidence.is_empty()
            && (!Path::new(&self.evidence).is_absolute() || !Path::new(&self.evidence).is_file())
        {
            return Err("Select an existing absolute qualification evidence JSON path".into());
        }
        Ok(())
    }
    pub fn inspection_command(&self) -> Result<Command, String> {
        self.validate_paths()?;
        let mut cmd = Command::new(&self.python);
        cmd.current_dir(&self.root).args([
            "-u",
            "-m",
            "vllm_apple",
            "inspect-model",
            &self.model,
            "--backend",
            &self.backend,
            "--backend-executable",
            &self.executable,
            "--max-model-len",
            &self.context.to_string(),
            "--max-concurrent-requests",
            "1",
        ]);
        if !self.evidence.is_empty() {
            cmd.args(["--architecture-evidence", &self.evidence]);
        }
        Ok(cmd)
    }
    pub fn command(&self, port: u16, backend_port: u16, key: &str) -> Result<Command, String> {
        self.validate_paths()?;
        let mut cmd = Command::new(&self.python);
        cmd.current_dir(&self.root)
            // Inject the private token in-process, never into OS command-line arguments.
            .args(["-u", "-c", "import os,sys; from vllm_apple.cli import main; sys.exit(main(sys.argv[1:]+['--session-token',os.environ.pop('SOLITON_ENGINE_TOKEN')]))", "serve", &self.model,
                "--host", "127.0.0.1", "--port", &port.to_string(),
                "--backend-port", &backend_port.to_string(), "--backend-kind", &self.backend,
                "--backend-executable", &self.executable, "--max-concurrent-requests", "1",
                "--shutdown-grace-period", "1"])
            .env("SOLITON_ENGINE_TOKEN", key);
        cmd.args(["--max-model-len", &self.context.to_string()]);
        if !self.evidence.is_empty() {
            cmd.args([
                "--architecture-evidence",
                &self.evidence,
                "--disable-metal-tuning",
            ]);
        }
        Ok(cmd)
    }
}
