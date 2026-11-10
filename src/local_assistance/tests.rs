use super::*;
use crate::ai_gateway::config::ProviderConfig;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};

#[tokio::test]
async fn maintenance_refreshes_without_reenable_and_respects_native_removal() {
    use std::sync::{Arc,Mutex as StdMutex,atomic::{AtomicUsize,Ordering}};
    use axum::response::IntoResponse;
    for mode in ["stale", "upgrade_reload_retry", "concurrent_local", "removed", "paused", "disable", "disable_reload_retry"] {
        let (_temp,state,mut s)=fixture();
        let mut native=settings::native_value(&s,state.config.lock().await.local_listen_port().unwrap());
        if matches!(mode,"upgrade_reload_retry"|"concurrent_local") { native["description"]="previous release description".into(); }
        native["is_connected"]=1.into();
        native["tools_json"]=if mode=="stale" {json!([])} else {json!(web::tools())};
        native["external_metadata"]="keep".into();
        s.native_fingerprint=Some(settings::native_fingerprint(&native));
        if mode=="paused" { native["status"]="inactive".into(); }
        s.native_catalog_pending=false;
        if mode.starts_with("disable") {s.enabled=false;s.native_disable_pending=true;}
        let (_,rev)=settings::read(&state.config_path).unwrap(); settings::write(&state.config_path,&s,&rev).unwrap();
        let (_,revision)=settings::read(&state.config_path).unwrap();
        let saved=Arc::new(StdMutex::new(if mode=="removed"{None}else{Some(native)}));
        let updates=Arc::new(AtomicUsize::new(0));let reloads=Arc::new(AtomicUsize::new(0));let deletes=Arc::new(AtomicUsize::new(0));
        let (read,write,delete,reload)=(saved.clone(),saved.clone(),saved.clone(),reloads.clone());
        let (writes,delete_count)=(updates.clone(),deletes.clone());
        let changed_settings=state.clone();
        let router=axum::Router::new()
            .route("/data/mcp/connections/tiancai-local-assistance",axum::routing::get(move ||{let read=read.clone();async move{
                read.lock().unwrap().clone().map(axum::Json).map(IntoResponse::into_response).unwrap_or_else(||axum::http::StatusCode::NOT_FOUND.into_response())
            }}).put(move |axum::Json(v):axum::Json<Value>|{let write=write.clone();let writes=writes.clone();let changed_settings=changed_settings.clone();async move{
                let count=writes.fetch_add(1,Ordering::SeqCst);write.lock().unwrap().as_mut().unwrap().as_object_mut().unwrap().extend(v.as_object().unwrap().clone());
                if mode=="concurrent_local" && count==0 {
                    let(mut newer,revision)=settings::read(&changed_settings.config_path).unwrap();newer.enabled=false;newer.native_disable_pending=true;newer.token="revoked-fixture-token".into();
                    settings::write(&changed_settings.config_path,&newer,&revision).unwrap();
                }
                axum::Json(json!({"ok":true}))
            }}).delete(move ||{let delete=delete.clone();let delete_count=delete_count.clone();async move{
                delete_count.fetch_add(1,Ordering::SeqCst);*delete.lock().unwrap()=None;axum::Json(json!({"ok":true}))
            }}))
            .route("/ctrl/config/reload",axum::routing::post(move ||{let reload=reload.clone();async move{
                let count=reload.fetch_add(1,Ordering::SeqCst);
                if matches!(mode,"upgrade_reload_retry"|"disable_reload_retry") && count==0 {axum::http::StatusCode::SERVICE_UNAVAILABLE} else {axum::http::StatusCode::OK}
            }}));
        let (base,job)=server(router).await;let client=crate::gmclaw_desktop::DesktopClient::fixture(&base);
        if mode.starts_with("disable") {
            assert!(web::view(&state).await.unwrap()["hostDetail"].as_str().unwrap().contains("等待天工"));
            let result=web::sync_disabled_native(&state,s,&revision,&client).await;
            if mode=="disable_reload_retry" {
                assert!(result.is_err());
                let(next,revision)=settings::read(&state.config_path).unwrap();assert!(next.native_disable_pending);
                web::sync_disabled_native(&state,next,&revision,&client).await.unwrap();
            } else { result.unwrap(); }
            assert!(saved.lock().unwrap().is_none());assert_eq!(deletes.load(Ordering::SeqCst),1);
            assert_eq!(updates.load(Ordering::SeqCst),0);
            assert!(!settings::read(&state.config_path).unwrap().0.native_disable_pending);
            assert!(web::view(&state).await.unwrap()["hostDetail"].as_str().unwrap().contains("已从天工移除"));
        } else {
            let first=web::sync_catalogue_native(&state,s.clone(),&revision,&client,json!(web::tools())).await;
            assert_eq!(first.is_ok(),!matches!(mode,"upgrade_reload_retry"|"concurrent_local"),"{mode}: {first:?}");
            let (next,revision)=settings::read(&state.config_path).unwrap();
            if mode=="concurrent_local" {
                assert!(!next.enabled && next.native_disable_pending);assert_eq!(next.token,"revoked-fixture-token");
                assert_eq!(updates.load(Ordering::SeqCst),1);assert_eq!(reloads.load(Ordering::SeqCst),0);
            } else if matches!(mode,"removed"|"paused") {
                assert!(!next.enabled);assert_eq!(next.native_fingerprint.is_none(),mode=="removed");assert_ne!(next.token,s.token);
                assert_eq!(updates.load(Ordering::SeqCst),0);assert_eq!(reloads.load(Ordering::SeqCst),0);
            } else {
                assert!(next.enabled);
                web::sync_catalogue_native(&state,next,&revision,&client,json!(web::tools())).await.unwrap();
                let (next,revision)=settings::read(&state.config_path).unwrap();
                assert!(!next.native_catalog_pending);
                let writes=updates.load(Ordering::SeqCst);let reloaded=reloads.load(Ordering::SeqCst);
                web::sync_catalogue_native(&state,next,&revision,&client,json!(web::tools())).await.unwrap();
                assert_eq!(updates.load(Ordering::SeqCst),writes);assert_eq!(reloads.load(Ordering::SeqCst),reloaded);
                assert_eq!(settings::read(&state.config_path).unwrap().1,revision);
                let actual=saved.lock().unwrap().clone().unwrap();
                assert_eq!(actual["tools_json"],json!(web::tools()));assert_eq!(actual["external_metadata"],"keep");
                assert_eq!(actual["header_config"]["Authorization"],format!("Bearer {}",s.token));
            }
        }
        job.abort();
    }
}

#[tokio::test]
async fn unavailable_native_api_never_revokes_settings_as_if_deleted() {
    let (_temp,state,mut s)=fixture();s.native_fingerprint=Some("owned-fixture".into());
    let (_,revision)=settings::read(&state.config_path).unwrap();settings::write(&state.config_path,&s,&revision).unwrap();
    let (_,revision)=settings::read(&state.config_path).unwrap();
    let (base,job)=server(axum::Router::new().fallback(||async{axum::http::StatusCode::SERVICE_UNAVAILABLE})).await;
    let client=crate::gmclaw_desktop::DesktopClient::fixture(&base);
    assert!(web::sync_catalogue_native(&state,s.clone(),&revision,&client,json!(web::tools())).await.is_err());
    let (after,after_revision)=settings::read(&state.config_path).unwrap();
    assert!(after.enabled);assert_eq!(after.token,s.token);assert_eq!(revision,after_revision);job.abort();
}

fn fixture() -> (tempfile::TempDir, SharedState, settings::Settings) {
    let temp = tempfile::tempdir().unwrap();
    let provider = ProviderConfig {
        name: "合成普通来源".into(),
        base_url: "http://127.0.0.1:9/v1".into(),
        models: vec!["fixture-model".into()],
        ..Default::default()
    };
    let mut config = crate::config::AppConfig {
        state_path: temp.path().join("state.json"),
        ..Default::default()
    };
    config.ai_gateway.enabled = true;
    config.ai_gateway.providers = vec![provider.clone()];
    config.ai_gateway.codex_visible_models = vec!["fixture-model".into()];
    let state =
        crate::app_state::AppState::new(temp.path().join("config.json"), config, None, None);
    let binary = temp
        .path()
        .join(if cfg!(windows) { "codex.exe" } else { "codex" });
    std::fs::write(&binary, b"synthetic fixture; never executable").unwrap();
    let s = settings::Settings {
        enabled: true,
        model: "fixture-model".into(),
        route: crate::ai_gateway::config::provider_route_id(&provider),
        binary: Some(binary),
        token: "fixture-mcp-token".into(),
        ..Default::default()
    };
    let (_, revision) = settings::read(&state.config_path).unwrap();
    settings::write(&state.config_path, &s, &revision).unwrap();
    (temp, state, s)
}
async fn parent(state: &SharedState, root: &std::path::Path, session: &str) -> String {
    state.local_assistance.inner.lock().await.host = Some("fixture-host".into());
    host::event_from_host(state,"fixture-host",&json!({"op":"register","parent":{"session_id":session,"request_id":format!("req-{session}"),"query":"检查合成项目，不得发布或发送消息","project":{"path":root},"tool_execution_mode":"confirm"}})).await.unwrap()["contextRef"].as_str().unwrap().into()
}
async fn reserved(state: &SharedState, context: &str, key: &str) -> Work {
    reserve(state, context, "分析这个合成失败，只读取", key, true)
        .await
        .unwrap()
        .1
        .unwrap()
}
fn prompt(method: &str, params: Value) -> Prompt {
    Prompt {
        nonce: settings::token(),
        request_id: json!(91),
        method: method.into(),
        params,
        at_ms: crate::types::now_ms(),
    }
}
async fn read(reader: &mut BufReader<tokio::io::ReadHalf<DuplexStream>>) -> Value {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(3), reader.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str(&line).unwrap()
}
async fn write(writer: &mut tokio::io::WriteHalf<DuplexStream>, value: Value) {
    let mut data = serde_json::to_vec(&value).unwrap();
    data.push(b'\n');
    writer.write_all(&data).await.unwrap();
}
async fn wait_phase(state: &SharedState, id: &str, phase: Phase) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if state.local_assistance.inner.lock().await.tasks[id].phase == phase {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}
struct Fake {
    id: String,
    context: String,
    reader: BufReader<tokio::io::ReadHalf<DuplexStream>>,
    writer: tokio::io::WriteHalf<DuplexStream>,
    job: tokio::task::JoinHandle<Result<()>>,
}
async fn fake(state: &SharedState, root: &std::path::Path) -> Fake {
    fake_with_effort(state, root, None).await
}
async fn fake_with_effort(state: &SharedState, root: &std::path::Path, effort: Option<&str>) -> Fake {
    let context = parent(state, root, "fake-session").await;
    let work = reserved(state, &context, "fake-problem").await;
    let id = work.id.clone();
    state.local_assistance.inner.lock().await.tasks.get_mut(&id).unwrap().reasoning_effort=effort.map(str::to_string);
    let (hub, server) = tokio::io::duplex(MAX_WIRE * 2);
    let (r, mut w) = tokio::io::split(hub);
    let (r2, w2) = tokio::io::split(server);
    let s = state.clone();
    let task = id.clone();
    let mut control = work.control;
    let job = tokio::spawn(async move {
        worker::conversation_io(
            &s,
            &task,
            &work.parent,
            "fixture-model",
            "合成定位",
            false,
            &mut w,
            &mut BufReader::new(r),
            &mut control,
        )
        .await
    });
    let mut f = Fake {
        id,
        context,
        reader: BufReader::new(r2),
        writer: w2,
        job,
    };
    let init = read(&mut f.reader).await;
    assert_eq!(init["method"], "initialize");
    write(&mut f.writer, json!({"id":init["id"],"result":{}})).await;
    assert_eq!(read(&mut f.reader).await["method"], "initialized");
    let thread = read(&mut f.reader).await;
    assert_eq!(thread["method"], "thread/start");
    assert_eq!(thread["params"]["sandbox"], "read-only");
    assert_eq!(thread["params"]["approvalsReviewer"], "user");
    write(
        &mut f.writer,
        json!({"id":thread["id"],"result":{"thread":{"id":"thread-fixture"}}}),
    )
    .await;
    let turn = read(&mut f.reader).await;
    assert_eq!(turn["method"], "turn/start");
    assert_eq!(turn["params"]["approvalPolicy"], "on-request");
    assert_eq!(turn["params"]["sandboxPolicy"]["type"], "readOnly");
    assert_eq!(turn["params"].get("effort").and_then(Value::as_str),effort);
    assert!(
        turn["params"]["input"][0]["text"]
            .as_str()
            .unwrap()
            .contains("不得发布")
    );
    write(
        &mut f.writer,
        json!({"id":turn["id"],"result":{"turn":{"id":"turn-1","status":"inProgress"}}}),
    )
    .await;
    wait_phase(state, &f.id, Phase::Running).await;
    f
}
async fn completed(f: &mut Fake, state: &SharedState) {
    write(&mut f.writer,json!({"method":"turn/completed","params":{"threadId":"thread-fixture","turn":{"id":"turn-1","status":"completed"}}})).await;
    wait_phase(state, &f.id, Phase::Completed).await;
}

#[test]
fn no_approval_tool_is_exposed_to_models() {
    let tools = web::tools();
    assert_eq!(tools.len(), 5);
    assert!(
        tools
            .iter()
            .all(|t| !t["name"].as_str().unwrap().contains("approv"))
    );
}
fn native_catalogue_fixture() -> Value {
    json!({"server_name":settings::MCP_NAME,"server_id":settings::MCP_NAME,"status":"active","is_connected":1,
        "tools_json":web::tools(),"header_config":{"Authorization":"fixture-secret-never-in-prompts"},"token":"fixture-secret-never-in-prompts"})
}
#[test]
fn native_extra_prompt_keeps_the_complete_allowed_catalogue_without_credentials() {
    let own=native_catalogue_fixture();
    let other=json!({"server_name":"fixture-other","is_connected":1,"tools":{"lookup":{"description":"合成查询","input_schema":{"type":"object","properties":{"term":{"type":"string"}},"required":["term"]}}},
        "server_url":"http://127.0.0.1/fixture-private-address","header_param":{"Authorization":"fixture-other-secret"}});
    let mut disabled=other.clone();disabled["server_name"]="fixture-disabled".into();disabled["is_connected"]=0.into();
    let text=catalogue::prompt(&[own.clone(),other.clone(),disabled],&[]).unwrap();
    assert!(text.contains("fixture-other") && text.contains("lookup") && text.contains("context_ref") && text.contains("required"));
    for absent in ["fixture-disabled","fixture-secret-never-in-prompts","fixture-other-secret","fixture-private-address"] {assert!(!text.contains(absent));}
    if let Some(dir)=std::env::var_os("TIANCAI_NATIVE_REVIEW_DIR") {
        let dir=PathBuf::from(dir);std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("assistance-prompt-contract.json"),serde_json::to_vec(&json!({
            "context":"fixture-contract-context","cataloguePrompt":text,
            "systemPrompt":catalogue::instructions("fixture-contract-context",&text)
        })).unwrap()).unwrap();
    }
    let selected=catalogue::prompt(&[own.clone(),other.clone()],&[settings::MCP_NAME.into()]).unwrap();
    assert!(!selected.contains("fixture-other"));
    assert!(catalogue::prompt(&[own,other],&["fixture-other".into()]).is_err());
}
#[test]
fn incomplete_or_disabled_native_assistance_catalogue_never_produces_ready_rules() {
    let mut own=native_catalogue_fixture();
    own["tools_json"][0]["inputSchema"]["required"]=json!(["goal","problem_key"]);
    assert!(catalogue::prompt(&[own],&[]).unwrap_err().to_string().contains("参数不完整"));
    for value in [json!(0),json!(false),json!("0"),json!("FALSE")] {
        let mut own=native_catalogue_fixture();own["is_connected"]=value;
        assert!(catalogue::prompt(&[own],&[]).is_err());
    }
    let mut own=native_catalogue_fixture();own.as_object_mut().unwrap().remove("is_connected");
    assert!(catalogue::prompt(&[own],&[]).is_err());
}
#[tokio::test]
async fn generated_mcp_call_examples_bind_the_exact_native_task_context() {
    let (temp,state,_)=fixture();
    let one=parent(&state,temp.path(),"one").await;
    let two=parent(&state,temp.path(),"two").await;
    assert_ne!(one,two);
    let catalog=catalogue::prompt(&[native_catalogue_fixture()],&[]).unwrap();
    for context in [&one,&two] {
        let prompt=catalogue::instructions(context,&catalog);
        let calls:Vec<Value>=prompt.lines().filter(|l|l.starts_with('{')).map(|l|serde_json::from_str(l).unwrap()).collect();
        assert_eq!(calls.len(),2);
        for call in &calls {
            assert_eq!(call["mcp_id"],settings::MCP_NAME);assert_eq!(call["arguments"]["context_ref"],*context);
            assert!(call.get("context_ref").is_none());
        }
        assert_eq!(calls[0]["tool_name"],"assist_start");assert_eq!(calls[1]["tool_name"],"assist_wait");
    }
    let missing=web::tool(&state,&json!({"name":"assist_start","arguments":{"goal":"合成分析","problem_key":"fixture"}})).await.unwrap_err();
    assert!(missing.to_string().contains("arguments.context_ref") && missing.to_string().contains("不要向用户索取"));
    assert!(state.local_assistance.inner.lock().await.tasks.is_empty());
}
#[test]
fn reasoning_defaults_to_xhigh_when_available_and_rejects_lost_capabilities() {
    let (_,state,mut settings)=fixture();
    let mut config=state.config.try_lock().unwrap().clone();
    config.ai_gateway.providers[0].models=vec!["gpt-5.6-sol".into(),"fixture-unknown".into()];
    config.ai_gateway.codex_visible_models=config.ai_gateway.providers[0].models.clone();
    settings.model="gpt-5.6-sol".into();settings.route=crate::ai_gateway::config::provider_route_id(&config.ai_gateway.providers[0]);
    assert_eq!(settings::resolved_effort(&config,&settings).unwrap().as_deref(),Some("xhigh"));
    settings.reasoning_effort=Some("high".into());assert_eq!(settings::resolved_effort(&config,&settings).unwrap().as_deref(),Some("high"));
    let bytes=serde_json::to_vec(&settings).unwrap();let restored:settings::Settings=serde_json::from_slice(&bytes).unwrap();assert_eq!(restored.reasoning_effort,settings.reasoning_effort);
    config.ai_gateway.codex_model_profiles.push(crate::ai_gateway::config::CodexModelProfile{id:settings.model.clone(),supports_reasoning:Some(false),..Default::default()});
    assert!(settings::resolved_effort(&config,&settings).is_err());assert_eq!(settings.reasoning_effort.as_deref(),Some("high"));
    settings.model="fixture-unknown".into();settings.reasoning_effort=None;
    assert_eq!(settings::resolved_effort(&config,&settings).unwrap(),None);
    let old:settings::Settings=serde_json::from_value(json!({"enabled":false,"model":"legacy","route":"legacy"})).unwrap();assert_eq!(old.reasoning_effort,None);
}
#[test]
fn command_approval_is_once_only() {
    let p = Prompt {
        nonce: "fixture".into(),
        request_id: json!(1),
        method: "item/commandExecution/requestApproval".into(),
        params: json!({"availableDecisions":["accept","decline"]}),
        at_ms: 0,
    };
    assert_eq!(
        protocol::decision(&p, &json!({"choice":"allow"})).unwrap(),
        json!({"decision":"accept"})
    );
    assert_eq!(
        protocol::decision(&p, &json!({"choice":"deny"})).unwrap(),
        json!({"decision":"decline"})
    );
}
#[test]
fn mismatched_completion_cannot_finish_another_turn() {
    let v = json!({"method":"turn/completed","params":{"threadId":"a","turn":{"id":"b","status":"completed"}}});
    assert_eq!(
        protocol::turn_outcome(&v, "a", Some("b")),
        Some(Phase::Completed)
    );
    assert_eq!(protocol::turn_outcome(&v, "a", Some("older")), None);
    assert_eq!(protocol::turn_outcome(&v, "another", Some("b")), None);
}

#[tokio::test]
async fn wire_reader_preserves_partial_json_across_cancelled_waits() {
    let (a, mut b) = tokio::io::duplex(128);
    let mut reader = worker::WireReader::new(BufReader::new(a));
    b.write_all(b"{\"fixture\":").await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(20), reader.next())
            .await
            .is_err()
    );
    b.write_all(b"true}\n").await.unwrap();
    assert_eq!(reader.next().await.unwrap(), Some(json!({"fixture":true})));
}
#[tokio::test]
async fn wire_reader_handles_byte_fragments_and_multiple_frames() {
    let (a, mut b) = tokio::io::duplex(8);
    let mut reader = worker::WireReader::new(BufReader::new(a));
    let writer = tokio::spawn(async move {
        for byte in b"{\"a\":1}\n{\"b\":2}\n" {
            b.write_all(&[*byte]).await.unwrap();
            tokio::task::yield_now().await;
        }
    });
    assert_eq!(reader.next().await.unwrap(), Some(json!({"a":1})));
    assert_eq!(reader.next().await.unwrap(), Some(json!({"b":2})));
    assert!(reader.next().await.unwrap().is_none());
    writer.await.unwrap();
}
#[tokio::test]
async fn wire_reader_refuses_truncated_and_oversized_messages() {
    let mut reader = worker::WireReader::new(std::io::Cursor::new(b"{\"a\":1"));
    assert!(
        reader
            .next()
            .await
            .unwrap_err()
            .to_string()
            .contains("不完整")
    );
    let mut reader = worker::WireReader::new(std::io::Cursor::new(vec![b' '; MAX_WIRE + 1]));
    assert!(
        reader
            .next()
            .await
            .unwrap_err()
            .to_string()
            .contains("超过限制")
    );
}
#[tokio::test]
async fn admission_is_idempotent_and_never_borrows_another_context() {
    let (temp, state, _) = fixture();
    let ctx = parent(&state, temp.path(), "s1").await;
    let first = reserved(&state, &ctx, "same").await;
    let (second, work) = reserve(&state, &ctx, "分析这个合成失败，只读取", "same", true)
        .await
        .unwrap();
    assert_eq!(second["id"], first.id);
    assert!(work.is_none());
    assert_eq!(state.local_assistance.inner.lock().await.tasks.len(), 1);
    assert!(
        reserve(&state, &ctx, "另一个目标", "same", true)
            .await
            .is_err()
    );
    assert!(task_view(&state, "wrong", &first.id).await.is_err());
}
#[tokio::test]
async fn cleared_idempotency_tombstone_prevents_replaying_a_problem() {
    let (temp, state, _) = fixture();
    let ctx = parent(&state, temp.path(), "s1").await;
    let work = reserved(&state, &ctx, "same").await;
    state
        .local_assistance
        .inner
        .lock()
        .await
        .tasks
        .remove(&work.id);
    let (view, new) = reserve(&state, &ctx, "分析这个合成失败，只读取", "same", true)
        .await
        .unwrap();
    assert!(new.is_none());
    assert_eq!(view["phase"], "cleared");
}
#[tokio::test]
async fn same_project_and_ancestor_paths_are_serialized() {
    let (temp, state, _) = fixture();
    let ctx = parent(&state, temp.path(), "s1").await;
    let _one = reserved(&state, &ctx, "one").await;
    let child = temp.path().join("child");
    std::fs::create_dir(&child).unwrap();
    let ctx2 = parent(&state, &child, "s2").await;
    assert!(
        reserve(&state, &ctx2, "合成第二任务", "two", true)
            .await
            .is_err()
    );
    assert!(paths_overlap(temp.path(), &child));
    assert!(!paths_overlap(
        &temp.path().join("x"),
        &temp.path().join("y")
    ));
}
#[tokio::test]
async fn completed_old_task_cannot_follow_up_over_a_new_project_task() {
    let (temp, state, _) = fixture();
    let ctx = parent(&state, temp.path(), "s1").await;
    let one = reserved(&state, &ctx, "one").await;
    state
        .local_assistance
        .inner
        .lock()
        .await
        .tasks
        .get_mut(&one.id)
        .unwrap()
        .phase = Phase::Completed;
    let two = reserved(&state, &ctx, "two").await;
    assert!(
        control(
            &state,
            Some(&ctx),
            &one.id,
            Control::Followup("新证据".into())
        )
        .await
        .is_err()
    );
    assert_eq!(
        state.local_assistance.inner.lock().await.tasks[&two.id].phase,
        Phase::Starting
    );
}
#[tokio::test]
async fn full_control_is_still_intersected_with_user_and_setup_limits() {
    let (temp, state, mut settings) = fixture();
    let ctx = parent(&state, temp.path(), "s1").await;
    let work = reserved(&state, &ctx, "one").await;
    assert!(!work.writable);
    {
        let mut inner = state.local_assistance.inner.lock().await;
        inner.tasks.get_mut(&work.id).unwrap().worker_stopped = true;
        inner.contexts.get_mut(&ctx).unwrap().writable = true;
    }
    let (_, rev) = settings::read(&state.config_path).unwrap();
    settings.allow_workspace_write = true;
    settings::write(&state.config_path, &settings, &rev).unwrap();
    let analysis = reserved(&state, &ctx, "two").await;
    assert!(!analysis.writable);
    state
        .local_assistance
        .inner
        .lock()
        .await
        .tasks
        .get_mut(&analysis.id)
        .unwrap()
        .worker_stopped = true;
    let (_, repair) = reserve(&state, &ctx, "修复明确授权的项目", "three", false)
        .await
        .unwrap();
    assert!(repair.unwrap().writable);
}
#[tokio::test]
async fn closing_tasks_cannot_be_followed_up_and_a_full_queue_keeps_the_old_state() {
    let (temp, state, _) = fixture();
    let ctx = parent(&state, temp.path(), "s1").await;
    let work = reserved(&state, &ctx, "one").await;
    {
        let mut inner = state.local_assistance.inner.lock().await;
        let t = inner.tasks.get_mut(&work.id).unwrap();
        t.phase = Phase::Completed;
        for _ in 0..16 {
            t.controller.try_send(Control::Cancel).unwrap();
        }
    }
    assert!(
        control(
            &state,
            Some(&ctx),
            &work.id,
            Control::Followup("证据".into())
        )
        .await
        .is_err()
    );
    assert_eq!(
        state.local_assistance.inner.lock().await.tasks[&work.id].phase,
        Phase::Completed
    );
}
#[tokio::test]
async fn disconnect_revokes_contexts_and_model_requests_immediately() {
    let (temp, state, _) = fixture();
    let ctx = parent(&state, temp.path(), "s1").await;
    let work = reserved(&state, &ctx, "one").await;
    let token = state.local_assistance.inner.lock().await.tasks[&work.id]
        .api_token
        .clone();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    assert!(web::model_pin(&state, &work.id, &headers).await.is_ok());
    disconnect(&state, "fixture-host").await;
    assert!(web::model_pin(&state, &work.id, &headers).await.is_err());
    assert!(ensure_live(&state, &work.id).await.is_err());
}
#[tokio::test]
async fn pending_approval_is_human_only_single_use_and_host_scoped() {
    let (temp, state, _) = fixture();
    let ctx = parent(&state, temp.path(), "s1").await;
    let mut work = reserved(&state, &ctx, "one").await;
    let p = prompt(
        "item/commandExecution/requestApproval",
        json!({"availableDecisions":["accept","decline"]}),
    );
    let nonce = p.nonce.clone();
    state
        .local_assistance
        .inner
        .lock()
        .await
        .tasks
        .get_mut(&work.id)
        .unwrap()
        .pending
        .push(p);
    let event = json!({"op":"decision","task":work.id,"nonce":nonce,"response":{"choice":"allow"}});
    assert!(
        host::event_from_host(&state, "wrong-host", &event)
            .await
            .is_err()
    );
    let mut stale = event.clone();
    stale["nonce"] = "stale".into();
    assert!(
        host::event_from_host(&state, "fixture-host", &stale)
            .await
            .is_err()
    );
    assert!(
        host::event_from_host(&state, "fixture-host", &event)
            .await
            .is_ok()
    );
    assert!(
        host::event_from_host(&state, "fixture-host", &event)
            .await
            .is_err()
    );
    assert!(matches!(
        work.control.recv().await,
        Some(Control::Decision { .. })
    ));
    assert!(
        web::tool(
            &state,
            &json!({"name":"approve","arguments":{"context_ref":ctx,"task":work.id}})
        )
        .await
        .is_err()
    );
}
#[test]
fn single_turn_permissions_cannot_be_expanded_by_the_reply() {
    let p = prompt(
        "item/permissions/requestApproval",
        json!({"permissions":{"fileSystem":{"write":["fixture"]}}}),
    );
    let result = protocol::decision(
        &p,
        &json!({"choice":"allow","permissions":{"network":true},"scope":"session"}),
    )
    .unwrap();
    assert_eq!(result["scope"], "turn");
    assert_eq!(result["permissions"], p.params["permissions"]);
    assert_eq!(
        protocol::decision(&p, &json!({"choice":"deny"})).unwrap()["permissions"],
        json!({})
    );
}
#[test]
fn unsupported_and_session_only_approvals_fail_closed() {
    assert!(!protocol::supported(
        "mcpServer/elicitation/request",
        &json!({"mode":"url"})
    ));
    assert!(!protocol::supported("future/approval", &json!({})));
    let p = prompt(
        "item/commandExecution/requestApproval",
        json!({"availableDecisions":["acceptForSession","decline"]}),
    );
    assert!(protocol::decision(&p, &json!({"choice":"allow"})).is_err());
    assert!(protocol::decision(&p, &json!({"choice":"deny"})).is_ok());
}
#[test]
fn questions_preserve_options_and_forms_keep_primitive_types() {
    let p = prompt(
        "item/tool/requestUserInput",
        json!({"questions":[{"id":"q","question":"选择？","options":[{"label":"只读"}],"isOther":false}]}),
    );
    assert_eq!(
        protocol::decision(&p, &json!({"choice":"allow","answers":{"q":"只读"}})).unwrap()["answers"]
            ["q"]["answers"],
        json!(["只读"])
    );
    assert!(protocol::decision(&p, &json!({"choice":"allow","answers":{"q":"任意扩大"}})).is_err());
    let form = prompt(
        "mcpServer/elicitation/request",
        json!({"mode":"form","requestedSchema":{"properties":{"n":{"type":"integer","minimum":1,"maximum":4},"yes":{"type":"boolean"}},"required":["n"]}}),
    );
    assert!(protocol::supported(&form.method, &form.params));
    assert_eq!(
        protocol::decision(
            &form,
            &json!({"choice":"allow","answers":{"n":2,"yes":false}})
        )
        .unwrap()["content"]["yes"],
        false
    );
    assert!(protocol::decision(&form, &json!({"choice":"allow","answers":{"n":8}})).is_err());
    assert!(protocol::decision(&form, &json!({"choice":"allow","answers":{"n":"2"}})).is_err());
}
#[test]
fn only_verified_main_execution_context_can_send_host_events() {
    let event = json!({"method":"Runtime.bindingCalled","params":{"name":"fixture-binding","executionContextId":11}});
    assert!(host::binding_event(&event, "fixture-binding", 11));
    assert!(!host::binding_event(&event, "fixture-binding", 12));
    assert!(!host::binding_event(&event, "wrong-binding", 11));
}
#[tokio::test]
async fn terminal_result_and_evidence_reach_the_same_tiangong_task() {
    let (temp, state, _) = fixture();
    let mut f = fake(&state, temp.path()).await;
    write(&mut f.writer,json!({"method":"item/completed","params":{"threadId":"thread-fixture","turnId":"turn-1","item":{"id":"msg","type":"agentMessage","text":"已定位：合成配置缺少字段。检查通过，未修改文件。"}}})).await;
    write(&mut f.writer,json!({"method":"item/completed","params":{"threadId":"thread-fixture","turnId":"turn-1","item":{"id":"cmd","type":"commandExecution","command":"fixture-check","exitCode":0,"status":"completed","aggregatedOutput":"synthetic check passed"}}})).await;
    completed(&mut f, &state).await;
    let view = task_view(&state, &f.context, &f.id).await.unwrap();
    assert!(view["result"].as_str().unwrap().contains("已定位"));
    assert_eq!(view["commands"][0]["exitCode"], 0);
    assert_eq!(view["commands"][0]["output"], "synthetic check passed");
    control(&state, Some(&f.context), &f.id, Control::Finish)
        .await
        .unwrap();
    assert!(f.job.await.unwrap().is_ok());
}
#[tokio::test]
async fn proposed_or_declined_files_are_not_reported_as_changed() {
    let (temp, state, _) = fixture();
    let mut f = fake(&state, temp.path()).await;
    for (status, method, path) in [
        ("inProgress", "item/started", "proposed.txt"),
        ("declined", "item/completed", "declined.txt"),
        ("completed", "item/completed", "changed.txt"),
    ] {
        write(&mut f.writer,json!({"method":method,"params":{"threadId":"thread-fixture","turnId":"turn-1","item":{"id":path,"type":"fileChange","status":status,"changes":[{"path":path,"diff":"synthetic diff"}]}}})).await;
    }
    completed(&mut f, &state).await;
    let view = task_view(&state, &f.context, &f.id).await.unwrap();
    assert_eq!(view["files"], json!(["changed.txt"]));
    control(&state, Some(&f.context), &f.id, Control::Finish)
        .await
        .unwrap();
    assert!(f.job.await.unwrap().is_ok());
}
#[tokio::test]
async fn approval_round_trip_uses_original_item_preview_and_never_auto_approves() {
    let (temp, state, _) = fixture();
    let mut f = fake(&state, temp.path()).await;
    write(&mut f.writer,json!({"method":"item/started","params":{"threadId":"thread-fixture","turnId":"turn-1","item":{"id":"edit","type":"fileChange","status":"inProgress","changes":[{"path":"fixture.txt","diff":"+ synthetic"}]}}})).await;
    write(&mut f.writer,json!({"id":91,"method":"item/fileChange/requestApproval","params":{"threadId":"thread-fixture","turnId":"turn-1","itemId":"edit","reason":"请确认此次修改"}})).await;
    wait_phase(&state, &f.id, Phase::WaitingUser).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(30), read(&mut f.reader))
            .await
            .is_err()
    );
    let v = task_view(&state, &f.context, &f.id).await.unwrap();
    let p = &v["pending"][0];
    assert_eq!(
        p["params"]["operationPreview"]["changes"][0]["path"],
        "fixture.txt"
    );
    host::event_from_host(
        &state,
        "fixture-host",
        &json!({"op":"decision","task":f.id,"nonce":p["nonce"],"response":{"choice":"deny"}}),
    )
    .await
    .unwrap();
    let reply = read(&mut f.reader).await;
    assert_eq!(reply, json!({"id":91,"result":{"decision":"decline"}}));
    completed(&mut f, &state).await;
    control(&state, Some(&f.context), &f.id, Control::Finish)
        .await
        .unwrap();
    assert!(f.job.await.unwrap().is_ok());
}
#[tokio::test]
async fn stale_turn_events_and_reasoning_do_not_pollute_the_current_result() {
    let (temp, state, _) = fixture();
    let mut f = fake(&state, temp.path()).await;
    for (thread, turn, kind, text) in [
        ("wrong", "turn-1", "agentMessage", "wrong thread"),
        ("thread-fixture", "old-turn", "agentMessage", "old turn"),
        ("thread-fixture", "turn-1", "reasoning", "private reasoning"),
    ] {
        write(&mut f.writer,json!({"method":"item/completed","params":{"threadId":thread,"turnId":turn,"item":{"type":kind,"text":text}}})).await;
    }
    completed(&mut f, &state).await;
    assert_eq!(
        task_view(&state, &f.context, &f.id).await.unwrap()["result"],
        ""
    );
    control(&state, Some(&f.context), &f.id, Control::Finish)
        .await
        .unwrap();
    assert!(f.job.await.unwrap().is_ok());
}
#[tokio::test]
async fn failures_carry_the_actual_reason_and_are_not_silently_retried() {
    let (temp, state, _) = fixture();
    let mut f = fake(&state, temp.path()).await;
    write(&mut f.writer,json!({"method":"turn/completed","params":{"threadId":"thread-fixture","turn":{"id":"turn-1","status":"failed","error":{"message":"合成渠道额度不足"}}}})).await;
    wait_phase(&state, &f.id, Phase::Failed).await;
    assert_eq!(
        task_view(&state, &f.context, &f.id).await.unwrap()["detail"],
        "合成渠道额度不足"
    );
    control(&state, Some(&f.context), &f.id, Control::Finish)
        .await
        .unwrap();
    assert!(f.job.await.unwrap().is_ok());
}
#[tokio::test]
async fn stop_is_acknowledged_by_the_current_turn_instead_of_restarting() {
    let (temp, state, _) = fixture();
    let mut f = fake(&state, temp.path()).await;
    control(&state, Some(&f.context), &f.id, Control::Cancel)
        .await
        .unwrap();
    let interrupt = read(&mut f.reader).await;
    assert_eq!(interrupt["method"], "turn/interrupt");
    write(&mut f.writer,json!({"method":"turn/completed","params":{"threadId":"thread-fixture","turn":{"id":"turn-1","status":"interrupted"}}})).await;
    assert!(f.job.await.unwrap().is_ok());
    assert_eq!(
        state.local_assistance.inner.lock().await.tasks[&f.id].phase,
        Phase::Cancelled
    );
}
#[tokio::test]
async fn cancellation_during_initialize_cannot_send_thread_or_turn_start() {
    let (temp, state, _) = fixture();
    let ctx = parent(&state, temp.path(), "s1").await;
    let mut work = reserved(&state, &ctx, "one").await;
    let (hub, server) = tokio::io::duplex(4096);
    let (r, mut w) = tokio::io::split(hub);
    let (r2, mut w2) = tokio::io::split(server);
    let mut reader = BufReader::new(r2);
    let s = state.clone();
    let id = work.id.clone();
    let job = tokio::spawn(async move {
        worker::conversation_io(
            &s,
            &id,
            &work.parent,
            "fixture-model",
            "fixture",
            false,
            &mut w,
            &mut BufReader::new(r),
            &mut work.control,
        )
        .await
    });
    let init = read(&mut reader).await;
    disconnect(&state, "fixture-host").await;
    write(&mut w2, json!({"id":init["id"],"result":{}})).await;
    assert!(job.await.unwrap().is_err());
    let mut extra = String::new();
    reader.read_to_string(&mut extra).await.unwrap();
    assert!(!extra.contains("thread/start") && !extra.contains("turn/start"));
}
#[test]
fn isolated_configuration_contains_only_the_selected_local_model_connection() {
    let (temp, _, _) = fixture();
    let s = worker::configuration(
        "fixture-model",
        Some("xhigh"),
        12345,
        "fixture-task",
        "fixture-secret",
        temp.path(),
    )
    .unwrap();
    let v: toml::Value = toml::from_str(&s).unwrap();
    assert_eq!(v["history"]["persistence"].as_str(), Some("none"));
    assert_eq!(v["approval_policy"].as_str(), Some("on-request"));
    assert_eq!(v["model_provider"].as_str(), Some("tiancai_assistance"));
    assert_eq!(v["model_reasoning_effort"].as_str(), Some("xhigh"));
    assert_eq!(
        v["projects"][temp.path().to_str().unwrap()]["trust_level"].as_str(),
        Some("untrusted")
    );
    assert!(!s.contains("OPENAI_API_KEY"));
}
fn owned_dir(path: &std::path::Path) -> (String, PathBuf) {
    let id = uuid::Uuid::new_v4().to_string();
    let dir = worker::run_dir(path, &id).unwrap();
    std::fs::create_dir_all(dir.join("codex-home")).unwrap();
    crate::desktop::transaction::atomic_write(
        &dir.join("owner.json"),
        &serde_json::to_vec(&json!({"owner":"tiancai-local-assistance-v1","task":id,"pid":null}))
            .unwrap(),
    )
    .unwrap();
    (id, dir)
}
#[test]
fn cleanup_only_erases_the_owned_uuid_directory_and_is_repeatable() {
    let (temp, state, _) = fixture();
    let (id, dir) = owned_dir(&state.config_path);
    let project = temp.path().join("project.txt");
    std::fs::write(&project, b"preserve").unwrap();
    std::fs::write(dir.join("codex-home/history.jsonl"), b"synthetic").unwrap();
    worker::remove_owned(&state.config_path, &id).unwrap();
    worker::remove_owned(&state.config_path, &id).unwrap();
    assert!(!dir.exists());
    assert_eq!(std::fs::read(project).unwrap(), b"preserve");
    assert!(worker::remove_owned(&state.config_path, "../project").is_err());
}
#[test]
fn cleanup_refuses_wrong_ownership_and_a_live_pid() {
    let (_temp, state, _) = fixture();
    let (id, dir) = owned_dir(&state.config_path);
    crate::desktop::transaction::atomic_write(
        &dir.join("owner.json"),
        &serde_json::to_vec(
            &json!({"owner":"tiancai-local-assistance-v1","task":id,"pid":std::process::id()}),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(worker::remove_owned(&state.config_path, &id).is_err());
    assert!(dir.exists());
    crate::desktop::transaction::atomic_write(
        &dir.join("owner.json"),
        br#"{"owner":"somebody-else"}"#,
    )
    .unwrap();
    assert!(worker::remove_owned(&state.config_path, &id).is_err());
}
#[tokio::test]
async fn restart_inventory_can_clear_owned_records_without_any_codex_home_scan() {
    let (temp, state, _) = fixture();
    let (id, dir) = owned_dir(&state.config_path);
    let outside = temp.path().join(".codex");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("history.jsonl"), b"other history").unwrap();
    assert_eq!(records::list(&state.config_path).unwrap()[0]["id"], id);
    let result = records::clear_finished(&state).await.unwrap();
    assert_eq!(result["cleared"], 1);
    assert!(!dir.exists());
    assert!(outside.join("history.jsonl").exists());
}
#[test]
fn settings_revision_and_normalized_native_fields_preserve_independent_edits() {
    let (_temp, state, mut s) = fixture();
    let (_, old) = settings::read(&state.config_path).unwrap();
    s.auto_clear = false;
    settings::write(&state.config_path, &s, &old).unwrap();
    assert!(settings::write(&state.config_path, &s, &old).is_err());
    let a = json!({"header_config":{"a":"b"},"status":"active","id":123});
    let b = json!({"header_config":"{\"a\":\"b\"}","status":"active","id":999});
    assert_eq!(
        settings::native_fingerprint(&a),
        settings::native_fingerprint(&b)
    );
    assert!(settings::native_fields(&a).get("id").is_none());
}

#[tokio::test]
async fn only_selected_codex_models_on_ordinary_sources_are_offered() {
    let (_temp, state, _) = fixture();
    let mut config = state.config.lock().await.clone();
    for name in ["workbuddy:one", "gmclaw:two"] {
        let mut p = config.ai_gateway.providers[0].clone();
        p.name = name.into();
        config.ai_gateway.providers.push(p);
    }
    assert_eq!(settings::models(&config).len(), 1);
    config.ai_gateway.codex_visible_models.clear();
    assert!(settings::models(&config).is_empty());
}

#[tokio::test]
async fn native_registration_starts_unverified_and_only_a_strict_local_probe_provides_the_five_tools()
 {
    let (_temp, state, settings) = fixture();
    let native = settings::native_value(&settings, 12345);
    assert_eq!(native["is_connected"], 0);
    assert!(native["tools_json"].as_array().unwrap().is_empty());
    let (base, job) = server(web::router().with_state(state)).await;
    let port = url::Url::parse(&base).unwrap().port().unwrap();
    assert_eq!(
        web::probe_local(port, &settings.token).await.unwrap(),
        json!(web::tools())
    );
    assert!(web::probe_local(port, "wrong-local-token").await.is_err());
    job.abort();
}
#[tokio::test]
async fn weak_or_incomplete_mcp_probe_cannot_mark_the_native_connection_ready() {
    let(base,job)=server(axum::Router::new().route("/api/local-assistance/mcp",axum::routing::post(|axum::Json(v):axum::Json<Value>|async move{
        axum::Json(if v["method"]=="initialize"{json!({"id":v["id"],"result":{"serverInfo":{"name":settings::MCP_NAME},"protocolVersion":"2025-06-18"}})}else{json!({"id":v["id"],"result":{"tools":[]},"ok":true})})
    }))).await;
    let port = url::Url::parse(&base).unwrap().port().unwrap();
    assert!(web::probe_local(port, "fixture-token").await.is_err());
    job.abort();
}
#[tokio::test]
async fn source_changes_are_refused_but_verified_same_origin_v1_repair_is_preserved() {
    let (temp, state, _) = fixture();
    let ctx = parent(&state, temp.path(), "s1").await;
    let work = reserved(&state, &ctx, "one").await;
    let mut headers = axum::http::HeaderMap::new();
    let token = state.local_assistance.inner.lock().await.tasks[&work.id]
        .api_token
        .clone();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    state.config.lock().await.ai_gateway.providers[0]
        .model_aliases
        .insert("fixture-model".into(), "other-model".into());
    assert!(web::model_pin(&state, &work.id, &headers).await.is_err());
    let mut p = ProviderConfig {
        base_url: "https://synthetic.invalid".into(),
        ..Default::default()
    };
    let before = settings::source_fingerprint(&p).unwrap();
    assert!(p.confirm_base_url("https://synthetic.invalid/v1"));
    assert_eq!(before, settings::source_fingerprint(&p).unwrap());
}
#[tokio::test]
async fn catalogue_native_write_requires_exact_readback_reload_and_unchanged_owned_configuration() {
    use std::sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
    };
    for mode in [
        "ready",
        "edited_before",
        "edited_after",
        "incomplete",
        "reload_failed",
    ] {
        let (_temp, state, mut s) = fixture();
        let native = settings::native_value(&s, state.config.lock().await.local_listen_port().unwrap());
        s.native_fingerprint = Some(settings::native_fingerprint(&native));
        s.native_catalog_pending = true;
        let (_, revision) = settings::read(&state.config_path).unwrap();
        settings::write(&state.config_path, &s, &revision).unwrap();
        let (_, revision) = settings::read(&state.config_path).unwrap();
        let saved = Arc::new(StdMutex::new(native));
        if mode == "edited_before" {
            saved.lock().unwrap()["server_url"] = json!("http://127.0.0.1:9/changed");
        }
        let writes = Arc::new(AtomicUsize::new(0));
        let reloads = Arc::new(AtomicUsize::new(0));
        let read = saved.clone();
        let update = saved.clone();
        let count = writes.clone();
        let reload = reloads.clone();
        let router = axum::Router::new()
            .route(
                "/data/mcp/connections/tiancai-local-assistance",
                axum::routing::get(move || {
                    let read = read.clone();
                    async move { axum::Json(read.lock().unwrap().clone()) }
                })
                .put(move |axum::Json(v): axum::Json<Value>| {
                    let update = update.clone();
                    let count = count.clone();
                    async move {
                        assert_eq!(
                            v.as_object().unwrap().len(),
                            4,
                            "directory sync must not overwrite authentication or URL"
                        );
                        count.fetch_add(1, Ordering::SeqCst);
                        let mut saved = update.lock().unwrap();
                        saved
                            .as_object_mut()
                            .unwrap()
                            .extend(v.as_object().unwrap().clone());
                        if mode == "edited_after" {
                            saved["server_url"] = json!("http://127.0.0.1:9/changed");
                        }
                        if mode == "incomplete" {
                            saved["tools_json"] = json!([]);
                        }
                        axum::Json(json!({"ok":true}))
                    }
                }),
            )
            .route(
                "/ctrl/config/reload",
                axum::routing::post(move || {
                    let reload = reload.clone();
                    async move {
                        reload.fetch_add(1, Ordering::SeqCst);
                        if mode == "reload_failed" {
                            axum::http::StatusCode::SERVICE_UNAVAILABLE
                        } else {
                            axum::http::StatusCode::OK
                        }
                    }
                }),
            );
        let (base, server) = server(router).await;
        let client = crate::gmclaw_desktop::DesktopClient::fixture(&base);
        let outcome =
            web::sync_catalogue_native(&state, s, &revision, &client, json!(web::tools())).await;
        let (actual, _) = settings::read(&state.config_path).unwrap();
        assert_eq!(outcome.is_ok(), mode == "ready", "{mode}: {outcome:?}");
        assert_eq!(!actual.native_catalog_pending, mode == "ready");
        assert_eq!(
            writes.load(Ordering::SeqCst),
            usize::from(mode != "edited_before")
        );
        assert_eq!(
            reloads.load(Ordering::SeqCst),
            usize::from(matches!(mode, "ready" | "reload_failed"))
        );
        if mode == "ready" {
            assert_eq!(saved.lock().unwrap()["tools_json"], json!(web::tools()));
        }
        server.abort();
    }
}
#[tokio::test]
async fn pending_catalogue_status_keeps_the_recoverable_failure_visible() {
    let (_temp, state, mut s) = fixture();
    s.native_catalog_pending = true;
    let (_, revision) = settings::read(&state.config_path).unwrap();
    settings::write(&state.config_path, &s, &revision).unwrap();
    state.local_assistance.inner.lock().await.host_detail = "天工协助配置已独立修改，请核对".into();
    let view = web::view(&state).await.unwrap();
    assert_eq!(view["hostReady"], false);
    assert!(view["hostDetail"].as_str().unwrap().contains("独立修改"));
    assert!(view["hostDetail"].as_str().unwrap().contains("重新打开"));
}
async fn server(router: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let job = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (base, job)
}
#[tokio::test]
async fn native_catalogue_read_keeps_agent_user_and_project_scope() {
    let (base,job)=server(axum::Router::new().route("/data/mcp/catalog/fixture-agent",axum::routing::get(
        |axum::extract::Query(query):axum::extract::Query<std::collections::HashMap<String,String>>| async move {
            assert_eq!(query.get("user_id").map(String::as_str),Some("fixture-user"));
            assert_eq!(query.get("project_id").map(String::as_str),Some("fixture/project"));
            axum::Json(json!({"catalog":[native_catalogue_fixture()]}))
        }))).await;
    let client=crate::gmclaw_desktop::DesktopClient::fixture(&base);
    let rows=client.task_mcp_catalog("fixture-agent","fixture-user","fixture/project").await.unwrap();
    assert!(catalogue::prompt(&rows,&[]).unwrap().contains("context_ref"));
    assert!(client.task_mcp_catalog("../invalid","fixture-user","fixture/project").await.is_err());
    job.abort();
}
#[tokio::test]
async fn a_contextless_mcp_call_reports_the_missing_parameter_and_never_starts_work() {
    let (_temp,state,settings)=fixture();
    let (base,job)=server(web::router().with_state(state.clone())).await;
    let reply:Value=reqwest::Client::new().post(format!("{base}/api/local-assistance/mcp")).bearer_auth(&settings.token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"assist_start","arguments":{"goal":"合成分析","problem_key":"same"}}}))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(reply["result"]["isError"],true);assert!(reply["result"]["content"][0]["text"].as_str().unwrap().contains("arguments.context_ref"));
    let inner=state.local_assistance.inner.lock().await;assert_eq!(inner.trigger_state,"call_error");assert!(inner.tasks.is_empty());
    job.abort();
}
#[tokio::test]
async fn mcp_calls_are_stateless_but_the_same_task_and_feedback_survive_connections() {
    let (temp, state, settings) = fixture();
    let ctx = parent(&state, temp.path(), "s1").await;
    let work = reserved(&state, &ctx, "one").await;
    {
        let mut inner = state.local_assistance.inner.lock().await;
        let t = inner.tasks.get_mut(&work.id).unwrap();
        t.phase = Phase::WaitingUser;
        t.pending
            .push(prompt("item/commandExecution/requestApproval", json!({})));
    }
    let (base, job) = server(web::router().with_state(state.clone())).await;
    for sequence in 1..=2 {
        let client = reqwest::Client::new();
        let result:Value=client.post(format!("{base}/api/local-assistance/mcp")).bearer_auth(&settings.token)
            .json(&json!({"jsonrpc":"2.0","id":sequence,"method":"tools/call","params":{"name":"assist_wait","arguments":{"context_ref":ctx,"task":work.id,"seconds":0}}})).send().await.unwrap().json().await.unwrap();
        let text = result["result"]["content"][0]["text"].as_str().unwrap();
        let v: Value = serde_json::from_str(text).unwrap();
        assert_eq!(v["id"], work.id);
        assert_eq!(v["pendingCount"], 1);
        assert!(!text.contains("nonce") && v.get("pending").is_none());
    }
    let bad = reqwest::Client::new()
        .post(format!("{base}/api/local-assistance/mcp"))
        .json(&json!({"id":1,"method":"tools/list"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), reqwest::StatusCode::UNAUTHORIZED);
    let mut origin = axum::http::HeaderMap::new();
    origin.insert("origin", "https://untrusted.invalid".parse().unwrap());
    let bad = reqwest::Client::new()
        .post(format!("{base}/api/local-assistance/mcp"))
        .headers(origin)
        .bearer_auth(settings.token)
        .json(&json!({"id":1,"method":"tools/list"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), reqwest::StatusCode::UNAUTHORIZED);
    job.abort();
}
#[tokio::test]
async fn disabling_does_not_require_a_running_tiangong_and_revokes_the_token() {
    let (_temp, state, settings) = fixture();
    let (_, revision) = settings::read(&state.config_path).unwrap();
    let (base, job) = server(web::router().with_state(state.clone())).await;
    let response=reqwest::Client::new().post(format!("{base}/api/desktop/assistance")).header("x-tiancai-desktop","1")
        .json(&json!({"enabled":false,"revision":revision,"model":settings.model,"route":settings.route,"binary":settings.binary,"allowWorkspaceWrite":false,"autoClear":true})).send().await.unwrap();
    assert!(response.status().is_success());
    let (actual, _) = settings::read(&state.config_path).unwrap();
    assert!(!actual.enabled);
    assert_ne!(actual.token, settings.token);
    job.abort();
}
#[tokio::test]
async fn fixed_route_rechecks_fingerprint_on_the_exact_gateway_config_snapshot() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    let(base,job)=server(axum::Router::new().route("/v1/responses",axum::routing::post(move||{let c=c.clone();async move{c.fetch_add(1,Ordering::SeqCst);axum::Json(json!({"id":"resp_fixture","object":"response","status":"completed","model":"fixture-model","output":[],"usage":{"input_tokens":1,"output_tokens":0,"total_tokens":1}}))}}))).await;
    let (_temp, state, _) = fixture();
    let (route, fp) = {
        let mut config = state.config.lock().await;
        let p = &mut config.ai_gateway.providers[0];
        p.base_url = format!("{base}/v1");
        p.api_key = "synthetic-key".into();
        (
            crate::ai_gateway::config::provider_route_id(p),
            settings::source_fingerprint(p).unwrap(),
        )
    };
    state.config.lock().await.ai_gateway.providers[0]
        .model_aliases
        .insert("fixture-model".into(), "changed-after-pin".into());
    let body = axum::body::Bytes::from(
        serde_json::to_vec(&json!({"model":"fixture-model","input":"synthetic"})).unwrap(),
    );
    let response = crate::ai_gateway::handler::handle_responses_pinned(
        state,
        axum::http::HeaderMap::new(),
        body,
        None,
        None,
        Some((route, fp)),
    )
    .await;
    assert!(!response.status().is_success());
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["error"]["code"],
        "invalid_model",
        "must reach fixed provider resolution"
    );
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    job.abort();
}
#[tokio::test]
async fn a_valid_pin_calls_only_its_ordinary_source_even_with_same_named_reserved_models() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let chosen = Arc::new(AtomicUsize::new(0));
    let other = Arc::new(AtomicUsize::new(0));
    let c = chosen.clone();
    let o = other.clone();
    let(base,job)=server(axum::Router::new()
        .route("/chosen/responses",axum::routing::post(move||{let c=c.clone();async move{c.fetch_add(1,Ordering::SeqCst);axum::Json(json!({"id":"resp_fixture","object":"response","created_at":0,"status":"completed","model":"fixture-model","output":[],"usage":{"input_tokens":1,"output_tokens":0,"total_tokens":1}}))}}))
        .route("/other/responses",axum::routing::post(move||{let o=o.clone();async move{o.fetch_add(1,Ordering::SeqCst);axum::Json(json!({"error":"must not route here"}))}}))).await;
    let (_temp, state, _) = fixture();
    let (route, fp) = {
        let mut config = state.config.lock().await;
        let p = &mut config.ai_gateway.providers[0];
        p.base_url = format!("{base}/chosen");
        p.api_key = "synthetic-key".into();
        let route = crate::ai_gateway::config::provider_route_id(p);
        let fp = settings::source_fingerprint(p).unwrap();
        let mut dedicated = p.clone();
        dedicated.name = "workbuddy:fixture".into();
        dedicated.base_url = format!("{base}/other");
        dedicated.weight = 100000;
        config.ai_gateway.providers.push(dedicated);
        (route, fp)
    };
    let body = axum::body::Bytes::from(
        serde_json::to_vec(&json!({"model":"fixture-model","input":"synthetic","stream":false}))
            .unwrap(),
    );
    let response = crate::ai_gateway::handler::handle_responses_pinned(
        state,
        axum::http::HeaderMap::new(),
        body,
        None,
        None,
        Some((route, fp)),
    )
    .await;
    assert!(
        response.status().is_success(),
        "valid pinned synthetic response must succeed"
    );
    assert_eq!(chosen.load(Ordering::SeqCst), 1);
    assert_eq!(other.load(Ordering::SeqCst), 0);
    job.abort();
}
#[tokio::test]
async fn two_followups_continue_the_same_thread_and_finish_closes_it() {
    let (temp, state, _) = fixture();
    let mut f = fake_with_effort(&state, temp.path(), Some("xhigh")).await;
    let (mut next,revision)=settings::read(&state.config_path).unwrap();
    next.reasoning_effort=Some("high".into());settings::write(&state.config_path,&next,&revision).unwrap();
    completed(&mut f, &state).await;
    for turn in 2..=3 {
        control(
            &state,
            Some(&f.context),
            &f.id,
            Control::Followup(format!("合成新证据 {turn}")),
        )
        .await
        .unwrap();
        let request = read(&mut f.reader).await;
        assert_eq!(request["method"], "turn/start");
        assert_eq!(request["params"]["threadId"], "thread-fixture");
        assert_eq!(request["params"]["effort"], "xhigh");
        let turn_id = format!("turn-{turn}");
        write(
            &mut f.writer,
            json!({"id":request["id"],"result":{"turn":{"id":turn_id,"status":"inProgress"}}}),
        )
        .await;
        wait_phase(&state, &f.id, Phase::Running).await;
        write(&mut f.writer,json!({"method":"turn/completed","params":{"threadId":"thread-fixture","turn":{"id":turn_id,"status":"completed"}}})).await;
        wait_phase(&state, &f.id, Phase::Completed).await;
    }
    assert!(
        control(
            &state,
            Some(&f.context),
            &f.id,
            Control::Followup("超过限额".into())
        )
        .await
        .is_err()
    );
    control(&state, Some(&f.context), &f.id, Control::Finish)
        .await
        .unwrap();
    assert!(
        control(
            &state,
            Some(&f.context),
            &f.id,
            Control::Followup("已结束".into())
        )
        .await
        .is_err()
    );
    assert!(f.job.await.unwrap().is_ok());
}
#[tokio::test]
async fn unknown_server_interactions_are_refused_and_reported_to_tiangong() {
    let (temp, state, _) = fixture();
    let mut f = fake(&state, temp.path()).await;
    write(&mut f.writer,json!({"id":91,"method":"future/privilegedApproval","params":{"threadId":"thread-fixture","turnId":"turn-1"}})).await;
    let reply = read(&mut f.reader).await;
    assert_eq!(reply["error"]["code"], -32601);
    completed(&mut f, &state).await;
    assert!(
        task_view(&state, &f.context, &f.id).await.unwrap()["progress"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap().contains("拒绝"))
    );
    control(&state, Some(&f.context), &f.id, Control::Finish)
        .await
        .unwrap();
    assert!(f.job.await.unwrap().is_ok());
}
#[cfg(windows)]
#[tokio::test]
async fn synthetic_child_is_owned_by_a_kill_on_close_job_and_cleanup_waits_for_exit() {
    use std::process::Stdio;
    let mut child = tokio::process::Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::In.ReadToEnd() | Out-Null",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    let tree = process::ProcessTree::attach(&child).unwrap();
    assert!(process::alive(pid).unwrap());
    tree.stop_and_confirm().await.unwrap();
    child.wait().await.unwrap();
    assert!(!process::alive(pid).unwrap());
}
