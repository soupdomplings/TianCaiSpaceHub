use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ClientId {
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "workbuddy", alias = "work-buddy")]
    WorkBuddy,
    #[serde(rename = "tiangong")]
    Tiangong,
    #[serde(rename = "vscode", alias = "vs-code")]
    VsCode,
    #[serde(rename = "deepseek-harness", alias = "deep-seek-harness")]
    DeepSeekHarness,
    #[serde(rename = "zcode", alias = "z-code")]
    ZCode,
    #[serde(rename = "webstorm", alias = "web-storm")]
    WebStorm,
}

impl ClientId {
    pub const ALL: [Self; 7] = [
        Self::Codex,
        Self::WorkBuddy,
        Self::Tiangong,
        Self::VsCode,
        Self::DeepSeekHarness,
        Self::ZCode,
        Self::WebStorm,
    ];
    /// Desktop navigation and user-selectable destinations. Hidden clients
    /// remain in ALL so existing configurations and API integrations work.
    pub const VISIBLE: [Self; 5] = [
        Self::Codex,
        Self::WorkBuddy,
        Self::Tiangong,
        Self::DeepSeekHarness,
        Self::ZCode,
    ];
    pub fn is_visible(self) -> bool {
        Self::VISIBLE.contains(&self)
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Codex => "ChatGPT / Codex",
            Self::WorkBuddy => "WorkBuddy",
            Self::Tiangong => "天工 Work",
            Self::VsCode => "VS Code",
            Self::DeepSeekHarness => "DeepSeek Harness",
            Self::ZCode => "ZCode",
            Self::WebStorm => "WebStorm",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::WorkBuddy => "workbuddy",
            Self::Tiangong => "tiangong",
            Self::VsCode => "vscode",
            Self::DeepSeekHarness => "deepseek-harness",
            Self::ZCode => "zcode",
            Self::WebStorm => "webstorm",
        }
    }
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Codex => "C",
            Self::WorkBuddy => "W",
            Self::Tiangong => "天",
            Self::VsCode => "V",
            Self::DeepSeekHarness => "D",
            Self::ZCode => "Z",
            Self::WebStorm => "WS",
        }
    }
}

/// Fields actually consumed by the current native writers. Existing unknown
/// metadata stays in Binding; hiding a field must not delete it on save.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParameterSupport {
    pub limits: bool,
    pub image: bool,
    pub efforts: bool,
    pub protocol: bool,
}

/// UI presets, not a promise that a third-party endpoint supports each level.
pub fn reasoning_presets(model: &str, protocol: &str) -> &'static [&'static str] {
    let name = model.to_ascii_lowercase();
    if name.contains("glm") {
        &["none", "high", "max"]
    } else if name.contains("deepseek") {
        &["none", "low", "high", "max"]
    } else if protocol == "anthropic-messages" || name.contains("claude") {
        &["low", "medium", "high", "xhigh", "max"]
    } else {
        &["none", "minimal", "low", "medium", "high", "xhigh", "max"]
    }
}
pub fn new_reasoning_default(choices: &[&str]) -> &'static str {
    if choices.contains(&"xhigh") {
        "xhigh"
    } else {
        "high"
    }
}
pub fn parameter_support(client: ClientId, mode: &str) -> ParameterSupport {
    match client {
        ClientId::VsCode | ClientId::ZCode => ParameterSupport {
            limits: true,
            image: true,
            efforts: true,
            protocol: false,
        },
        ClientId::DeepSeekHarness => ParameterSupport {
            limits: true,
            image: true,
            efforts: true,
            protocol: true,
        },
        ClientId::WebStorm if mode == "acp" => ParameterSupport {
            limits: true,
            image: true,
            efforts: false,
            protocol: false,
        },
        _ => ParameterSupport::default(),
    }
}

/// Slot order is stable. Accesses update recency without moving a visible slot.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecentClients {
    slots: [ClientId; 3],
    last_used: [u64; 3],
    serial: u64,
}

impl Default for RecentClients {
    fn default() -> Self {
        Self {
            slots: [ClientId::Codex, ClientId::WorkBuddy, ClientId::Tiangong],
            last_used: [0; 3],
            serial: 0,
        }
    }
}

impl RecentClients {
    pub fn slots(&self) -> &[ClientId; 3] {
        &self.slots
    }
    pub fn normalize_visible(&mut self) {
        for index in 0..self.slots.len() {
            if !self.slots[index].is_visible() {
                self.slots[index] = ClientId::VISIBLE
                    .into_iter()
                    .find(|id| !self.slots.contains(id))
                    .expect("visible clients can fill three recent slots");
                self.last_used[index] = 0;
            }
        }
    }
    pub fn visit(&mut self, client: ClientId) {
        if !client.is_visible() {
            return;
        }
        if self.serial == u64::MAX {
            let minimum = *self.last_used.iter().min().unwrap_or(&0);
            for v in &mut self.last_used {
                *v = v.saturating_sub(minimum);
            }
            self.serial = *self.last_used.iter().max().unwrap_or(&0);
        }
        self.serial = self.serial.saturating_add(1);
        let index = self
            .slots
            .iter()
            .position(|v| *v == client)
            .unwrap_or_else(|| (0..3).min_by_key(|i| (self.last_used[*i], *i)).unwrap());
        self.slots[index] = client;
        self.last_used[index] = self.serial;
    }
    pub fn validate(&self) -> bool {
        self.slots[0] != self.slots[1]
            && self.slots[0] != self.slots[2]
            && self.slots[1] != self.slots[2]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_parameter_fields_follow_client_and_route_writer_contracts() {
        for id in [ClientId::VsCode, ClientId::ZCode] {
            assert_eq!(
                parameter_support(id, "chat"),
                ParameterSupport {
                    limits: true,
                    image: true,
                    efforts: true,
                    protocol: false
                }
            );
        }
        assert_eq!(
            parameter_support(ClientId::DeepSeekHarness, "gateway"),
            ParameterSupport {
                limits: true,
                image: true,
                efforts: true,
                protocol: true
            }
        );
        assert_eq!(
            parameter_support(ClientId::WebStorm, "acp"),
            ParameterSupport {
                limits: true,
                image: true,
                efforts: false,
                protocol: false
            }
        );
        for id in [
            ClientId::WebStorm,
            ClientId::Codex,
            ClientId::WorkBuddy,
            ClientId::Tiangong,
        ] {
            assert_eq!(parameter_support(id, "native"), ParameterSupport::default());
        }
    }
    #[test]
    fn recent_keeps_slots_and_replaces_only_oldest() {
        let mut recent = RecentClients::default();
        recent.visit(ClientId::Tiangong);
        recent.visit(ClientId::Codex);
        assert_eq!(recent.slots(), &RecentClients::default().slots);
        recent.visit(ClientId::ZCode);
        assert_eq!(
            recent.slots(),
            &[ClientId::Codex, ClientId::ZCode, ClientId::Tiangong]
        );
        recent.visit(ClientId::Codex);
        assert_eq!(
            recent.slots(),
            &[ClientId::Codex, ClientId::ZCode, ClientId::Tiangong]
        );
        recent.visit(ClientId::DeepSeekHarness);
        assert_eq!(
            recent.slots(),
            &[ClientId::Codex, ClientId::ZCode, ClientId::DeepSeekHarness]
        );
    }
    #[test]
    fn hidden_clients_keep_backend_identity_but_do_not_enter_recent_navigation() {
        for id in [ClientId::VsCode, ClientId::WebStorm] {
            assert!(ClientId::ALL.contains(&id));
            assert!(!id.is_visible());
            assert_eq!(
                serde_json::from_value::<ClientId>(serde_json::json!(id.key())).unwrap(),
                id
            );
        }
        let mut recent: RecentClients = serde_json::from_value(serde_json::json!({
            "slots":["vscode","zcode","webstorm"],"last_used":[9,7,8],"serial":9
        }))
        .unwrap();
        recent.normalize_visible();
        assert_eq!(
            recent.slots(),
            &[ClientId::Codex, ClientId::ZCode, ClientId::WorkBuddy]
        );
        assert!(recent.validate());
        assert_eq!(recent.last_used, [0, 7, 0]);
        let saved = serde_json::to_value(&recent).unwrap();
        recent.visit(ClientId::VsCode);
        recent.visit(ClientId::WebStorm);
        recent.normalize_visible();
        assert_eq!(serde_json::to_value(&recent).unwrap(), saved);
        recent.visit(ClientId::DeepSeekHarness);
        assert_eq!(
            recent.slots(),
            &[
                ClientId::DeepSeekHarness,
                ClientId::ZCode,
                ClientId::WorkBuddy
            ]
        );
    }
}
