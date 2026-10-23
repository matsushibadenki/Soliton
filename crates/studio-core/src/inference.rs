//! Incremental upstream parsing. Timings/counts are reported values, never estimates.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
const TEXT_LIMIT: usize = 1024 * 1024;
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub prompt: Option<u64>,
    pub completion: Option<u64>,
    pub total: Option<u64>,
    pub cached: Option<u64>,
    pub reasoning: Option<u64>,
}
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timings {
    pub prompt_ms: Option<f64>,
    pub decode_ms: Option<f64>,
    pub tokens_per_second: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub id: String,
    pub source: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    pub started_at: u64,
    pub status: String,
    pub elapsed_ms: Option<u64>,
    pub first_output_ms: Option<u64>,
    pub first_answer_ms: Option<u64>,
    pub finish_reason: Option<String>,
    pub usage: Option<Usage>,
    pub timings: Option<Timings>,
}
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub index: u64,
    pub id: String,
    pub name: String,
    pub arguments: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inference {
    pub reasoning: String,
    pub tools: Vec<ToolCall>,
    pub metrics: Metrics,
    pub truncated: bool,
}
pub struct Tracker {
    pub info: Inference,
    pub answer: String,
    start: Instant,
    buffer: Vec<u8>,
    finished: bool,
}
fn number(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite() && *n >= 0.)
}
fn append(dest: &mut String, text: &str, limit: usize) -> bool {
    let mut remaining = limit.saturating_sub(dest.len()).min(text.len());
    while !text.is_char_boundary(remaining) {
        remaining -= 1;
    }
    dest.push_str(&text[..remaining]);
    remaining < text.len()
}
impl Tracker {
    pub fn new(source: &str, model: &str) -> Self {
        Self {
            info: Inference {
                reasoning: String::new(),
                tools: Vec::new(),
                truncated: false,
                metrics: Metrics {
                    id: uuid::Uuid::new_v4().to_string(),
                    source: source.into(),
                    engine: None,
                    model: model.chars().take(128).collect(),
                    started_at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    status: "running".into(),
                    elapsed_ms: None,
                    first_output_ms: None,
                    first_answer_ms: None,
                    finish_reason: None,
                    usage: None,
                    timings: None,
                },
            },
            answer: String::new(),
            start: Instant::now(),
            buffer: Vec::new(),
            finished: false,
        }
    }
    pub fn value(&mut self, v: &Value) {
        // Normalize Anthropic events to the same display model without executing calls.
        let kind = v["type"].as_str().unwrap_or("");
        if kind == "message_start" {
            self.value(&v["message"]);
            return;
        }
        let u = &v["usage"];
        if u.get("input_tokens").is_some() || u.get("output_tokens").is_some() {
            let usage = self.info.metrics.usage.get_or_insert_with(Usage::default);
            if let Some(n) = u["input_tokens"].as_u64() {
                usage.prompt = Some(n);
            }
            if let Some(n) = u["output_tokens"].as_u64() {
                usage.completion = Some(n);
            }
            if let Some(n) = u["cache_read_input_tokens"].as_u64() {
                usage.cached = Some(n);
            }
        }
        if kind == "message_delta" || kind == "message" {
            let reason = if kind == "message" {
                &v["stop_reason"]
            } else {
                &v["delta"]["stop_reason"]
            };
            if let Some(reason) = reason.as_str() {
                self.finished = true;
                self.info.metrics.finish_reason = Some(reason.chars().take(80).collect());
            }
        }
        if kind == "message_stop" {
            self.finished = true;
        }
        if kind == "error" {
            self.info.metrics.status = "error".into();
        }
        if kind == "message" {
            if let Some(blocks) = v["content"].as_array() {
                for (index, block) in blocks.iter().enumerate() {
                    self.anthropic_block(block, index as u64, false);
                }
            }
        } else if kind == "content_block_start" {
            self.anthropic_block(&v["content_block"], v["index"].as_u64().unwrap_or(0), true);
        } else if kind == "content_block_delta" {
            let d = &v["delta"];
            let delta = match d["type"].as_str().unwrap_or("") {
                "thinking_delta" => serde_json::json!({"reasoning_content":d["thinking"]}),
                "text_delta" => serde_json::json!({"content":d["text"]}),
                "input_json_delta" => {
                    serde_json::json!({"tool_calls":[{"index":v["index"],"function":{"arguments":d["partial_json"]}}]})
                }
                _ => Value::Null,
            };
            if !delta.is_null() {
                self.value(&serde_json::json!({"choices":[{"delta":delta}]}));
            }
        }
        let ms = self.start.elapsed().as_millis() as u64;
        self.info.metrics.elapsed_ms = Some(ms);
        if let Some(model) = v["model"].as_str() {
            self.info.metrics.model = model.chars().take(128).collect();
        }
        if v.get("error").is_some_and(|e| !e.is_null()) {
            self.info.metrics.status = "error".into();
        }
        if v["usage"].is_object()
            && (u.get("prompt_tokens").is_some()
                || u.get("completion_tokens").is_some()
                || u.get("total_tokens").is_some())
        {
            let u = &v["usage"];
            self.info.metrics.usage = Some(Usage {
                prompt: u["prompt_tokens"].as_u64(),
                completion: u["completion_tokens"].as_u64(),
                total: u["total_tokens"].as_u64(),
                cached: u["prompt_tokens_details"]["cached_tokens"].as_u64(),
                reasoning: u["completion_tokens_details"]["reasoning_tokens"].as_u64(),
            });
        }
        if v["timings"].is_object() {
            let t = &v["timings"];
            self.info.metrics.timings = Some(Timings {
                prompt_ms: number(&t["prompt_ms"]),
                decode_ms: number(&t["predicted_ms"]),
                tokens_per_second: number(&t["predicted_per_second"]),
            });
        }
        let Some(choice) = v["choices"].as_array().and_then(|a| a.first()) else {
            return;
        };
        if let Some(reason) = choice["finish_reason"].as_str() {
            self.finished = true;
            self.info.metrics.finish_reason = Some(reason.chars().take(80).collect());
        }
        let d = if choice["delta"].is_object() {
            &choice["delta"]
        } else {
            &choice["message"]
        };
        let content = d["content"].as_str().unwrap_or("");
        let reasoning = d["reasoning_content"].as_str().unwrap_or("");
        let calls = d["tool_calls"].as_array();
        if !content.is_empty() || !reasoning.is_empty() || calls.is_some_and(|c| !c.is_empty()) {
            self.info.metrics.first_output_ms.get_or_insert(ms);
        }
        if !content.is_empty() {
            self.info.metrics.first_answer_ms.get_or_insert(ms);
        }
        self.info.truncated |= append(&mut self.answer, content, TEXT_LIMIT);
        self.info.truncated |= append(&mut self.info.reasoning, reasoning, TEXT_LIMIT);
        if let Some(calls) = calls {
            for (position, call) in calls.iter().enumerate() {
                let index = call["index"].as_u64().unwrap_or(position as u64);
                if index >= 64 {
                    self.info.truncated = true;
                    continue;
                }
                if !self.info.tools.iter().any(|c| c.index == index) {
                    self.info.tools.push(ToolCall {
                        index,
                        ..Default::default()
                    });
                }
                let tool = self
                    .info
                    .tools
                    .iter_mut()
                    .find(|c| c.index == index)
                    .unwrap();
                if let Some(id) = call["id"].as_str() {
                    self.info.truncated |= append(&mut tool.id, id, 256);
                }
                if let Some(name) = call["function"]["name"].as_str() {
                    self.info.truncated |= append(&mut tool.name, name, 256);
                }
                if let Some(args) = call["function"]["arguments"].as_str() {
                    self.info.truncated |= append(&mut tool.arguments, args, 65536);
                }
            }
        }
    }
    fn anthropic_block(&mut self, b: &Value, index: u64, streaming: bool) {
        let delta = match b["type"].as_str().unwrap_or("") {
            "thinking" => serde_json::json!({"reasoning_content":b["thinking"]}),
            "text" => serde_json::json!({"content":b["text"]}),
            "tool_use" => {
                serde_json::json!({"tool_calls":[{"index":index,"id":b["id"],"function":{"name":b["name"],"arguments":if streaming && b["input"].as_object().is_some_and(|o|o.is_empty()) {String::new()}else{b["input"].to_string()}}}]})
            }
            _ => Value::Null,
        };
        if !delta.is_null() {
            self.value(&serde_json::json!({"choices":[{"delta":delta}]}));
        }
    }
    pub fn feed_json(&mut self, bytes: &[u8]) {
        self.info.metrics.elapsed_ms = Some(self.start.elapsed().as_millis() as u64);
        if self.buffer.len().saturating_add(bytes.len()) > TEXT_LIMIT {
            self.buffer.clear();
            self.info.truncated = true;
            self.info.metrics.status = "error".into();
        } else if !self.info.truncated {
            self.buffer.extend_from_slice(bytes);
        }
    }
    pub fn feed(&mut self, bytes: &[u8]) {
        self.info.metrics.elapsed_ms = Some(self.start.elapsed().as_millis() as u64);
        self.buffer.extend_from_slice(bytes);
        loop {
            let lf = self
                .buffer
                .windows(2)
                .position(|w| w == b"\n\n")
                .map(|i| (i, 2));
            let cr = self
                .buffer
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|i| (i, 4));
            let Some((end, n)) = lf.into_iter().chain(cr).min_by_key(|p| p.0) else {
                break;
            };
            let event: Vec<_> = self.buffer.drain(..end + n).collect();
            let text = String::from_utf8_lossy(&event);
            let data = text
                .lines()
                .filter_map(|l| l.strip_prefix("data:").map(str::trim_start))
                .collect::<Vec<_>>()
                .join("\n");
            if data == "[DONE]" {
                self.finished = true;
            } else if !data.is_empty() {
                match serde_json::from_str::<Value>(&data) {
                    Ok(v) => self.value(&v),
                    Err(_) => self.info.metrics.status = "error".into(),
                }
            }
        }
        if self.buffer.len() > TEXT_LIMIT {
            self.buffer.clear();
            self.info.truncated = true;
            self.info.metrics.status = "error".into();
        }
    }
    pub fn finish(&mut self, fallback: &str, json: bool) {
        if !json && fallback != "cancelled" && !self.buffer.is_empty() {
            self.feed(b"\n\n");
        }
        if json {
            if let Ok(v) = serde_json::from_slice::<Value>(&self.buffer) {
                self.value(&v);
                self.finished = self.finished
                    || v["choices"].is_array()
                    || (v["type"] == "message" && v["content"].is_array());
            }
        }
        self.info.metrics.elapsed_ms = Some(self.start.elapsed().as_millis() as u64);
        if self.info.metrics.status != "error" {
            self.info.metrics.status = if self.finished { "completed" } else { fallback }.into();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_utf8_crlf_reasoning_tools_and_usage_are_not_mixed() {
        let mut t = Tracker::new("chat", "test");
        let input=concat!("data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"考える\"}}]}\r\n\r\n", "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"lookup\",\"arguments\":\"{\"}}]}}]}\n\n", "data: {\"choices\":[{\"delta\":{\"content\":\"答え\",\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"}\"}}]},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2},\"timings\":{\"predicted_per_second\":3.7}}\n\n");
        for b in input.as_bytes() {
            t.feed(&[*b]);
        }
        t.finish("error", false);
        assert_eq!(t.answer, "答え");
        assert_eq!(t.info.reasoning, "考える");
        assert_eq!(t.info.tools[0].arguments, "{}");
        assert_eq!(t.info.metrics.usage.unwrap().completion, Some(2));
        assert_eq!(t.info.metrics.timings.unwrap().tokens_per_second, Some(3.7));
        assert_eq!(t.info.metrics.status, "completed");
    }
    #[test]
    fn cancelled_missing_and_error_results_never_invent_counts_or_speed() {
        let mut t = Tracker::new("chat", "test");
        t.feed(b": keep-alive\n\n");
        t.finish("cancelled", false);
        assert_eq!(t.info.metrics.status, "cancelled");
        assert!(t.info.metrics.usage.is_none());
        assert!(t.info.metrics.first_output_ms.is_none());
        assert!(t.info.metrics.timings.is_none());
        let mut t = Tracker::new("api", "test");
        t.feed(b"data: {\"error\":{\"message\":\"failed\"}}\n\n");
        t.finish("cancelled", false);
        assert_eq!(t.info.metrics.status, "error");
    }
    #[test]
    fn anthropic_stream_merges_usage_and_preserves_reasoning() {
        let mut t = Tracker::new("api", "test");
        for v in [
            serde_json::json!({"type":"message_start","message":{"type":"message","usage":{"input_tokens":10,"cache_read_input_tokens":3}}}),
            serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"考える"}}),
            serde_json::json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"4"}}),
            serde_json::json!({"type":"message_delta","usage":{"output_tokens":2},"delta":{"stop_reason":"end_turn"}}),
        ] {
            t.value(&v);
        }
        t.finish("error", false);
        assert_eq!(t.answer, "4");
        assert_eq!(t.info.reasoning, "考える");
        assert_eq!(t.info.metrics.status, "completed");
        let u = t.info.metrics.usage.unwrap();
        assert_eq!(u.prompt, Some(10));
        assert_eq!(u.completion, Some(2));
        assert_eq!(u.cached, Some(3));
        assert!(u.total.is_none());
    }
    #[test]
    fn nonstream_json_and_unicode_limits_work() {
        let mut t = Tracker::new("api", "test");
        t.feed_json(
            br#"{

"choices":[{"message":{"content":"answer"},"finish_reason":"stop"}],"usage":{"total_tokens":8}}"#,
        );
        t.finish("cancelled", true);
        assert_eq!(t.answer, "answer");
        assert_eq!(t.info.metrics.status, "completed");
        assert_eq!(t.info.metrics.usage.unwrap().total, Some(8));
        let mut out = String::new();
        assert!(append(&mut out, "日本語", 4));
        assert_eq!(out, "日");
    }
}

pub fn mark_interrupted(messages: &mut [crate::Message]) {
    for message in messages {
        if let Some(info) = message.inference.as_mut() {
            if info.metrics.status == "running" {
                info.metrics.status = "cancelled".into();
                info.metrics.elapsed_ms = None; // Final duration cannot be recovered after a crash.
            }
        }
    }
}
