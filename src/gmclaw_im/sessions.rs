//! GMClaw implementation of the shared IM session backend. No transport UI here.
use super::*;
use crate::{
    im::core::{
        thread::{
            ThreadCreateCapabilities, ThreadCreateDefaults, ThreadCreateForm, ThreadListEntry,
            expand_home_prefix, next_thread_routing_request_id,
            validate_thread_create_capabilities,
        },
        thread_list::ThreadRoutingPage,
    },
    im_runtime::{ThreadRoutingRequestState, ThreadRoutingStage},
    remote_control_backend::ThreadStartOptions,
};
use anyhow::Context;

async fn checked_config(
    state: &SharedState,
    message: &InboundMessage,
    binding: &SenderBinding,
) -> Result<GmClawBridgeConfig> {
    let latest = state.config.lock().await;
    validate_access(&latest, message, binding)?;
    Ok(latest.gmclaw_bridge.clone())
}

pub(crate) async fn validate_menu_access(
    state: &SharedState,
    message: &InboundMessage,
) -> Result<()> {
    let binding = binding_for(state, message)
        .await
        .context("天工会话入口已失效，请发送 /tg 重新打开")?;
    checked_config(state, message, &binding).await?;
    Ok(())
}

fn validate_access(
    latest: &crate::config::AppConfig,
    message: &InboundMessage,
    binding: &SenderBinding,
) -> Result<()> {
    ensure!(
        sender_allowed(&latest, message),
        "当前账号或发送者已无接入权限"
    );
    let config = &latest.gmclaw_bridge;
    ensure!(
        config.enabled && binding.selected.load(Ordering::Acquire),
        "当前执行端已变化，请重新打开会话入口"
    );
    config.validate()?;
    ensure!(
        message.session_scope.as_deref() == Some(session_scope(binding, config, message).as_str()),
        "会话卡片已过期，请用 /q 重新打开"
    );
    Ok(())
}

async fn checked_request(
    state: &SharedState,
    message: &InboundMessage,
    id: Option<&str>,
) -> Result<ThreadRoutingRequestState> {
    let id = id.context("缺少会话请求，请重新打开会话入口")?;
    let request = state
        .runtime
        .lock()
        .await
        .thread_routing_request(id)
        .context("会话卡片已过期，请重新打开")?;
    ensure!(
        request.conversation_key == message.conversation_key(),
        "会话卡片不属于当前执行端或发送者"
    );
    Ok(request)
}

pub(super) fn default_project(config: &GmClawBridgeConfig) -> String {
    if let Ok(path) = validated_path(&config.project_path) {
        return path;
    }
    crate::gmclaw_config::config_path()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("workspace")
        .to_string_lossy()
        .into_owned()
}

fn selected(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty() && v != "__default__")
}

fn validated_path(value: &str) -> Result<String> {
    let value = value.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|p| p.strip_suffix('"'))
        .unwrap_or(value);
    ensure!(
        !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control),
        "项目目录格式无效"
    );
    let path = expand_home_prefix(value);
    ensure!(
        path.is_absolute(),
        "项目目录必须是运行 Hub 的电脑上的绝对路径"
    );
    Ok(path.to_string_lossy().into_owned())
}

async fn model_catalog() -> Result<(
    Vec<crate::im::core::thread::ThreadModelChoice>,
    Option<String>,
)> {
    tokio::time::timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking(crate::gmclaw_config::model_choices),
    )
    .await
    .context("读取天工模型列表超时，请重试")?
    .context("无法读取天工模型列表")?
}

pub(super) async fn validate_session_model(model_id: Option<&str>) -> Result<()> {
    let model = model_id.context("当前会话未绑定模型，请重新创建会话选择模型")?;
    ensure!(
        model_catalog()
            .await?
            .0
            .iter()
            .any(|choice| choice.value == model),
        "当前会话的天工模型已不可用，请恢复原模型条目或新建会话选择模型"
    );
    Ok(())
}

fn catalog_config_matches(original: &GmClawBridgeConfig, latest: &GmClawBridgeConfig) -> bool {
    original.fingerprint() == latest.fingerprint()
        && original.project_path == latest.project_path
        && original.desktop_path == latest.desktop_path
}

fn merge_project_paths(
    default: &str,
    projects: Vec<crate::gmclaw_desktop::DesktopProject>,
    history: &[String],
    native_workspace: &str,
) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    let mut identities = std::collections::HashSet::new();
    let mut insert = |path: &str| -> Result<()> {
        let path = validated_path(path)?;
        if identities.insert(crate::gmclaw_desktop::project_path_key(&path)) {
            ensure!(
                paths.len() < crate::gmclaw_desktop::MAX_PROJECTS,
                "合并后的天工项目目录超过完整展示范围"
            );
            paths.push(path);
        }
        Ok(())
    };
    insert(default)?;
    for project in projects {
        if project.work_dir.is_empty() {
            if project.scenario_id == "default" {
                insert(native_workspace)?;
            }
        } else {
            insert(&project.work_dir)?;
        }
    }
    for path in history {
        insert(path)?;
    }
    Ok(paths)
}

async fn desktop_project_catalog(
    state: &SharedState,
    message: &InboundMessage,
    binding: &SenderBinding,
    config: &GmClawBridgeConfig,
) -> Result<(Vec<crate::gmclaw_desktop::DesktopProject>, String)> {
    // Bound connection discovery and the complete scenario GET together.
    // The menu never creates directories, tasks, or session metadata.
    tokio::time::timeout(Duration::from_secs(20), async {
        let original = checked_config(state, message, binding).await?;
        ensure!(
            catalog_config_matches(config, &original),
            "读取期间天工项目连接配置已变化，请重新打开会话设置"
        );
        let instance = desktop::instance_identity(config).await?;
        let client = desktop::client(config).await?;
        let projects = client.list_projects().await?;
        ensure!(
            instance == desktop::instance_identity(config).await?,
            "读取期间天工运行实例已变化，请重新打开会话设置"
        );
        let latest = checked_config(state, message, binding).await?;
        ensure!(
            catalog_config_matches(config, &latest),
            "读取期间天工项目连接配置已变化，请重新打开会话设置"
        );
        Ok((projects, instance))
    })
    .await
    .context("完整读取天工项目列表超过 20 秒，请重试")?
}

fn unavailable_defaults(notice: impl Into<String>) -> ThreadCreateDefaults {
    ThreadCreateDefaults {
        remote_name: Some("天工 Claw".into()),
        capabilities: ThreadCreateCapabilities {
            reasoning: false,
            permissions: false,
        },
        settings_notice: Some(notice.into()),
        ..Default::default()
    }
}

pub(crate) async fn defaults(
    state: &SharedState,
    message: &InboundMessage,
) -> ThreadCreateDefaults {
    let Some(binding) = binding_for(state, message).await else {
        return unavailable_defaults("天工会话入口已失效，请发送 /tg 重新打开。");
    };
    let config = match checked_config(state, message, &binding).await {
        Ok(config) => config,
        Err(error) => return unavailable_defaults(error.to_string()),
    };
    let default = default_project(&config);
    let mut history = Vec::new();
    if let Ok(sessions) = binding.sessions.try_lock() {
        for session in sessions.current.iter().chain(sessions.history.iter()) {
            if session.config_fingerprint == config.fingerprint() {
                history.push(session.project_path.clone());
            }
        }
    }
    let native_workspace = desktop::native_workspace();
    let local_projects = merge_project_paths(&default, Vec::new(), &history, &native_workspace);
    let (catalog, model_result) = tokio::join!(
        desktop_project_catalog(state, message, &binding, &config),
        model_catalog(),
    );
    // If the model catalog was slower, do not display a directory snapshot
    // belonging to an instance that exited during that unrelated read.
    let catalog = match catalog {
        Ok((projects, instance)) => {
            match tokio::time::timeout(Duration::from_secs(5), desktop::instance_identity(&config))
                .await
            {
                Ok(Ok(latest)) if latest == instance => {
                    merge_project_paths(&default, projects, &history, &native_workspace)
                }
                Ok(Ok(_)) => Err(anyhow::anyhow!(
                    "读取期间天工运行实例已变化，请重新打开会话设置"
                )),
                Ok(Err(error)) => Err(error),
                Err(_) => Err(anyhow::anyhow!("核对天工项目运行实例超时，请重试")),
            }
        }
        Err(error) => Err(error),
    };
    let (projects, project_notice) = match catalog {
        Ok(projects) => (projects, String::new()),
        Err(error) => (
            local_projects.unwrap_or_else(|_| vec![default.clone()]),
            format!(
                "未能完整读取天工项目目录：{error}。当前只提供默认及此发送者已有会话目录，可填写自定义绝对路径；稍后重新打开设置可重试。\n"
            ),
        ),
    };
    let (models, active, catalog_notice) = match model_result {
        Ok((models, active)) => (models, active, String::new()),
        Err(_) => (
            Vec::new(),
            None,
            "暂时无法读取天工模型列表，创建前将重新检查。\n".to_owned(),
        ),
    };
    // Do not disclose a previously read project/model catalog after access or
    // menu ownership changed while awaiting the local metadata operations.
    let latest = match checked_config(state, message, &binding).await {
        Ok(latest) => latest,
        Err(error) => return unavailable_defaults(error.to_string()),
    };
    if !catalog_config_matches(&config, &latest) {
        return unavailable_defaults("读取期间天工连接或默认目录已变化，请重新打开会话设置。");
    }
    ThreadCreateDefaults {
        remote_name: Some("天工 Claw".into()),
        cwd: Some(default),
        model: config.model_id.or(active),
        projects,
        models,
        capabilities: ThreadCreateCapabilities {
            reasoning: false,
            permissions: false,
        },
        settings_notice: Some(format!(
            "{project_notice}{catalog_notice}思考参数沿用所选模型接入配置；工具权限沿用天工策略，需审批时在当前聊天确认。"
        )),
        ..Default::default()
    }
}

pub(crate) async fn options(
    state: &SharedState,
    message: &InboundMessage,
    form: ThreadCreateForm,
) -> Result<ThreadStartOptions> {
    let binding = binding_for(state, message).await.context("请先发送 /tg")?;
    let config = checked_config(state, message, &binding).await?;
    let capabilities = ThreadCreateDefaults {
        capabilities: ThreadCreateCapabilities {
            reasoning: false,
            permissions: false,
        },
        ..Default::default()
    };
    validate_thread_create_capabilities(&capabilities, &form)?;
    let custom = selected(form.cwd_custom);
    let choice = selected(form.cwd_choice);
    ensure!(
        choice.as_deref() != Some("__custom__") || custom.is_some(),
        "选择自定义目录时需要填写绝对路径"
    );
    let cwd = validated_path(
        &custom
            .or(choice)
            .unwrap_or_else(|| default_project(&config)),
    )?;
    let (models, active) = model_catalog().await?;
    let model = selected(form.model)
        .or(config.model_id)
        .or(active)
        .context("天工没有默认模型，请在模型接入页配置模型后重新打开会话设置")?;
    ensure!(
        models.iter().any(|m| m.value == model),
        "所选天工模型已不可用，请重新打开会话设置选择模型"
    );
    // Pure validation here. Only the final shared Create action creates folders.
    Ok(ThreadStartOptions {
        cwd: Some(cwd),
        model: Some(model),
        ..Default::default()
    })
}

async fn create_directory(path: &str) -> Result<String> {
    let path = validated_path(path)?;
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::fs::create_dir_all(&path)
            .await
            .context("无法创建项目目录，请检查路径和权限")?;
        let path = tokio::fs::canonicalize(&path)
            .await
            .context("无法读取项目目录")?;
        ensure!(
            tokio::fs::metadata(&path).await?.is_dir(),
            "项目路径不是文件夹"
        );
        Ok::<_, anyhow::Error>(path.to_string_lossy().into_owned())
    })
    .await
    .context("目录处理超过 10 秒，目录操作可能仍在完成；未创建会话，请检查后重试")?;
    result.map_err(|e| anyhow::anyhow!("{e}；部分目录可能已创建，本次未创建会话"))
}

pub(crate) async fn create(
    state: &SharedState,
    message: &InboundMessage,
    options: ThreadStartOptions,
    request_id: Option<&str>,
) -> Result<String> {
    let binding = binding_for(state, message).await.context("请先发送 /tg")?;
    let mut sessions = binding
        .sessions
        .try_lock()
        .map_err(|_| anyhow::anyhow!("天工正在执行，请等待完成"))?;
    let config = checked_config(state, message, &binding).await?;
    let request = checked_request(state, message, request_id).await?;
    ensure!(
        matches!(
            request.stage,
            ThreadRoutingStage::Choice
                | ThreadRoutingStage::CreateSettings
                | ThreadRoutingStage::CreateOptions
        ),
        "会话设置已变化，请重新打开"
    );
    ensure!(
        sessions.current.is_none(),
        "已有当前会话，请先 /q 退出再新建"
    );
    ensure!(
        options.reasoning_effort.is_none()
            && options.permissions.is_none()
            && options.approval_policy.is_none()
            && options.approvals_reviewer.is_none(),
        "当前执行端不支持覆盖这些会话参数"
    );
    let model = options.model.context("请选择天工模型")?;
    ensure!(
        model_catalog().await?.0.iter().any(|m| m.value == model),
        "所选天工模型已不可用，请重新选择"
    );
    // Recheck after asynchronous catalog access, immediately before filesystem side effects.
    checked_config(state, message, &binding).await?;
    let project_path = create_directory(options.cwd.as_deref().context("请选择项目目录")?).await?;
    checked_request(state, message, request_id).await?;
    let latest = state.config.lock().await;
    validate_access(&latest, message, &binding)
        .map_err(|e| anyhow::anyhow!("{e}；目录可能已创建，本次未绑定会话"))?;
    let mut session = Session::new(&config, project_path);
    session.model_id = Some(model);
    drop(latest);
    let client = desktop::client(&config).await?;
    if let Err(error) = desktop::register(&client, &config, &mut session).await {
        session.uncertain = true;
        let _ = desktop::save(state, &session).await;
        return Err(anyhow::anyhow!(
            "{error}；天工可能已保存任务，请在桌面核对，必要时手动处理未完成的空任务；不会自动重复创建"
        ));
    }
    checked_config(state, message, &binding).await?;
    checked_request(state, message, request_id).await?;
    desktop::claim(state, &binding, &session.id).await?;
    if let Err(error) = desktop::save(state, &session).await {
        desktop::release(state, &binding, Some(&session)).await;
        return Err(error);
    }
    let id = session.id.clone();
    sessions.current = Some(session);
    binding.ui_epoch.fetch_add(1, Ordering::AcqRel);
    state
        .runtime
        .lock()
        .await
        .thread_routing_requests
        .retain(|_, r| r.conversation_key != message.conversation_key());
    Ok(id)
}

pub(crate) async fn resume(
    state: &SharedState,
    message: &InboundMessage,
    id: &str,
    request_id: Option<&str>,
) -> Result<serde_json::Value> {
    let binding = binding_for(state, message).await.context("请先发送 /tg")?;
    let mut sessions = binding
        .sessions
        .try_lock()
        .map_err(|_| anyhow::anyhow!("天工正在执行，请等待完成"))?;
    let config = checked_config(state, message, &binding).await?;
    let request = checked_request(state, message, request_id).await?;
    ensure!(
        request.stage == ThreadRoutingStage::ResumeList
            && request
                .thread_ids_by_page
                .iter()
                .flatten()
                .any(|listed| listed == id),
        "请选择当前列表中已显示的会话"
    );
    ensure!(sessions.current.is_none(), "请先 /q 退出当前会话");
    let client = desktop::client(&config).await?;
    let task = client
        .task(id)
        .await?
        .context("此天工桌面会话已被移除，请刷新列表")?;
    if let Some(previous) = sessions
        .history
        .iter()
        .find(|session| session.id == task.session_id)
    {
        ensure!(
            !previous.uncertain && previous.pending.is_none(),
            "此会话有未确认执行或工具审批，请先到天工桌面核对"
        );
    }
    let session = desktop::restore(state, &config, &client, task).await?;
    checked_request(state, message, request_id).await?;
    let latest = state.config.lock().await;
    validate_access(&latest, message, &binding)?;
    drop(latest);
    desktop::ensure_registered(&client, &session).await?;
    checked_config(state, message, &binding).await?;
    checked_request(state, message, request_id).await?;
    desktop::claim(state, &binding, &session.id).await?;
    if let Err(error) = desktop::save(state, &session).await {
        desktop::release(state, &binding, Some(&session)).await;
        return Err(error);
    }
    sessions
        .history
        .retain(|previous| previous.id != session.id);
    let value = serde_json::json!({"id":session.id,"name":session.title,"cwd":session.project_path,"status":{"type":"idle"}});
    sessions.current = Some(session);
    binding.ui_epoch.fetch_add(1, Ordering::AcqRel);
    state
        .runtime
        .lock()
        .await
        .thread_routing_requests
        .retain(|_, r| r.conversation_key != message.conversation_key());
    Ok(value)
}

pub(crate) async fn page(
    state: &SharedState,
    message: &InboundMessage,
    existing: Option<&ThreadRoutingRequestState>,
    cursor: Option<&str>,
    page: usize,
    size: u32,
) -> Result<ThreadRoutingPage> {
    let binding = binding_for(state, message).await.context("请先发送 /tg")?;
    let config = checked_config(state, message, &binding).await?;
    let sessions = binding
        .sessions
        .try_lock()
        .map_err(|_| anyhow::anyhow!("天工正在执行，请等待完成"))?;
    if let Some(existing) = existing {
        ensure!(
            existing.conversation_key == message.conversation_key(),
            "会话列表已失效"
        );
    }
    let client = desktop::client(&config).await?;
    let mut entries = client.list_tasks().await?;
    entries.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.task_id.cmp(&right.task_id))
    });
    checked_config(state, message, &binding).await?;
    let links = desktop::links(state).await?;
    let claims = state.gmclaw_im.desktop_claims.lock().await;
    let size = (size as usize).clamp(1, 32);
    let page = page.max(1);
    let offset = cursor
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or((page - 1).saturating_mul(size));
    let shown = entries
        .iter()
        .skip(offset)
        .take(size)
        .map(|s| ThreadListEntry {
            thread_id: s.task_id.clone(),
            title: format!("{} · {}", s.scenario_name, s.title),
            cwd: Some(if s.work_dir.is_empty() {
                if s.scenario_id == "default" {
                    desktop::native_workspace()
                } else {
                    "目录未知，恢复前核对".into()
                }
            } else {
                s.work_dir.clone()
            }),
            state: if claims
                .get(&s.session_id)
                .is_some_and(|owner| owner != &binding.nonce)
            {
                "其他 IM 发送者正在使用".into()
            } else if links
                .iter()
                .any(|link| link.session_id == s.session_id && (link.uncertain || link.pending))
                || sessions.history.iter().any(|session| {
                    session.id == s.session_id && (session.uncertain || session.pending.is_some())
                })
            {
                "执行状态未知，不能恢复".into()
            } else {
                "天工桌面会话".into()
            },
        })
        .collect::<Vec<_>>();
    let next_cursor =
        (offset.saturating_add(size) < entries.len()).then(|| (offset + size).to_string());
    let mut cursors = existing.map(|r| r.page_cursors.clone()).unwrap_or_default();
    cursors.resize(page + 1, None);
    cursors[page - 1] = Some(offset.to_string());
    cursors[page] = next_cursor.clone();
    let mut ids = existing
        .map(|r| r.thread_ids_by_page.clone())
        .unwrap_or_default();
    ids.resize(page, vec![]);
    ids[page - 1] = shown.iter().map(|e| e.thread_id.clone()).collect();
    Ok(ThreadRoutingPage {
        request_id: existing
            .map(|r| r.request_id.clone())
            .unwrap_or_else(next_thread_routing_request_id),
        page,
        page_cursors: cursors,
        thread_ids_by_page: ids,
        entries: shown,
        next_cursor,
        model_provider_filter: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_project_path(name: &str) -> String {
        if cfg!(windows) {
            format!("D:/fixture/{name}")
        } else {
            format!("/fixture/{name}")
        }
    }

    #[test]
    fn directories_merge_empty_manual_projects_default_and_history_without_a_twenty_item_cap() {
        use crate::gmclaw_desktop::DesktopProject;
        let default = fixture_project_path("hub-default");
        let workspace = fixture_project_path("native-workspace");
        let mut projects = vec![DesktopProject {
            scenario_id: "default".into(),
            work_dir: String::new(),
        }];
        for index in 0..25 {
            projects.push(DesktopProject {
                scenario_id: format!("manual-{index}"),
                work_dir: fixture_project_path(&format!("manual-{index}")),
            });
        }
        projects.push(DesktopProject {
            scenario_id: "without-directory".into(),
            work_dir: String::new(),
        });
        let history = vec![
            fixture_project_path("manual-24"),
            fixture_project_path("history"),
        ];
        let paths = merge_project_paths(&default, projects, &history, &workspace).unwrap();
        assert_eq!(paths.len(), 28);
        assert_eq!(paths[0], default);
        assert_eq!(paths[1], workspace);
        assert!(paths.contains(&fixture_project_path("manual-24")));
        assert_eq!(paths.last(), Some(&fixture_project_path("history")));
        #[cfg(windows)]
        {
            let paths = merge_project_paths(
                "D:/fixture/Example",
                vec![DesktopProject {
                    scenario_id: "native".into(),
                    work_dir: r"d:\FIXTURE\example\".into(),
                }],
                &[r"\\?\D:\fixture\EXAMPLE".into()],
                &workspace,
            )
            .unwrap();
            assert_eq!(paths, vec!["D:/fixture/Example"]);
        }
    }

    #[test]
    fn directory_merge_and_snapshot_errors_do_not_silently_claim_completeness() {
        let default = fixture_project_path("default");
        let projects = (0..crate::gmclaw_desktop::MAX_PROJECTS)
            .map(|index| crate::gmclaw_desktop::DesktopProject {
                scenario_id: format!("project-{index}"),
                work_dir: fixture_project_path(&format!("project-{index}")),
            })
            .collect();
        assert!(merge_project_paths(&default, projects, &[], &default).is_err());
        let original = GmClawBridgeConfig::default();
        let mut changed = original.clone();
        changed.project_path = default;
        assert_eq!(original.fingerprint(), changed.fingerprint());
        assert!(!catalog_config_matches(&original, &changed));
        changed = original.clone();
        changed.desktop_path = Some("fixture-installation".into());
        assert!(!catalog_config_matches(&original, &changed));
        changed = original.clone();
        changed.auth_token = "fixture-changed-authorization".into();
        assert!(!catalog_config_matches(&original, &changed));
    }

    #[test]
    fn internal_ui_scope_cannot_be_supplied_by_transport_json() {
        let message: InboundMessage = serde_json::from_value(serde_json::json!({
            "accountId":"a","senderId":"s","chatId":"c","chatType":"direct","messageId":"m","text":"hello","mentioned":true,
            "sessionScope":"spoofed","sessionEntry":"Create"
        })).unwrap();
        assert!(message.session_scope.is_none());
        assert!(message.session_entry.is_none());
    }

    #[test]
    fn card_scope_changes_on_sender_connection_or_selection_generation() {
        let binding = SenderBinding {
            selected: AtomicBool::new(true),
            ui_epoch: AtomicU64::new(0),
            nonce: "run".into(),
            sessions: Mutex::new(SenderSessions::default()),
        };
        let config = GmClawBridgeConfig::default();
        let mut message: InboundMessage = serde_json::from_value(serde_json::json!({"accountId":"a","senderId":"s","chatId":"c","chatType":"direct","messageId":"m","text":"","mentioned":true})).unwrap();
        let original = session_scope(&binding, &config, &message);
        message.sender_id = "other".into();
        assert_ne!(original, session_scope(&binding, &config, &message));
        message.sender_id = "s".into();
        binding.ui_epoch.fetch_add(1, Ordering::AcqRel);
        assert_ne!(original, session_scope(&binding, &config, &message));
        let mut changed = config.clone();
        changed.auth_token = "new".into();
        assert_ne!(
            session_scope(&binding, &config, &message),
            session_scope(&binding, &changed, &message)
        );
        let mut legacy = config.clone();
        legacy.project_path = "other-default".into();
        assert_eq!(
            session_scope(&binding, &config, &message),
            session_scope(&binding, &legacy, &message)
        );
    }
}
