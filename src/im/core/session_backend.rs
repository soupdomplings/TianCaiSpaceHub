//! Shared session UI operations. Transport flows own the cards and menus;
//! this boundary supplies data and dispatches the selected execution backend.
use super::{
    routing::route_for_message,
    session,
    thread::{self, ThreadCreateDefaults, ThreadCreateForm},
    thread_list::{self, ThreadRoutingPage},
};
use crate::{
    app_state::SharedState, im_runtime::ThreadRoutingRequestState,
    remote_control_backend::ThreadStartOptions, types::InboundMessage,
};
use anyhow::Result;

pub(crate) async fn defaults(
    state: &SharedState,
    message: &InboundMessage,
) -> ThreadCreateDefaults {
    if message.session_scope.is_some() {
        crate::gmclaw_im::sessions::defaults(state, message).await
    } else {
        thread::load_thread_create_defaults_for_client(
            state,
            &route_for_message(message).remote_client_key,
        )
        .await
    }
}

pub(crate) async fn options_from_form(
    state: &SharedState,
    message: &InboundMessage,
    form: ThreadCreateForm,
) -> Result<ThreadStartOptions> {
    if message.session_scope.is_some() {
        crate::gmclaw_im::sessions::options(state, message, form).await
    } else {
        thread::thread_start_options_from_form_for_client(
            state,
            &route_for_message(message).remote_client_key,
            form,
        )
        .await
    }
}

pub(crate) async fn default_options(
    state: &SharedState,
    message: &InboundMessage,
) -> Result<ThreadStartOptions> {
    if message.session_scope.is_some() {
        options_from_form(state, message, ThreadCreateForm::default()).await
    } else {
        Ok(thread::thread_start_options_with_current_provider(
            ThreadStartOptions::default(),
        ))
    }
}

pub(crate) async fn create(
    state: &SharedState,
    message: &InboundMessage,
    options: ThreadStartOptions,
    request_id: Option<&str>,
) -> Result<String> {
    if message.session_scope.is_some() {
        crate::gmclaw_im::sessions::create(state, message, options, request_id).await
    } else {
        session::create_and_bind_thread(state, &route_for_message(message), options, request_id)
            .await
    }
}

pub(crate) async fn resume(
    state: &SharedState,
    message: &InboundMessage,
    id: &str,
    request_id: Option<&str>,
) -> Result<serde_json::Value> {
    if message.session_scope.is_some() {
        crate::gmclaw_im::sessions::resume(state, message, id, request_id).await
    } else {
        session::resume_and_bind_thread(state, &route_for_message(message), id, request_id).await
    }
}

pub(crate) async fn page(
    state: &SharedState,
    message: &InboundMessage,
    existing: Option<&ThreadRoutingRequestState>,
    cursor: Option<&str>,
    page: usize,
    size: u32,
) -> Result<ThreadRoutingPage> {
    if message.session_scope.is_some() {
        crate::gmclaw_im::sessions::page(state, message, existing, cursor, page, size).await
    } else {
        thread_list::load_thread_routing_page(
            state,
            &route_for_message(message),
            existing,
            cursor,
            page,
            size,
        )
        .await
    }
}
