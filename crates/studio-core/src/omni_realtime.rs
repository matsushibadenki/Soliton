//! Bounded native Realtime output collection. PCM stays in Rust; UI gets metadata.
//! No public Realtime forwarding route is enabled by this module.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::{fs, io::Write, path::Path};
const EVENT_LIMIT: usize = 1024 * 1024;
const AUDIO_LIMIT: usize = 16 * 1024 * 1024;
const TEXT_LIMIT: usize = 64 * 1024;
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
    pub fn metadata(&self) -> Value {
        json!({"done":self.done,"cancelled":self.cancelled,"sampleRate":self.rate,"audioBytes":self.pcm.len(),"text":self.text,"usage":self.usage})
    }
    pub fn write_wav(&self, path: &Path) -> Result<(), String> {
        if !self.done || self.rate != Some(24000) || self.pcm.is_empty() || self.pcm.len() % 2 != 0
        {
            return Err("Speech response is incomplete or invalid PCM16".into());
        }
        let size = self.pcm.len() as u32;
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
        f.write_all(&header)
            .and_then(|_| f.write_all(&self.pcm))
            .and_then(|_| f.sync_all())
            .map_err(|e| e.to_string())
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
        o.ingest(br#"{"type":"response.output_audio.delta","delta":"AAAAAA=="}"#)
            .unwrap();
        o.ingest(br#"{"type":"response.done","response":{"status":"completed","usage":{"input_tokens":1,"output_tokens":1}}}"#).unwrap();
        let p = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        o.write_wav(&p).unwrap();
        let b = fs::read(&p).unwrap();
        assert_eq!(&b[..4], b"RIFF");
        assert_eq!(b.len(), 48);
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
