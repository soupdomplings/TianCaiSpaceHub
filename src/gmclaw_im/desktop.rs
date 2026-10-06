//! Real desktop task bindings and public metadata for the shared session flow.
use super::*;
use crate::gmclaw_desktop::{
    DesktopClient, DesktopSessionEvent, DesktopSessionMetadata, DesktopTask,
};
use crate::gmclaw_executor::MAX_SESSION_STEPS;
use anyhow::Context;
use serde_json::json;
use std::{
    io::{Read, Write},
    path::Path,
};

const MAX_LINK_BYTES: u64 = 8 * 1024 * 1024;
const MAX_OWNED_REPLY_IDS: usize = 128;
const MAX_SAFE_MESSAGE_ID: i64 = 9_007_199_254_740_991;
static LINK_IO: std::sync::LazyLock<Arc<Mutex<()>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(())));

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct DesktopContext {
    pub task_id: String,
    pub user_id: String,
    pub project: GmClawProject,
    pub model_name: String,
    pub max_steps: u32,
    // IDs come only from this Hub's confirmed official system-message insert.
    // Keep display ownership separate from message content and task execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owned_reply_ids: Vec<i64>,
}

impl DesktopContext {
    pub(super) fn remember_owned_reply(&mut self, message_id: i64) {
        if !(1..=MAX_SAFE_MESSAGE_ID).contains(&message_id)
            || self.owned_reply_ids.contains(&message_id)
        {
            return;
        }
        self.owned_reply_ids.push(message_id);
        if self.owned_reply_ids.len() > MAX_OWNED_REPLY_IDS {
            self.owned_reply_ids.remove(0);
        }
    }
}

fn valid_owned_reply_ids(ids: &[i64]) -> bool {
    ids.len() <= MAX_OWNED_REPLY_IDS
        && ids.iter().all(|id| (1..=MAX_SAFE_MESSAGE_ID).contains(id))
        && ids.iter().collect::<std::collections::HashSet<_>>().len() == ids.len()
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct SessionLink {
    pub session_id: String,
    pub title: String,
    pub model_id: Option<String>,
    pub config_fingerprint: String,
    pub pending: bool,
    pub uncertain: bool,
    pub context: DesktopContext,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct LinkFile {
    version: u32,
    links: Vec<SessionLink>,
}

fn read_links(path: &Path) -> Result<LinkFile> {
    if !path.exists() {
        return Ok(LinkFile {
            version: 1,
            ..Default::default()
        });
    }
    let mut file =
        std::fs::File::open(path).map_err(|_| anyhow::anyhow!("无法读取天工会话关联记录"))?;
    ensure!(
        file.metadata()?.len() <= MAX_LINK_BYTES,
        "天工会话关联记录超出读取范围"
    );
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_LINK_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_LINK_BYTES,
        "天工会话关联记录超出读取范围"
    );
    let file: LinkFile = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("天工会话关联记录格式无效，请先保留备份"))?;
    ensure!(
        file.version == 1 && file.links.len() <= 20_000,
        "天工会话关联记录版本或数量无效"
    );
    ensure!(
        file.links
            .iter()
            .all(|link| valid_owned_reply_ids(&link.context.owned_reply_ids)),
        "天工会话关联记录的回复行标识无效，请先保留备份"
    );
    Ok(file)
}

pub(super) async fn links(state: &SharedState) -> Result<Vec<SessionLink>> {
    let path = state.config_path.with_extension("gmclaw-sessions.json");
    tokio::time::timeout(Duration::from_secs(5), async move {
        let guard = LINK_IO.clone().lock_owned().await;
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            read_links(&path)
        })
        .await
    })
    .await
    .context("读取天工会话关联记录超时")?
    .context("无法读取天工会话关联记录")?
    .map(|file| file.links)
}

pub(super) async fn save(state: &SharedState, session: &Session) -> Result<()> {
    let Some(context) = session.desktop.clone() else {
        return Ok(());
    };
    ensure!(
        valid_owned_reply_ids(&context.owned_reply_ids),
        "天工会话关联记录的回复行标识超出保存范围"
    );
    let link = SessionLink {
        session_id: session.id.clone(),
        title: session.title.clone(),
        model_id: session.model_id.clone(),
        config_fingerprint: session.config_fingerprint.clone(),
        pending: session.pending.is_some(),
        uncertain: session.uncertain,
        context,
    };
    let path = state.config_path.with_extension("gmclaw-sessions.json");
    tokio::time::timeout(Duration::from_secs(5), async move {
        let guard = LINK_IO.clone().lock_owned().await;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let _guard = guard;
            let mut file = read_links(&path)?;
            if let Some(existing) = file
                .links
                .iter_mut()
                .find(|old| old.session_id == link.session_id)
            {
                *existing = link;
            } else {
                ensure!(file.links.len() < 20_000, "天工会话关联数量已达上限");
                file.links.push(link);
            }
            let raw = serde_json::to_vec_pretty(&file)?;
            ensure!(
                raw.len() as u64 <= MAX_LINK_BYTES,
                "天工会话关联记录超出保存范围"
            );
            let parent = path.parent().context("天工会话关联目录无效")?;
            std::fs::create_dir_all(parent)?;
            let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
            temporary.write_all(&raw)?;
            temporary.as_file().sync_all()?;
            temporary
                .persist(&path)
                .map_err(|_| anyhow::anyhow!("天工会话关联记录保存失败"))?;
            Ok(())
        })
        .await
    })
    .await
    .context("保存天工会话关联记录超时；请核对记录")?
    .context("无法保存天工会话关联记录")?
}

pub(super) async fn client(config: &GmClawBridgeConfig) -> Result<DesktopClient> {
    let status = crate::gmclaw_runtime::status(config).await;
    ensure!(
        status.state == crate::gmclaw_runtime::GmClawConnectionState::Connected,
        "{}；天工启动后会自动恢复连接，请稍后重试",
        status.detail
    );
    DesktopClient::with_authorization(
        &config.endpoint,
        crate::gmclaw_runtime::connection_authorization(config).await?,
    )
}

async fn models() -> Result<Vec<(String, String, bool)>> {
    tokio::time::timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking(crate::gmclaw_config::session_model_metadata),
    )
    .await
    .context("读取天工会话模型超时")?
    .context("读取天工会话模型失败")?
}

pub(super) async fn register(
    client: &DesktopClient,
    config: &GmClawBridgeConfig,
    session: &mut Session,
) -> Result<()> {
    let model = session.model_id.as_deref().context("天工会话未选择模型")?;
    let model_name = models()
        .await?
        .into_iter()
        .find(|row| row.0 == model)
        .context("所选天工模型已不存在")?
        .1;
    let task_id = format!("tiancaispacehub-{}", uuid::Uuid::new_v4());
    session.desktop = Some(DesktopContext {
        task_id: task_id.clone(),
        user_id: "local".into(),
        model_name: model_name.clone(),
        max_steps: config.max_steps,
        owned_reply_ids: Vec::new(),
        project: GmClawProject {
            id: format!("pending-{}", session.id),
            name: "TianCaiSpaceHub".into(),
            path: session.project_path.clone(),
        },
    });
    let task = client
        .create_task(&session.project_path, &session.title, &session.id, &task_id)
        .await?;
    let project = project_for(&task, &session.project_path);
    let metadata = DesktopSessionMetadata {
        session_id: session.id.clone(),
        title: session.title.clone(),
        model_name: model_name.clone(),
        project_id: project.id.clone(),
        max_steps: config.max_steps,
        extra_data: json!({"desktop":true,"project":project}),
        ..Default::default()
    };
    session.desktop = Some(DesktopContext {
        task_id,
        user_id: "local".into(),
        project,
        model_name,
        max_steps: config.max_steps,
        owned_reply_ids: Vec::new(),
    });
    client.create_session_metadata(&metadata).await?;
    Ok(())
}

fn project_for(task: &DesktopTask, fallback: &str) -> GmClawProject {
    GmClawProject {
        id: if task.scenario_id == "default" {
            "0".into()
        } else {
            task.scenario_id.clone()
        },
        name: task.scenario_name.clone(),
        path: if task.work_dir.is_empty() {
            fallback.to_owned()
        } else {
            task.work_dir.clone()
        },
    }
}

pub(super) fn native_workspace() -> String {
    crate::gmclaw_config::native_config_path()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("workspace")
        .to_string_lossy()
        .into_owned()
}

fn safe_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.chars().any(char::is_control)
        && !value.contains(['/', '\\'])
        && value != "."
        && value != ".."
}

pub(super) async fn restore(
    state: &SharedState,
    config: &GmClawBridgeConfig,
    client: &DesktopClient,
    task: DesktopTask,
) -> Result<Session> {
    let link = links(state)
        .await?
        .into_iter()
        .find(|link| link.session_id == task.session_id);
    if let Some(link) = &link {
        ensure!(
            !link.pending && !link.uncertain,
            "此会话有未确认的 IM 执行或审批，请先到天工桌面核对；本次未恢复或重放"
        );
        ensure!(
            link.config_fingerprint == config.fingerprint(),
            "此会话的 Hub 连接设置已变化，请新建会话"
        );
    }
    let mut metadata = client.get_session_metadata(&task.session_id).await?;
    if let Some(current) = metadata.take() {
        metadata = Some(checked_idle(client, current).await?);
    }
    let owned_reply_ids = if let Some(link) = link
        .as_ref()
        .filter(|link| !link.context.owned_reply_ids.is_empty())
    {
        ensure!(
            link.context.task_id == task.task_id
                && metadata.as_ref().is_some_and(|metadata| {
                    metadata.session_id == task.session_id
                        && metadata.user_id == link.context.user_id
                        && metadata.project_id == link.context.project.id
                }),
            "原会话任务或记忆身份与 Hub 关联记录不一致，未恢复"
        );
        link.context.owned_reply_ids.clone()
    } else {
        Vec::new()
    };
    let catalog = models().await?;
    let model_id = if let Some(link) = &link {
        link.model_id.clone().context("原会话没有模型记录")?
    } else if let Some(metadata) = metadata
        .as_ref()
        .filter(|metadata| !metadata.model_name.is_empty())
    {
        let matches: Vec<_> = catalog
            .iter()
            .filter(|row| row.1 == metadata.model_name)
            .collect();
        ensure!(
            matches.len() == 1,
            "历史模型名称无法唯一匹配天工配置，请在天工确认对应模型后重新选择会话；不会替换为默认模型"
        );
        matches[0].0.clone()
    } else {
        ensure!(
            client
                .messages(&task.task_id, None)
                .await?
                .messages
                .is_empty(),
            "此非空历史会话缺少可确认的模型记录，未替换为默认模型；请在天工核对"
        );
        catalog
            .iter()
            .find(|row| row.2)
            .context("此空会话没有模型，请先在天工设置默认模型")?
            .0
            .clone()
    };
    let model_name = catalog
        .iter()
        .find(|row| row.0 == model_id)
        .context("原会话绑定模型已不可用")?
        .1
        .clone();
    let default_path = if task.scenario_id == "default" {
        native_workspace()
    } else {
        String::new()
    };
    let mut project = metadata
        .as_ref()
        .and_then(|metadata| {
            serde_json::from_value::<GmClawProject>(metadata.extra_data.get("project")?.clone())
                .ok()
        })
        .or_else(|| link.as_ref().map(|link| link.context.project.clone()))
        .unwrap_or_else(|| project_for(&task, &default_path));
    if let Some(metadata) = &metadata {
        if !metadata.project_id.is_empty() {
            project.id = metadata.project_id.clone();
        }
    }
    let user_id = metadata
        .as_ref()
        .map(|metadata| metadata.user_id.clone())
        .filter(|id| !id.is_empty())
        .or_else(|| link.as_ref().map(|link| link.context.user_id.clone()))
        .unwrap_or_else(|| "local".into());
    let max_steps = metadata
        .as_ref()
        .map(|metadata| metadata.max_steps)
        .unwrap_or(config.max_steps);
    ensure!(
        (1..=MAX_SESSION_STEPS).contains(&max_steps),
        "原会话执行步数超出支持范围（1–1000），未恢复"
    );
    ensure!(
        safe_identity(&user_id) && safe_identity(&project.id),
        "原会话用户或项目身份无效，未恢复"
    );
    ensure!(
        Path::new(&project.path).is_absolute()
            && tokio::fs::metadata(&project.path)
                .await
                .is_ok_and(|metadata| metadata.is_dir()),
        "原项目目录不可用，未恢复会话，请恢复目录或新建会话"
    );
    let restored = Session {
        id: task.session_id,
        title: task.title,
        project_path: project.path.clone(),
        model_id: Some(model_id),
        config_fingerprint: config.fingerprint(),
        pending: None,
        uncertain: false,
        desktop: Some(DesktopContext {
            task_id: task.task_id,
            user_id,
            project,
            model_name,
            max_steps,
            owned_reply_ids,
        }),
    };
    if let Some(context) = &restored.desktop {
        if context.owned_reply_ids.is_empty() {
            crate::gmclaw_runtime::queue_desktop_refresh(&context.task_id, &restored.id, None)
                .await;
        } else {
            for id in &context.owned_reply_ids {
                crate::gmclaw_runtime::queue_desktop_refresh(
                    &context.task_id,
                    &restored.id,
                    Some(*id),
                )
                .await;
            }
        }
    }
    Ok(restored)
}

pub(super) async fn claim(
    state: &SharedState,
    binding: &SenderBinding,
    session_id: &str,
) -> Result<()> {
    let mut claims = state.gmclaw_im.desktop_claims.lock().await;
    ensure!(
        claims
            .get(session_id)
            .is_none_or(|owner| owner == &binding.nonce),
        "此天工会话已由另一个 IM 发送者使用，请先让其退出会话"
    );
    claims.insert(session_id.to_owned(), binding.nonce.clone());
    Ok(())
}

pub(super) async fn release(
    state: &SharedState,
    binding: &SenderBinding,
    session: Option<&Session>,
) {
    if let Some(session) = session {
        let mut claims = state.gmclaw_im.desktop_claims.lock().await;
        if claims.get(&session.id) == Some(&binding.nonce) {
            claims.remove(&session.id);
        }
    }
}

pub(super) async fn instance_identity(config: &GmClawBridgeConfig) -> Result<String> {
    let header = crate::gmclaw_runtime::connection_authorization(config).await?;
    let process_ids = crate::gmclaw_runtime::desktop_process_ids().await;
    let authorization = crate::gmclaw_runtime::runtime_authorization_identity(&header);
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(
        authorization,
        process_ids,
    ))?)))
}

pub(super) async fn validate_live(
    client: &DesktopClient,
    session: &Session,
    approving: bool,
) -> Result<()> {
    let context = session
        .desktop
        .as_ref()
        .context("此旧会话尚未绑定天工桌面，请新建真实桌面会话")?;
    let task = client
        .task(&context.task_id)
        .await?
        .context("此会话已从天工桌面移除，请重新选择会话")?;
    ensure!(
        task.session_id == session.id,
        "天工桌面任务已更换会话，请重新选择"
    );
    let mut metadata = client
        .get_session_metadata(&session.id)
        .await?
        .context("天工桌面会话记录已移除")?;
    if approving {
        ensure!(
            matches!(
                metadata.status.as_str(),
                "active" | "completed" | "terminated" | "paused" | "awaiting_confirmation"
            ),
            "天工桌面正在执行此会话或状态尚未确认，请等待完成"
        );
    } else {
        metadata = checked_idle(client, metadata).await?;
    }
    ensure!(
        metadata.user_id == context.user_id && metadata.project_id == context.project.id,
        "天工会话记忆身份已变化，请退出当前会话并重新选择"
    );
    Ok(())
}

/// The native renderer aborts its SSE on `over`, before Harness writes the
/// completed status. A persisted `processing` flag is not a live-task query.
/// Require a complete last turn with no approval, then observe the same stored
/// turn twice. This does not lock out future desktop submissions.
async fn checked_idle(
    client: &DesktopClient,
    metadata: DesktopSessionMetadata,
) -> Result<DesktopSessionMetadata> {
    let events = client.get_session_events(&metadata.session_id).await?;
    let stale_processing = idle_evidence(&metadata, &events)?;
    if stale_processing {
        // SQLite timestamps only have second precision. Recheck after that
        // interval to avoid declaring a just-written terminal frame stable.
        tokio::time::sleep(Duration::from_millis(1100)).await;
        let latest = client
            .get_session_metadata(&metadata.session_id)
            .await?
            .context("天工会话记录已变化，请刷新历史列表")?;
        let latest_events = client.get_session_events(&metadata.session_id).await?;
        idle_evidence(&latest, &latest_events)?;
        ensure!(
            latest == metadata && latest_events == events,
            "天工会话状态正在变化，请稍后重新选择；本次未绑定或提交任务"
        );
        return Ok(latest);
    }
    Ok(metadata)
}

/// Returns true only for a finished turn with a stale `processing` flag.
fn idle_evidence(
    metadata: &DesktopSessionMetadata,
    events: &[DesktopSessionEvent],
) -> Result<bool> {
    ensure!(
        metadata.status != "awaiting_confirmation",
        "此天工会话正在等待桌面工具审批，请先在天工处理"
    );
    ensure!(
        matches!(
            metadata.status.as_str(),
            "active" | "processing" | "paused" | "completed" | "terminated"
        ),
        "天工会话仍在执行或状态未知，请稍后重新选择"
    );
    ensure!(
        metadata.message_count == events.len() as u64,
        "天工会话事件数量与保存状态不一致，请稍后重新选择；本次未绑定或提交任务"
    );
    let last_user = events.iter().rev().find(|event| event.event_type == "user");
    let last_user_id = last_user.map_or(0, |event| event.id);
    let last_confirm_id = events
        .iter()
        .filter(|event| event.event_type == "confirm")
        .map(|event| event.id)
        .max()
        .unwrap_or(0);
    ensure!(
        last_confirm_id <= last_user_id,
        "此天工会话正在等待桌面工具审批，请先在天工处理；不会从 IM 重建审批"
    );
    if events.is_empty() {
        ensure!(
            metadata.status == "active" && metadata.message_count == 0,
            "天工会话缺少可确认的结束记录，请先在桌面核对"
        );
        return Ok(false);
    }
    let last = events.last().context("天工会话没有结束记录")?;
    ensure!(
        last_user.is_some_and(|event| event.source == "harness_sidecar")
            && last.source == "harness_sidecar"
            && last.event_type == "over"
            && last.id > last_user_id,
        "天工会话最后一轮尚未确认结束，请等待完成后重新选择；本次未绑定或提交任务"
    );
    if metadata.status == "processing" {
        ensure!(
            sqlite_timestamp(&metadata.updated_at)
                && sqlite_timestamp(&last.created_at)
                && metadata.updated_at <= last.created_at,
            "天工会话有较新的执行状态，暂不能确认结束，请稍后重新选择"
        );
        ensure!(
            metadata
                .extra_data
                .get("chat_id")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| !value.is_empty()),
            "天工会话缺少当前轮次身份，请先在桌面核对"
        );
        return Ok(true);
    }
    Ok(false)
}

fn sqlite_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 19
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b' '
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7 | 10 | 13 | 16) || byte.is_ascii_digit())
        && ("01"..="12").contains(&&value[5..7])
        && ("01"..="31").contains(&&value[8..10])
        && ("00"..="23").contains(&&value[11..13])
        && ("00"..="59").contains(&&value[14..16])
        && ("00"..="59").contains(&&value[17..19])
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    fn metadata(status: &str, message_count: u64) -> DesktopSessionMetadata {
        DesktopSessionMetadata {
            session_id: "session".into(),
            status: status.into(),
            updated_at: "2026-10-06 04:00:00".into(),
            message_count,
            extra_data: json!({"chat_id":"turn-1"}),
            ..Default::default()
        }
    }

    fn event(id: i64, kind: &str) -> DesktopSessionEvent {
        DesktopSessionEvent {
            id,
            session_id: "session".into(),
            role: if kind == "user" { "user" } else { "assistant" }.into(),
            event_type: kind.into(),
            source: "harness_sidecar".into(),
            created_at: "2026-10-06 04:00:00".into(),
        }
    }

    #[test]
    fn completed_native_turn_with_stale_processing_needs_stable_observation() {
        let events = [event(1, "user"), event(2, "over")];
        assert!(idle_evidence(&metadata("processing", 2), &events).unwrap());
        assert!(!idle_evidence(&metadata("completed", 2), &events).unwrap());
        let mut newer = metadata("processing", 2);
        newer.updated_at = "2026-10-06 04:00:01".into();
        assert!(idle_evidence(&newer, &events).is_err());
    }

    #[test]
    fn approval_over_does_not_allow_recovery() {
        let events = [event(1, "user"), event(2, "confirm"), event(3, "over")];
        for status in ["processing", "completed", "awaiting_confirmation"] {
            assert!(idle_evidence(&metadata(status, 3), &events).is_err());
        }
    }

    #[test]
    fn approval_resolution_requires_a_complete_later_turn() {
        let mut events = vec![event(1, "user"), event(2, "confirm"), event(3, "over")];
        events.push(event(4, "user"));
        assert!(idle_evidence(&metadata("completed", 4), &events).is_err());
        events.push(event(5, "over"));
        assert!(!idle_evidence(&metadata("completed", 5), &events).unwrap());
        events.push(event(6, "user"));
        events.push(event(7, "confirm"));
        events.push(event(8, "over"));
        assert!(idle_evidence(&metadata("completed", 8), &events).is_err());
    }

    #[test]
    fn running_unknown_and_incomplete_turns_stay_blocked() {
        let events = [event(1, "user"), event(2, "over")];
        for status in ["running", "", "unrecognized"] {
            assert!(idle_evidence(&metadata(status, 2), &events).is_err());
        }
        assert!(idle_evidence(&metadata("completed", 1), &[event(1, "user")]).is_err());
        assert!(idle_evidence(&metadata("completed", 1), &events).is_err());
        let mut missing = metadata("processing", 2);
        missing.updated_at.clear();
        assert!(idle_evidence(&missing, &events).is_err());
        missing = metadata("processing", 2);
        missing.extra_data = json!({});
        assert!(idle_evidence(&missing, &events).is_err());
    }

    #[test]
    fn empty_registered_task_stays_available() {
        let empty = metadata("active", 0);
        assert!(!idle_evidence(&empty, &[]).unwrap());
        assert!(idle_evidence(&metadata("processing", 0), &[]).is_err());
    }
}

pub(super) async fn ensure_registered(client: &DesktopClient, session: &Session) -> Result<()> {
    if client.get_session_metadata(&session.id).await?.is_some() {
        return Ok(());
    }
    let context = session.desktop.as_ref().context("缺少天工桌面会话关联")?;
    ensure!(
        client
            .messages(&context.task_id, None)
            .await?
            .messages
            .is_empty(),
        "此非空桌面会话缺少原记忆记录，未创建替代会话"
    );
    client
        .create_session_metadata(&DesktopSessionMetadata {
            session_id: session.id.clone(),
            title: session.title.clone(),
            user_id: context.user_id.clone(),
            project_id: context.project.id.clone(),
            model_name: context.model_name.clone(),
            max_steps: context.max_steps,
            extra_data: json!({"desktop":true,"project":context.project}),
            ..Default::default()
        })
        .await
}

pub(super) struct TurnRecorder {
    client: DesktopClient,
    task_id: String,
    pub message_id: i64,
    content: String,
    last_save: Instant,
    pub failed: bool,
}

impl TurnRecorder {
    pub fn new(client: DesktopClient, task_id: String, message_id: i64) -> Self {
        Self {
            client,
            task_id,
            message_id,
            content: String::new(),
            last_save: Instant::now(),
            failed: false,
        }
    }

    pub async fn event(&mut self, event: crate::gmclaw_executor::GmClawEvent) {
        use crate::gmclaw_executor::GmClawEvent;
        let immediate = match event {
            GmClawEvent::Text(text) => {
                if self.content.len().saturating_add(text.len()) <= 4 * 1024 * 1024 {
                    self.content.push_str(&text);
                } else {
                    self.failed = true;
                }
                false
            }
            GmClawEvent::Confirmation(pending) => {
                self.content = format!("{}\n（等待确认：{}）", self.content, pending.prompt);
                true
            }
            GmClawEvent::Finished { answer, error } => {
                if !answer.is_empty() {
                    self.content = answer;
                }
                if let Some(error) = error {
                    self.content.push_str(&format!("\n天工返回错误：{error}"));
                }
                true
            }
            GmClawEvent::Progress { .. } => false,
        };
        if immediate || self.last_save.elapsed() >= Duration::from_secs(1) {
            self.last_save = Instant::now();
            if self
                .client
                .update_message(&self.task_id, self.message_id, &self.content, &[])
                .await
                .is_err()
            {
                self.failed = true;
            }
        }
    }

    pub async fn finish(&mut self, content: &str) -> Result<()> {
        self.content = content.to_owned();
        self.client
            .update_message(&self.task_id, self.message_id, &self.content, &[])
            .await?;
        self.failed = false;
        Ok(())
    }
}
