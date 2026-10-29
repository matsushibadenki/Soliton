use futures_util::StreamExt;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use tokio::io::AsyncWriteExt;

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && !p.ends_with(".soliton-part")
                && !p.contains(['\\', '%', ':'])
        })
}

pub async fn download(
    base: &Path,
    input: &str,
    token: &str,
    cancel: &AtomicBool,
    progress: impl Fn(u64, Option<u64>),
) -> Result<String, String> {
    let url = reqwest::Url::parse(input).map_err(|_| "Invalid Hugging Face URL")?;
    if url.scheme() != "https"
        || url.host_str() != Some("huggingface.co")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
    {
        return Err(
            "Use an HTTPS huggingface.co URL without credentials or query parameters".into(),
        );
    }
    let parts: Vec<_> = url
        .path()
        .trim_end_matches('/')
        .split('/')
        .skip(1)
        .collect();
    if parts.len() < 2 || !safe_path(&parts.join("/")) {
        return Err("Invalid repository path".into());
    }
    let repo = format!("{}/{}", parts[0], parts[1]);
    let (revision, selected) = if parts.len() == 2 {
        ("main", None)
    } else if parts.len() >= 4 && ["tree", "resolve", "blob"].contains(&parts[2]) {
        (
            parts[3],
            if parts[2] == "tree" {
                None
            } else {
                Some(parts[4..].join("/"))
            },
        )
    } else {
        return Err("Use a repository, tree, resolve or blob URL".into());
    };
    if parts.get(2) == Some(&"tree") && parts.len() != 4 {
        return Err("Use the repository root tree URL, without a subfolder".into());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::custom(|a| {
            let h = a.url().host_str().unwrap_or("");
            if a.previous().len() < 10
                && a.url().scheme() == "https"
                && (h == "huggingface.co"
                    || h.ends_with(".huggingface.co")
                    || h.ends_with(".hf.co"))
            {
                a.follow()
            } else {
                a.stop()
            }
        }))
        .build()
        .map_err(|_| "Cannot create download client")?;
    let auth = |request: reqwest::RequestBuilder| {
        if token.is_empty() {
            request
        } else {
            request.bearer_auth(token)
        }
    };
    let metadata: serde_json::Value = auth(client.get(format!(
        "https://huggingface.co/api/models/{repo}/revision/{revision}"
    )))
    .send()
    .await
    .map_err(|_| "Repository request failed")?
    .error_for_status()
    .map_err(|e| {
        format!(
            "Repository access failed: {}",
            e.status().map(|s| s.as_u16()).unwrap_or(0)
        )
    })?
    .json()
    .await
    .map_err(|_| "Invalid repository metadata")?;
    let sha = metadata["sha"]
        .as_str()
        .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("Missing immutable repository revision")?;
    let dir = base.join(&repo).join(sha);
    let files = metadata["siblings"]
        .as_array()
        .ok_or("Missing file inventory")?;
    let mut count = 0;
    for file in files {
        let name = file["rfilename"].as_str().ok_or("Invalid filename")?;
        if !safe_path(name) {
            return Err("Unsafe repository filename".into());
        }
        if selected.as_ref().is_some_and(|s| s != name) {
            continue;
        }
        if cancel.load(Ordering::SeqCst) {
            return Err("Download paused; submit the same URL to resume".into());
        }
        count += 1;
        let destination = dir.join(name);
        if destination.exists() {
            continue;
        }
        tokio::fs::create_dir_all(destination.parent().unwrap())
            .await
            .map_err(|e| e.to_string())?;
        let partial = destination.with_file_name(format!(
            ".{}.soliton-part",
            destination.file_name().unwrap().to_string_lossy()
        ));
        let mut offset = tokio::fs::metadata(&partial)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        let request = auth(client.get(format!(
            "https://huggingface.co/{repo}/resolve/{sha}/{name}"
        )));
        let response = if offset > 0 {
            request.header("Range", format!("bytes={offset}-"))
        } else {
            request
        }
        .send()
        .await
        .map_err(|_| "File request failed; retry to resume")?;
        let status = response.status();
        if status != reqwest::StatusCode::OK && status != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(format!("File download failed: {}", status.as_u16()));
        }
        if status == reqwest::StatusCode::PARTIAL_CONTENT {
            let expected = format!("bytes {offset}-");
            if !response
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.starts_with(&expected))
            {
                return Err("Invalid resumed response".into());
            }
        } else {
            offset = 0;
        }
        let total = response.content_length().map(|n| n + offset);
        let mut output = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(offset > 0)
            .truncate(offset == 0)
            .open(&partial)
            .await
            .map_err(|e| e.to_string())?;
        let mut stream = response.bytes_stream();
        let mut received = offset;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err("Download paused; submit the same URL to resume".into());
            }
            let chunk = tokio::select! { value = stream.next() => value, _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => continue };
            let Some(chunk) = chunk else {
                break;
            };
            let chunk = chunk.map_err(|_| "Transfer interrupted; retry to resume")?;
            output.write_all(&chunk).await.map_err(|e| e.to_string())?;
            received += chunk.len() as u64;
            progress(received, total);
        }
        output.sync_all().await.map_err(|e| e.to_string())?;
        drop(output);
        if total.is_some_and(|n| n != received) {
            return Err("Incomplete transfer; retry to resume".into());
        }
        tokio::fs::hard_link(&partial, &destination)
            .await
            .map_err(|e| e.to_string())?;
        tokio::fs::remove_file(&partial)
            .await
            .map_err(|e| e.to_string())?;
    }
    if count == 0 {
        return Err("No matching repository files".into());
    }
    Ok(dir.to_string_lossy().into())
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    #[ignore = "downloads a public Hugging Face configuration"]
    async fn live_download_and_repeat() {
        let root = std::env::temp_dir().join(format!("soliton-hub-{}", uuid::Uuid::new_v4()));
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let url = "https://huggingface.co/sshleifer/tiny-gpt2/resolve/main/config.json";
        let dir = super::download(&root, url, "", &cancel, |_, _| {})
            .await
            .unwrap();
        let bytes = std::fs::read(std::path::Path::new(&dir).join("config.json")).unwrap();
        assert!(serde_json::from_slice::<serde_json::Value>(&bytes).is_ok());
        super::download(&root, url, "", &cancel, |_, _| {})
            .await
            .unwrap();
        let target = std::path::Path::new(&dir).join("config.json");
        let partial = target.with_file_name(".config.json.soliton-part");
        std::fs::write(&partial, &bytes[..100]).unwrap();
        std::fs::remove_file(&target).unwrap();
        super::download(&root, url, "", &cancel, |_, _| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), bytes);
        super::download(
            &root,
            "https://huggingface.co/sshleifer/tiny-gpt2",
            "",
            &cancel,
            |_, _| {},
        )
        .await
        .unwrap();
        assert!(std::path::Path::new(&dir)
            .join("pytorch_model.bin")
            .is_file());

        std::fs::remove_dir_all(root).unwrap();
    }
}
