//! Qualification-only native probe; not an enabled Studio Realtime API.
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("OMNI_PROBE_KEY")?;
    let output = std::path::PathBuf::from(std::env::var("OMNI_PROBE_OUTPUT")?);
    let mut results = vec![];
    let audio = std::env::var_os("OMNI_PROBE_AUDIO_DIR")
        .map(|path| -> std::io::Result<_> {
            let path = std::path::PathBuf::from(path);
            [1, 2]
                .map(|turn| {
                    Ok::<_, std::io::Error>((
                        STANDARD.encode(std::fs::read(path.join(format!("turn{turn}.pcm")))?),
                        STANDARD.encode(std::fs::read(path.join(format!("turn{turn}.wav")))?),
                    ))
                })
                .into_iter()
                .collect::<std::io::Result<Vec<_>>>()
        })
        .transpose()?;
    let max_output = std::env::var("OMNI_PROBE_MAX_OUTPUT")
        .unwrap_or_else(|_| "128".into())
        .parse::<u32>()?;
    let selected_mode = std::env::var("OMNI_PROBE_MODE").ok();
    let instruction_override = std::env::var("OMNI_PROBE_INSTRUCTIONS").ok();
    let cancel = std::env::var_os("OMNI_PROBE_CANCEL").is_some();
    let boundary = std::env::var_os("OMNI_PROBE_BOUNDARY").is_some();
    let turn_events = |turn: usize, text: &str| {
        if let Some(audio) = &audio {
            vec![
                json!({"type":"input_audio_buffer.append","audio":audio[turn].0}),
                json!({"type":"input_audio_buffer.commit"}),
            ]
        } else {
            vec![
                json!({"type":"conversation.item.create","item":{"type":"message","role":"user","content":[{"type":"input_text","text":text}]}}),
            ]
        }
    };
    for (mode, modalities, instructions) in [
        ("text", json!(["text"]), "Respond in text."),
        ("audio", json!(["audio"]), "Respond in audio."),
        (
            "interleaved",
            json!(["text", "audio"]),
            "Respond in interleaved text and audio.",
        ),
    ] {
        let instructions = instruction_override.as_deref().unwrap_or(instructions);
        if selected_mode
            .as_deref()
            .is_some_and(|selected| selected != mode)
            || (boundary && selected_mode.is_none() && mode != "interleaved")
        {
            continue;
        }
        let question = "Remember the code 42. Say hello in English.";
        let content = if let Some(audio) = &audio {
            json!([{"type":"input_audio","input_audio":{"data":audio[0].1,"format":"wav"}}])
        } else {
            json!(question)
        };
        let studio = std::env::var_os("OMNI_PROBE_STUDIO").is_some();
        let count: Value = if studio {
            Value::Null
        } else {
            reqwest::Client::new().post("http://127.0.0.1:19341/v1/chat/completions/input_tokens").bearer_auth(&key).json(&json!({"messages":[{"role":"system","content":instructions},{"role":"user","content":content}],"max_tokens":max_output,"temperature":0})).send().await?.error_for_status()?.json().await?
        };

        let mut request = std::env::var("OMNI_PROBE_WS")
            .unwrap_or_else(|_| "ws://127.0.0.1:19341/v1/realtime".into())
            .into_client_request()?;
        request.headers_mut().insert(
            if studio {
                "x-studio-token"
            } else {
                "authorization"
            },
            if studio {
                key.parse()?
            } else {
                format!("Bearer {key}").parse()?
            },
        );
        let (mut ws, _) = connect_async(request).await?;
        let mut collector = studio_core::omni_realtime::Output::default();
        let mut sent = false;
        let mut admission = None;
        let mut cancel_sent = false;
        let mut turns = 0;
        let mut terminal_usage = Vec::new();
        let mut collector_error = None;
        let mut wire = std::fs::File::create(output.join(format!("{mode}.jsonl")))?;
        use std::io::Write;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(180);
        loop {
            let item = tokio::time::timeout_at(deadline, ws.next())
                .await?
                .ok_or("Realtime closed early")??;
            if let Message::Text(text) = item {
                let event: Value = serde_json::from_str(&text)?;
                writeln!(wire, "{text}")?;
                if event["type"] == "soliton.realtime.admission" {
                    admission = Some(studio_core::omni_realtime::validate_admission(
                        &event,
                        4096,
                        max_output.into(),
                    )?);
                }
                if (boundary || cancel)
                    && matches!(
                        event["type"].as_str(),
                        Some("response.output_audio.delta" | "response.output_text.delta")
                    )
                    && admission != Some(true)
                {
                    return Err("Output arrived without accepted admission".into());
                }
                if collector_error.is_none() {
                    collector_error = collector
                        .ingest(text.as_bytes())
                        .err()
                        .map(|err| err.to_string());
                }
                if cancel
                    && !cancel_sent
                    && matches!(
                        event["type"].as_str(),
                        Some("response.output_audio.delta" | "soliton.audio.buffered")
                    )
                {
                    collector.cancel();
                    ws.send(Message::Text(
                        json!({"type":"response.cancel"}).to_string().into(),
                    ))
                    .await?;
                    cancel_sent = true;
                }
                if event["type"] == "session.created" && !sent {
                    ws.send(Message::Text(json!({"type":"session.update","session":{"type":"realtime","instructions":instructions,"output_modalities":modalities,"max_output_tokens":max_output}}).to_string().into())).await?;
                    sent = true;
                }
                if event["type"] == "session.updated" {
                    for event in turn_events(0, question) {
                        ws.send(Message::Text(event.to_string().into())).await?;
                    }
                    ws.send(Message::Text(
                        json!({"type":"response.create"}).to_string().into(),
                    ))
                    .await?;
                }
                if boundary && event["type"] == "error" {
                    break;
                }
                if event["type"] == "response.done" {
                    if cancel {
                        assert!(cancel_sent && event["response"]["status"] == "cancelled");
                        assert_eq!(collector.metadata()["audioBytes"], 0);
                        assert!(collector.write_wav(&output.join("cancelled.wav")).is_err());
                    }
                    terminal_usage.push(event["response"]["usage"].clone());
                    turns += 1;
                    if turns == if boundary || cancel { 1 } else { 2 } {
                        break;
                    }
                    for event in turn_events(
                        1,
                        "What code did I ask you to remember? Reply with only the number.",
                    ) {
                        ws.send(Message::Text(event.to_string().into())).await?;
                    }
                    ws.send(Message::Text(
                        json!({"type":"response.create"}).to_string().into(),
                    ))
                    .await?;
                    collector = studio_core::omni_realtime::Output::default();
                }
            }
        }
        let mut metadata = collector.metadata();
        metadata["mode"] = json!(mode);
        metadata["chatProcessorTokens"] = count["input_tokens"].clone();
        metadata["chatRealtimeCountParity"] = json!(terminal_usage.first().unwrap_or(&Value::Null)
            ["input_tokens"]
            .as_u64()
            .zip(count["input_tokens"].as_u64())
            .map(|(a, b)| a == b));
        metadata["terminalUsage"] = json!(terminal_usage);
        metadata["requestedMaxOutputTokens"] = json!(max_output);
        metadata["turns"] = json!(turns);
        metadata["collectorError"] = json!(collector_error);
        if mode != "text"
            && collector_error.is_none()
            && turns > 0
            && !boundary
            && !cancel
            && metadata["audioBytes"].as_u64().unwrap_or(0) > 0
        {
            let path = output.join(format!("{mode}.wav"));
            collector.write_wav(&path)?;
            metadata["audioFile"] = json!(path);
        }
        ws.close(None).await?;
        results.push(metadata);
    }
    std::fs::write(
        output.join("results.json"),
        serde_json::to_vec_pretty(
            &json!({"certification":false,"scope":"native Realtime transport probe; not admission approval","cases":results}),
        )?,
    )?;
    Ok(())
}
