//! Omni contracts: processed media token counts, never text-only estimates.
//! Media bytes remain in Rust/backend storage; WebView messages carry asset IDs.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const MODEL_ID: &str = "LiquidAI/LFM3-Omni-2B";
pub const GGUF_REVISION: &str = "2918dd47cfceb4f260cd1734cd1aac1b22bbe4b6";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InputMode {
    Text,
    Image,
    Audio,
    Video,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OutputMode {
    Text,
    Audio,
    Interleaved,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OmniRequest {
    pub inputs: Vec<InputMode>,
    pub output: OutputMode,
    /// An opaque Rust-owned asset ID; never base64 frames or filesystem paths.
    pub asset_ids: Vec<String>,
    pub max_text_tokens: u64,
    pub max_audio_tokens: u64,
}
impl OmniRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.inputs.is_empty() || self.inputs.len() > 16 || self.asset_ids.len() > 16 {
            return Err("invalid_omni_inputs");
        }
        if self
            .asset_ids
            .iter()
            .any(|id| uuid::Uuid::parse_str(id).is_err())
        {
            return Err("invalid_omni_asset_id");
        }
        let media_count = self
            .inputs
            .iter()
            .filter(|x| **x != InputMode::Text)
            .count();
        if media_count != self.asset_ids.len() {
            return Err("omni_media_asset_mismatch");
        }
        match self.output {
            OutputMode::Text if self.max_text_tokens == 0 || self.max_audio_tokens != 0 => {
                return Err("invalid_omni_output_budget")
            }
            OutputMode::Audio if self.max_audio_tokens == 0 || self.max_text_tokens != 0 => {
                return Err("invalid_omni_output_budget")
            }
            OutputMode::Interleaved if self.max_audio_tokens == 0 || self.max_text_tokens == 0 => {
                return Err("invalid_omni_output_budget")
            }
            _ => (),
        }
        Ok(())
    }
    /// Called only with the actual backend processor's count, including all
    /// image/video/audio expansion and template tokens. None always refuses.
    pub fn admit(
        &self,
        processed_input_tokens: Option<u64>,
        qualified_context: u64,
    ) -> Result<u64, &'static str> {
        self.validate()?;
        let input = processed_input_tokens.ok_or("omni_processed_token_count_required")?;
        if input == 0 || qualified_context == 0 {
            return Err("invalid_omni_token_budget");
        }
        let total = input
            .checked_add(self.max_text_tokens)
            .and_then(|n| n.checked_add(self.max_audio_tokens))
            .ok_or("omni_token_budget_overflow")?;
        if total > qualified_context {
            return Err("omni_context_exceeded");
        }
        Ok(total)
    }
}

/// Architecture discovery is distinct from local certified support.
pub fn capabilities() -> Value {
    json!({
        "model": MODEL_ID, "ggufRepository":"LiquidAI/LFM3-Omni-2B-GGUF",
        "revision":GGUF_REVISION, "runtime":"llama.cpp", "minimumReviewedBuild":11960,
        "inputModes":["text","image","audio","video"],
        "outputModes":["text","audio","interleaved"],
        "audioInputHz":16000,"audioOutputHz":24000,"reasoning":false,
        "qualified":false,"runnable":false,"implementedInferenceModes":[],
        "requires":["owned_exclusive_engine","processor_exact_tokens","identity_bound_qualification","media_asset_storage","realtime_bridge"],
        "status":{
            "en":"Official runtime smoke tested; Soliton multimodal inference is not qualified.",
            "ja":"公式ランタイムの起動試験済みです。Solitonのマルチモーダル推論は未認証です。",
            "zh-CN":"官方运行时已完成启动测试，Soliton多模态推理尚未认证。"
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> OmniRequest {
        OmniRequest {
            inputs: vec![InputMode::Video],
            output: OutputMode::Interleaved,
            asset_ids: vec![uuid::Uuid::new_v4().to_string()],
            max_text_tokens: 64,
            max_audio_tokens: 128,
        }
    }
    #[test]
    fn refuses_missing_processor_count() {
        assert_eq!(
            request().admit(None, 4096),
            Err("omni_processed_token_count_required")
        );
    }
    #[test]
    fn exact_boundary_and_one_token_over() {
        assert_eq!(request().admit(Some(3904), 4096), Ok(4096));
        assert_eq!(
            request().admit(Some(3905), 4096),
            Err("omni_context_exceeded")
        );
    }
    #[test]
    fn audio_output_reservation_cannot_be_omitted() {
        let mut r = request();
        r.max_audio_tokens = 0;
        assert!(r.admit(Some(1), 4096).is_err());
    }
    #[test]
    fn refuses_token_overflow() {
        let mut r = request();
        r.max_audio_tokens = u64::MAX;
        assert_eq!(r.admit(Some(1), 4096), Err("omni_token_budget_overflow"));
    }
    #[test]
    fn rejects_paths_and_inline_media() {
        let mut r = request();
        r.asset_ids = vec!["../../private.wav".into()];
        assert!(r.validate().is_err());
        r.asset_ids = vec!["data:video/mp4;base64,AAAA".into()];
        assert!(r.validate().is_err());
    }
    #[test]
    fn discovery_is_not_qualification() {
        let v = capabilities();
        assert_eq!(v["qualified"], false);
        assert_eq!(v["runnable"], false);
    }
}
