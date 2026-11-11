//! In-conversation choices resume the waiting tool in the current turn.
use crate::{app_state::SharedState, desktop::squad::ExpectedAccount};
use anyhow::{Context, Result, ensure};
use axum::{Json, Router, extract::State, http::HeaderMap, response::Response, routing::post};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use tokio::sync::oneshot;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    id: String,
    label: String,
    #[serde(default)]
    description: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Question {
    id: String,
    title: String,
    #[serde(default)]
    options: Vec<Choice>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    questions: Vec<Question>,
}
pub(super) struct Pending {
    nonce: String,
    questions: Vec<Question>,
    view: Value,
    response: oneshot::Sender<Value>,
    scope: (String, ExpectedAccount),
}
impl Pending {
    pub(super) fn view(&self) -> &Value {
        &self.view
    }
}

pub(super) fn definition() -> Value {
    super::tools::spec(
        "hub_ask_user",
        "只有缺少必要信息或存在多个有效候选时，显示对话内选择/填写卡片；用户提交后继续当前任务。每次1至3个简短问题，每题可给最多6个真实候选，并始终允许自由填写。已有明确指令或可查询的信息直接执行/查询；不重复索要删除确认，不收密码或API密钥（使用hub_request_credential）。",
        json!({"questions":{"type":"array","minItems":1,"maxItems":3,"items":{
            "type":"object","properties":{
                "id":{"type":"string"},"title":{"type":"string"},
                "options":{"type":"array","maxItems":6,"items":{"type":"object","properties":{
                    "id":{"type":"string"},"label":{"type":"string"},"description":{"type":"string"}
                },"required":["id","label"],"additionalProperties":false}}
            },"required":["id","title","options"],"additionalProperties":false
        }}}),
        &["questions"],
    )
}
fn parse_request(args: Value) -> Result<Vec<Question>> {
    let request: Request = serde_json::from_value(args).context("追问内容无效")?;
    ensure!(
        (1..=3).contains(&request.questions.len()),
        "请一次询问1至3个问题"
    );
    let mut ids = HashSet::new();
    for question in &request.questions {
        ensure!(
            !question.id.trim().is_empty() && ids.insert(&question.id),
            "问题标识不能重复或为空"
        );
        ensure!(
            !question.title.trim().is_empty() && question.title.chars().count() <= 240,
            "请使用简短问题"
        );
        ensure!(question.options.len() <= 6, "每个问题最多6个选项");
        let mut options = HashSet::new();
        for option in &question.options {
            ensure!(
                !option.id.trim().is_empty()
                    && options.insert(&option.id)
                    && !option.label.trim().is_empty()
                    && option.label.chars().count() <= 100
                    && option.description.chars().count() <= 240,
                "选项标识或内容无效"
            );
        }
    }
    Ok(request.questions)
}
pub(super) async fn ask(state: &SharedState, args: Value) -> Result<Value> {
    let questions = parse_request(args)?;
    let scope = super::TOOL_ACCOUNT
        .try_with(Clone::clone)
        .context("问题缺少当前会话身份")?;
    let nonce = uuid::Uuid::new_v4().to_string();
    let (send, receive) = oneshot::channel();
    {
        let mut tools = state.assistant.tools.lock().await;
        ensure!(
            tools.question.is_none(),
            "已有问题等待回答，请先完成当前选择"
        );
        tools.question = Some(Pending {
            view: json!({"nonce":nonce,"questions":questions}),
            nonce,
            questions,
            response: send,
            scope,
        });
    }
    let result = receive.await.context("问题已取消，未继续执行")?;
    super::ensure_current_account(state).await?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Answer {
    id: String,
    option_id: Option<String>,
    text: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submission {
    nonce: String,
    #[serde(default)]
    cancelled: bool,
    #[serde(default)]
    answers: Vec<Answer>,
}
fn checked_answers(questions: &[Question], answers: &[Answer]) -> Result<Value> {
    ensure!(questions.len() == answers.len(), "请完成全部问题后继续");
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for answer in answers {
        ensure!(seen.insert(&answer.id), "同一个问题只能提交一次");
        let question = questions
            .iter()
            .find(|q| q.id == answer.id)
            .context("问题已变化，请重新选择")?;
        let option = answer
            .option_id
            .as_ref()
            .map(|id| {
                question
                    .options
                    .iter()
                    .find(|option| option.id == *id)
                    .context("选项已变化，请重新选择")
            })
            .transpose()?;
        let text = answer.text.as_deref().unwrap_or_default().trim();
        ensure!(option.is_some() || !text.is_empty(), "请选择一项或填写回答");
        ensure!(text.len() <= 8000, "回答太长，请精简后继续");
        result.push(json!({"id":question.id,"question":question.title,
            "optionId":option.map(|v|&v.id),"label":option.map(|v|&v.label),"text":text}));
    }
    Ok(json!({"cancelled":false,"answers":result}))
}
pub(super) fn router() -> Router<SharedState> {
    Router::new().route("/api/desktop/assistant/question/answer", post(answer))
}
async fn answer(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(input): Json<Submission>,
) -> Response {
    super::tools::response(
        async {
            super::tools::trusted(&headers)?;
            let bridge = state.assistant.inner.lock().await.bridge.clone();
            let mut tools = state.assistant.tools.lock().await;
            let pending = tools
                .question
                .as_ref()
                .filter(|p| p.nonce == input.nonce)
                .context("这个问题已结束，请查看当前对话")?;
            let current = bridge
                .as_ref()
                .context("天才搭档会话已结束")
                .and_then(|bridge| bridge.ensure_account(&pending.scope.0, &pending.scope.1));
            if let Err(error) = current {
                tools.question = None;
                return Err(error);
            }
            let value = if input.cancelled {
                json!({"cancelled":true,"answers":[]})
            } else {
                checked_answers(&pending.questions, &input.answers)?
            };
            let pending = tools.question.take().unwrap();
            let _ = pending.response.send(value);
            Ok(json!({"accepted":true}))
        }
        .await,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request as HttpRequest, StatusCode},
    };
    use tower::ServiceExt;

    fn questions() -> Value {
        json!({"questions":[{"id":"source","title":"使用哪个渠道？","options":[
            {"id":"a","label":"渠道A"},{"id":"b","label":"渠道B","description":"已有模型"}]}]})
    }
    fn fixture() -> (tempfile::TempDir, SharedState) {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::config::AppConfig {
            state_path: dir.path().join("state.json"),
            ..Default::default()
        };
        let state =
            crate::app_state::AppState::new(dir.path().join("hub.toml"), config, None, None);
        (dir, state)
    }
    fn start_question(state: &SharedState) -> tokio::task::JoinHandle<Result<Value>> {
        let current = state.clone();
        tokio::spawn(async move {
            super::super::test_tool_scope(
                &current,
                super::super::tools::call(&current, "hub_ask_user", questions()),
            )
            .await
        })
    }
    async fn pending_nonce(state: &SharedState) -> String {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Some(nonce) =
                    super::super::tools::public_status(state).await["pendingQuestion"]["nonce"]
                        .as_str()
                {
                    break nonce.to_owned();
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap()
    }
    fn submission(body: Value) -> HttpRequest<Body> {
        HttpRequest::builder()
            .method("POST")
            .uri("/api/desktop/assistant/question/answer")
            .header("content-type", "application/json")
            .header("x-tiancai-desktop", "1")
            .body(Body::from(body.to_string()))
            .unwrap()
    }
    async fn submit(app: &Router, body: Value) -> (StatusCode, Value) {
        let response = app.clone().oneshot(submission(body)).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 10000).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }
    #[test]
    fn choices_and_free_text_preserve_real_candidate_identity() {
        let request = parse_request(questions()).unwrap();
        let choices = checked_answers(
            &request,
            &[Answer {
                id: "source".into(),
                option_id: Some("b".into()),
                text: None,
            }],
        )
        .unwrap();
        assert_eq!(choices["answers"][0]["label"], "渠道B");
        let text = checked_answers(
            &request,
            &[Answer {
                id: "source".into(),
                option_id: None,
                text: Some("  其他渠道  ".into()),
            }],
        )
        .unwrap();
        assert_eq!(text["answers"][0]["text"], "其他渠道");
        for candidate in ["missing", ""] {
            assert!(
                checked_answers(
                    &request,
                    &[Answer {
                        id: "source".into(),
                        option_id: Some(candidate.into()),
                        text: None
                    }]
                )
                .is_err()
            );
        }
        assert!(checked_answers(&request, &[]).is_err());
        assert!(parse_request(json!({"questions":[]})).is_err());
        let mut duplicate = questions();
        let first = duplicate["questions"][0].clone();
        duplicate["questions"].as_array_mut().unwrap().push(first);
        assert!(parse_request(duplicate).is_err());
        let mut duplicate_option = questions();
        duplicate_option["questions"][0]["options"][1]["id"] = "a".into();
        assert!(parse_request(duplicate_option).is_err());
        let mut extra = questions();
        extra["questions"][0]["secret"] = "unsupported-field".into();
        assert!(parse_request(extra).is_err());
        assert!(
            checked_answers(
                &request,
                &[Answer {
                    id: "source".into(),
                    option_id: None,
                    text: Some(" ".into())
                }]
            )
            .is_err()
        );
        assert!(
            checked_answers(
                &request,
                &[Answer {
                    id: "source".into(),
                    option_id: None,
                    text: Some("a".repeat(8001))
                }]
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn answer_resumes_one_tool_and_invalid_submissions_keep_the_current_card() {
        let (_dir, state) = fixture();
        let task = start_question(&state);
        let nonce = pending_nonce(&state).await;
        // Exercise the actual tools router merge as well as the tool dispatch.
        let app = super::super::tools::router().with_state(state.clone());
        assert_eq!(
            submit(
                &app,
                json!({"nonce":"unknown","answers":[{"id":"source","optionId":"a"}]})
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        for answers in [
            json!([]),
            json!([{"id":"source","optionId":"missing"}]),
            json!([{"id":"other","optionId":"a"}]),
        ] {
            assert_eq!(
                submit(&app, json!({"nonce":nonce,"answers":answers}))
                    .await
                    .0,
                StatusCode::CONFLICT
            );
            assert_eq!(pending_nonce(&state).await, nonce);
            assert!(!task.is_finished());
        }
        let mut browser =
            submission(json!({"nonce":nonce,"answers":[{"id":"source","optionId":"a"}]}));
        browser
            .headers_mut()
            .insert("origin", "https://fixture.invalid".parse().unwrap());
        assert_eq!(
            app.clone().oneshot(browser).await.unwrap().status(),
            StatusCode::CONFLICT
        );
        assert!(!task.is_finished());
        assert_eq!(
            submit(
                &app,
                json!({"nonce":nonce,"answers":[{"id":"source","optionId":"a"}]})
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(task.await.unwrap().unwrap()["answers"][0]["optionId"], "a");
        assert_eq!(
            submit(
                &app,
                json!({"nonce":nonce,"answers":[{"id":"source","optionId":"a"}]})
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        assert!(super::super::tools::public_status(&state).await["pendingQuestion"].is_null());
    }
    #[tokio::test]
    async fn cancelling_a_question_releases_the_tool_and_allows_the_next_question() {
        let (_dir, state) = fixture();
        let app = router().with_state(state.clone());
        let task = start_question(&state);
        let old = pending_nonce(&state).await;
        assert_eq!(
            submit(&app, json!({"nonce":old,"cancelled":true})).await.0,
            StatusCode::OK
        );
        assert_eq!(task.await.unwrap().unwrap()["cancelled"], true);
        let next = start_question(&state);
        let nonce = pending_nonce(&state).await;
        assert_ne!(nonce, old);
        assert_eq!(
            submit(&app, json!({"nonce":old,"cancelled":true})).await.0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            submit(
                &app,
                json!({"nonce":nonce,"answers":[{"id":"source","text":"  自定义渠道  "}]})
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(
            next.await.unwrap().unwrap()["answers"][0]["text"],
            "自定义渠道"
        );
    }
    #[tokio::test]
    async fn stopping_and_clearing_release_waiters_and_reject_old_answer_cards() {
        for clear in [false, true] {
            let (_dir, state) = fixture();
            let task = start_question(&state);
            let nonce = pending_nonce(&state).await;
            if clear {
                super::super::tools::clear(&state).await;
            } else {
                super::super::tools::cancel_pending(&state).await;
            }
            assert!(task.await.unwrap().is_err());
            assert!(super::super::tools::public_status(&state).await["pendingQuestion"].is_null());
            let app = router().with_state(state);
            let (status, body) = submit(
                &app,
                json!({"nonce":nonce,"answers":[{"id":"source","optionId":"a"}]}),
            )
            .await;
            assert_eq!(status, StatusCode::CONFLICT);
            assert!(body["error"].as_str().unwrap().contains("已结束"));
        }
    }
    #[tokio::test]
    async fn switched_account_rejects_and_releases_the_old_question_before_accepting_an_answer() {
        let (_dir, state) = fixture();
        let task = start_question(&state);
        let nonce = pending_nonce(&state).await;
        super::super::test_tool_scope(&state, async {
            let (squad, _) = super::super::tool_account(&state).await.unwrap();
            squad
                .assistant_test_identity("replacement-generation")
                .unwrap();
        })
        .await;
        let app = router().with_state(state.clone());
        let (status, body) = submit(
            &app,
            json!({"nonce":nonce,"answers":[{"id":"source","optionId":"a"}]}),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body["error"].as_str().unwrap().contains("已切换"));
        assert!(task.await.unwrap().is_err());
        assert!(super::super::tools::public_status(&state).await["pendingQuestion"].is_null());
    }
}
