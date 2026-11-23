//! Resolve explicit assistant links against this session's catalog and native workspace.
use super::{Catalog, StoredFile, read_catalog, root, save_catalog};
use crate::{
    app_state::SharedState,
    remote_sessions::types::{RemoteFile, Scope},
};
use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    ops::Range,
    path::{Component, Path, PathBuf},
};

// Markdown source links (including line references), code examples and external URLs stay text.
fn destinations(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut fence = None;
    let mut pos = 0;
    for line in text.split_inclusive('\n') {
        let start = pos;
        pos += line.len();
        let trim = line.trim_start();
        let marker = trim.bytes().next().filter(|b| matches!(b, 96 | b'~'));
        let run = marker.map_or(0, |m| trim.bytes().take_while(|&b| b == m).count());
        if let Some((m, length)) = fence {
            if marker == Some(m) && run >= length && trim[run..].trim().is_empty() {
                fence = None;
            }
            continue;
        }
        if run >= 3 {
            fence = Some((marker.unwrap(), run));
            continue;
        }
        if line.starts_with("    ") || line.starts_with('\t') {
            continue;
        }
        let bytes = line.as_bytes();
        let mut i = 0;
        let mut labels = 0usize;
        let mut inline = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b == 96 {
                let n = bytes[i..].iter().take_while(|&&v| v == 96).count();
                inline = if inline == n {
                    0
                } else if inline == 0 {
                    n
                } else {
                    inline
                };
                i += n;
            } else if inline != 0 {
                i += 1;
            } else if b == b'\\' {
                i += 2;
            } else if b == b'[' {
                labels += 1;
                i += 1;
            } else if b == b']' && labels > 0 {
                labels -= 1;
                i += 1;
                if bytes.get(i) != Some(&b'(') {
                    continue;
                }
                let mut from = i + 1;
                if bytes.get(from) == Some(&b'<') {
                    from += 1;
                    if let Some(end) = line[from..].find(">)") {
                        ranges.push(start + from..start + from + end);
                        i = from + end + 2;
                    }
                } else {
                    let mut j = from;
                    let mut depth = 1usize;
                    while j < bytes.len() {
                        match bytes[j] {
                            b'(' => depth += 1,
                            b')' => {
                                depth -= 1;
                                if depth == 0 {
                                    ranges.push(start + from..start + j);
                                    i = j + 1;
                                    break;
                                }
                            }
                            _ => (),
                        }
                        j += 1;
                    }
                    if depth > 0 {
                        break;
                    }
                }
                if ranges.len() >= 32 {
                    return ranges;
                }
            } else {
                i += 1;
            }
        }
    }
    ranges
}

fn target(raw: &str) -> Option<String> {
    // Keep encoded client/session components exactly as assigned by Scope::file_url.
    if raw.starts_with("/api/remote/v1/clients/") && raw.len() <= 4096 {
        return Some(raw.strip_suffix("?download=1").unwrap_or(raw).to_owned());
    }
    if raw.len() > 4096 || raw.is_empty() || raw.contains(['#', '?']) {
        return None;
    }
    let value = url::form_urlencoded::parse(
        format!("p={}", raw.replace('+', "%2B").replace('&', "%26")).as_bytes(),
    )
    .next()?
    .1
    .into_owned()
    .replace('\\', "/");
    let value = if value.as_bytes().get(0) == Some(&b'/')
        && value.as_bytes().get(1).is_some_and(u8::is_ascii_alphabetic)
        && value.get(2..4) == Some(":/")
    {
        value[1..].to_owned()
    } else {
        value
    };
    if value.chars().any(char::is_control)
        || value.starts_with("//")
        || value.contains(['#', '?', '"', '<', '>'])
    {
        return None;
    }
    let path = value.strip_prefix("sandbox:/mnt/data/").unwrap_or(&value);
    let drive = path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && path.get(1..3) == Some(":/");
    if (if drive { &path[2..] } else { path }).contains(':')
        || path.split('/').any(|s| {
            s == ".." || s.eq_ignore_ascii_case(".obsidian") || s.eq_ignore_ascii_case("zhenduan")
        })
    {
        return None;
    }
    Some(value)
}

pub(in crate::remote_sessions) fn has_local_link(text: &str) -> bool {
    text.contains("](")
        && destinations(text)
            .into_iter()
            .filter_map(|r| target(&text[r]))
            .any(|s| {
                !s.starts_with("/api/remote/")
                    && !s.starts_with("sandbox:")
                    && deliverable(Path::new(&s))
            })
}

fn matches_path(a: &Path, b: &Path) -> bool {
    #[cfg(windows)]
    {
        // Strip only the verbatim disk prefix that canonicalize/GetFinalPathName add.
        let spelling = |p: &Path| {
            p.to_string_lossy()
                .replace('/', "\\")
                .trim_start_matches(r"\\?\")
                .to_owned()
        };
        spelling(a) == spelling(b)
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

#[cfg(windows)]
fn opened_path(file: &File) -> Result<PathBuf> {
    use std::{
        ffi::OsString,
        os::windows::{ffi::OsStringExt, io::AsRawHandle},
    };
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
    let mut buf = vec![0u16; 32768];
    let count = unsafe {
        GetFinalPathNameByHandleW(file.as_raw_handle(), buf.as_mut_ptr(), buf.len() as u32, 0)
    };
    ensure!(
        count > 0 && (count as usize) < buf.len(),
        "无法核对关联文件的位置"
    );
    Ok(PathBuf::from(OsString::from_wide(&buf[..count as usize])))
}
#[cfg(target_os = "macos")]
fn opened_path(file: &File) -> Result<PathBuf> {
    use std::{
        ffi::{CStr, OsStr},
        os::{fd::AsRawFd, unix::ffi::OsStrExt},
    };
    let mut buf = [0i8; libc::MAXPATHLEN as usize];
    ensure!(
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, buf.as_mut_ptr()) } == 0,
        "无法核对关联文件的位置"
    );
    Ok(PathBuf::from(OsStr::from_bytes(
        unsafe { CStr::from_ptr(buf.as_ptr()) }.to_bytes(),
    )))
}
#[cfg(not(any(windows, target_os = "macos")))]
fn opened_path(file: &File) -> Result<PathBuf> {
    use std::os::fd::AsRawFd;
    Ok(std::fs::read_link(format!(
        "/proc/self/fd/{}",
        file.as_raw_fd()
    ))?)
}
#[cfg(windows)]
fn single_link(file: &File) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    unsafe {
        GetFileInformationByHandle(file.as_raw_handle(), &mut info) != 0 && info.nNumberOfLinks == 1
    }
}
#[cfg(not(windows))]
fn single_link(file: &File) -> bool {
    use std::os::unix::fs::MetadataExt;
    file.metadata().is_ok_and(|m| m.nlink() == 1)
}

pub(super) fn open_checked(stored: &StoredFile) -> Result<File> {
    let expected = stored.pinned_path.as_deref().unwrap_or(&stored.path);
    let file = File::open(&stored.path).context("关联文件已移除或无法读取")?;
    let actual = opened_path(&file)?;
    ensure!(
        file.metadata()?.is_file() && matches_path(&actual, expected),
        "关联文件的位置已改变"
    );
    if let Some(root) = &stored.output_root {
        ensure!(
            actual.parent() == Some(root.as_path()) && single_link(&file),
            "产物已离开本会话的交付目录"
        );
    }
    Ok(file)
}

fn deliverable(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "msi"
            | "zip"
            | "7z"
            | "dmg"
            | "pkg"
            | "pdf"
            | "md"
            | "txt"
            | "html"
            | "docx"
            | "pptx"
            | "xlsx"
            | "csv"
            | "png"
            | "jpg"
            | "jpeg"
            | "webp"
            | "gif"
            | "mp4"
            | "mp3"
            | "wav"
    )
}

fn resolve(
    catalog: &mut Catalog,
    scope: &Scope,
    cwd: Option<&Path>,
    value: &str,
) -> Option<RemoteFile> {
    let source = Path::new(value);
    let sandbox = value.strip_prefix("sandbox:/mnt/data/");
    // sandbox:/mnt/data is the originating runtime's namespace. A basename alone
    // may only identify an already registered, unambiguous file in this session.
    let candidate = if sandbox.is_some() {
        None
    } else if source.is_absolute() {
        Some(source.to_owned())
    } else {
        cwd.map(|p| p.join(source))
    };
    let mut matching = catalog.files.iter().filter(|f| {
        !f.pending
            && (f.public.url == value
                || candidate.as_ref().is_some_and(|p| matches_path(&f.path, p))
                || sandbox.is_some_and(|s| !s.contains('/') && f.public.name == s))
    });
    if let Some(first) = matching.next() {
        return matching.next().is_none().then(|| first.public.clone());
    }
    let cwd = cwd.filter(|p| p.is_absolute() && p.parent().is_some())?;
    let candidate = candidate?;
    let rel = candidate.strip_prefix(cwd).ok()?;
    let mut parts = rel.components();
    let Component::Normal(dir) = parts.next()? else {
        return None;
    };
    let Component::Normal(_) = parts.next()? else {
        return None;
    };
    if parts.next().is_some()
        || !matches!(
            dir.to_str()?,
            "target" | "dist" | "artifacts" | "outputs" | "output" | "deliverables"
        )
        || candidate.file_name()?.to_str()?.starts_with('.')
        || !deliverable(&candidate)
    {
        return None;
    }
    let root = cwd.canonicalize().ok()?.join(dir);
    let pinned = root.join(candidate.file_name()?);
    // Reject reparse/symlink substitutions and hardlinks before registration and again on read.
    if candidate.canonicalize().ok()? != pinned || !candidate.is_file() {
        return None;
    }
    let file = File::open(&candidate).ok()?;
    let actual = opened_path(&file).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || !matches_path(&actual, &pinned) || !single_link(&file) {
        return None;
    }
    let id = uuid::Uuid::new_v4().to_string();
    let public = RemoteFile {
        id: id.clone(),
        name: candidate.file_name()?.to_str()?.into(),
        size: meta.len(),
        kind: "artifact".into(),
        url: scope.file_url(&id),
        mime_type: mime_guess::from_path(&candidate)
            .first_or_octet_stream()
            .to_string(),
    };
    catalog.files.push(StoredFile {
        public: public.clone(),
        path: candidate,
        pending: false,
        pinned_path: Some(actual),
        output_root: Some(root),
    });
    Some(public)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_sessions::{codex, codex_fixture::Desktop, types::PageQuery};
    use serde_json::json;
    #[tokio::test]
    async fn delivery_history_and_completed_reply_use_native_cwd_and_same_download() {
        let (temp, state) = crate::remote_sessions::test_state();
        let out = temp.path().join("target");
        std::fs::create_dir(&out).unwrap();
        let path = out.join("报告 (最终).msi");
        std::fs::write(&path, b"sample package").unwrap();
        let spelling = path.to_str().unwrap().replace('\\', "/");
        let dest = if cfg!(windows) {
            format!("/{spelling}")
        } else {
            spelling
        };
        let body = format!("请下载 [报告](<{dest}>)，源码见 [src](src/main.rs:12)");
        let item = json!({"id":"report","type":"agentMessage","phase":"final_answer","text":body});
        let scope = Scope::new("codex:default", "source");
        let mut desktop = Desktop::connect(&state).await;
        let serve = async {
            desktop
                .reply("thread/turns/list", |_| {
                    Ok(json!({"data":[{"id":"turn","status":"completed","items":[item]}]}))
                })
                .await;
            desktop
                .reply("thread/read", |p| {
                    assert_eq!(p["includeTurns"], false);
                    Ok(json!({"thread":{"id":"source","cwd":temp.path()}}))
                })
                .await;
        };
        let page = PageQuery::default();
        let ((), history) = tokio::time::timeout(std::time::Duration::from_secs(4), async {
            tokio::join!(serve, codex::history(&state, &scope, &page))
        })
        .await
        .expect("expected native history and cwd response");
        let history = history.unwrap();
        let message = &history["items"][0];
        let record = &message["files"][0];
        let id = record["id"].as_str().unwrap();
        let expected = format!(
            "请下载 [报告](<{}>)，源码见 [src](src/main.rs:12)",
            scope.file_url(id)
        );
        assert_eq!(message["text"], expected);
        let (_, mut file) = super::super::open_registered(&state, &scope, id)
            .await
            .unwrap();
        let mut contents = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut file, &mut contents)
            .await
            .unwrap();
        assert_eq!(contents, b"sample package");
        assert!(
            super::super::open_registered(&state, &Scope::new("codex:default", "other"), id)
                .await
                .is_err()
        );
        codex::observe(
            &state,
            7,
            "app",
            "stream",
            &json!({"method":"item/completed","params":{"threadId":"source","item":item}}),
        )
        .await;
        assert_eq!(
            state.remote_sessions.feeds.lock().await[&scope]
                .events
                .back()
                .unwrap()
                .data["item"]["text"],
            expected
        );
        let (alias, files) = project_links(
            &state,
            &scope,
            None,
            "[再次下载](sandbox:/mnt/data/报告%20(最终).msi)",
        )
        .await;
        assert!(alias.contains(&scope.file_url(id)) && files[0].id == id);
    }

    #[tokio::test]
    async fn delivery_skips_source_examples_and_rechecks_registered_file_on_open() {
        let (tmp, state) = crate::remote_sessions::test_state();
        let scope = Scope::new("client", "thread");
        let output = tmp.path().join("target");
        std::fs::create_dir(&output).unwrap();
        let path = output.join("test.zip");
        std::fs::write(&path, b"test deliverable").unwrap();
        let other = tmp.path().join("other.zip");
        std::fs::write(&other, b"unrelated").unwrap();
        let back = char::from(96);
        let text = format!(
            "{}[example](target/test.zip){} [outside](other.zip) [line](target/test.zip:2) [missing](sandbox:/mnt/data/test.zip)",
            back, back
        );
        assert_eq!(
            project_links(&state, &scope, Some(tmp.path()), &text)
                .await
                .1
                .len(),
            0
        );
        let (_, files) = project_links(
            &state,
            &scope,
            Some(tmp.path()),
            "[package](target/test.zip)",
        )
        .await;
        assert_eq!(files.len(), 1);
        std::fs::remove_file(&path).unwrap();
        std::fs::hard_link(&other, &path).unwrap();
        assert!(
            super::super::open_registered(&state, &scope, &files[0].id)
                .await
                .is_err()
        );
    }
}

pub(in crate::remote_sessions) async fn project_links(
    state: &SharedState,
    scope: &Scope,
    cwd: Option<&Path>,
    text: &str,
) -> (String, Vec<RemoteFile>) {
    if !text.contains("](") {
        return (text.into(), Vec::new());
    }
    let dests: Vec<_> = destinations(text)
        .into_iter()
        .filter_map(|r| target(&text[r.clone()]).map(|v| (r, v)))
        .collect();
    if dests.is_empty() {
        return (text.into(), Vec::new());
    }
    let cwd = cwd.map(Path::to_owned);
    let scope = scope.clone();
    let root = root(state, &scope);
    let _gate = state.remote_sessions.file_gate.lock().await;
    let resolved = tokio::task::spawn_blocking(move || -> Result<_> {
        let mut catalog = read_catalog(&root)?;
        let original = catalog.files.len();
        let mut results = Vec::new();
        for (range, value) in dests {
            if let Some(file) = resolve(&mut catalog, &scope, cwd.as_deref(), &value) {
                results.push((range, file));
            }
        }
        if catalog.files.len() != original {
            save_catalog(&root, &catalog)?;
        }
        Ok(results)
    })
    .await;
    let mut output = text.to_owned();
    let mut files = Vec::new();
    if let Ok(Ok(results)) = resolved {
        for (range, file) in results.iter().rev() {
            output.replace_range(range.clone(), &file.url);
        }
        for (_, file) in results {
            if !files.iter().any(|f: &RemoteFile| f.id == file.id) {
                files.push(file);
            }
        }
    }
    (output, files)
}
