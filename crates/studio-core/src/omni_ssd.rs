//! Opt-in text chat snapshots scoped to one authenticated engine session.
use crate::prefix_store::{Identity, Store};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub(crate) struct Cache {
    store: Store,
    stage: PathBuf,
    session: String,
    pub(crate) verification_delay: Duration,
}

impl Cache {
    #[cfg(target_os = "macos")]
    pub(crate) fn configure(
        command: &mut tokio::process::Command,
        root: &Path,
        key: &str,
    ) -> Result<Self, String> {
        use std::os::unix::fs::DirBuilderExt;
        Store::existing(root, 512 * 1024 * 1024).map_err(|e| e.to_string())?;
        let store =
            Store::open(&root.join("store"), 512 * 1024 * 1024).map_err(|e| e.to_string())?;
        let stage = root
            .canonicalize()
            .map_err(|e| e.to_string())?
            .join(format!("stage-{key}"));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&stage)
            .map_err(|e| e.to_string())?;
        command.arg("--slot-save-path").arg(&stage);
        // SAFETY: the child uses only async-signal-safe libc operations before exec.
        unsafe {
            command.pre_exec(|| {
                let limit = libc::rlimit {
                    rlim_cur: 64 << 20,
                    rlim_max: 64 << 20,
                };
                libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
                if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let verification_delay = if std::env::var_os("SOLITON_OMNI_SSD_VERIFY").is_some() {
            Duration::from_millis(500)
        } else {
            Duration::ZERO
        };
        Ok(Self {
            store,
            stage,
            session: key.to_owned(),
            verification_delay,
        })
    }

    pub(crate) fn matches(&self, key: &str) -> bool {
        self.session == key
    }

    pub(crate) fn identity(&self, conversation: &str, messages: &[Value]) -> Identity {
        let session = digest(self.session.as_bytes());
        Identity {
            model: session.clone(),
            tokenizer: session.clone(),
            runtime: session,
            prefix: digest(json!([conversation, messages]).to_string().as_bytes()),
            format: "omni-desktop-slot-v1".into(),
        }
    }

    pub(crate) async fn action(
        &self,
        client: &reqwest::Client,
        port: u16,
        action: &str,
    ) -> Result<(), reqwest::Error> {
        client
            .post(format!("http://127.0.0.1:{port}/slots/0?action={action}"))
            .bearer_auth(&self.session)
            .timeout(Duration::from_secs(30))
            .json(&json!({"filename":"slot.bin"}))
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        Ok(())
    }

    pub(crate) fn restore(&self, identity: &Identity) -> std::io::Result<bool> {
        self.store
            .restore_to(identity, &self.stage.join("slot.bin"))
            .map(|result| result.is_some())
    }

    pub(crate) fn save(&self, identity: &Identity) -> std::io::Result<()> {
        let mut file = fs::File::open(self.stage.join("slot.bin"))?;
        let bytes = file.metadata()?.len();
        self.store
            .put_evicting(identity, &mut file, bytes)
            .map(|_| ())
    }

    pub(crate) fn clear(&self) {
        let _ = fs::remove_file(self.stage.join("slot.bin"));
    }
}

impl Drop for Cache {
    fn drop(&mut self) {
        self.clear();
        let _ = fs::remove_dir(&self.stage);
    }
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
