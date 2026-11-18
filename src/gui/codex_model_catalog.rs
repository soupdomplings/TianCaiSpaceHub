//! Source channels supply candidates; the client's saved list is independent.
use crate::ai_gateway::config::AiGatewayConfig;
use std::collections::BTreeMap;

pub(super) fn channel_groups(config: &AiGatewayConfig) -> (Vec<String>, Vec<Vec<String>>) {
    config
        .providers
        .iter()
        .filter(|p| p.enabled && p.is_client_model_source())
        .map(|p| {
            let mut by_id = BTreeMap::new();
            for id in p.models.iter().chain(p.model_aliases.keys()) {
                by_id.entry(id.to_ascii_lowercase()).or_insert(id);
            }
            let models = by_id.into_values().cloned().collect();
            (p.name.clone(), models)
        })
        .unzip()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_gateway::config::ProviderConfig;
    #[test]
    fn candidates_only_come_from_enabled_source_channels_and_include_aliases() {
        let mut config = AiGatewayConfig::default();
        let mut p = ProviderConfig::default();
        p.name = "Codex · synthetic · abc123".into(); // A user-chosen name, not a copy.
        p.enabled = true;
        p.models = vec!["a".into(), "b".into(), "A".into()];
        p.model_aliases.insert("alias".into(), "a".into());
        config.providers.push(p);
        let mut reserved = ProviderConfig::default();
        reserved.name = format!("{}fixture", crate::desktop::models::PREFIX);
        reserved.models = vec!["client-only".into()];
        reserved.enabled = true;
        config.providers.push(reserved);
        let mut disabled = ProviderConfig::default();
        disabled.name = "disabled".into();
        disabled.enabled = false;
        disabled.models = vec!["disabled-only".into()];
        config.providers.push(disabled);
        config.providers.push(ProviderConfig {
            enabled: true,
            name: "Codex · old-model · abc123".into(),
            models: vec!["old-model".into()],
            model_aliases: [("old-model-abc123".into(), "old-model".into())].into(),
            ..Default::default()
        });
        config.codex_visible_models = vec!["custom-old".into()];
        let (names, groups) = channel_groups(&config);
        assert_eq!(names, ["Codex · synthetic · abc123"]);
        assert_eq!(groups, [vec!["a", "alias", "b"]]);
    }
}
