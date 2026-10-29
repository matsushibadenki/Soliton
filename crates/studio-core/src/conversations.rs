//! Rust-owned conversation archive. Only summaries are sent in regular snapshots.
use super::{Document, Message};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
const LIMIT: usize = 50;
const BYTE_LIMIT: usize = 64 * 1024 * 1024;
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    id: String,
    updated_at: u64,
    messages: Vec<Message>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub active_id: String,
    pub updated_at: u64,
    archived: Vec<Conversation>,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            active_id: uuid::Uuid::new_v4().to_string(),
            updated_at: 0,
            archived: Vec::new(),
        }
    }
}
fn title(messages: &[Message]) -> String {
    messages
        .iter()
        .find(|m| m.role == "user")
        .map(|m| {
            m.content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(80)
                .collect()
        })
        .unwrap_or_default()
}
fn size(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| {
            m.attachments.iter().map(|a| a.id.len() + 32).sum::<usize>()
                + m.content.len()
                + m.inference
                    .as_ref()
                    .map(|i| {
                        i.reasoning.len()
                            + i.tools
                                .iter()
                                .map(|t| t.arguments.len() + t.name.len() + t.id.len())
                                .sum::<usize>()
                    })
                    .unwrap_or(0)
        })
        .sum()
}
impl Library {
    pub fn restore_runtime(&mut self) {
        for c in &mut self.archived {
            crate::inference::mark_interrupted(&mut c.messages);
        }
    }
    pub fn touch(&mut self) {
        self.updated_at = now();
    }
    pub fn summaries(&self, messages: &[Message]) -> Value {
        let mut rows: Vec<Value> = self.archived.iter().map(|c|json!({"id":c.id,"title":title(&c.messages),"updatedAt":c.updated_at,"messageCount":c.messages.len()})).collect();
        rows.push(json!({"id":self.active_id,"title":title(messages),"updatedAt":self.updated_at,"messageCount":messages.len()}));
        rows.sort_by_key(|r| std::cmp::Reverse(r["updatedAt"].as_u64().unwrap_or(0)));
        json!({"activeId":self.active_id,"items":rows})
    }
}
pub fn change(d: &mut Document, action: &str, id: &str) -> Result<(), String> {
    if d.generating {
        return Err("Conversation is generating".into());
    }
    if action == "deleteConversation" {
        if id == d.conversations.active_id {
            d.messages.clear();
            d.conversations.active_id = uuid::Uuid::new_v4().to_string();
            d.conversations.touch();
            return Ok(());
        }
        let at = d
            .conversations
            .archived
            .iter()
            .position(|c| c.id == id)
            .ok_or("Conversation not found")?;
        d.conversations.archived.remove(at);
        return Ok(());
    }
    if action == "switchConversation" && id == d.conversations.active_id {
        return Ok(());
    }
    let target = if action == "switchConversation" {
        Some(
            d.conversations
                .archived
                .iter()
                .position(|c| c.id == id)
                .ok_or("Conversation not found")?,
        )
    } else {
        None
    };
    if action == "newConversation" && d.messages.is_empty() {
        return Ok(());
    }
    if !d.messages.is_empty() {
        if target.is_none() && d.conversations.archived.len() + 1 >= LIMIT {
            return Err(
                "Conversation limit reached (50). Export and delete old conversations first."
                    .into(),
            );
        }
        if size(&d.messages).saturating_add(
            d.conversations
                .archived
                .iter()
                .map(|c| size(&c.messages))
                .sum(),
        ) > BYTE_LIMIT
        {
            return Err(
                "Conversation library exceeds 64 MiB. Export and delete old conversations first."
                    .into(),
            );
        }
    }
    let next = target.map(|i| d.conversations.archived.remove(i));
    if !d.messages.is_empty() {
        d.conversations.archived.push(Conversation {
            id: d.conversations.active_id.clone(),
            updated_at: d.conversations.updated_at,
            messages: std::mem::take(&mut d.messages),
        });
    }
    if let Some(next) = next {
        d.conversations.active_id = next.id;
        d.conversations.updated_at = next.updated_at;
        d.messages = next.messages;
    } else {
        d.conversations.active_id = uuid::Uuid::new_v4().to_string();
        d.conversations.touch();
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn message(text: &str) -> Message {
        Message {
            attachments: Vec::new(),
            role: "user".into(),
            content: text.into(),
            ..Default::default()
        }
    }
    #[test]
    fn switching_and_new_preserve_history_and_summaries_hide_message_bodies() {
        let mut d = Document::default();
        let first = d.conversations.active_id.clone();
        d.messages.push(message("First chat"));
        change(&mut d, "newConversation", "").unwrap();
        let second = d.conversations.active_id.clone();
        d.messages.push(message("Second chat"));
        change(&mut d, "switchConversation", &first).unwrap();
        assert_eq!(d.messages[0].content, "First chat");
        let summary = d.conversations.summaries(&d.messages);
        assert_eq!(summary["items"].as_array().unwrap().len(), 2);
        assert!(summary["items"][0].get("messages").is_none());
        change(&mut d, "switchConversation", &second).unwrap();
        assert_eq!(d.messages[0].content, "Second chat");
        change(&mut d, "deleteConversation", &first).unwrap();
        assert!(change(&mut d, "switchConversation", &first).is_err());
        change(&mut d, "deleteConversation", &second).unwrap();
        assert!(d.messages.is_empty());
    }
    #[test]
    fn generation_unknown_ids_and_limits_do_not_discard_current_messages() {
        let mut d = Document::default();
        d.messages.push(message("Keep"));
        d.generating = true;
        assert!(change(&mut d, "newConversation", "").is_err());
        d.generating = false;
        assert!(change(&mut d, "switchConversation", "missing").is_err());
        for _ in 0..49 {
            change(&mut d, "newConversation", "").unwrap();
            d.messages.push(message("Keep"));
        }
        assert!(change(&mut d, "newConversation", "").is_err());
        assert_eq!(d.messages[0].content, "Keep");
        let id = d.conversations.archived[0].id.clone();
        change(&mut d, "switchConversation", &id).unwrap();
    }
    #[test]
    fn old_document_migrates_and_library_roundtrips() {
        let mut d: Document = serde_json::from_value(
            json!({"messages":[{"role":"user","content":"以前の会話"}],"detached":[]}),
        )
        .unwrap();
        let old = d.conversations.active_id.clone();
        change(&mut d, "newConversation", "").unwrap();
        let mut restored: Document =
            serde_json::from_slice(&serde_json::to_vec(&d).unwrap()).unwrap();
        change(&mut restored, "switchConversation", &old).unwrap();
        assert_eq!(restored.messages[0].content, "以前の会話");
        change(&mut restored, "newConversation", "").unwrap();
        let id = restored.conversations.active_id.clone();
        change(&mut restored, "newConversation", "").unwrap();
        assert_eq!(id, restored.conversations.active_id);
    }
}
