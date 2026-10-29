//! Qualification-only native probe; not an enabled Studio Realtime API.
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
    for (mode, modalities, instructions) in [
        ("text", json!(["text"]), "Respond in text."),
        ("audio", json!(["audio"]), "Respond in audio."),
        (
            "interleaved",
            json!(["text", "audio"]),
            "Respond in interleaved text and audio.",
        ),
    ] {
        let question = "Say hello in English.";
        let count:Value=reqwest::Client::new().post("http://127.0.0.1:19341/v1/chat/completions/input_tokens").bearer_auth(&key).json(&json!({"messages":[{"role":"system","content":instructions},{"role":"user","content":question}],"max_tokens":128,"temperature":0})).send().await?.error_for_status()?.json().await?;
        let mut request = "ws://127.0.0.1:19341/v1/realtime".into_client_request()?;
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {key}").parse()?);
        let (mut ws, _) = connect_async(request).await?;
        let mut collector = studio_core::omni_realtime::Output::default();
        let mut sent = false;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(180);
        loop {
            let item = tokio::time::timeout_at(deadline, ws.next())
                .await?
                .ok_or("Realtime closed early")??;
            if let Message::Text(text) = item {
                let event: Value = serde_json::from_str(&text)?;
                collector.ingest(text.as_bytes())?;
                if event["type"] == "session.created" && !sent {
                    ws.send(Message::Text(json!({"type":"session.update","session":{"type":"realtime","instructions":instructions,"output_modalities":modalities,"max_output_tokens":128}}).to_string().into())).await?;
                    sent = true;
                }
                if event["type"] == "session.updated" {
                    ws.send(Message::Text(json!({"type":"conversation.item.create","item":{"type":"message","role":"user","content":[{"type":"input_text","text":question}]}}).to_string().into())).await?;
                    ws.send(Message::Text(
                        json!({"type":"response.create"}).to_string().into(),
                    ))
                    .await?;
                }
                if event["type"] == "response.done" {
                    break;
                }
            }
        }
        let mut metadata = collector.metadata();
        metadata["mode"] = json!(mode);
        metadata["chatProcessorTokens"] = count["input_tokens"].clone();
        metadata["chatRealtimeCountParity"] =
            json!(metadata["usage"]["input_tokens"] == count["input_tokens"]);
        if mode != "text" {
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
