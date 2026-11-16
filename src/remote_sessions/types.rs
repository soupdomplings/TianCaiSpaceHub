use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(super) struct Scope {
    pub client: String,
    pub session: String,
}
impl Scope {
    pub fn new(client: impl Into<String>, session: impl Into<String>) -> Self {
        Self {
            client: client.into(),
            session: session.into(),
        }
    }
    pub fn file_url(&self, id: &str) -> String {
        format!(
            "/api/remote/v1/clients/{}/sessions/{}/files/{}",
            encode(&self.client),
            encode(&self.session),
            encode(id)
        )
    }
}
pub(super) fn encode(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RemoteEvent {
    pub id: u64,
    #[serde(rename = "type")]
    pub kind: String,
    pub client_id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    pub data: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RemoteFile {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub mime_type: String,
    pub url: String,
    pub kind: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CreateRequest {
    pub title: Option<String>,
    pub project_id: Option<String>,
    pub cwd: Option<String>,
    pub model: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TurnRequest {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub attachment_ids: Vec<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub request_id: String,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StopRequest {
    pub turn_id: Option<String>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ApprovalDecision {
    pub decision: String,
    #[serde(default)]
    pub answers: Value,
}
#[derive(Default, Deserialize)]
pub(super) struct PageQuery {
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}
impl PageQuery {
    pub fn limit(&self) -> usize {
        self.limit.unwrap_or(50).clamp(1, 100)
    }
    pub fn offset(&self) -> anyhow::Result<usize> {
        self.cursor
            .as_deref()
            .map(str::parse::<usize>)
            .transpose()
            .map(|v| v.unwrap_or(0))
            .map_err(|_| anyhow::anyhow!("分页位置无效，请刷新会话列表"))
    }
}
pub(super) fn busy(status: &str) -> bool {
    matches!(
        status,
        "starting"
            | "running"
            | "stopping"
            | "waiting_approval"
            | "waiting_confirmation"
            | "unknown"
    )
}
