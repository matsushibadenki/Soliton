use serde::{Deserialize, Serialize};

/// Persisted dock geometry. WebViews render this state; pointer drags are temporary previews.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Layout {
    pub engine_width: u32,
    pub api_width: u32,
    pub logs_height: u32,
    pub workspace_order: Vec<String>,
    pub workspace_active: String,
    pub workspace_split: Option<String>,
    pub workspace_ratio: u8,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            engine_width: 270,
            api_width: 300,
            logs_height: 174,
            workspace_order: vec!["chat".into(), "logs".into()],
            workspace_active: "chat".into(),
            workspace_split: None,
            workspace_ratio: 50,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceGroup {
    pub id: String,
    pub tabs: Vec<String>,
    pub active: String,
}
impl Layout {
    pub fn groups(&self) -> Vec<WorkspaceGroup> {
        if self.workspace_split.is_some() {
            self.workspace_order
                .iter()
                .map(|tab| WorkspaceGroup {
                    id: format!("group-{tab}"),
                    tabs: vec![tab.clone()],
                    active: tab.clone(),
                })
                .collect()
        } else {
            vec![WorkspaceGroup {
                id: "workspace".into(),
                tabs: self.workspace_order.clone(),
                active: self.workspace_active.clone(),
            }]
        }
    }
    pub fn is_group(&self, id: &str) -> bool {
        self.groups().iter().any(|g| g.id == id)
    }
    /// Dock operations work on tab identity, never trust a WebView's stale group array.
    pub fn dock(&mut self, tab: &str, target: &str, placement: &str) -> Result<(), String> {
        if !["chat", "logs"].contains(&tab) || !["chat", "logs"].contains(&target) || tab == target
        {
            return Err("Invalid docking tabs".into());
        }
        let (split, order) = match placement {
            "center" => (None, vec![target.into(), tab.into()]),
            "left" => (Some("horizontal".into()), vec![tab.into(), target.into()]),
            "right" => (Some("horizontal".into()), vec![target.into(), tab.into()]),
            "top" => (Some("vertical".into()), vec![tab.into(), target.into()]),
            "bottom" => (Some("vertical".into()), vec![target.into(), tab.into()]),
            _ => return Err("Invalid docking position".into()),
        };
        self.workspace_split = split;
        self.workspace_order = order;
        self.workspace_active = tab.into();
        self.workspace_ratio = 50;
        Ok(())
    }
    pub fn resize_workspace(&mut self, ratio: u64) -> Result<(), String> {
        if !(25..=75).contains(&ratio) {
            return Err("Workspace ratio must be between 25 and 75".into());
        }
        self.workspace_ratio = ratio as u8;
        Ok(())
    }

    pub fn patch(&self, value: &serde_json::Value) -> Result<Self, String> {
        let fields = value.as_object().ok_or("Expected dock dimensions")?;
        if fields.is_empty()
            || fields
                .keys()
                .any(|k| !["engineWidth", "apiWidth", "logsHeight"].contains(&k.as_str()))
        {
            return Err("Unknown dock dimension".into());
        }
        let mut merged = serde_json::to_value(self).map_err(|e| e.to_string())?;
        for (key, value) in fields {
            merged[key] = value.clone();
        }
        let layout: Self = serde_json::from_value(merged).map_err(|e| e.to_string())?;
        layout.validate()?;
        Ok(layout)
    }
    /// Atomic moves preserve changes from other WebViews without a read/replace race.
    pub fn workspace(&mut self, tab: &str, before: Option<&str>) -> Result<(), String> {
        if !["chat", "logs"].contains(&tab) {
            return Err("Unknown workspace tab".into());
        }
        if let Some(before) = before {
            if !["chat", "logs"].contains(&before) || before == tab {
                return Err("Invalid workspace destination".into());
            }
            self.workspace_order.retain(|id| id != tab);
            let index = self
                .workspace_order
                .iter()
                .position(|id| id == before)
                .unwrap_or(0);
            self.workspace_order.insert(index, tab.into());
        } else {
            self.workspace_active = tab.into();
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        if ![None, Some("horizontal"), Some("vertical")].contains(&self.workspace_split.as_deref())
            || !(25..=75).contains(&self.workspace_ratio)
        {
            return Err("Invalid workspace split".into());
        }
        if self.workspace_order.len() != 2
            || !self.workspace_order.contains(&"chat".to_string())
            || !self.workspace_order.contains(&"logs".to_string())
            || !["chat", "logs"].contains(&self.workspace_active.as_str())
        {
            return Err("Invalid workspace tabs".into());
        }
        if !(220..=480).contains(&self.engine_width)
            || !(240..=480).contains(&self.api_width)
            || !(100..=420).contains(&self.logs_height)
        {
            return Err("Dock dimensions are outside supported bounds".into());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_dock_merge_and_resize_never_duplicate_or_lose_tabs() {
        let mut l = Layout::default();
        for (placement, axis, order) in [
            ("left", "horizontal", ["logs", "chat"]),
            ("right", "horizontal", ["chat", "logs"]),
            ("top", "vertical", ["logs", "chat"]),
            ("bottom", "vertical", ["chat", "logs"]),
        ] {
            l.dock("logs", "chat", placement).unwrap();
            assert_eq!(l.workspace_order, order);
            assert_eq!(l.workspace_split.as_deref(), Some(axis));
            assert_eq!(l.groups().len(), 2);
            assert!(l.is_group("group-chat") && l.is_group("group-logs"));
            assert!(!l.is_group("workspace"));
            l.resize_workspace(65).unwrap();
            assert_eq!(l.workspace_ratio, 65);
            l.validate().unwrap();
        }
        let before = l.clone();
        assert!(l.dock("logs", "logs", "left").is_err());
        assert!(l.dock("bad", "chat", "left").is_err());
        assert!(l.dock("logs", "chat", "outside").is_err());
        assert!(l.resize_workspace(24).is_err());
        assert!(l.resize_workspace(76).is_err());
        assert_eq!(before, l);
        l.dock("logs", "chat", "center").unwrap();
        assert!(l.workspace_split.is_none());
        assert_eq!(l.groups().len(), 1);
        assert_eq!(l.groups()[0].tabs, ["chat", "logs"]);
        assert_eq!(l.groups()[0].active, "logs");
        assert_eq!(l.workspace_ratio, 50);
    }
    #[test]
    fn split_layout_roundtrips_and_invalid_geometry_is_rejected() {
        let mut l = Layout::default();
        l.dock("logs", "chat", "bottom").unwrap();
        l.resize_workspace(40).unwrap();
        let decoded: Layout = serde_json::from_value(serde_json::to_value(&l).unwrap()).unwrap();
        assert_eq!(l, decoded);
        l.workspace_split = Some("diagonal".into());
        assert!(l.validate().is_err());
        l.workspace_split = None;
        l.workspace_ratio = 0;
        assert!(l.validate().is_err());
    }
    #[test]
    fn workspace_moves_and_selection_are_validated_independently() {
        let mut layout = Layout::default();
        layout.workspace("logs", Some("chat")).unwrap();
        layout.workspace("logs", None).unwrap();
        assert_eq!(layout.workspace_order, ["logs", "chat"]);
        assert_eq!(layout.workspace_active, "logs");
        let before = layout.clone();
        assert!(layout.workspace("unknown", None).is_err());
        assert!(layout.workspace("logs", Some("logs")).is_err());
        assert_eq!(before, layout);
        let roundtrip: Layout =
            serde_json::from_value(serde_json::to_value(&layout).unwrap()).unwrap();
        assert_eq!(roundtrip, layout);
        let legacy: Layout = serde_json::from_str(r#"{"engineWidth":340}"#).unwrap();
        assert_eq!(legacy.workspace_order, ["chat", "logs"]);
        assert_eq!(legacy.engine_width, 340);
        layout.workspace_order = vec!["logs".into(), "logs".into()];
        assert!(layout.validate().is_err());
    }
    #[test]
    fn rejects_layout_that_can_hide_the_workspace() {
        assert!(Layout {
            engine_width: 0,
            ..Layout::default()
        }
        .validate()
        .is_err());
        assert!(Layout {
            logs_height: u32::MAX,
            ..Layout::default()
        }
        .validate()
        .is_err());
        assert!(Layout::default().validate().is_ok());
    }
    #[test]
    fn partial_updates_preserve_other_windows_dimensions() {
        let layout = Layout {
            engine_width: 340,
            ..Layout::default()
        };
        let updated = layout.patch(&serde_json::json!({"apiWidth":310})).unwrap();
        assert_eq!(updated.engine_width, 340);
        assert_eq!(updated.api_width, 310);
        assert!(updated.patch(&serde_json::json!({"typo":100})).is_err());
    }
}
