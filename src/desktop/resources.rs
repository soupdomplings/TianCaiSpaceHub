use super::{
    clients::ClientId,
    mcp::{self, McpServer},
    skills::{self, SkillPackage},
    targets::{self, ClientTarget, McpFormat},
    transaction::{self, FileChange, TransactionStore},
};
use crate::nvwa::secrets::SecretStore;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
#[cfg(test)]
#[path = "resources/maintenance_tests.rs"]
mod maintenance_tests;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    Skill,
    Mcp,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub id: String,
    pub name: String,
    pub description: String,
    pub kind: ResourceKind,
    pub fingerprint: String,
    pub folder: String,
    pub source: String,
    #[serde(default)]
    pub secret_ref: Option<String>,
    #[serde(default)]
    pub skill_folders: BTreeMap<String, String>,
}
impl Resource {
    pub(super) fn skill_folder(&self, target: &ClientTarget) -> Result<String> {
        let key = if cfg!(windows) {
            target.skills.to_string_lossy().to_lowercase()
        } else {
            target.skills.to_string_lossy().into_owned()
        };
        let name = self.skill_folders.get(&key).unwrap_or(&self.folder);
        ensure!(
            std::path::Path::new(name).components().count() == 1
                && !name.contains(['/', '\\', ':'])
                && name != "."
                && name != "..",
            "Skill 目录身份无效"
        );
        Ok(name.clone())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedFile {
    pub path: PathBuf,
    pub fingerprint: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UseRecord {
    pub resource_id: String,
    pub target_id: String,
    pub location: PathBuf,
    pub files: Vec<OwnedFile>,
    pub entry_fingerprint: Option<String>,
    pub undo: String,
    #[serde(default)]
    pub pending: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub version: u32,
    #[serde(skip)]
    pub revision: String,
    pub resources: Vec<Resource>,
    pub uses: Vec<UseRecord>,
    #[serde(default)]
    pub target_overrides: Vec<ClientTarget>,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            version: 1,
            revision: "missing".into(),
            resources: vec![],
            uses: vec![],
            target_overrides: vec![],
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetUse {
    pub target_id: String,
    pub client: ClientId,
    pub label: String,
    pub using: Option<bool>,
    pub error: Option<String>,
    pub shared_with: Vec<String>,
    #[serde(default)]
    pub managed: bool,
    #[serde(default = "default_compatible")]
    pub compatible: bool,
}
fn default_compatible() -> bool {
    true
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceView {
    pub resource: Resource,
    pub targets: Vec<TargetUse>,
}
impl ResourceView {
    /// A client's default page shows its actual use and owned configurations
    /// requiring attention. Available items are an explicit, compatible view.
    pub fn visible_for_client(&self, client: ClientId, available: bool) -> bool {
        self.targets.iter().any(|t| {
            t.client == client
                && if available {
                    t.compatible && t.using != Some(true) && !t.managed
                } else {
                    t.using == Some(true) || t.managed
                }
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryView {
    pub revision: String,
    pub resources: Vec<ResourceView>,
    pub targets: Vec<ClientTarget>,
    #[serde(default)]
    pub recoveries: Vec<Value>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryIssue {
    pub client: ClientId,
    pub kind: ResourceKind,
    pub name: String,
    pub location: PathBuf,
    pub reason: String,
    pub guidance: String,
}
impl DiscoveryIssue {
    fn new(
        target: &ClientTarget,
        kind: ResourceKind,
        location: PathBuf,
        error: &anyhow::Error,
    ) -> Self {
        let reason = error.to_string();
        let guidance = if kind == ResourceKind::Skill && reason.contains("64 MiB") {
            "这个 Skill 包超过 Hub 的 64 MiB 或 2048 个文件限制，暂不能跨端复制。原客户端仍可继续使用。可打开文件夹核对是否选中了整个仓库，或把需要共享的 Skill 整理成独立小包后重新导入。Hub 不会自动删减文件。"
        } else if reason.starts_with("SKILL.md 过大") {
            "这个 Skill 的说明文件超过 256 KiB，暂未读取。打开文件夹核对 SKILL.md，将长篇参考材料整理为独立附件后再导入；原文件不会自动删减。"
        } else if reason.contains("过大") || reason.contains("32 MiB") {
            "该文件超过 Hub 的 32 MiB 单文件读取限制，已跳过且未修改。打开文件夹核对是否为正确配置文件；如路径正确且客户端仍能使用，可继续在原客户端管理此项。不要为消除提示直接删除配置。"
        } else if reason.contains("符号") || reason.contains("链接") {
            "此位置包含链接，Hub 无法安全复制。原客户端内容保持；如需跨端使用，请导入不含链接的独立副本。"
        } else {
            "原文件未改动。打开文件夹核对文件是否存在、客户端能否读取及是否有访问权限；修正后点击重新读取。仍无法读取时，可继续在原客户端管理，不必删除现有配置。"
        }.to_string();
        let name = location
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| target.label.clone());
        Self {
            client: target.client,
            kind,
            name,
            location,
            reason,
            guidance,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UseOutcome {
    pub target_id: String,
    pub success: bool,
    pub using: Option<bool>,
    pub message: String,
    pub undo: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionChange {
    pub target_id: String,
    pub using: bool,
}

pub fn validate_selection(
    view: &LibraryView,
    resource_id: &str,
    revision: &str,
    changes: &[SelectionChange],
) -> Result<()> {
    ensure!(
        view.revision == revision,
        "客户端配置已变化，请返回列表刷新后重新选择"
    );
    ensure!(
        !changes.is_empty() && changes.len() <= 128,
        "请选择需要更改的客户端"
    );
    let row = view
        .resources
        .iter()
        .find(|r| r.resource.id == resource_id)
        .context("此项已变化，请返回列表刷新")?;
    let mut seen = std::collections::BTreeSet::new();
    let mut shared = BTreeMap::new();
    for change in changes {
        ensure!(seen.insert(&change.target_id), "客户端选择重复，请重新选择");
        let target = row
            .targets
            .iter()
            .find(|t| t.target_id == change.target_id)
            .context("客户端配置位置已变化，请刷新后重新选择")?;
        ensure!(
            target.compatible && target.using == Some(!change.using),
            "{} 的使用状态已变化或尚未读取，请刷新后重新选择",
            target.client.name()
        );
        if row.resource.kind == ResourceKind::Skill {
            let native = view
                .targets
                .iter()
                .find(|t| t.id == change.target_id)
                .context("客户端位置已移除")?;
            let old = shared.insert(&native.skills, change.using);
            ensure!(
                old.is_none_or(|value| value == change.using),
                "共享 Skill 文件夹不能同时选择使用和停用"
            );
        }
    }
    Ok(())
}

pub struct Library {
    pub root: PathBuf,
    pub(super) transactions: TransactionStore,
    pub(super) secrets: SecretStore,
}

impl Library {
    pub fn new(config_path: &Path) -> Result<Self> {
        let root = super::state_root(config_path);
        Ok(Self {
            transactions: TransactionStore::new(root.clone())?,
            secrets: SecretStore::new(root.join("resource-secrets"))?,
            root,
        })
    }
    pub fn load(&self) -> Result<Catalog> {
        let raw = transaction::read(&self.root.join("library.json"))?;
        let mut catalog = match &raw {
            Some(raw) => serde_json::from_slice::<Catalog>(raw)
                .context("管理记录无法读取，已保留原文件，请从设置恢复")?,
            None => Catalog::default(),
        };
        ensure!(catalog.version == 1, "管理记录版本较新，请使用新版 Hub");
        catalog.revision = transaction::fingerprint(raw.as_deref());
        Ok(catalog)
    }
    pub fn targets(&self, catalog: &Catalog) -> Result<Vec<ClientTarget>> {
        let mut result = targets::discover_for_library(&self.root)?;
        for custom in &catalog.target_overrides {
            transaction::reject_links(&custom.root)?;
            transaction::reject_links(&custom.skills)?;
            transaction::reject_links(&custom.mcp)?;
            transaction::reject_links(custom.models_root())?;
            result.retain(|t| t.id != custom.id);
            result.push(custom.clone());
        }
        Ok(result)
    }
    pub(super) fn catalog_change(&self, catalog: &Catalog) -> Result<FileChange> {
        Ok(FileChange {
            path: self.root.join("library.json"),
            expected: catalog.revision.clone(),
            after: Some(serde_json::to_vec_pretty(catalog)?),
        })
    }
    pub fn save_targets(&self, mut catalog: Catalog, overrides: Vec<ClientTarget>) -> Result<()> {
        ensure!(overrides.len() <= 128, "客户端目标过多");
        for target in &overrides {
            transaction::reject_links(&target.root)?;
            transaction::reject_links(&target.skills)?;
            transaction::reject_links(&target.mcp)?;
            transaction::reject_links(target.models_root())?;
        }
        catalog.target_overrides = overrides;
        self.transactions
            .apply(vec![self.catalog_change(&catalog)?])?;
        Ok(())
    }
    fn protect(&self, bytes: &[u8]) -> Result<String> {
        ensure!(bytes.len() <= 1024 * 1024, "配置快照过大");
        let key = uuid::Uuid::new_v4().to_string();
        let count = bytes.len().div_ceil(48 * 1024);
        for (i, chunk) in bytes.chunks(48 * 1024).enumerate() {
            self.secrets.set(&format!("resource:{key}:{i}"), chunk)?;
        }
        self.secrets
            .set(&format!("resource:{key}"), count.to_string().as_bytes())?;
        Ok(key)
    }
    fn unprotect(&self, key: &str) -> Result<Vec<u8>> {
        ensure!(uuid::Uuid::parse_str(key).is_ok(), "配置引用无效");
        let count = self
            .secrets
            .get(&format!("resource:{key}"))?
            .context("系统保护配置不存在")?;
        let count: usize = std::str::from_utf8(&count)?.parse()?;
        ensure!(count > 0 && count <= 22, "系统保护配置损坏");
        let mut bytes = Vec::new();
        for i in 0..count {
            bytes.extend(
                self.secrets
                    .get(&format!("resource:{key}:{i}"))?
                    .context("系统保护配置不完整")?,
            );
        }
        Ok(bytes)
    }
    pub fn mcp_config(&self, resource: &Resource) -> Result<McpServer> {
        ensure!(resource.kind == ResourceKind::Mcp, "所选内容不是 MCP");
        let bytes = self.unprotect(resource.secret_ref.as_deref().context("MCP 保护引用丢失")?)?;
        serde_json::from_slice(&bytes).context("MCP 配置无法恢复，请重新编辑")
    }
    pub fn bridge_token(&self, id: &str) -> Result<String> {
        ensure!(uuid::Uuid::parse_str(id).is_ok(), "MCP 身份无效");
        let key = format!("mcp-bridge:{id}");
        if let Some(value) = self.secrets.get(&key)? {
            return Ok(String::from_utf8(value)?);
        }
        let value = format!("tch-mcp-{}", uuid::Uuid::new_v4());
        self.secrets.set(&key, value.as_bytes())?;
        Ok(value)
    }
    pub fn save_mcp(
        &self,
        id: Option<&str>,
        server: McpServer,
        description: String,
        revision: &str,
    ) -> Result<Resource> {
        server.validate()?;
        ensure!(!builtin_assistance_mcp(&server), "本机协助由天工 Work → 接入设置统一管理，请在该处启用或移除");
        let mut catalog = self.load()?;
        ensure!(
            catalog.revision == revision,
            "列表已被其他操作修改，请重新读取"
        );
        let bytes = serde_json::to_vec(&server)?;
        let mut resource = Resource {
            id: id.unwrap_or("").into(),
            name: server.name.clone(),
            description,
            kind: ResourceKind::Mcp,
            fingerprint: transaction::fingerprint(Some(&bytes)),
            folder: String::new(),
            source: "MCP 配置".into(),
            secret_ref: Some(self.protect(&bytes)?),
            skill_folders: BTreeMap::new(),
        };
        if resource.id.is_empty() {
            resource.id = uuid::Uuid::new_v4().to_string();
        }
        if let Some(existing) = catalog.resources.iter_mut().find(|r| r.id == resource.id) {
            ensure!(existing.kind == ResourceKind::Mcp, "资源类型不能更改");
            ensure!(
                existing.name == resource.name
                    || !catalog.uses.iter().any(|u| u.resource_id == resource.id),
                "正在使用的 MCP 请保留名称，或添加为新的服务"
            );
            *existing = resource.clone();
        } else {
            ensure!(
                !catalog
                    .resources
                    .iter()
                    .any(|r| r.kind == ResourceKind::Mcp && r.name == resource.name),
                "已有同名 MCP，请编辑原条目"
            );
            catalog.resources.push(resource.clone());
        }
        self.transactions
            .apply(vec![self.catalog_change(&catalog)?])?;
        Ok(resource)
    }
    pub fn import_skill(
        &self,
        package: SkillPackage,
        source: String,
        revision: &str,
    ) -> Result<Resource> {
        let mut catalog = self.load()?;
        ensure!(catalog.revision == revision, "列表已变化，请重新读取");
        if let Some(existing) = catalog
            .resources
            .iter()
            .find(|r| r.kind == ResourceKind::Skill && r.fingerprint == package.fingerprint)
        {
            return Ok(existing.clone());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let folder = skills::folder_name(&package.metadata.name);
        let resource = Resource {
            id: id.clone(),
            name: package.metadata.name,
            description: package.metadata.description,
            kind: ResourceKind::Skill,
            fingerprint: package.fingerprint,
            folder,
            source,
            secret_ref: None,
            skill_folders: BTreeMap::new(),
        };
        let mut changes = Vec::new();
        for (path, bytes) in package.files {
            changes.push(FileChange::plan(
                self.root.join("skills").join(&id).join(path),
                None,
                Some(bytes),
            ));
        }
        catalog.resources.push(resource.clone());
        changes.push(self.catalog_change(&catalog)?);
        self.transactions.apply(changes)?;
        Ok(resource)
    }
    pub fn view(&self) -> Result<LibraryView> {
        self.retire_builtin_mcp_imports()?;
        self.reconcile_missing_file_mcps()?;
        let catalog = self.load()?;
        let targets = self.targets(&catalog)?;
        self.view_targets(catalog, targets)
    }
    fn retire_builtin_mcp_imports(&self) -> Result<()> {
        let mut catalog = self.load()?;
        let targets = self.targets(&catalog)?;
        let ids: std::collections::BTreeSet<_> = catalog.resources.iter().filter(|r|
            r.kind == ResourceKind::Mcp && r.name == crate::local_assistance::settings::MCP_NAME && self.mcp_config(r).is_ok_and(|s|builtin_assistance_mcp(&s))
            && !catalog.uses.iter().any(|u|u.resource_id == r.id && (u.pending || !targets.iter().any(|t|t.id == u.target_id && t.client == ClientId::Tiangong))))
            .map(|r|r.id.clone()).collect();
        if ids.is_empty() { return Ok(()); }
        // Retire only the accidental generic index, never the native connection
        // or this feature's own settings. No local token is re-exported.
        catalog.resources.retain(|r|!ids.contains(&r.id));
        catalog.uses.retain(|u|!ids.contains(&u.resource_id));
        self.transactions.apply(vec![self.catalog_change(&catalog)?])?;
        Ok(())
    }
    fn reconcile_missing_file_mcps(&self) -> Result<()> {
        let mut catalog = self.load()?;
        let targets = self.targets(&catalog)?;
        let mut absent = std::collections::BTreeSet::new();
        for record in &catalog.uses {
            if record.pending { continue; }
            let Some(resource) = catalog.resources.iter().find(|r|r.id == record.resource_id && r.kind == ResourceKind::Mcp) else { continue; };
            let Some(target) = targets.iter().find(|t|t.id == record.target_id && t.client != ClientId::Tiangong) else { continue; };
            let present = transaction::read(&target.mcp)
                .and_then(|raw|mcp::read_entries(raw.as_deref().unwrap_or_default(),target.mcp_format));
            // Malformed, inaccessible or oversized config is unknown, never a
            // deletion. Only remove our own use metadata after a complete read.
            if present.is_ok_and(|entries|!entries.contains_key(&resource.name)) {
                absent.insert((record.resource_id.clone(),record.target_id.clone()));
            }
        }
        if !absent.is_empty() {
            catalog.uses.retain(|u|!absent.contains(&(u.resource_id.clone(),u.target_id.clone())));
            self.transactions.apply(vec![self.catalog_change(&catalog)?])?;
        }
        Ok(())
    }
    pub(super) fn reconcile_native_mcp_names(&self, target_id: &str, names: &std::collections::BTreeSet<String>, revision: &str) -> Result<()> {
        let mut catalog = self.load()?;
        ensure!(catalog.revision == revision, "MCP 使用记录已变化，保留现状并等待下次读取");
        let mcps: std::collections::BTreeSet<_> = catalog.resources.iter()
            .filter(|r|r.kind == ResourceKind::Mcp && !names.contains(&r.name)).map(|r|r.id.clone()).collect();
        let prior = catalog.uses.len();
        catalog.uses.retain(|u|u.target_id != target_id || u.pending || !mcps.contains(&u.resource_id));
        if prior != catalog.uses.len() { self.transactions.apply(vec![self.catalog_change(&catalog)?])?; }
        Ok(())
    }
    fn view_targets(&self, catalog: Catalog, targets: Vec<ClientTarget>) -> Result<LibraryView> {
        let mut resources = Vec::new();
        for resource in &catalog.resources {
            let states = targets
                .iter()
                .map(|target| {
                    let managed = catalog
                        .uses
                        .iter()
                        .any(|u| u.resource_id == resource.id && u.target_id == target.id);
                    let compatible = if resource.kind == ResourceKind::Skill {
                        !(target.client == ClientId::WebStorm && target.model_mode == "native")
                    } else if target.mcp_format == McpFormat::Tiangong {
                        true
                    } else {
                        self.mcp_config(resource)
                            .map(|s| s.native(target.mcp_format).is_ok())
                            .unwrap_or(true)
                    };
                    let result = self.is_using(resource, target);
                    let shared_with = if resource.kind == ResourceKind::Skill {
                        targets
                            .iter()
                            .filter(|t| t.id != target.id && t.skills == target.skills)
                            .map(|t| format!("{} · {}", t.client.name(), t.label))
                            .collect()
                    } else {
                        vec![]
                    };
                    match result {
                        Ok(using) => TargetUse {
                            target_id: target.id.clone(),
                            client: target.client,
                            label: target.label.clone(),
                            using: Some(using),
                            error: None,
                            shared_with,
                            managed,
                            compatible,
                        },
                        Err(error) => TargetUse {
                            target_id: target.id.clone(),
                            client: target.client,
                            label: target.label.clone(),
                            using: None,
                            error: Some(error.to_string()),
                            shared_with,
                            managed,
                            compatible,
                        },
                    }
                })
                .collect();
            resources.push(ResourceView {
                resource: resource.clone(),
                targets: states,
            });
        }
        let mut recoveries = Vec::new();
        for directory in ["resource-operations", "native-recovery"] {
            if let Ok(entries) = std::fs::read_dir(self.root.join(directory)) {
                for entry in entries.take(1024).flatten() {
                    if let Ok(Some(bytes)) = transaction::read(&entry.path()) {
                        if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                            if value["phase"] != "restored" {
                                recoveries.push(value);
                            }
                        }
                    }
                }
            }
        }
        recoveries.sort_by_key(|v| std::cmp::Reverse(v["at"].as_i64().unwrap_or(0)));
        recoveries.truncate(100);
        Ok(LibraryView {
            revision: catalog.revision,
            resources,
            targets,
            recoveries,
        })
    }
    pub fn is_using(&self, resource: &Resource, target: &ClientTarget) -> Result<bool> {
        if resource.kind == ResourceKind::Skill {
            if target.client == ClientId::Tiangong {
                bail!("天工 Skill 需要同时核对本机登记状态");
            }
            if target.client == ClientId::WebStorm && target.model_mode == "native" {
                bail!("请在 WebStorm 选择实际使用的代理；原生普通聊天不加载 Skill");
            }
            let path = target.skills.join(resource.skill_folder(target)?);
            if transaction::read(&path.join("SKILL.md"))?.is_none() {
                return Ok(false);
            }
            let actual = skills::from_directory(&path)?;
            ensure!(
                actual.fingerprint == resource.fingerprint,
                "客户端已有同名但不同内容的 Skill，请核对后选择保留或替换"
            );
            return Ok(true);
        }
        if target.mcp_format == McpFormat::Tiangong {
            bail!("等待天工本机授权状态");
        }
        let raw = transaction::read(&target.mcp)?.unwrap_or_default();
        let entries = mcp::read_entries(&raw, target.mcp_format)?;
        let Some(entry) = entries.get(&resource.name) else {
            return Ok(false);
        };
        if entry.get("disabled").and_then(Value::as_bool) == Some(true)
            || entry.get("enabled").and_then(Value::as_bool) == Some(false)
            || entry.get("status").and_then(Value::as_str) == Some("disabled")
        {
            return Ok(false);
        }
        let server = self.mcp_config(resource)?;
        ensure!(
            mcp::equivalent(entry, &server.native(target.mcp_format)?),
            "客户端同名 MCP 已独立调整，请核对后选择保留或替换"
        );
        Ok(true)
    }
    pub fn use_files(
        &self,
        resource_id: &str,
        target_id: &str,
        using: bool,
        replace_conflict: bool,
    ) -> Result<UseOutcome> {
        let mut catalog = self.load()?;
        let resource = catalog
            .resources
            .iter()
            .find(|r| r.id == resource_id)
            .cloned()
            .context("所选资源不存在，请重新读取")?;
        let target = self
            .targets(&catalog)?
            .into_iter()
            .find(|t| t.id == target_id)
            .context("客户端配置目标已变化，请重新选择")?;
        ensure!(
            target.client != ClientId::Tiangong,
            "天工配置需要已授权的本机服务"
        );
        ensure!(
            target.client != ClientId::WebStorm
                || resource.kind != ResourceKind::Skill
                || target.model_mode != "native",
            "AI Assistant 普通聊天不加载 Skill；请选择 WebStorm 的 Air / ACP 代理"
        );
        let location = if resource.kind == ResourceKind::Skill {
            target.skills.join(resource.skill_folder(&target)?)
        } else {
            target.mcp.clone()
        };
        let prior = catalog
            .uses
            .iter()
            .find(|u| u.resource_id == resource_id && u.location == location)
            .cloned();
        let mut changes = Vec::new();
        let mut owned = Vec::new();
        let mut entry_fingerprint = None;
        if resource.kind == ResourceKind::Mcp {
            let raw = transaction::read(&target.mcp)?;
            let entries = mcp::read_entries(raw.as_deref().unwrap_or_default(), target.mcp_format)?;
            let current = entries.get(&resource.name);
            let current_fp = entry_hash(current)?;
            if let Some(prior) = &prior {
                ensure!(
                    replace_conflict || prior.entry_fingerprint.as_deref() == Some(&current_fp),
                    "客户端已独立修改此 MCP。重新读取后可选择保留或用 Hub 配置替换"
                );
            } else if let Some(current) = current {
                let desired = self.mcp_config(&resource)?.native(target.mcp_format)?;
                ensure!(
                    (using && mcp::equivalent(current, &desired)) || replace_conflict,
                    "客户端已有同名 MCP。请核对后选择是否替换或移除"
                );
            }
            let server = self.mcp_config(&resource)?;
            let desired = if using {
                Some(server.native(target.mcp_format)?)
            } else {
                None
            };
            let next = mcp::replace(
                raw.as_deref().unwrap_or_default(),
                target.mcp_format,
                &resource.name,
                desired.as_ref(),
            )?;
            entry_fingerprint = Some(entry_hash(desired.as_ref())?);
            owned.push(OwnedFile {
                path: target.mcp.clone(),
                fingerprint: transaction::fingerprint(Some(&next)),
            });
            changes.push(FileChange::plan(
                target.mcp.clone(),
                raw.as_deref(),
                Some(next),
            ));
        } else {
            let package = skills::from_directory(&self.root.join("skills").join(resource_id))?;
            let existing = if location.join("SKILL.md").is_file() {
                Some(skills::from_directory(&location)?)
            } else {
                None
            };
            if let Some(existing) = &existing {
                let matches = existing.fingerprint == resource.fingerprint;
                ensure!(
                    matches || replace_conflict,
                    "客户端已有同名但不同内容的 Skill，请核对后再替换"
                );
                if !using {
                    ensure!(matches || replace_conflict, "Skill 已被客户端修改，未删除");
                }
            }
            let mut paths: BTreeMap<PathBuf, Option<Vec<u8>>> = BTreeMap::new();
            if let Some(prior) = &prior {
                for file in &prior.files {
                    let raw = transaction::read(&file.path)?;
                    ensure!(
                        replace_conflict
                            || transaction::fingerprint(raw.as_deref()) == file.fingerprint,
                        "Skill 文件已被修改，请核对后重试"
                    );
                    paths.insert(file.path.clone(), None);
                }
            }
            if !using && prior.is_none() {
                ensure!(
                    existing.is_none() || replace_conflict,
                    "此 Skill 来自客户端，确认后可从此客户端移除"
                );
                if let Some(existing) = &existing {
                    for file in existing.files.keys() {
                        paths.insert(location.join(file), None);
                    }
                }
            }
            if using {
                for (path, bytes) in package.files {
                    paths.insert(location.join(path), Some(bytes));
                }
            }
            for (path, after) in paths {
                let before = transaction::read(&path)?;
                if let Some(after) = &after {
                    owned.push(OwnedFile {
                        path: path.clone(),
                        fingerprint: transaction::fingerprint(Some(after)),
                    });
                }
                changes.push(FileChange::plan(path, before.as_deref(), after));
            }
        }
        let undo = uuid::Uuid::new_v4().to_string();
        catalog
            .uses
            .retain(|u| !(u.resource_id == resource_id && u.location == location));
        if using {
            catalog.uses.push(UseRecord {
                resource_id: resource_id.into(),
                target_id: target_id.into(),
                location,
                files: owned,
                entry_fingerprint,
                undo: undo.clone(),
                pending: false,
            });
        }
        changes.push(self.catalog_change(&catalog)?);
        transaction::atomic_write(
            &self
                .root
                .join("resource-operations")
                .join(format!("{undo}.json")),
            &serde_json::to_vec(
                &serde_json::json!({"id":undo,"resourceId":resource_id,"targetId":target_id,"name":resource.name,"client":target.client,"using":using,"at":crate::ai_gateway::request_log::now_ms()}),
            )?,
        )?;
        let actual_undo = self.transactions.apply_as(&undo, changes)?;
        let using_after = self.is_using(&resource, &target)?;
        ensure!(
            using_after == using,
            "保存后状态未能确认，请重新读取；不会重复执行"
        );
        Ok(UseOutcome {
            target_id: target_id.into(),
            success: true,
            using: Some(using_after),
            message: if target.client == ClientId::ZCode {
                "已配置，请重新打开 ZCode 后选择模型或工具"
            } else {
                "已更新客户端配置；当前会话未加载时请新建会话或重开客户端"
            }
            .into(),
            undo: Some(actual_undo),
        })
    }
    pub fn restore(&self, undo: &str) -> Result<()> {
        self.transactions.restore(undo)?;
        self.finish_restore(undo)
    }
    pub fn finish_restore(&self, id: &str) -> Result<()> {
        ensure!(uuid::Uuid::parse_str(id).is_ok(), "恢复身份无效");
        for directory in ["resource-operations", "native-recovery"] {
            let path = self.root.join(directory).join(format!("{id}.json"));
            if let Some(raw) = transaction::read(&path)? {
                let mut value: Value = serde_json::from_slice(&raw)?;
                value["phase"] = "restored".into();
                transaction::atomic_write(&path, &serde_json::to_vec(&value)?)?;
            }
        }
        Ok(())
    }
    // Record the exact native baseline when it is discovered. This permits a
    // later edit/removal while still refusing intervening client changes.
    pub fn adopt(
        &self,
        resource: &Resource,
        target: &ClientTarget,
        entry: Option<&Value>,
    ) -> Result<()> {
        let mut catalog = self.load()?;
        if catalog
            .uses
            .iter()
            .any(|u| u.resource_id == resource.id && u.target_id == target.id)
        {
            return Ok(());
        }
        let location = if resource.kind == ResourceKind::Skill {
            target.skills.join(resource.skill_folder(target)?)
        } else {
            target.mcp.clone()
        };
        let files = if resource.kind == ResourceKind::Skill {
            skills::from_directory(&location)?
                .files
                .into_iter()
                .map(|(p, b)| OwnedFile {
                    path: location.join(p),
                    fingerprint: transaction::fingerprint(Some(&b)),
                })
                .collect()
        } else {
            vec![]
        };
        catalog.uses.push(UseRecord {
            resource_id: resource.id.clone(),
            target_id: target.id.clone(),
            location,
            files,
            entry_fingerprint: if resource.kind == ResourceKind::Mcp || entry.is_some() {
                Some(entry_hash(entry)?)
            } else {
                None
            },
            undo: String::new(),
            pending: false,
        });
        self.transactions
            .apply(vec![self.catalog_change(&catalog)?])?;
        Ok(())
    }
    /// Explicit scan of supported native config locations. No commands are run.
    pub fn discover_existing(&self) -> Result<Vec<String>> {
        self.discover_issues().map(|issues| {
            issues
                .iter()
                .map(|issue| format!("{} · {}：{}", issue.client.name(), issue.name, issue.reason))
                .collect()
        })
    }
    pub fn discover_issues(&self) -> Result<Vec<DiscoveryIssue>> {
        let targets = self.targets(&self.load()?)?;
        let mut messages = Vec::new();
        let mut locations = std::collections::BTreeSet::new();
        for target in targets {
            if target.client == ClientId::Tiangong {
                continue;
            }
            if locations.insert(target.skills.clone()) && target.skills.is_dir() {
                let entries = match std::fs::read_dir(&target.skills) {
                    Ok(entries) => Some(entries),
                    Err(error) => {
                        messages.push(DiscoveryIssue::new(
                            &target,
                            ResourceKind::Skill,
                            target.skills.clone(),
                            &error.into(),
                        ));
                        None
                    }
                };
                for entry in entries.into_iter().flatten().take(2048) {
                    let entry = match entry {
                        Ok(entry) => entry,
                        Err(error) => {
                            messages.push(DiscoveryIssue::new(
                                &target,
                                ResourceKind::Skill,
                                target.skills.clone(),
                                &error.into(),
                            ));
                            continue;
                        }
                    };
                    let path = entry.path();
                    if !path.join("SKILL.md").is_file() {
                        continue;
                    }
                    let result = (|| -> Result<Resource> {
                        let package = skills::from_directory(&path)?;
                        let resource = self.import_skill(
                            package,
                            format!("{} · {}", target.client.name(), target.label),
                            &self.load()?.revision,
                        )?;
                        let mut catalog = self.load()?;
                        let current = catalog
                            .resources
                            .iter_mut()
                            .find(|r| r.id == resource.id)
                            .context("Skill 索引已变化")?;
                        let key = if cfg!(windows) {
                            target.skills.to_string_lossy().to_lowercase()
                        } else {
                            target.skills.to_string_lossy().into_owned()
                        };
                        let folder = entry.file_name().to_string_lossy().into_owned();
                        ensure!(
                            current
                                .skill_folders
                                .get(&key)
                                .is_none_or(|old| *old == folder),
                            "相同 Skill 存在于多个目录，已保留首个位置；请在客户端整理后重新读取"
                        );
                        current.skill_folders.insert(key, folder);
                        let resource = current.clone();
                        self.transactions
                            .apply(vec![self.catalog_change(&catalog)?])?;
                        self.adopt(&resource, &target, None)?;
                        Ok(resource)
                    })();
                    if let Err(error) = result {
                        messages.push(DiscoveryIssue::new(
                            &target,
                            ResourceKind::Skill,
                            path.clone(),
                            &error,
                        ));
                    }
                }
            }
            if locations.insert(target.mcp.clone()) && target.mcp.is_file() {
                let result = (|| -> Result<()> {
                    let raw = transaction::read(&target.mcp)?.unwrap_or_default();
                    let entries = mcp::read_entries(&raw, target.mcp_format)?;
                    for (name, config) in entries {
                        let server = mcp::from_native(&name, config.clone(), target.mcp_format)?;
                        if builtin_assistance_mcp(&server) { continue; }
                        let bytes = serde_json::to_vec(&server)?;
                        let fp = transaction::fingerprint(Some(&bytes));
                        let mut catalog = self.load()?;
                        if let Some(resource) = catalog
                            .resources
                            .iter()
                            .find(|r| r.kind == ResourceKind::Mcp && r.fingerprint == fp)
                            .cloned()
                        {
                            self.adopt(&resource, &target, Some(&config))?;
                            continue;
                        }
                        let resource = Resource {
                            id: uuid::Uuid::new_v4().to_string(),
                            name,
                            description: String::new(),
                            kind: ResourceKind::Mcp,
                            fingerprint: fp,
                            folder: String::new(),
                            source: format!("{} · {}", target.client.name(), target.label),
                            secret_ref: Some(self.protect(&bytes)?),
                            skill_folders: BTreeMap::new(),
                        };
                        catalog.resources.push(resource.clone());
                        self.transactions
                            .apply(vec![self.catalog_change(&catalog)?])?;
                        self.adopt(&resource, &target, Some(&config))?;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    messages.push(DiscoveryIssue::new(
                        &target,
                        ResourceKind::Mcp,
                        target.mcp.clone(),
                        &error,
                    ));
                }
            }
        }
        Ok(messages)
    }
    pub fn remove(&self, id: &str, revision: &str) -> Result<()> {
        let mut catalog = self.load()?;
        ensure!(catalog.revision == revision, "列表已变化，请重新读取");
        ensure!(
            !catalog.uses.iter().any(|u| u.resource_id == id),
            "请先停用所有客户端中的此项，再删除"
        );
        let resource = catalog
            .resources
            .iter()
            .find(|r| r.id == id)
            .context("资源不存在，请重新读取")?;
        for target in self.targets(&catalog)?.iter().filter(|t| {
            t.client != ClientId::Tiangong
                && !(resource.kind == ResourceKind::Skill
                    && t.client == ClientId::WebStorm
                    && t.model_mode == "native")
        }) {
            ensure!(
                !self.is_using(resource, target).with_context(|| format!(
                    "{} 的配置未能读取，请核对后再删除",
                    target.client.name()
                ))?,
                "此资源仍由客户端使用，请先停用后再删除"
            );
        }
        catalog.resources.retain(|r| r.id != id);
        self.transactions
            .apply(vec![self.catalog_change(&catalog)?])?;
        Ok(())
    }
}

fn entry_hash(value: Option<&Value>) -> Result<String> {
    Ok(transaction::fingerprint(
        value.map(serde_json::to_vec).transpose()?.as_deref(),
    ))
}

pub(super) fn builtin_assistance_mcp(server: &McpServer) -> bool {
    server.config["url"].as_str().and_then(|s|url::Url::parse(s).ok()).is_some_and(|u|
        matches!(u.host_str(),Some("127.0.0.1"|"localhost"|"[::1]")) && u.path() == "/api/local-assistance/mcp")
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;
    fn selection_fixture(kind: ResourceKind) -> LibraryView {
        let targets = [ClientId::WorkBuddy, ClientId::ZCode]
            .into_iter()
            .enumerate()
            .map(|(i, client)| {
                let root = std::env::temp_dir().join(format!("selection-fixture-{i}"));
                targets::target(
                    client,
                    "fixture",
                    root.clone(),
                    root.join("skills"),
                    root.join("mcp.json"),
                    McpFormat::McpServers,
                    "fixture",
                )
            })
            .collect::<Vec<_>>();
        let resource = Resource {
            id: "fixture".into(),
            name: "Fixture".into(),
            description: String::new(),
            kind,
            fingerprint: String::new(),
            folder: "fixture".into(),
            source: "synthetic".into(),
            secret_ref: None,
            skill_folders: BTreeMap::new(),
        };
        LibraryView {
            revision: "fixture-revision".into(),
            resources: vec![ResourceView {
                resource,
                targets: targets
                    .iter()
                    .map(|t| TargetUse {
                        target_id: t.id.clone(),
                        client: t.client,
                        label: t.label.clone(),
                        using: Some(false),
                        error: None,
                        shared_with: vec![],
                        managed: false,
                        compatible: true,
                    })
                    .collect(),
            }],
            targets,
            recoveries: vec![],
        }
    }
    #[test]
    fn client_selection_rejects_stale_unknown_duplicate_and_unchanged_targets() {
        let mut view = selection_fixture(ResourceKind::Mcp);
        let change = SelectionChange {
            target_id: view.targets[0].id.clone(),
            using: true,
        };
        assert!(
            validate_selection(
                &view,
                "fixture",
                "fixture-revision",
                std::slice::from_ref(&change)
            )
            .is_ok()
        );
        assert!(
            validate_selection(
                &view,
                "fixture",
                "old-revision",
                std::slice::from_ref(&change)
            )
            .is_err()
        );
        assert!(
            validate_selection(
                &view,
                "missing",
                "fixture-revision",
                std::slice::from_ref(&change)
            )
            .is_err()
        );
        assert!(validate_selection(&view, "fixture", "fixture-revision", &[]).is_err());
        assert!(
            validate_selection(
                &view,
                "fixture",
                "fixture-revision",
                &[change.clone(), change.clone()]
            )
            .is_err()
        );
        assert!(
            validate_selection(
                &view,
                "fixture",
                "fixture-revision",
                &[SelectionChange {
                    target_id: "unknown".into(),
                    using: true
                }]
            )
            .is_err()
        );
        for (compatible, using) in [(false, Some(false)), (true, None), (true, Some(true))] {
            let t = &mut view.resources[0].targets[0];
            t.compatible = compatible;
            t.using = using;
            assert!(
                validate_selection(
                    &view,
                    "fixture",
                    "fixture-revision",
                    std::slice::from_ref(&change)
                )
                .is_err()
            );
        }
    }
    #[test]
    fn client_selection_can_apply_opposite_changes_to_independent_locations() {
        let mut view = selection_fixture(ResourceKind::Mcp);
        view.resources[0].targets[0].using = Some(true);
        let changes = vec![
            SelectionChange {
                target_id: view.targets[0].id.clone(),
                using: false,
            },
            SelectionChange {
                target_id: view.targets[1].id.clone(),
                using: true,
            },
        ];
        assert!(validate_selection(&view, "fixture", "fixture-revision", &changes).is_ok());
        view.resources[0].resource.kind = ResourceKind::Skill;
        view.targets[1].skills = view.targets[0].skills.clone();
        assert!(validate_selection(&view, "fixture", "fixture-revision", &changes).is_err());
    }
    #[test]
    fn discovery_reports_exact_oversized_items_and_keeps_source_files() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::new(&dir.path().join("hub.toml")).unwrap();
        let root = dir.path().join("fixture-client");
        let target = targets::target(
            ClientId::Codex,
            "fixture",
            root.clone(),
            root.join("skills"),
            root.join("config.toml"),
            McpFormat::Codex,
            "fixture",
        );
        library
            .save_targets(library.load().unwrap(), vec![target.clone()])
            .unwrap();
        let skill = target.skills.join("too-large");
        std::fs::create_dir_all(&skill).unwrap();
        let manifest = b"---\nname: fixture-large\ndescription: synthetic\n---\nFixture";
        std::fs::write(skill.join("SKILL.md"), manifest).unwrap();
        for i in 0..10 {
            std::fs::File::create(skill.join(format!("data-{i}.bin")))
                .unwrap()
                .set_len(7 * 1024 * 1024)
                .unwrap();
        }
        std::fs::File::create(&target.mcp)
            .unwrap()
            .set_len(transaction::MAX_FILE_BYTES as u64 + 1)
            .unwrap();
        let issues = library.discover_issues().unwrap();
        let skill_issue = issues
            .iter()
            .find(|i| i.kind == ResourceKind::Skill && i.location == skill)
            .unwrap();
        assert_eq!(skill_issue.client, ClientId::Codex);
        assert_eq!(skill_issue.name, "too-large");
        assert!(skill_issue.guidance.contains("64 MiB"));
        assert!(skill_issue.guidance.contains("不会自动删减"));
        let mcp_issue = issues
            .iter()
            .find(|i| i.kind == ResourceKind::Mcp && i.location == target.mcp)
            .unwrap();
        assert!(mcp_issue.guidance.contains("32 MiB"));
        assert_eq!(
            std::fs::metadata(&target.mcp).unwrap().len(),
            transaction::MAX_FILE_BYTES as u64 + 1
        );
        assert_eq!(std::fs::read(skill.join("SKILL.md")).unwrap(), manifest);
        assert_eq!(
            std::fs::metadata(skill.join("data-4.bin")).unwrap().len(),
            7 * 1024 * 1024
        );
    }
    #[test]
    fn partial_targets_keep_success_and_retry_does_not_rewrite_the_other_client() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::new(&dir.path().join("hub.toml")).unwrap();
        let mk = |client, label| {
            let root = dir.path().join(label);
            targets::target(
                client,
                label,
                root.clone(),
                root.join("skills"),
                root.join("mcp.json"),
                McpFormat::McpServers,
                "fixture",
            )
        };
        let a = mk(ClientId::WorkBuddy, "a");
        let b = mk(ClientId::ZCode, "b");
        library
            .save_targets(library.load().unwrap(), vec![a.clone(), b.clone()])
            .unwrap();
        let resource=library.save_mcp(None,McpServer{name:"fixture".into(),config:serde_json::json!({"url":"https://example.invalid/mcp","type":"http","headers":{"X-Fixture":"value"}})},String::new(),&library.load().unwrap().revision).unwrap();
        std::fs::create_dir_all(&b.root).unwrap();
        std::fs::write(&b.mcp, b"invalid-json").unwrap();
        let first = library.use_files(&resource.id, &a.id, true, false).unwrap();
        assert!(library.use_files(&resource.id, &b.id, true, false).is_err());
        let bytes = std::fs::read(&a.mcp).unwrap();
        assert!(library.is_using(&resource, &a).unwrap());
        assert!(library.is_using(&resource, &b).is_err());
        std::fs::write(&b.mcp, b"{}").unwrap();
        let second = library.use_files(&resource.id, &b.id, true, false).unwrap();
        assert!(second.success && library.is_using(&resource, &b).unwrap());
        assert_eq!(std::fs::read(&a.mcp).unwrap(), bytes);
        // Undo the latest target; the first remains in use.
        library.restore(second.undo.as_deref().unwrap()).unwrap();
        assert!(!library.is_using(&resource, &b).unwrap());
        assert!(library.is_using(&resource, &a).unwrap());
        library.restore(first.undo.as_deref().unwrap()).unwrap();
        assert!(!library.is_using(&resource, &a).unwrap());
    }
    #[test]
    fn resources_use_real_configuration_and_refuse_later_changes() {
        let dir = tempfile::tempdir().unwrap();
        let hub = dir.path().join("hub.toml");
        let library = Library::new(&hub).unwrap();
        let mut catalog = library.load().unwrap();
        let root = dir.path().join("fixture-client");
        let target = targets::target(
            ClientId::WorkBuddy,
            "fixture",
            root.clone(),
            root.join("skills"),
            root.join("mcp.json"),
            McpFormat::McpServers,
            "fixture",
        );
        let other_root = dir.path().join("other-client");
        let other = targets::target(
            ClientId::ZCode,
            "other",
            other_root.clone(),
            other_root.join("skills"),
            other_root.join("mcp.json"),
            McpFormat::McpServers,
            "fixture",
        );
        catalog.target_overrides.push(target.clone());
        library.save_targets(catalog, vec![target.clone()]).unwrap();
        let resource = library
            .save_mcp(
                None,
                McpServer {
                    name: "fixture".into(),
                    config: serde_json::json!({"command":"test-only","args":["not-executed"]}),
                },
                String::new(),
                &library.load().unwrap().revision,
            )
            .unwrap();
        let outcome = library
            .use_files(&resource.id, &target.id, true, false)
            .unwrap();
        assert!(outcome.success && target.mcp.is_file());
        assert!(library.is_using(&resource, &target).unwrap());
        let row = library
            .view_targets(library.load().unwrap(), vec![target.clone(), other.clone()])
            .unwrap()
            .resources
            .into_iter()
            .find(|r| r.resource.id == resource.id)
            .unwrap();
        assert!(row.visible_for_client(ClientId::WorkBuddy, false));
        assert!(!row.visible_for_client(ClientId::ZCode, false));
        assert!(row.visible_for_client(ClientId::ZCode, true));
        assert!(!row.visible_for_client(ClientId::WorkBuddy, true));
        let mut native: Value =
            serde_json::from_slice(&std::fs::read(&target.mcp).unwrap()).unwrap();
        native["mcpServers"]["fixture"]["args"] = serde_json::json!(["user-edit"]);
        std::fs::write(&target.mcp, serde_json::to_vec(&native).unwrap()).unwrap();
        let row = library
            .view_targets(library.load().unwrap(), vec![target.clone(), other])
            .unwrap()
            .resources
            .into_iter()
            .find(|r| r.resource.id == resource.id)
            .unwrap();
        assert!(
            row.visible_for_client(ClientId::WorkBuddy, false),
            "a changed owned entry must remain actionable"
        );
        assert!(
            row.targets
                .iter()
                .any(|t| t.target_id == target.id && t.managed && t.using.is_none())
        );
        assert!(
            library
                .use_files(&resource.id, &target.id, false, false)
                .is_err()
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&target.mcp).unwrap()).unwrap()["mcpServers"]
                ["fixture"]["args"][0],
            "user-edit"
        );
    }
}
