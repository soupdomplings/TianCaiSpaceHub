//! One source save followed by independent client/model writes. A failed or
//! uncertain write is reported once, never replayed by this batch.
use super::ApiClient;
use crate::{
    ai_gateway::config::provider_route_id,
    config::AppConfig,
    desktop::{clients::ClientId, targets::ClientTarget},
    external_import::CommitImport,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Deserialize)]
struct TargetCatalog {
    targets: Vec<ClientTarget>,
}

pub(super) fn load_targets(api: &ApiClient) -> Result<Vec<ClientTarget>, String> {
    let view: TargetCatalog =
        api.get_with_timeout("/api/desktop/library", Duration::from_secs(30))?;
    let defaults = crate::desktop::targets::discover().map_err(|e| e.to_string())?;
    Ok(eligible_targets(view.targets, &defaults))
}

fn eligible_targets(targets: Vec<ClientTarget>, defaults: &[ClientTarget]) -> Vec<ClientTarget> {
    let mut targets = targets
        .into_iter()
        .filter(|t| {
            t.present
                && t.client.is_visible()
                && !t.models_inherited
                && (!matches!(
                    t.client,
                    ClientId::Codex | ClientId::WorkBuddy | ClientId::Tiangong
                ) || defaults.iter().any(|d| {
                    d.id == t.id
                        && d.client == t.client
                        && d.models_root() == t.models_root()
                        && d.model_mode == t.model_mode
                }))
        })
        .collect::<Vec<_>>();
    targets.sort_by_key(|t| {
        (
            ClientId::VISIBLE
                .iter()
                .position(|c| *c == t.client)
                .unwrap_or(usize::MAX),
            t.label.clone(),
        )
    });
    targets.dedup_by(|a, b| a.id == b.id);
    targets
}

pub(super) trait BatchApi {
    fn commit(&self, request: &CommitImport) -> Result<(), String>;
    fn latest(&self) -> Result<AppConfig, String>;
    fn configure(&self, path: &str, body: &Value) -> Result<Value, String>;
}

impl BatchApi for ApiClient {
    fn commit(&self, request: &CommitImport) -> Result<(), String> {
        self.post_json_with_timeout::<_, Value>(
            "/api/external-import/commit",
            request,
            Duration::from_secs(15),
        )
        .map(|_| ())
    }
    fn latest(&self) -> Result<AppConfig, String> {
        self.get_app_config()
    }
    fn configure(&self, path: &str, body: &Value) -> Result<Value, String> {
        self.desktop_post(path, body)
    }
}

#[derive(Clone)]
pub(super) struct Outcome {
    pub target: String,
    pub model: String,
    pub result: Result<String, String>,
}

pub(super) enum Progress {
    Saved,
    Item(Outcome),
}

pub(super) struct Report {
    pub save_error: Option<String>,
    pub outcomes: Vec<Outcome>,
}

impl Report {
    pub fn message(&self) -> String {
        if let Some(error) = &self.save_error {
            return format!("渠道保存未确认：{error}\n请在渠道页核对，本批不会自动再次提交。");
        }
        let successes = self.outcomes.iter().filter(|o| o.result.is_ok()).count();
        let failures = self.outcomes.len() - successes;
        if self.outcomes.is_empty() {
            "渠道已保存，可在渠道页继续管理。".into()
        } else if failures == 0 {
            format!("渠道已保存，已接入 {successes} 项。")
        } else {
            format!(
                "渠道已保存，已接入 {successes} 项，{failures} 项未完成。请在对应客户端页核对后处理，成功项已保留。"
            )
        }
    }
}

pub(super) fn run(
    api: &impl BatchApi,
    mut request: CommitImport,
    targets: Vec<ClientTarget>,
    mut progress: impl FnMut(Progress),
) -> Report {
    // Client destinations use the real client writers below, never the old
    // shortcut that merely exposed the source model in Codex's global list.
    request.visible_models = false;
    if !targets.is_empty() {
        request.enabled = true;
    }
    if let Err(error) = api.commit(&request) {
        return Report {
            save_error: Some(error),
            outcomes: vec![],
        };
    }
    progress(Progress::Saved);
    let mut outcomes = Vec::new();
    for target in targets {
        for model in &request.models {
            let result = configure_one(api, &request, &target, model);
            let outcome = Outcome {
                target: format!("{} · {}", target.client.name(), target.label),
                model: model.clone(),
                result,
            };
            progress(Progress::Item(outcome.clone()));
            outcomes.push(outcome);
        }
    }
    Report {
        save_error: None,
        outcomes,
    }
}

fn configure_one(
    api: &impl BatchApi,
    request: &CommitImport,
    target: &ClientTarget,
    model: &str,
) -> Result<String, String> {
    // Every preceding native write may change the shared config revision,
    // including one that ultimately reports a native-file error.
    let latest = api.latest()?;
    let revision = latest
        .revision
        .as_deref()
        .filter(|r| !r.is_empty())
        .ok_or("无法读取最新配置版本，请到客户端页重新读取")?;
    let source = latest
        .ai_gateway
        .providers
        .iter()
        .find(|p| {
            p.name == request.name.trim()
                && p.enabled
                && !p.is_client_reserved()
                && p.import_source
                    .as_ref()
                    .is_some_and(|s| s.same_key(&request.draft.source))
        })
        .ok_or("来源渠道已停用、改名或删除，请重新读取")?;
    let path = if matches!(
        target.client,
        ClientId::Codex | ClientId::WorkBuddy | ClientId::Tiangong
    ) {
        "/api/desktop/models/copy"
    } else {
        "/api/desktop/models/use"
    };
    let value = api.configure(
        path,
        &json!({
            "revision": revision, "sourceRoute": provider_route_id(source),
            "targetId": target.id, "model": model, "client": target.client,
            "using": true, "label": model,
        }),
    )?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(error.into());
    }
    Ok(value["message"]
        .as_str()
        .or_else(|| value["status"]["message"].as_str())
        .unwrap_or("已接入")
        .into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ai_gateway::config::ProviderConfig,
        desktop::targets::McpFormat,
        external_import::{ImportDraft, ImportSource},
    };
    use std::{
        cell::{Cell, RefCell},
        path::PathBuf,
    };

    fn target(client: ClientId, id: &str) -> ClientTarget {
        ClientTarget {
            id: id.into(),
            client,
            label: "默认".into(),
            root: PathBuf::from(id),
            skills: PathBuf::new(),
            mcp: PathBuf::new(),
            mcp_format: McpFormat::Codex,
            present: true,
            model_mode: client.key().into(),
            model_root: None,
            models_inherited: false,
        }
    }
    fn request() -> CommitImport {
        CommitImport {
            draft: ImportDraft {
                source: ImportSource {
                    origin: "https://fixture.example".into(),
                    key_id: "key-1".into(),
                    site_name: "合成站点".into(),
                    key_name: "合成密钥".into(),
                },
                provider: ProviderConfig {
                    name: "合成渠道".into(),
                    api_key: "fixture-secret".into(),
                    base_url: "https://fixture.example/v1".into(),
                    models: vec!["model-a".into(), "model-b".into()],
                    ..Default::default()
                },
                model_warning: None,
            },
            name: "合成渠道".into(),
            update: None,
            models: vec!["model-a".into(), "model-b".into()],
            enabled: false,
            visible_models: true,
            replace_aliases: false,
        }
    }
    #[derive(Default)]
    struct MockApi {
        config: RefCell<AppConfig>,
        writes: RefCell<Vec<(String, Value)>>,
        reads: Cell<usize>,
        fail: Option<(String, String)>,
        commit_fails: bool,
    }
    impl BatchApi for MockApi {
        fn commit(&self, request: &CommitImport) -> Result<(), String> {
            self.writes
                .borrow_mut()
                .push(("commit".into(), serde_json::to_value(request).unwrap()));
            if self.commit_fails {
                return Err("保存超时，结果未知".into());
            }
            let mut config = self.config.borrow_mut();
            crate::external_import::merge_import(&mut config, request)?;
            config.revision = Some("rev-1".into());
            Ok(())
        }
        fn latest(&self) -> Result<AppConfig, String> {
            self.reads.set(self.reads.get() + 1);
            Ok(self.config.borrow().clone())
        }
        fn configure(&self, path: &str, body: &Value) -> Result<Value, String> {
            let next = self.writes.borrow().len() + 1;
            assert_eq!(
                body["revision"],
                self.config.borrow().revision.as_deref().unwrap()
            );
            self.writes.borrow_mut().push((path.into(), body.clone()));
            self.config.borrow_mut().revision = Some(format!("rev-{next}"));
            if self.fail.as_ref().is_some_and(|(target, model)| {
                body["targetId"] == *target && body["model"] == *model
            }) {
                return Err("原生保存超时，结果未知".into());
            }
            Ok(json!({"revision":format!("rev-{next}"),"message":"已配置"}))
        }
    }
    #[test]
    fn import_batch_saves_once_and_uses_native_writers_with_fresh_revisions() {
        let api = MockApi::default();
        let targets = ClientId::VISIBLE
            .iter()
            .map(|c| target(*c, c.key()))
            .collect();
        let report = run(&api, request(), targets, |_| {});
        assert!(report.save_error.is_none());
        assert_eq!(report.outcomes.len(), 10);
        assert!(report.outcomes.iter().all(|o| o.result.is_ok()));
        let writes = api.writes.borrow();
        assert_eq!(writes.len(), 11);
        assert_eq!(writes[0].1["enabled"], true);
        assert_eq!(writes[0].1["visible_models"], false);
        assert_eq!(api.reads.get(), 10);
        let source = api.config.borrow().ai_gateway.providers[0].clone();
        for (index, client) in ClientId::VISIBLE.iter().enumerate() {
            for (m, model) in ["model-a", "model-b"].iter().enumerate() {
                let (path, body) = &writes[1 + index * 2 + m];
                assert_eq!(
                    path,
                    if index < 3 {
                        "/api/desktop/models/copy"
                    } else {
                        "/api/desktop/models/use"
                    }
                );
                assert_eq!(body["targetId"], client.key());
                assert_eq!(body["sourceRoute"], provider_route_id(&source));
                assert_eq!(body["model"], *model);
                assert_eq!(body["using"], true);
            }
        }
    }
    #[test]
    fn import_batch_keeps_successes_and_never_replays_an_uncertain_item() {
        let api = MockApi {
            fail: Some(("deepseek-harness".into(), "model-a".into())),
            ..Default::default()
        };
        let report = run(
            &api,
            request(),
            vec![
                target(ClientId::DeepSeekHarness, "deepseek-harness"),
                target(ClientId::ZCode, "zcode"),
            ],
            |_| {},
        );
        assert_eq!(
            report.outcomes.iter().filter(|o| o.result.is_ok()).count(),
            3
        );
        assert_eq!(
            report.outcomes.iter().filter(|o| o.result.is_err()).count(),
            1
        );
        assert_eq!(api.writes.borrow().len(), 5);
        assert_eq!(
            api.writes
                .borrow()
                .iter()
                .filter(|(_, b)| b["targetId"] == "deepseek-harness" && b["model"] == "model-a")
                .count(),
            1
        );
    }
    #[test]
    fn import_batch_commit_failure_does_not_start_or_repeat_client_writes() {
        let api = MockApi {
            commit_fails: true,
            ..Default::default()
        };
        let report = run(
            &api,
            request(),
            vec![target(ClientId::Codex, "codex")],
            |_| {},
        );
        assert!(report.save_error.is_some());
        assert!(report.outcomes.is_empty());
        assert_eq!(api.writes.borrow().len(), 1);
        assert_eq!(api.reads.get(), 0);
        let only_source = MockApi::default();
        let report = run(&only_source, request(), vec![], |_| {});
        assert!(report.outcomes.is_empty());
        assert!(!only_source.config.borrow().ai_gateway.providers[0].enabled);
        assert!(
            only_source
                .config
                .borrow()
                .ai_gateway
                .codex_visible_models
                .is_empty()
        );
    }
    #[test]
    fn import_targets_hide_ide_missing_inherited_and_legacy_resource_only_locations() {
        let defaults = ClientId::VISIBLE
            .iter()
            .map(|c| target(*c, c.key()))
            .collect::<Vec<_>>();
        let mut candidates = defaults.clone();
        candidates.extend([
            target(ClientId::VsCode, "vscode"),
            target(ClientId::WebStorm, "webstorm"),
            target(ClientId::Codex, "resource-only"),
        ]);
        let mut inherited = target(ClientId::DeepSeekHarness, "inherited");
        inherited.models_inherited = true;
        let mut missing = target(ClientId::ZCode, "missing");
        missing.present = false;
        candidates.extend([inherited, missing]);
        let eligible = eligible_targets(candidates, &defaults);
        assert_eq!(
            eligible.iter().map(|t| t.client).collect::<Vec<_>>(),
            ClientId::VISIBLE
        );
    }
}
