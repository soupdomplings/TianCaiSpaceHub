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
        // Only retry a missed initial capture. A partial adaptation is usable;
        // unsupported optional components must not trigger another refresh.
        if (source?.version !== 2 && window.__CODEXHUB_ENHANCED_MODE__?.modelControlsEnabled === true) {
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
        {"urlPattern": "app://*/assets/model-list-filter-*.js", "requestStage": "Response"},
        {"urlPattern": "app://*/assets/configuration-panel-*.js", "requestStage": "Response"}
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
            "configuration-panel-",
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
    settings_sections: bool,
}

impl AdaptedSource {
    fn changed(&self) -> bool {
        self.fast_ui || self.fast_request || self.ultra_models || self.settings_sections
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
        // Boolean availability and access-state layouts share this auth/host
        // prefix. Validate the matching result and requirements in the tail.
        Regex::new(r#"(?P<host>[$\w]+)=e\?\.hostId\?\?[$\w]+,(?P<auth>[$\w]+)=[$\w]+\((?P<auth_host>[$\w]+)\),(?P<allowed>[$\w]+)=(?P<chat_auth>[$\w]+)\?\.authMethod===`chatgpt`(?:\|\|(?P<pat_auth>[$\w]+)\?\.authMethod===`personalAccessToken`)?,"#).expect("Fast UI adapter pattern")
    });
    static FAST_REQUEST: LazyLock<Regex> = LazyLock::new(|| {
        // Preserve each helper's original return type, query/cache operations
        // and requirements result; only scope its authentication early return.
        Regex::new(r#"async function [$\w]+\((?P<scope>[$\w]+),(?P<host>[$\w]+)\)\{let (?P<auth>[$\w]+)=await [$\w]+\((?P<auth_scope>[$\w]+),(?P<auth_host>[$\w]+)\);(?P<denial>if\((?P<chat_auth>[$\w]+)!==`chatgpt`(?:&&(?P<pat_auth>[$\w]+)!==`personalAccessToken`)?\))return(?: null|!1);let (?P<requirements>[$\w]+)=await [$\w]+\((?P<requirements_scope>[$\w]+),(?P<requirements_host>[$\w]+),\{priority:`critical`\}\);return (?P<cache_scope>[$\w]+)\.query\.setData\([$\w]+,\{authMethod:(?P<cache_auth>[$\w]+),hostId:(?P<cache_host>[$\w]+)\},(?P<cache_requirements>[$\w]+)\),(?:[$\w]+\((?P<access_requirements>[$\w]+)\)|(?P<boolean_requirements>[$\w]+)\.requirements\?\.featureRequirements\?\.fast_mode!==!1)\}"#).expect("Fast request adapter pattern")
    });
    static ULTRA_MODELS: LazyLock<Regex> = LazyLock::new(|| {
        // 26.1007 omits teamCatalog; 26.1013 adds it to the same query.
        // Both layouts still require the exact gate and verified reused locals.
        Regex::new(r#"hostId:(?P<host>[$\w]+),includeUltraReasoningEffort:(?P<include>[$\w]+),isCustomModelProvider:[$\w]+,limit:[$\w]+,modelCatalogPath:[$\w]+,modelProvider:[$\w]+(?:,teamCatalog:[$\w]+)?\},\{get:(?P<get>[$\w]+),queryClient:[$\w]+,scope:[$\w]+\}\)=>\{[^;\n]{0,1600}?,(?P<gate>[$\w]+)=(?P<requested>[$\w]+)&&(?P<getter>[$\w]+)\([$\w]+,`1186680773`\);return\{queryKey:"#).expect("Ultra models adapter pattern")
    });
    static SETTINGS_SECTIONS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"function [$\w]+\(e\)\{let [$\w]+=[^;\n]{0,100}?,\{workConfiguration:(?P<work>[$\w]+)\}=e,\{selectedHostId:(?P<host>[$\w]+)\}=[$\w]+\(\),[^;\n]{0,300}?,(?P<dependencies>[$\w]+)=(?P<gate_fn>[$\w]+)\((?P<dependencies_gate>[$\w]+)\),(?P<other>[$\w]+)=(?P<other_gate_fn>[$\w]+)\(`2106641128`\),(?P<models>[$\w]+)=(?P<models_gate_fn>[$\w]+)\(`3693343337`\)"#).expect("Configuration sections adapter pattern")
    });
    let mut adapted = AdaptedSource {
        source,
        ..Default::default()
    };
    if adapted.source.contains(SOURCE_MARKER) {
        return adapted;
    }
    adapted.fast_ui = replace_unique(&mut adapted.source, &FAST_UI, |c, tail| {
        if c["auth"] != c["chat_auth"]
            || c.name("pat_auth")
                .is_some_and(|auth| auth.as_str() != &c["auth"])
            || c["host"] != c["auth_host"]
        {
            return None;
        }
        let tail: String = tail.chars().take(1000).collect();
        let tail = tail.split("}var ").next()?;
        let recognized_result = tail.contains("serviceTierAccess:")
            || (tail.contains("isServiceTierAllowed:")
                && tail.contains("?.requirements?.featureRequirements?.fast_mode!==!1"));
        if !recognized_result
            || !tail.contains(&format!("={}&&!", &c["allowed"]))
            || !tail.contains(&format!("hostId:{}", &c["host"]))
        {
            return None;
        }
        Some(format!(
            "{}||{},",
            c[0].strip_suffix(',')?,
            local_guard(&c["host"])
        ))
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
            ("auth", &["chat_auth", "cache_auth"][..]),
            ("requirements", &["cache_requirements"][..]),
        ] {
            if names.iter().any(|name| c[expected] != c[*name]) {
                return None;
            }
        }
        let returned_requirements = c
            .name("access_requirements")
            .or_else(|| c.name("boolean_requirements"))?;
        if returned_requirements.as_str() != &c["requirements"]
            || c.name("pat_auth")
                .is_some_and(|auth| auth.as_str() != &c["auth"])
        {
            return None;
        }
        let denial = &c["denial"];
        let scoped_denial = format!(
            "{}&&!{})",
            denial.strip_suffix(')')?,
            local_guard(&c["host"])
        );
        Some(c[0].replacen(denial, &scoped_denial, 1))
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
    // These rows are lazy loaded. Keep the Work-mode exclusion, host kind and
    // primaryRuntime availability checks in the original component, and never
    // wait for a settings page visit during enhanced startup.
    let dependency_gate_matches = SETTINGS_SECTIONS
        .captures(&adapted.source)
        .is_some_and(|c| {
            adapted
                .source
                .contains(&format!("{}=`3026692602`", &c["dependencies_gate"]))
        });
    adapted.settings_sections =
        replace_unique(&mut adapted.source, &SETTINGS_SECTIONS, |c, tail| {
            if c["gate_fn"] != c["other_gate_fn"] || c["gate_fn"] != c["models_gate_fn"] {
                return None;
            }
            if !dependency_gate_matches {
                return None;
            }
            let tail: String = tail.chars().take(4200).collect();
            if !tail.contains(&format!("!{}?.selected&&{}?", &c["work"], &c["models"]))
                || !tail.contains(&format!("{}&&", &c["dependencies"]))
                || !tail.contains(".kind===`local`?")
            {
                return None;
            }
            let guard = local_guard(&c["host"]);
            let mut next = c[0].to_string();
            for (name, gate) in [
                (&c["dependencies"], c["dependencies_gate"].to_string()),
                (&c["models"], "`3693343337`".to_string()),
            ] {
                let original = format!("{name}={}({gate})", &c["gate_fn"]);
                next = next.replacen(
                    &original,
                    &format!("{name}=({}({gate})||{guard})", &c["gate_fn"]),
                    1,
                );
            }
            Some(next)
        });
    if adapted.changed() {
        let markers = json!({
            "version": 2,
            "fastUi": adapted.fast_ui,
            "fastRequest": adapted.fast_request,
            "ultraModels": adapted.ultra_models,
            "settingsSections": adapted.settings_sections,
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
                        ![
                            "content-length",
                            "content-encoding",
                            "etag",
                            "cache-control",
                            "last-modified",
                        ]
                        .iter()
                        .any(|name| {
                            header["name"]
                                .as_str()
                                .is_some_and(|actual| actual.eq_ignore_ascii_case(name))
                        })
                    });
                    headers.push(json!({"name": "Cache-Control", "value": "no-store"}));
                    let id = send_cdp_request(socket, command_id, Some(&paused.session_id), "Fetch.fulfillRequest", json!({
                        "requestId": paused.request_id,
                        "responseCode": paused.status,
                        "responseHeaders": headers,
                        "body": base64::engine::general_purpose::STANDARD.encode(adapted.source.as_bytes()),
                    })).await?;
                    self.pending_fulfillments.insert(id, paused);
                    chain_log::write_line(format!(
                        "[codex_app_enhanced] event=model_controls_source_adapted fast_ui={} fast_request={} ultra_models={} settings_sections={}",
                        adapted.fast_ui,
                        adapted.fast_request,
                        adapted.ultra_models,
                        adapted.settings_sections
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
    const BOOLEAN_UI: &str = "function picker(e){let root=getRoot(),host=e?.hostId??root,auth=readAuth(host),allowed=auth?.authMethod===`chatgpt`,method=auth?.authMethod??null,query={authMethod:method,hostId:host};let{data:requirements,isPending:pending}=readRequirements(query),loading=!!auth?.isLoading||allowed&&pending,result=allowed&&!loading&&requirements!=null&&requirements?.requirements?.featureRequirements?.fast_mode!==!1;return{isServiceTierAllowed:result,isLoading:loading}}var next;";
    const BOOLEAN_REQUEST: &str = "async function request(scope,host){let auth=await readAuth(scope,host);if(auth!==`chatgpt`)return!1;let requirements=await readRequirements(scope,host,{priority:`critical`});return scope.query.setData(requirementsQuery,{authMethod:auth,hostId:host},requirements),requirements.requirements?.featureRequirements?.fast_mode!==!1}";
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
            BOOLEAN_REQUEST.replace("hostId:host", "hostId:different"),
            BOOLEAN_REQUEST.replace("),requirements.requirements?", "),different.requirements?"),
        ] {
            let adapted = adapt_source(altered.clone());
            assert!(!adapted.changed());
            assert_eq!(adapted.source, altered);
        }
    }

    #[test]
    fn boolean_fast_controls_keep_loading_requirements_and_return_types() {
        let adapted = adapt_source(format!("{BOOLEAN_UI}{BOOLEAN_REQUEST}"));
        assert!(adapted.fast_ui && adapted.fast_request);
        assert!(!adapted.ultra_models && !adapted.settings_sections);
        assert!(adapted.source.contains("if(auth!==`chatgpt`&&!"));
        assert!(adapted.source.contains(
            "loading=!!auth?.isLoading||allowed&&pending,result=allowed&&!loading&&requirements!=null&&requirements?.requirements?.featureRequirements?.fast_mode!==!1"
        ));
        assert!(adapted.source.contains(
            "requirements),requirements.requirements?.featureRequirements?.fast_mode!==!1"
        ));
        assert!(
            adapted
                .source
                .contains("readRequirements(scope,host,{priority:`critical`})")
        );
        assert!(adapted.source.contains(
            "query.setData(requirementsQuery,{authMethod:auth,hostId:host},requirements)"
        ));
        assert!(
            adapted
                .source
                .contains("modelControlsEnabled===true&&host===`local`")
        );
        assert!(adapted.source.contains(")return!1;"));
        assert!(!adapted.source.contains("authMethod:`chatgpt`"));
    }

    #[test]
    fn unknown_and_ambiguous_modules_keep_original_source() {
        for source in [
            "export const unrelated=true;".to_string(),
            format!("{REQUEST}{REQUEST}"),
            format!("{REQUEST}{BOOLEAN_REQUEST}"),
            format!("{UI}{BOOLEAN_UI}"),
        ] {
            let adapted = adapt_source(source.clone());
            assert!(!adapted.changed());
            assert_eq!(adapted.source, source);
        }
    }

    #[test]
    fn only_packaged_model_control_assets_are_eligible() {
        assert!(supported_script_url("app://-/assets/app-initial-123.js"));
        assert!(supported_script_url(
            "app://-/assets/configuration-panel-123.js"
        ));
        for url in [
            "https://example.local/assets/app-initial-123.js",
            "app://-/assets/unrelated.js",
            "app://-/other/app-initial-123.js",
            "app://-/assets/subdir/app-initial-123.js",
        ] {
            assert!(!supported_script_url(url));
        }
    }

    #[test]
    fn settings_rows_keep_work_host_and_runtime_checks() {
        let source = "var depGate=`3026692602`;function settings(e){let cache=memo(27),{workConfiguration:work}=e,{selectedHostId:host}=hosts(),kind=hostKind(host),notices=read(noticesQuery,host),ready=read(loaded),deps=gate(depGate),other=gate(`2106641128`),models=gate(`3693343337`),unused;let m=!work?.selected&&models?jsx(ModelFeatures,{hostId:host}):null;let d=work?.selected?null:deps&&kind.kind===`local`?jsx(Dependencies,{hostId:host}):null;return[m,d]}";
        let adapted = adapt_source(source.to_string());
        assert!(adapted.settings_sections);
        assert!(!adapted.fast_ui && !adapted.fast_request && !adapted.ultra_models);
        assert!(adapted.source.contains("deps=(gate(depGate)||"));
        assert!(adapted.source.contains("models=(gate(`3693343337`)||"));
        assert!(adapted.source.contains("!work?.selected&&models?"));
        assert!(adapted.source.contains("deps&&kind.kind===`local`?"));
        let unknown_gate = adapt_source(source.replace("3026692602", "unknown"));
        assert!(!unknown_gate.changed());
    }
}
