//! Explicit opt-in real-model probe. Ephemeral settings/API; always stops its engine.
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc, time::Duration};
use studio_core::{router, Studio};
#[tokio::main]
async fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "usage: allocation-check ROOT ABSOLUTE_GEMMA_CONFIG | allocation-check --vllm VLLM_SETTINGS_JSON (loads existing model)".into(),
        );
    }
    let before = std::fs::read(&args[1]).map_err(|e| e.to_string())?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let mlx = args[0] == "--vllm";
    let vllm = if mlx {
        Some(
            serde_json::from_slice::<studio_core::engine::VllmSettings>(&before)
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    let root = vllm.as_ref().map(|v| v.root.as_str()).unwrap_or(&args[0]);
    let studio = Arc::new(Studio::new(PathBuf::from(root), port));
    {
        let mut settings = studio.settings.write().await;
        if let Some(v) = vllm {
            settings.engine = studio_core::engine::Engine::VllmApple;
            settings.vllm = v;
        } else {
            settings.strata_root = args[0].clone();
            settings.config = args[1].clone();
        }
        settings.max_tokens = 16;
        settings.temperature = 0.0;
    }
    let app = router(studio.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let outcome = probe(&studio, port, mlx).await;
    if outcome.is_err() {
        if let Some(logs) = studio.snapshot().await["logs"].as_array() {
            for line in logs
                .iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.contains("key") && !s.contains("token"))
            {
                eprintln!("{line}");
            }
        }
    }
    let stopped = studio.stop().await;
    server.abort();
    stopped?;
    if std::fs::read(&args[1]).map_err(|e| e.to_string())? != before {
        return Err("Config changed".into());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&outcome?).map_err(|e| e.to_string())?
    );
    Ok(())
}
async fn probe(studio: &Arc<Studio>, port: u16, mlx: bool) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let mut rounds = Vec::new();
    for _ in 0..2 {
        studio.start().await?;
        let state = tokio::time::timeout(Duration::from_secs(150), async {
            loop {
                let state = studio.snapshot().await;
                if state["status"] == "offline" {
                    return Err(
                        "Engine stopped during startup; inspect the startup log".to_string()
                    );
                }
                if mlx
                    && state["status"] == "ready"
                    && state["modelPlan"]["backendBudget"].is_null()
                {
                    studio.refresh_backend_budget().await?;
                    continue;
                }
                if state["status"] == "ready"
                    && if mlx {
                        state["modelPlan"]["backendBudget"].is_object()
                    } else {
                        state["modelPlan"]["runtimeBuffers"]["workspaceBytes"].is_u64()
                    }
                {
                    return Ok(state);
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        })
        .await
        .map_err(|_| "Allocation/startup deadline exceeded".to_string())??;
        let response=client.post(format!("http://127.0.0.1:{port}/v1/chat/completions")).header("x-studio-token", &studio.token)
            .json(&json!({"model":if mlx {"default_model"} else {"strata-gemma3"},"messages":[{"role":"user","content":"What is 2+3? Reply with only the digit."}],"max_tokens":8,"temperature":0,"stream":false}))
            .send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?.json::<Value>().await.map_err(|e|e.to_string())?;
        let answer = response["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .trim();
        if answer != "5" {
            return Err(format!("Unexpected probe answer: {answer:?}"));
        }
        rounds.push(json!({"beforeRequest":state["modelPlan"],"afterRequest":studio.snapshot().await["modelPlan"],"answer":answer}));
        studio.stop().await?;
        let stopped = studio.snapshot().await;
        if !stopped["modelPlan"]["runtimeBuffers"].is_null()
            || !stopped["modelPlan"]["backendBudget"].is_null()
        {
            return Err("Stopped report remained visible".into());
        }
    }
    if !mlx
        && rounds[0]["beforeRequest"]["runtimeBuffers"]["session"]
            == rounds[1]["beforeRequest"]["runtimeBuffers"]["session"]
    {
        return Err("Restart reused session".into());
    }
    Ok(
        json!({"rounds":rounds,"stoppedReportCleared":true,"restartSessionChanged":if mlx {Value::Null}else{json!(true)},"configUnchanged":true,"ordinarySettingsUsed":false}),
    )
}
