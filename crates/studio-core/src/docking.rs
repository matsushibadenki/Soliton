//! Transient native hosts are separate from persisted group identities.
use crate::Document;
use std::collections::BTreeSet;
pub fn workspace_host(id: &str) -> bool {
    ["workspace", "group-chat", "group-logs"].contains(&id)
}
impl Document {
    pub fn host_for(&self, group: &str) -> &str {
        self.workspace_hosts
            .get(group)
            .map(String::as_str)
            .unwrap_or("main")
    }
    fn sync_workspace_detached(&mut self) {
        self.detached.retain(|id| !workspace_host(id));
        self.detached.extend(self.workspace_hosts.keys().cloned());
    }
    pub fn float_group(&mut self, group: &str, host: &str) -> Result<(), String> {
        if !self.layout.is_group(group) || !workspace_host(host) || self.host_for(group) != "main" {
            return Err("Unknown or already floating workspace group".into());
        }
        self.workspace_hosts.insert(group.into(), host.into());
        self.sync_workspace_detached();
        Ok(())
    }
    /// Return one group to its main slot; keep siblings in their existing native host.
    pub fn return_group(&mut self, group: &str) -> Option<String> {
        let host = self.workspace_hosts.remove(group)?;
        self.sync_workspace_detached();
        (!self.workspace_hosts.values().any(|id| id == &host)).then_some(host)
    }
    pub fn release_host(&mut self, host: &str) {
        self.workspace_hosts.retain(|_, id| id != host);
        self.sync_workspace_detached();
    }
    /// Compute first, then commit layout and host membership together under the document lock.
    pub fn dock_workspace(
        &mut self,
        tab: &str,
        target: &str,
        placement: &str,
        destination: &str,
    ) -> Result<Vec<String>, String> {
        if destination != "main"
            && (!workspace_host(destination)
                || !self.workspace_hosts.values().any(|id| id == destination))
        {
            return Err("Stale workspace destination".into());
        }
        let mut next = self.layout.clone();
        next.dock(tab, target, placement)?;
        let retired: BTreeSet<_> = self
            .workspace_hosts
            .values()
            .filter(|host| host.as_str() != destination)
            .cloned()
            .collect();
        let mut hosts = std::collections::BTreeMap::new();
        if destination != "main" {
            for group in next.groups() {
                hosts.insert(group.id, destination.to_string());
            }
        }
        self.layout = next;
        self.workspace_hosts = hosts;
        self.sync_workspace_detached();
        Ok(retired.into_iter().collect())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn split() -> Document {
        let mut d = Document::default();
        d.layout.dock("logs", "chat", "right").unwrap();
        d
    }
    #[test]
    fn merge_floating_to_floating_keeps_destination_host_and_retires_source() {
        let mut d = split();
        d.float_group("group-chat", "group-chat").unwrap();
        d.float_group("group-logs", "group-logs").unwrap();
        assert_eq!(
            d.dock_workspace("chat", "logs", "center", "group-logs")
                .unwrap(),
            ["group-chat"]
        );
        assert_eq!(d.host_for("workspace"), "group-logs");
        assert_eq!(d.detached, ["workspace"]);
        assert_eq!(d.layout.groups()[0].tabs, ["logs", "chat"]);
        assert_eq!(d.return_group("workspace"), Some("group-logs".into()));
        assert!(d.detached.is_empty());
    }
    #[test]
    fn split_within_floating_host_then_return_one_group_keeps_sibling_alive() {
        let mut d = Document::default();
        d.float_group("workspace", "workspace").unwrap();
        assert!(d
            .dock_workspace("logs", "chat", "bottom", "workspace")
            .unwrap()
            .is_empty());
        assert_eq!(d.host_for("group-chat"), "workspace");
        assert_eq!(d.host_for("group-logs"), "workspace");
        assert_eq!(d.return_group("group-chat"), None);
        assert_eq!(d.host_for("group-chat"), "main");
        assert_eq!(d.host_for("group-logs"), "workspace");
        d.release_host("workspace");
        assert!(d.detached.is_empty());
        assert_eq!(d.layout.groups().len(), 2);
    }
    #[test]
    fn floating_to_main_drop_is_atomic_and_invalid_destinations_leave_everything_unchanged() {
        let mut d = split();
        d.float_group("group-chat", "group-chat").unwrap();
        d.detached.push("api".into());
        let before = serde_json::to_value(&d.layout).unwrap();
        let hosts = d.workspace_hosts.clone();
        assert!(d.dock_workspace("logs", "chat", "left", "unknown").is_err());
        assert!(d.dock_workspace("chat", "chat", "center", "main").is_err());
        assert_eq!(serde_json::to_value(&d.layout).unwrap(), before);
        assert_eq!(d.workspace_hosts, hosts);
        assert_eq!(
            d.dock_workspace("chat", "logs", "top", "main").unwrap(),
            ["group-chat"]
        );
        assert!(d.workspace_hosts.is_empty());
        assert_eq!(d.detached, ["api"]);
        assert_eq!(d.layout.workspace_split.as_deref(), Some("vertical"));
        assert_eq!(d.layout.groups().len(), 2);
    }
    #[test]
    fn native_hosts_never_survive_serialization_or_allow_stale_window_reentry() {
        let mut d = Document::default();
        d.float_group("workspace", "workspace").unwrap();
        assert!(d.float_group("workspace", "workspace").is_err());
        assert!(d.float_group("missing", "workspace").is_err());
        let saved = serde_json::to_value(&d).unwrap();
        assert!(saved.get("workspaceHosts").is_none());
        let loaded: Document = serde_json::from_value(saved).unwrap();
        assert!(loaded.workspace_hosts.is_empty());
        d.dock_workspace("logs", "chat", "center", "main").unwrap();
        assert!(d
            .dock_workspace("logs", "chat", "right", "workspace")
            .is_err());
    }
}

/// Opt-in bounded metadata only: never capture dragged text or conversation content.
#[derive(Default, serde::Serialize)]
pub struct DragDiagnostics {
    pub enabled: bool,
    pub events: std::collections::VecDeque<serde_json::Value>,
}
impl DragDiagnostics {
    pub fn record(&mut self, window: &str, value: serde_json::Value) {
        if !self.enabled {
            return;
        }
        let mut clean = serde_json::Map::new();
        clean.insert("window".into(), window.into());
        for key in [
            "phase", "trusted", "x", "y", "sx", "sy", "buttons", "button", "types", "tab",
            "target", "group", "element", "blocked", "hasDrag", "bubbles", "wx", "wy", "width",
            "height", "time",
        ] {
            if let Some(value) = value.get(key) {
                if value.to_string().len() <= 256 {
                    clean.insert(key.into(), value.clone());
                }
            }
        }
        self.events.push_back(clean.into());
        while self.events.len() > 128 {
            self.events.pop_front();
        }
    }
}

#[cfg(test)]
mod drag_diagnostics_tests {
    use super::*;
    #[test]
    fn trace_is_opt_in_bounded_and_does_not_accept_dragged_content() {
        let mut trace = DragDiagnostics::default();
        trace.record("main", serde_json::json!({"phase":"drop"}));
        assert!(trace.events.is_empty());
        trace.enabled = true;
        for i in 0..150 {
            trace.record("main", serde_json::json!({"phase":"pointermove","x":i,"content":"private message","payload":"private drag text"}));
        }
        assert_eq!(trace.events.len(), 128);
        assert_eq!(trace.events.front().unwrap()["x"], 22);
        assert!(!serde_json::to_string(&trace).unwrap().contains("private"));
    }
}
