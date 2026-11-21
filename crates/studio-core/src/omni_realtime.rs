//! Bounded native Realtime output collection. PCM stays in Rust; UI gets metadata.
//! No public Realtime forwarding route is enabled by this module.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::{fs, io::Write, path::Path};
const EVENT_LIMIT: usize = 1024 * 1024;
const AUDIO_LIMIT: usize = 16 * 1024 * 1024;
const TEXT_LIMIT: usize = 64 * 1024;
/// Verify the native admission event before releasing any generated output.
pub fn validate_admission(event: &Value, context: u64, reservation: u64) -> Result<bool, String> {
    let invalid = || "Invalid Realtime admission".to_string();
    let used = event["context_positions"].as_u64().ok_or_else(invalid)?;
    let input = event["input_tokens"].as_u64().ok_or_else(invalid)?;
    let available = context
        .checked_sub(used)
        .and_then(|n| n.checked_sub(1))
        .ok_or_else(invalid)?;
    let accepted = reservation > 0 && reservation <= available;
    if event["type"] != "soliton.realtime.admission"
        || input == 0
        || input > used
        || event["available_positions"].as_u64() != Some(available)
        || event["output_reservation"].as_u64() != Some(reservation)
        || event["accepted"].as_bool() != Some(accepted)
    {
        return Err(invalid());
    }
    Ok(accepted)
}

#[derive(Default)]
pub struct Output {
    rate: Option<u32>,
    text: String,
    pcm: Vec<u8>,
    usage: Option<Value>,
    done: bool,
    cancelled: bool,
}
impl Output {
    pub fn cancel(&mut self) {
        if !self.done {
            self.cancelled = true;
            self.pcm.clear();
            self.usage = None;
        }
    }
    pub fn ingest(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.done || self.cancelled {
            return Err("Realtime response is already terminal".into());
        }
        if bytes.len() > EVENT_LIMIT {
            return Err("Realtime event too large".into());
        }
        let e: Value = serde_json::from_slice(bytes).map_err(|_| "Invalid Realtime event")?;
        match e["type"].as_str().ok_or("Realtime event type missing")? {
            "session.created" | "session.updated" => {
                if let Some(rate) = e["session"]["audio"]["output"]["format"]["rate"].as_u64() {
                    if rate != 24000 {
                        return Err("Unreviewed speech sample rate".into());
                    }
                    self.rate = Some(rate as u32);
                }
            }
            "response.output_text.delta" => {
                let text = e["delta"].as_str().ok_or("Realtime text delta missing")?;
                if self.text.len() + text.len() > TEXT_LIMIT {
                    return Err("Realtime text too large".into());
                }
                self.text.push_str(text);
            }
            "response.output_audio.delta" => {
                if self.rate.is_none() {
                    return Err("Speech format not established".into());
                }
                let encoded = e["delta"].as_str().ok_or("Speech delta missing")?;
                let pcm = STANDARD
                    .decode(encoded)
                    .map_err(|_| "Invalid speech encoding")?;
                if self.pcm.len() + pcm.len() > AUDIO_LIMIT {
                    return Err("Speech output limit exceeded".into());
                }
                self.pcm.extend(pcm);
            }
            "response.done" => {
                let r = &e["response"];
                if r["status"] != "completed" {
                    self.cancel();
                    return Err("Realtime response incomplete".into());
                }
                if r["usage"]["input_tokens"].as_u64().is_none()
                    || r["usage"]["output_tokens"].as_u64().is_none()
                {
                    self.cancel();
                    return Err("Realtime usage is unavailable".into());
                }
                self.usage = r.get("usage").cloned();
                self.done = true;
            }
            "error" => {
                self.cancel();
                return Err("Realtime runtime reported an error".into());
            }
            _ => (),
        }
        Ok(())
    }
    fn finish_turn(
        &mut self,
        event: &Value,
        frame: &[u8],
        admission: Option<u64>,
        reservation: Option<u64>,
    ) -> Result<Self, String> {
        let response = &event["response"];
        let status = response["status"].as_str();
        if !(self.cancelled && status == Some("cancelled")) {
            if self.cancelled && status != Some("completed") {
                return Err("Unexpected terminal status after cancellation".into());
            }
            let usage = &response["usage"];
            if admission.is_none()
                || usage["input_tokens"].as_u64() != admission
                || !usage["output_tokens"]
                    .as_u64()
                    .zip(reservation)
                    .is_some_and(|(used, cap)| used <= cap)
            {
                return Err("Native usage differs from admission".into());
            }
            if !self.cancelled {
                self.ingest(frame)?;
            }
        }
        let next = Self {
            rate: self.rate,
            ..Self::default()
        };
        Ok(std::mem::replace(self, next))
    }

    pub fn metadata(&self) -> Value {
        json!({"done":self.done,"cancelled":self.cancelled,"sampleRate":self.rate,"audioBytes":self.pcm.len(),"text":self.text,"usage":self.usage})
    }
    pub fn write_wav(&self, path: &Path) -> Result<(), String> {
        let wav = self.wav_bytes()?;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        f.write_all(&wav)
            .and_then(|_| f.sync_all())
            .map_err(|e| e.to_string())
    }

    fn wav_bytes(&self) -> Result<Vec<u8>, String> {
        if !self.done || self.rate != Some(24000) || self.pcm.is_empty() || self.pcm.len() % 2 != 0
        {
            return Err("Speech response is incomplete or invalid PCM16".into());
        }
        let size = self.pcm.len() as u32;
        let mut header = Vec::new();
        header.extend(b"RIFF");
        header.extend((36 + size).to_le_bytes());
        header.extend(b"WAVEfmt ");
        header.extend(16u32.to_le_bytes());
        header.extend(1u16.to_le_bytes());
        header.extend(1u16.to_le_bytes());
        header.extend(24000u32.to_le_bytes());
        header.extend(48000u32.to_le_bytes());
        header.extend(2u16.to_le_bytes());
        header.extend(16u16.to_le_bytes());
        header.extend(b"data");
        header.extend(size.to_le_bytes());
        header.extend_from_slice(&self.pcm);
        Ok(header)
    }
}
/// A text-only certificate never authorizes an audio/Realtime bridge.
pub fn verify_audio_scope(proof: &Value, identity: &Value, now: u64) -> Result<(), String> {
    crate::omni_runtime::validate_full_qualification(proof, identity, now, "omni_realtime_30min")?;
    if proof["scope"] != "omni_realtime_30min" || proof["passed"] != true {
        return Err("Realtime requires separate qualification".into());
    }
    for key in [
        "exactInputCountParity",
        "outputReservationVerified",
        "cancellationVerified",
        "historyVerified",
        "sampleRateVerified",
    ] {
        if proof["realtimeChecks"][key] != true {
            return Err("Realtime qualification incomplete".into());
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn speech_needs_negotiated_format() {
        let mut o = Output::default();
        assert!(o
            .ingest(br#"{"type":"response.output_audio.delta","delta":"AAAA"}"#)
            .is_err());
    }
    #[test]
    fn text_certificate_never_enables_audio() {
        assert!(verify_audio_scope(
            &json!({"scope":"omni_media_text_30min","passed":true}),
            &json!({}),
            0
        )
        .is_err());
    }
    #[test]
    fn malformed_and_oversized_events_refused() {
        let mut o = Output::default();
        assert!(o.ingest(b"invalid").is_err());
        assert!(o.ingest(&vec![b' '; EVENT_LIMIT + 1]).is_err());
    }
    #[test]
    fn complete_pcm_wav_is_private_and_bounded() {
        let mut o = Output::default();
        o.ingest(br#"{"type":"session.created","session":{"audio":{"output":{"format":{"rate":24000}}}}}"#).unwrap();
        o.ingest(br#"{"type":"response.output_audio.delta","delta":"AQD+/w=="}"#)
            .unwrap();
        o.ingest(br#"{"type":"response.output_audio.delta","delta":"AID/fw=="}"#)
            .unwrap();
        o.ingest(br#"{"type":"response.done","response":{"status":"completed","usage":{"input_tokens":1,"output_tokens":1}}}"#).unwrap();
        let p = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        o.write_wav(&p).unwrap();
        let b = fs::read(&p).unwrap();
        assert_eq!(&b[..4], b"RIFF");
        assert_eq!(u16::from_le_bytes(b[22..24].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(b[24..28].try_into().unwrap()), 24_000);
        assert_eq!(u16::from_le_bytes(b[34..36].try_into().unwrap()), 16);
        assert_eq!(&b[44..], &[1, 0, 254, 255, 0, 128, 255, 127]);
        fs::remove_file(p).unwrap();
    }
    #[test]
    fn cancelled_audio_is_discarded_and_never_saved() {
        let mut o = Output::default();
        o.ingest(br#"{"type":"session.created","session":{"audio":{"output":{"format":{"rate":24000}}}}}"#).unwrap();
        o.ingest(br#"{"type":"response.output_audio.delta","delta":"AAAAAA=="}"#)
            .unwrap();
        o.cancel();
        assert_eq!(o.metadata()["audioBytes"], 0);
        assert_eq!(o.metadata()["cancelled"], true);
        let p = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        assert!(o.write_wav(&p).is_err());
        assert!(!p.exists());
        assert!(o.ingest(br#"{"type":"response.done","response":{"status":"completed","usage":{"input_tokens":1,"output_tokens":1}}}"#).is_err());
    }
    #[test]
    fn completion_after_cancel_discards_audio_and_preserves_format_for_next_turn() {
        let mut output = Output {
            rate: Some(24000),
            ..Output::default()
        };
        let delta = br#"{"type":"response.output_audio.delta","delta":"AAAAAA=="}"#;
        output.ingest(delta).unwrap();
        output.cancel();

        let invalid = json!({"type":"response.done","response":{
            "status":"completed","usage":{"input_tokens":7,"output_tokens":3}
        }});
        let frame = serde_json::to_vec(&invalid).unwrap();
        assert!(output.finish_turn(&invalid, &frame, Some(6), Some(3)).is_err());
        assert!(output.finish_turn(&invalid, &frame, Some(7), Some(2)).is_err());

        let discarded = output.finish_turn(&invalid, &frame, Some(7), Some(3)).unwrap();
        assert_eq!(discarded.metadata()["audioBytes"], 0);
        assert_eq!(discarded.metadata()["cancelled"], true);
        assert!(discarded.wav_bytes().is_err());

        output.ingest(delta).unwrap();
        let finished = output.finish_turn(&invalid, &frame, Some(7), Some(3)).unwrap();
        assert_eq!(finished.metadata()["cancelled"], false);
        assert_eq!(finished.wav_bytes().unwrap().len(), 48);
    }

    #[test]
    fn unknown_completion_or_usage_cannot_be_success() {
        for r in [
            json!({"usage":{"input_tokens":1,"output_tokens":1}}),
            json!({"status":"completed"}),
            json!({"status":"cancelled"}),
        ] {
            let mut o = Output::default();
            assert!(o
                .ingest(
                    serde_json::to_string(&json!({"type":"response.done","response":r}))
                        .unwrap()
                        .as_bytes()
                )
                .is_err());
            assert_eq!(o.metadata()["done"], false);
        }
    }
}

#[derive(Default)]
pub(crate) struct Playback {
    latest: tokio::sync::Mutex<Option<(uuid::Uuid, std::time::Instant, Vec<u8>)>>,
}

impl Playback {
    async fn publish(&self, wav: Vec<u8>) -> uuid::Uuid {
        let id = uuid::Uuid::new_v4();
        *self.latest.lock().await = Some((id, std::time::Instant::now(), wav));
        id
    }

    pub(crate) async fn take(&self, id: uuid::Uuid) -> Option<Vec<u8>> {
        let mut latest = self.latest.lock().await;
        if latest
            .as_ref()
            .is_some_and(|(stored, at, _)| *stored == id && at.elapsed().as_secs() < 120)
        {
            return latest.take().map(|(_, _, wav)| wav);
        }
        None
    }
}

/// Authenticated loopback transport shared by the qualification router and the
/// production route. The caller must establish the runtime identity first.
pub async fn relay(
    socket: axum::extract::ws::WebSocket,
    port: u16,
    key: String,
    context: u64,
) -> Result<(), String> {
    relay_with_playback(socket, port, key, context, None).await
}

pub(crate) async fn relay_with_playback(
    socket: axum::extract::ws::WebSocket,
    port: u16,
    key: String,
    context: u64,
    playback: Option<&Playback>,
) -> Result<(), String> {
    use axum::extract::ws::Message as ClientMessage;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::{
        connect_async,
        tungstenite::{client::IntoClientRequest, Message},
    };
    let mut request = format!("ws://127.0.0.1:{port}/v1/realtime")
        .into_client_request()
        .map_err(|e| e.to_string())?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {key}")
            .parse()
            .map_err(|_| "Invalid runtime key")?,
    );
    let (mut native, _) = connect_async(request).await.map_err(|e| e.to_string())?;
    let (mut tx, mut rx) = socket.split();
    let mut output = Output::default();
    let mut reservation = None;
    let mut pending = false;
    let mut admission = None;
    let mut cancelled = false;
    let mut audio_bytes = 0;
    loop {
        tokio::select! {
            client = rx.next() => {
                let frame = match client {
                    Some(Ok(ClientMessage::Text(frame))) => frame,
                    Some(Ok(ClientMessage::Ping(_) | ClientMessage::Pong(_))) => continue,
                    _ => break,
                };
                if frame.len() > EVENT_LIMIT { return Err("Oversized client event".into()); }
                let event: Value = serde_json::from_str(&frame).map_err(|_| "Invalid client event")?;
                match event["type"].as_str() {
                    Some("session.update") if reservation.is_none() => {
                        reservation = event["session"]["max_output_tokens"].as_u64()
                            .filter(|&n| n > 0 && n < context);
                        if reservation.is_none() { return Err("Explicit output reservation required".into()); }
                    }
                    Some("input_audio_buffer.append") if !pending => {
                        audio_bytes += frame.len();
                        if audio_bytes > AUDIO_LIMIT { return Err("Audio input limit exceeded".into()); }
                    }
                    Some("input_audio_buffer.commit" | "conversation.item.create") if !pending => {}
                    Some("response.create") if !pending && reservation.is_some() && event.get("response").is_none() => {
                        pending = true;
                        admission = None;
                        cancelled = false;
                        audio_bytes = 0;
                    }
                    // Completion can reach the relay while the client still shows a running turn.
                    Some("response.cancel") if !pending && reservation.is_some() => continue,
                    Some("response.cancel") if pending && admission.is_some() && !cancelled => {
                        cancelled = true;
                        output.cancel();
                    }
                    _ => return Err("Unsupported Realtime event or state".into()),
                }
                native.send(Message::Text(frame.as_str().into())).await.map_err(|e| e.to_string())?;
            }
            item = native.next() => {
                let frame = match item {
                    Some(Ok(Message::Text(frame))) => frame,
                    Some(Ok(Message::Ping(_))) => {
                        native.flush().await.map_err(|e| e.to_string())?;
                        continue;
                    }
                    Some(Ok(Message::Pong(_))) => continue,
                    _ => break,
                };
                if frame.len() > EVENT_LIMIT { return Err("Oversized native event".into()); }
                let event: Value = serde_json::from_str(&frame).map_err(|_| "Invalid native event")?;
                let send = match event["type"].as_str() {
                    Some("soliton.realtime.admission") if pending && admission.is_none() => {
                        if validate_admission(&event, context, reservation.ok_or("Missing reservation")?)? {
                            admission = event["input_tokens"].as_u64();
                        }
                        event
                    }
                    Some("response.output_text.delta" | "response.output_audio.delta") => {
                        if cancelled { continue; }
                        if !pending || admission.is_none() { return Err("Output preceded accepted admission".into()); }
                        output.ingest(frame.as_bytes())?;
                        if event["type"] == "response.output_audio.delta" {
                            json!({"type":"soliton.audio.buffered","audioBytes":output.metadata()["audioBytes"]})
                        } else { event }
                    }
                    Some("response.done") if pending => {
                        pending = false;
                        let finished = output.finish_turn(&event, frame.as_bytes(), admission, reservation)?;
                        let mut event = event;
                        event["soliton_output"] = finished.metadata();
                        if let Some(playback) = playback {
                            if !finished.cancelled && !finished.pcm.is_empty() {
                                let id = playback.publish(finished.wav_bytes()?).await;
                                event["soliton_output"]["audioUrl"] =
                                    json!(format!("/v1/realtime/audio/{id}"));
                            }
                        }
                        event
                    }
                    Some("session.created" | "session.updated") => {
                        output.ingest(frame.as_bytes())?;
                        event
                    }
                    Some("input_audio_buffer.committed" | "error") => event,
                    _ => continue,
                };
                tx.send(ClientMessage::Text(send.to_string().into())).await.map_err(|e| e.to_string())?;
            }
        }
    }
    output.cancel();
    let _ = native.close(None).await;
    Ok(())
}
