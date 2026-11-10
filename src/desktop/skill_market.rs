//! Public GitHub Skill repositories. Catalog data is real, imports are pinned
//! to the catalog's commit and retain the complete package. Nothing is run.
use super::{
    skills::{self, SkillPackage},
    transaction,
};
use anyhow::{Context, Result, ensure};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub const DEFAULT_REPOSITORY: &str = "https://github.com/anthropics/skills";
const MAX_INDEX: usize = 8 * 1024 * 1024;
const MAX_PACKAGE: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Repository {
    pub owner: String,
    pub name: String,
}
impl Repository {
    pub fn parse(raw: &str) -> Result<Self> {
        let u = url::Url::parse(raw.trim()).context("请填写公开 GitHub 仓库地址")?;
        ensure!(
            u.scheme() == "https"
                && u.host_str() == Some("github.com")
                && u.port().is_none()
                && u.username().is_empty()
                && u.password().is_none()
                && u.query().is_none()
                && u.fragment().is_none(),
            "请使用 https://github.com/作者/仓库，不含令牌或其他参数"
        );
        let parts = u
            .path()
            .trim_matches('/')
            .trim_end_matches(".git")
            .split('/')
            .collect::<Vec<_>>();
        ensure!(
            parts.len() == 2
                && parts.iter().all(|s| !s.is_empty()
                    && *s != "."
                    && *s != ".."
                    && s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))),
            "仓库地址格式应为 https://github.com/作者/仓库"
        );
        Ok(Self {
            owner: parts[0].into(),
            name: parts[1].into(),
        })
    }
    pub fn url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner, self.name)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub path: String,
    pub name: String,
    pub description: String,
    pub files: usize,
    pub bytes: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub repository: String,
    pub commit: String,
    pub entries: Vec<Entry>,
    pub fetched_at: i64,
}
#[derive(Clone, Debug, Deserialize)]
struct TreeFile {
    path: String,
    mode: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    size: u64,
}
#[derive(Deserialize)]
struct Tree {
    sha: String,
    tree: Vec<TreeFile>,
    #[serde(default)]
    truncated: bool,
}

async fn fetch(http: &Client, url: &str, max: usize) -> Result<Vec<u8>> {
    let mut response = http
        .get(url)
        .header(
            "User-Agent",
            concat!("TianCaiSpaceHub/", env!("CARGO_PKG_VERSION")),
        )
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(35))
        .send()
        .await
        .context("无法读取公共 Skill 仓库，请检查网络后刷新")?;
    ensure!(
        response.status().is_success(),
        "公共仓库返回 HTTP {}；请核对仓库是否公开，或稍后重试（GitHub 有匿名访问频率限制）",
        response.status().as_u16()
    );
    ensure!(
        response.content_length().is_none_or(|n| n <= max as u64),
        "仓库内容超过本次读取上限"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(bytes.len() + chunk.len() <= max, "仓库内容超过本次读取上限");
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.starts_with('/')
        && path.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && !p.contains(['\\', ':'])
                && !p.chars().any(char::is_control)
        })
}
fn commit_ok(commit: &str) -> bool {
    commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit())
}
async fn tree(http: &Client, repo: &Repository, commit: &str, api: &str) -> Result<Tree> {
    ensure!(
        commit == "HEAD" || commit_ok(commit),
        "仓库版本无效，请刷新广场"
    );
    let data: Tree = serde_json::from_slice(
        &fetch(
            http,
            &format!(
                "{api}/repos/{}/{}/git/trees/{commit}?recursive=1",
                repo.owner, repo.name
            ),
            MAX_INDEX,
        )
        .await?,
    )?;
    ensure!(
        !data.truncated && commit_ok(&data.sha),
        "仓库目录未完整返回，请改用较小的 Skill 专用仓库"
    );
    ensure!(
        data.tree.len() <= 30000,
        "仓库内容过多，请使用 Skill 专用仓库"
    );
    Ok(data)
}
fn entries(tree: &Tree) -> Result<Vec<Entry>> {
    let mut result = Vec::new();
    for file in tree
        .tree
        .iter()
        .filter(|f| f.kind == "blob" && (f.path == "SKILL.md" || f.path.ends_with("/SKILL.md")))
    {
        ensure!(safe_path(&file.path), "仓库包含无效 Skill 路径");
        let path = file
            .path
            .strip_suffix("SKILL.md")
            .unwrap()
            .trim_end_matches('/');
        if path
            .split('/')
            .any(|p| p.starts_with('.') || matches!(p, "node_modules" | "vendor"))
        {
            continue;
        }
        let prefix = format!("{path}/");
        let files = tree
            .tree
            .iter()
            .filter(|f| f.kind == "blob" && (path.is_empty() || f.path.starts_with(&prefix)))
            .collect::<Vec<_>>();
        result.push(Entry {
            path: path.into(),
            name: path
                .rsplit('/')
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or("Skill")
                .into(),
            description: String::new(),
            files: files.len(),
            bytes: files.iter().map(|f| f.size).sum(),
        });
    }
    ensure!(
        result.len() <= 256,
        "仓库包含超过 256 个 Skill，请拆分为多个专用仓库"
    );
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}
fn raw_url(repo: &Repository, commit: &str, path: &str, base: &str) -> Result<String> {
    ensure!(safe_path(path) && commit_ok(commit), "仓库文件身份无效");
    let mut u = url::Url::parse(base)?;
    u.path_segments_mut()
        .unwrap()
        .extend([repo.owner.as_str(), repo.name.as_str(), commit])
        .extend(path.split('/'));
    Ok(u.into())
}
pub fn cache_path(config: &Path) -> PathBuf {
    super::state_root(config).join("skill-market.json")
}
pub fn cached(config: &Path) -> Result<Option<Catalog>> {
    transaction::read(&cache_path(config))?
        .map(|b| serde_json::from_slice(&b).context("广场缓存无法读取，请刷新仓库"))
        .transpose()
}
pub async fn catalog(http: &Client, repository: &str) -> Result<Catalog> {
    catalog_from(
        http,
        repository,
        "https://api.github.com",
        "https://raw.githubusercontent.com/",
    )
    .await
}
async fn catalog_from(http: &Client, repository: &str, api: &str, raw: &str) -> Result<Catalog> {
    let repo = Repository::parse(repository)?;
    let resolved: Value = serde_json::from_slice(
        &fetch(
            http,
            &format!("{api}/repos/{}/{}/commits/HEAD", repo.owner, repo.name),
            1024 * 1024,
        )
        .await?,
    )?;
    let commit = resolved["sha"]
        .as_str()
        .filter(|s| commit_ok(s))
        .context("仓库版本无法确认")?
        .to_string();
    let tree = tree(http, &repo, &commit, api).await?;
    let mut entries = entries(&tree)?;
    // Descriptions come from actual metadata, never generated sample cards.
    use futures_util::{StreamExt, stream};
    let urls = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let path = if e.path.is_empty() {
                "SKILL.md".into()
            } else {
                format!("{}/SKILL.md", e.path)
            };
            (i, raw_url(&repo, &commit, &path, raw))
        })
        .collect::<Vec<_>>();
    let descriptions = stream::iter(urls)
        .map(|(i, url)| {
            let http = http.clone();
            async move {
                let result = match url {
                    Ok(url) => fetch(&http, &url, 256 * 1024)
                        .await
                        .and_then(|b| skills::metadata(&b)),
                    Err(e) => Err(e),
                };
                (i, result)
            }
        })
        .buffer_unordered(6)
        .collect::<Vec<_>>()
        .await;
    for (i, meta) in descriptions {
        if let Ok(meta) = meta {
            entries[i].name = meta.name;
            entries[i].description = meta.description;
        }
    }
    Ok(Catalog {
        repository: repo.url(),
        commit,
        entries,
        fetched_at: crate::ai_gateway::request_log::now_ms(),
    })
}
pub async fn download(http: &Client, catalog: &Catalog, path: &str) -> Result<SkillPackage> {
    download_from(
        http,
        catalog,
        path,
        "https://api.github.com",
        "https://raw.githubusercontent.com/",
    )
    .await
}
async fn download_from(
    http: &Client,
    catalog: &Catalog,
    path: &str,
    api: &str,
    raw: &str,
) -> Result<SkillPackage> {
    ensure!(
        catalog.entries.iter().any(|e| e.path == path),
        "Skill 已变化，请刷新广场"
    );
    let repo = Repository::parse(&catalog.repository)?;
    let tree = tree(http, &repo, &catalog.commit, api).await?;
    let prefix = if path.is_empty() {
        String::new()
    } else {
        format!("{path}/")
    };
    let mut files = BTreeMap::new();
    let mut bytes = 0usize;
    for file in tree
        .tree
        .iter()
        .filter(|f| f.path.starts_with(&prefix) && f.kind != "tree")
    {
        ensure!(
            file.kind == "blob" && matches!(file.mode.as_str(), "100644" | "100755"),
            "Skill 包含链接或子模块，请通过本地完整目录核对后导入"
        );
        let relative = &file.path[prefix.len()..];
        ensure!(safe_path(relative), "Skill 文件路径无效");
        ensure!(
            file.size <= transaction::MAX_FILE_BYTES as u64
                && bytes as u64 + file.size <= MAX_PACKAGE as u64
                && files.len() < 2048,
            "Skill 超过 32 MiB 单文件或 64 MiB/2048 文件的完整包上限"
        );
        let content = fetch(
            http,
            &raw_url(&repo, &catalog.commit, &file.path, raw)?,
            transaction::MAX_FILE_BYTES.min(MAX_PACKAGE - bytes),
        )
        .await?;
        ensure!(
            content.len() as u64 == file.size,
            "仓库文件大小变化，请刷新重试"
        );
        bytes += content.len();
        files.insert(PathBuf::from(relative), content);
    }
    // Keep repository-level attribution alongside the unmodified package.
    if !files.contains_key(Path::new("LICENSE")) {
        if let Some(license) = tree.tree.iter().find(|f| {
            f.path == "LICENSE" && f.kind == "blob" && f.mode == "100644" && f.size <= 256 * 1024
        }) {
            let content = fetch(
                http,
                &raw_url(&repo, &catalog.commit, &license.path, raw)?,
                256 * 1024,
            )
            .await?;
            ensure!(
                bytes + content.len() <= MAX_PACKAGE && files.len() < 2048,
                "Skill 含许可文件后超过完整包上限"
            );
            files.insert(PathBuf::from("LICENSE.repository"), content);
        }
    }
    skills::from_files(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repository_identity_rejects_credentials_and_other_hosts() {
        assert_eq!(
            Repository::parse(DEFAULT_REPOSITORY).unwrap().url(),
            DEFAULT_REPOSITORY
        );
        for url in [
            "http://github.com/a/b",
            "https://github.com.evil/a/b",
            "https://token@github.com/a/b",
            "https://github.com/a/b?token=x",
            "https://github.com/a/b/tree/main",
            "https://github.com/a/%2e%2e",
        ] {
            assert!(Repository::parse(url).is_err(), "{url}");
        }
    }
    #[test]
    fn index_is_scoped_to_real_skill_directories() {
        let t:Tree=serde_json::from_value(serde_json::json!({"sha":"a".repeat(40),"tree":[{"path":"skills/alpha/SKILL.md","mode":"100644","type":"blob","size":20},{"path":"skills/alpha/script.py","mode":"100755","type":"blob","size":5},{"path":"README.md","mode":"100644","type":"blob","size":10}]})).unwrap();
        let e = entries(&t).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].path, "skills/alpha");
        assert_eq!(e[0].files, 2);
        assert_eq!(e[0].bytes, 25);
        for p in ["../outside", "/root", "a/../../b", "a\\b", "a/c:bad"] {
            assert!(!safe_path(p));
        }
    }
    #[tokio::test]
    async fn repository_download_uses_selected_commit_and_keeps_supporting_files() {
        use axum::{
            Router,
            body::Body,
            http::{Request, StatusCode},
            response::IntoResponse,
        };
        use std::sync::{Arc, Mutex};
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let logged = calls.clone();
        let commit = "b".repeat(40);
        let version = commit.clone();
        let skill = "---\nname: fixture-skill\ndescription: Real synthetic metadata\n---\nUse scripts/helper.py";
        let router=Router::new().fallback(move |r:Request<Body>|{
            let path=r.uri().path().to_string();logged.lock().unwrap().push(path.clone());let sha=version.clone();
            async move {
                if path.ends_with("/commits/HEAD") {return axum::Json(serde_json::json!({"sha":sha})).into_response();}
                if path.ends_with(&format!("/git/trees/{sha}")) {return axum::Json(serde_json::json!({"sha":"c".repeat(40),"tree":[
                    {"path":"skills/fixture/SKILL.md","mode":"100644","type":"blob","size":skill.len()},
                    {"path":"skills/fixture/scripts/helper.py","mode":"100755","type":"blob","size":9},
                    {"path":"LICENSE","mode":"100644","type":"blob","size":7},
                    {"path":"skills/other/SKILL.md","mode":"100644","type":"blob","size":skill.len()}
                ]})).into_response();}
                if path.ends_with("/SKILL.md") {return skill.into_response();}
                if path.ends_with("/scripts/helper.py") {return "print(42)".into_response();}
                if path.ends_with("/LICENSE") {return "fixture".into_response();}
                StatusCode::NOT_FOUND.into_response()
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let base = format!("http://{address}");
        let raw = format!("{base}/");
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let catalog = catalog_from(&client, "https://github.com/fixture/skills", &base, &raw)
            .await
            .unwrap();
        assert_eq!(catalog.commit, commit);
        assert_eq!(catalog.entries[0].name, "fixture-skill");
        let package = download_from(&client, &catalog, "skills/fixture", &base, &raw)
            .await
            .unwrap();
        assert_eq!(package.files[Path::new("scripts/helper.py")], b"print(42)");
        assert_eq!(package.files[Path::new("LICENSE.repository")], b"fixture");
        assert_eq!(package.files.len(), 3);
        assert!(
            download_from(&client, &catalog, "../other", &base, &raw)
                .await
                .is_err()
        );
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|p| p.ends_with("/commits/HEAD"))
                .count(),
            1
        );
        server.abort();
    }
}
