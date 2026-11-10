//! Pure channel grouping for the existing Codex visible-model contract.
use crate::ai_gateway::{catalog::visible_catalog_model_options, config::AiGatewayConfig};
use std::collections::BTreeSet;

pub(super) fn channel_groups(config: &AiGatewayConfig) -> Vec<Vec<String>> {
    let providers = config
        .providers
        .iter()
        .filter(|p| p.enabled && !p.is_client_reserved())
        .collect::<Vec<_>>();
    let routed = providers
        .iter()
        .flat_map(|p| p.models.iter().chain(p.model_aliases.keys()))
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut built = visible_catalog_model_options()
        .into_iter()
        .map(|m| m.slug)
        .collect::<BTreeSet<_>>();
    built.extend(
        config
            .codex_visible_models
            .iter()
            .filter(|m| !routed.contains(*m))
            .cloned(),
    );
    std::iter::once(built.into_iter().collect())
        .chain(providers.into_iter().map(|p| {
            p.models
                .iter()
                .chain(p.model_aliases.keys())
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_gateway::config::ProviderConfig;
    #[test]
    fn groups_keep_aliases_custom_selections_and_reserved_scope() {
        let mut config = AiGatewayConfig::default();
        let mut p = ProviderConfig::default();
        p.name = "synthetic".into();
        p.enabled = true;
        p.models = vec!["a".into(), "b".into()];
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
        config.codex_visible_models = vec!["custom-old".into()];
        let groups = channel_groups(&config);
        assert!(groups[0].contains(&"custom-old".into()));
        assert_eq!(groups[1], vec!["a", "alias", "b"]);
        assert_eq!(groups.len(), 2);
        assert!(
            !groups
                .iter()
                .flatten()
                .any(|s| matches!(s.as_str(), "client-only" | "disabled-only"))
        );
    }
}
