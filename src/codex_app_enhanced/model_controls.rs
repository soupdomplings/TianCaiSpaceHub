use std::{
    collections::HashMap,
    sync::LazyLock,
    time::{Duration, Instant},
};

use anyhow::{Result, bail};
use base64::Engine;
use regex::{Captures, Regex};
use serde_json::{Value, json};
use url::Url;

use super::{CdpSocket, send_cdp_request};
use crate::chain_log;

const SOURCE_MARKER: &str = "__CODEXHUB_MODEL_CONTROLS_SOURCE__";
const MAX_SCRIPT_BYTES: usize = 24 * 1024 * 1024;

pub(super) fn startup_reload_guard() -> &'static str {
    // The main process may still be awaiting its first loadURL. Reloading before
    // load has completed cancels that promise and can send the app to an error
    // page. This is installed only on the first target of a fresh Hub launch.
    r#"(() => {
      if (window.__CODEXHUB_MODEL_CONTROLS_RELOAD_SCHEDULED__) return;
      window.__CODEXHUB_MODEL_CONTROLS_RELOAD_SCHEDULED__ = true;
      const afterLoad = () => setTimeout(() => {
        if (window.location.protocol !== 'app:') return;
        const source = window.__CODEXHUB_MODEL_CONTROLS_SOURCE__;
        const ready = source?.version === 1 && source.fastUi === true
          && source.fastRequest === true && source.ultraModels === true;
        if (!ready && window.__CODEXHUB_ENHANCED_MODE__?.modelControlsEnabled === true) {
          window.location.reload();
        }
      }, 250);
      if (document.readyState === 'complete') afterLoad();
      else window.addEventListener('load', afterLoad, {once: true});
    })();"#
}

pub(super) fn fetch_options() -> Value {
    json!({"patterns": [
        {"urlPattern": "app://*/assets/app-initial-*.js", "requestStage": "Response"},
        {"urlPattern": "app://*/assets/use-service-tier-settings-*.js", "requestStage": "Response"},
        {"urlPattern": "app://*/assets/read-service-tier-for-request-*.js", "requestStage": "Response"},
        {"urlPattern": "app://*/assets/model-list-filter-*.js", "requestStage": "Response"}
    ]})
}

fn supported_script_url(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    let Some(name) = url.path().strip_prefix("/assets/") else {
        return false;
    };
    url.scheme() == "app"
        && !name.contains('/')
        && name.ends_with(".js")
        && [
            "app-initial-",
            "use-service-tier-settings-",
            "read-service-tier-for-request-",
            "model-list-filter-",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn local_guard(host: &str) -> String {
    format!(
        "(globalThis.__CODEXHUB_ENHANCED_MODE__?.modelControlsEnabled===true&&{host}===`local`)"
    )
}

#[derive(Default)]
struct AdaptedSource {
    source: String,
    fast_ui: bool,
    fast_request: bool,
    ultra_models: bool,
}

impl AdaptedSource {
    fn changed(&self) -> bool {
        self.fast_ui || self.fast_request || self.ultra_models
    }
}

// Match behavior and validate reused locals, rather than depending on bundle
// hashes or minifier names. Unknown/ambiguous layouts are left intact.
fn replace_unique(
    source: &mut String,
    pattern: &Regex,
    replacement: impl Fn(&Captures<'_>, &str) -> Option<String>,
) -> bool {
    let matches: Vec<_> = pattern
        .captures_iter(source)
        .filter_map(|captures| {
            let matched = captures.get(0)?;
            let next = replacement(&captures, &source[matched.end()..])?;
            Some((matched.range(), next))
        })
        .collect();
    if matches.len() != 1 {
        return false;
    }
    let (range, next) = matches.into_iter().next().expect("one verified match");
    source.replace_range(range, &next);
    true
}

fn adapt_source(source: String) -> AdaptedSource {
    static FAST_UI: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?P<host>[$\w]+)=e\?\.hostId\?\?[$\w]+,(?P<auth>[$\w]+)=[$\w]+\((?P<auth_host>[$\w]+)\),(?P<allowed>[$\w]+)=(?P<chat_auth>[$\w]+)\?\.authMethod===`chatgpt`\|\|(?P<pat_auth>[$\w]+)\?\.authMethod===`personalAccessToken`"#).expect("Fast UI adapter pattern")
    });
    static FAST_REQUEST: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"async function [$\w]+\((?P<scope>[$\w]+),(?P<host>[$\w]+)\)\{let (?P<auth>[$\w]+)=await [$\w]+\((?P<auth_scope>[$\w]+),(?P<auth_host>[$\w]+)\);if\((?P<chat_auth>[$\w]+)!==`chatgpt`&&(?P<pat_auth>[$\w]+)!==`personalAccessToken`\)return null;let (?P<requirements>[$\w]+)=await [$\w]+\((?P<requirements_scope>[$\w]+),(?P<requirements_host>[$\w]+),\{priority:`critical`\}\);return (?P<cache_scope>[$\w]+)\.query\.setData\([$\w]+,\{authMethod:(?P<cache_auth>[$\w]+),hostId:(?P<cache_host>[$\w]+)\},(?P<cache_requirements>[$\w]+)\),[$\w]+\((?P<access_requirements>[$\w]+)\)\}"#).expect("Fast request adapter pattern")
    });
    static ULTRA_MODELS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"hostId:(?P<host>[$\w]+),includeUltraReasoningEffort:(?P<include>[$\w]+),isCustomModelProvider:[$\w]+,limit:[$\w]+,modelCatalogPath:[$\w]+,modelProvider:[$\w]+,teamCatalog:[$\w]+\},\{get:(?P<get>[$\w]+),queryClient:[$\w]+,scope:[$\w]+\}\)=>\{[^;\n]{0,1600}?,(?P<gate>[$\w]+)=(?P<requested>[$\w]+)&&(?P<getter>[$\w]+)\([$\w]+,`1186680773`\);return\{queryKey:"#).expect("Ultra models adapter pattern")
    });
    let mut adapted = AdaptedSource {
        source,
        ..Default::default()
    };
    if adapted.source.contains(SOURCE_MARKER) {
        return adapted;
    }
    adapted.fast_ui = replace_unique(&mut adapted.source, &FAST_UI, |c, tail| {
        if c["auth"] != c["chat_auth"] || c["auth"] != c["pat_auth"] || c["host"] != c["auth_host"]
        {
            return None;
        }
        let tail: String = tail.chars().take(1000).collect();
        let tail = tail.split("}var ").next()?;
        if !tail.contains("serviceTierAccess:")
            || !tail.contains(&format!("={}&&!", &c["allowed"]))
            || !tail.contains(&format!("hostId:{}", &c["host"]))
        {
            return None;
        }
        Some(format!("{}||{}", &c[0], local_guard(&c["host"])))
    });
    adapted.fast_request = replace_unique(&mut adapted.source, &FAST_REQUEST, |c, _| {
        for (expected, names) in [
            (
                "scope",
                &["auth_scope", "requirements_scope", "cache_scope"][..],
            ),
            (
                "host",
                &["auth_host", "requirements_host", "cache_host"][..],
            ),
            ("auth", &["chat_auth", "pat_auth", "cache_auth"][..]),
            (
                "requirements",
                &["cache_requirements", "access_requirements"][..],
            ),
        ] {
            if names.iter().any(|name| c[expected] != c[*name]) {
                return None;
            }
        }
        let denial = format!(
            "if({}!==`chatgpt`&&{}!==`personalAccessToken`)",
            &c["auth"], &c["auth"]
        );
        let scoped_denial = format!(
            "if({}!==`chatgpt`&&{}!==`personalAccessToken`&&!{})",
            &c["auth"],
            &c["auth"],
            local_guard(&c["host"])
        );
        Some(c[0].replacen(&denial, &scoped_denial, 1))
    });
    adapted.ultra_models = replace_unique(&mut adapted.source, &ULTRA_MODELS, |c, tail| {
        if c["include"] != c["requested"] || c["get"] != c["getter"] {
            return None;
        }
        let tail: String = tail.chars().take(1200).collect();
        if !tail.contains("sendRequest(`model/list`")
            || !tail.contains(&format!("includeUltraReasoningEffort:{}", &c["gate"]))
        {
            return None;
        }
        let start = c[0].rfind("&&")?;
        let end = c[0].rfind(";return")?;
        let mut next = c[0].to_string();
        next.replace_range(
            start + 2..end,
            &format!("({}||{})", &c[0][start + 2..end], local_guard(&c["host"])),
        );
        Some(next)
    });
    if adapted.changed() {
        let markers = json!({
            "version": 1,
            "fastUi": adapted.fast_ui,
            "fastRequest": adapted.fast_request,
            "ultraModels": adapted.ultra_models,
        });
        // Separate chunks can contribute independently; never reset a previously
        // applied component when a later module has no match for it.
        let true_markers: serde_json::Map<String, Value> = markers
            .as_object()
            .expect("adapter markers")
            .iter()
            .filter(|(_, value)| value.as_bool() != Some(false))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        adapted.source.push_str(&format!(
            "\n;globalThis.{SOURCE_MARKER}=Object.assign(globalThis.{SOURCE_MARKER}??{{}},{})\n",
            Value::Object(true_markers)
        ));
    }
    adapted
}

struct PausedScript {
    session_id: String,
    request_id: String,
    status: u64,
    headers: Value,
    started: Instant,
}

#[derive(Default)]
pub(super) struct Interceptor {
    pending_bodies: HashMap<u64, PausedScript>,
    pending_fulfillments: HashMap<u64, PausedScript>,
}

impl Interceptor {
    pub(super) async fn release_expired(
        &mut self,
        socket: &mut CdpSocket,
        command_id: &mut u64,
    ) -> Result<()> {
        let mut expired = Vec::new();
        for pending in [&mut self.pending_bodies, &mut self.pending_fulfillments] {
            let ids: Vec<_> = pending
                .iter()
                .filter(|(_, request)| request.started.elapsed() >= Duration::from_secs(10))
                .map(|(id, _)| *id)
                .collect();
            for id in ids {
                if let Some(request) = pending.remove(&id) {
                    expired.push(request);
                }
            }
        }
        for request in expired {
            continue_script(socket, command_id, &request).await?;
            chain_log::write_line(
                "[codex_app_enhanced] event=model_controls_request_released reason=timeout"
                    .to_string(),
            );
        }
        Ok(())
    }

    pub(super) async fn handle(
        &mut self,
        socket: &mut CdpSocket,
        command_id: &mut u64,
        message: &Value,
    ) -> Result<()> {
        if let Some(id) = message.get("id").and_then(Value::as_u64)
            && let Some(paused) = self.pending_fulfillments.remove(&id)
        {
            if message.get("error").is_some() {
                continue_script(socket, command_id, &paused).await?;
                chain_log::write_line("[codex_app_enhanced] event=model_controls_source_preserved reason=fulfill_failed".to_string());
            }
            return Ok(());
        }
        if let Some(id) = message.get("id").and_then(Value::as_u64)
            && let Some(paused) = self.pending_bodies.remove(&id)
        {
            let adapted = decode_and_adapt(message);
            match adapted {
                Ok(adapted) if adapted.changed() => {
                    let mut headers = paused.headers.as_array().cloned().unwrap_or_default();
                    headers.retain(|header| {
                        !["content-length", "content-encoding", "etag"]
                            .iter()
                            .any(|name| {
                                header["name"]
                                    .as_str()
                                    .is_some_and(|actual| actual.eq_ignore_ascii_case(name))
                            })
                    });
                    let id = send_cdp_request(socket, command_id, Some(&paused.session_id), "Fetch.fulfillRequest", json!({
                        "requestId": paused.request_id,
                        "responseCode": paused.status,
                        "responseHeaders": headers,
                        "body": base64::engine::general_purpose::STANDARD.encode(adapted.source.as_bytes()),
                    })).await?;
                    self.pending_fulfillments.insert(id, paused);
                    chain_log::write_line(format!(
                        "[codex_app_enhanced] event=model_controls_source_adapted fast_ui={} fast_request={} ultra_models={}",
                        adapted.fast_ui, adapted.fast_request, adapted.ultra_models
                    ));
                }
                other => {
                    chain_log::write_line(format!(
                        "[codex_app_enhanced] event=model_controls_source_preserved reason={}",
                        if other.is_err() {
                            "body_unavailable"
                        } else {
                            "unsupported_layout"
                        }
                    ));
                    continue_script(socket, command_id, &paused).await?;
                }
            }
            return Ok(());
        }
        if message["method"] != "Fetch.requestPaused" {
            return Ok(());
        }
        let Some(session_id) = message["sessionId"].as_str() else {
            return Ok(());
        };
        let params = &message["params"];
        let Some(request_id) = params["requestId"].as_str() else {
            return Ok(());
        };
        let paused = PausedScript {
            session_id: session_id.to_owned(),
            request_id: request_id.to_owned(),
            status: params["responseStatusCode"].as_u64().unwrap_or(0),
            headers: params["responseHeaders"].clone(),
            started: Instant::now(),
        };
        if paused.status != 200
            || !params["request"]["url"]
                .as_str()
                .is_some_and(supported_script_url)
            || self.pending_bodies.len() + self.pending_fulfillments.len() >= 16
        {
            return continue_script(socket, command_id, &paused).await;
        }
        let id = send_cdp_request(
            socket,
            command_id,
            Some(session_id),
            "Fetch.getResponseBody",
            json!({"requestId": request_id}),
        )
        .await?;
        self.pending_bodies.insert(id, paused);
        Ok(())
    }
}

fn decode_and_adapt(message: &Value) -> Result<AdaptedSource> {
    let result = &message["result"];
    let Some(body) = result["body"].as_str() else {
        bail!("script response body unavailable");
    };
    if body.len() > MAX_SCRIPT_BYTES * 4 / 3 + 4 {
        bail!("script response too large");
    }
    let bytes = if result["base64Encoded"] == true {
        base64::engine::general_purpose::STANDARD.decode(body)?
    } else {
        body.as_bytes().to_vec()
    };
    if bytes.len() > MAX_SCRIPT_BYTES {
        bail!("script response too large");
    }
    Ok(adapt_source(String::from_utf8(bytes)?))
}

async fn continue_script(
    socket: &mut CdpSocket,
    command_id: &mut u64,
    paused: &PausedScript,
) -> Result<()> {
    send_cdp_request(
        socket,
        command_id,
        Some(&paused.session_id),
        "Fetch.continueRequest",
        json!({"requestId": paused.request_id}),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const UI: &str = "function picker(e){let root=getRoot(),host=e?.hostId??root,auth=readAuth(host),allowed=auth?.authMethod===`chatgpt`||auth?.authMethod===`personalAccessToken`,method=auth?.authMethod??null,query={authMethod:method,hostId:host};let data=readRequirements(query),result=allowed&&!loading&&data!=null?access(data):null;return{serviceTierAccess:result,isLoading:loading}}var next;";
    const REQUEST: &str = "async function request(scope,host){let auth=await readAuth(scope,host);if(auth!==`chatgpt`&&auth!==`personalAccessToken`)return null;let requirements=await readRequirements(scope,host,{priority:`critical`});return scope.query.setData(requirementsQuery,{authMethod:auth,hostId:host},requirements),access(requirements)}";
    const MODELS: &str = "modelsQuery=readQuery(scope,({hostId:host,includeUltraReasoningEffort:include,isCustomModelProvider:custom,limit:limit,modelCatalogPath:path,modelProvider:provider,teamCatalog:team},{get:getter,queryClient:queryClient,scope:scope})=>{let enabled=include&&getter(gate,`1186680773`);return{queryKey:[host],queryFn:()=>client.sendRequest(`model/list`,{}),select:({data})=>select({includeUltraReasoningEffort:enabled,models:data})}})";

    #[test]
    fn current_model_controls_are_scoped_and_keep_requirements_and_capabilities() {
        // The models callback has other locals before its final gate expression.
        let models = MODELS.replace("let enabled=", "let catalog=provider,enabled=");
        let adapted = adapt_source(format!("{UI}{REQUEST}{models}"));
        assert!(adapted.fast_ui && adapted.fast_request && adapted.ultra_models);
        assert!(
            adapted
                .source
                .contains("modelControlsEnabled===true&&host===`local`")
        );
        assert!(
            adapted
                .source
                .contains("if(auth!==`chatgpt`&&auth!==`personalAccessToken`&&!")
        );
        assert!(
            adapted
                .source
                .contains("readRequirements(scope,host,{priority:`critical`})")
        );
        assert!(
            adapted
                .source
                .contains("requirements),access(requirements)")
        );
        assert!(adapted.source.contains("models:data"));
        assert!(!adapted.source.contains("authMethod:`chatgpt`"));
    }

    #[test]
    fn mismatched_auth_or_host_does_not_rewrite_request() {
        for altered in [
            REQUEST.replace(
                "auth!==`personalAccessToken`",
                "different!==`personalAccessToken`",
            ),
            REQUEST.replace("hostId:host", "hostId:different"),
            REQUEST.replace("access(requirements)", "access(different)"),
        ] {
            let adapted = adapt_source(altered.clone());
            assert!(!adapted.changed());
            assert_eq!(adapted.source, altered);
        }
    }

    #[test]
    fn unknown_and_ambiguous_modules_keep_original_source() {
        for source in [
            "export const unrelated=true;".to_string(),
            format!("{REQUEST}{REQUEST}"),
        ] {
            let adapted = adapt_source(source.clone());
            assert!(!adapted.changed());
            assert_eq!(adapted.source, source);
        }
    }

    #[test]
    fn only_packaged_model_control_assets_are_eligible() {
        assert!(supported_script_url("app://-/assets/app-initial-123.js"));
        for url in [
            "https://example.local/assets/app-initial-123.js",
            "app://-/assets/unrelated.js",
            "app://-/other/app-initial-123.js",
            "app://-/assets/subdir/app-initial-123.js",
        ] {
            assert!(!supported_script_url(url));
        }
    }
}
