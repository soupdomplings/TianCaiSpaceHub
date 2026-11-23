use crate::gui::controls::Button;
use crate::gui::controls::Choice;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use wxdragon::{prelude::*, timer::Timer};

use super::{
    api::{
        ApiClient, CodexAppSessionsResponse, CodexAppThread, MoveCodexAppSessionProviderRequest,
    },
    show_error, show_info,
    text::GuiText,
    theme,
    widgets::{
        apply_listctrl_row_theme, apply_listctrl_theme, card_section, listctrl_report_style,
    },
};

const AI_GATEWAY_PROVIDER: &str = "ai-gateway";
const OPENAI_PROVIDER: &str = "openai";
const ID_SESSION_MOVE_TO_GATEWAY: i32 = ID_HIGHEST + 310;
const ID_SESSION_MOVE_TO_PROVIDER: i32 = ID_HIGHEST + 311;

type SessionRows = Rc<RefCell<Vec<SessionRow>>>;
type ProviderChoices = Rc<RefCell<Vec<String>>>;
type SessionFetchResult = Arc<Mutex<Option<Result<CodexAppSessionsResponse, String>>>>;
type SessionMoveResult = Arc<Mutex<Option<Result<usize, String>>>>;

#[derive(Clone, PartialEq, Eq)]
struct SessionRow {
    thread_id: String,
    provider: String,
    preview: String,
    updated_at: i64,
    workspace: String,
    rollout_path: Option<String>,
}

pub(super) fn show_session_history_window(parent: &Frame, text: GuiText, api: ApiClient) {
    let frame = Frame::builder()
        .with_parent(parent)
        .with_title(text.session_history_title())
        .with_size(Size::new(1120, 620))
        .build();
    frame.set_background_color(theme::theme().bg_card_alt);

    let root = Panel::builder(&frame).build();
    root.set_background_color(theme::theme().bg_card_alt);
    let root_sizer = BoxSizer::builder(Orientation::Vertical).build();

    let toolbar = BoxSizer::builder(Orientation::Horizontal).build();
    let refresh_button = Button::builder(&root).with_label(text.refresh()).build();
    let move_to_button = Button::builder(&root)
        .with_label(text.move_to_ai_gateway())
        .build();
    let move_back_button = Button::builder(&root)
        .with_label(text.move_back_provider())
        .build();
    toolbar.add(&refresh_button, 0, SizerFlag::Right, 8);
    toolbar.add(&move_to_button, 0, SizerFlag::Right, 8);
    toolbar.add(&move_back_button, 0, SizerFlag::Right, 0);
    toolbar.add_stretch_spacer(1);
    root_sizer.add_sizer(
        &toolbar,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        10,
    );
    let hint = StaticText::builder(&root)
        .with_label(text.session_history_selection_hint())
        .build();
    hint.set_foreground_color(theme::theme().ink_muted);
    root_sizer.add(
        &hint,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        10,
    );

    let content = BoxSizer::builder(Orientation::Horizontal).build();
    let (left_box, left_sizer) = card_section(&root, text.other_provider_sessions());
    let (right_box, right_sizer) = card_section(&root, text.ai_gateway_sessions());

    let left_rows: SessionRows = Rc::new(RefCell::new(Vec::new()));
    let right_rows: SessionRows = Rc::new(RefCell::new(Vec::new()));
    let provider_choices: ProviderChoices = Rc::new(RefCell::new(Vec::new()));
    let left_list = create_session_list(&left_box, text);
    let right_list = create_session_list(&right_box, text);

    left_sizer.add(
        &left_list,
        1,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        10,
    );
    right_sizer.add(
        &right_list,
        1,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        10,
    );
    content.add(&left_box, 1, SizerFlag::Expand | SizerFlag::All, 10);
    content.add(&right_box, 1, SizerFlag::Expand | SizerFlag::All, 10);
    root_sizer.add_sizer(&content, 1, SizerFlag::Expand, 0);
    root.set_sizer(root_sizer, true);
    frame.set_sizer(BoxSizer::builder(Orientation::Vertical).build(), true);
    if let Some(sizer) = frame.get_sizer() {
        sizer.add(&root, 1, SizerFlag::Expand, 0);
    }

    let fetch_result: SessionFetchResult = Arc::new(Mutex::new(None));
    let move_result: SessionMoveResult = Arc::new(Mutex::new(None));
    let in_flight = Arc::new(AtomicBool::new(false));
    start_fetch(&api, &fetch_result, &in_flight);

    bind_refresh(
        &refresh_button,
        api.clone(),
        fetch_result.clone(),
        in_flight.clone(),
    );
    bind_move_actions(
        &frame,
        move_to_button,
        move_back_button,
        text,
        api.clone(),
        left_list,
        left_rows.clone(),
        right_list,
        right_rows.clone(),
        provider_choices.clone(),
        move_result.clone(),
        in_flight.clone(),
    );

    let timer_store: Rc<RefCell<Option<Timer<Frame>>>> = Rc::new(RefCell::new(None));
    let timer = Timer::new(&frame);
    {
        let frame = frame;
        let api = api.clone();
        let fetch_result = fetch_result.clone();
        let move_result = move_result.clone();
        let in_flight = in_flight.clone();
        let timer_store = timer_store.clone();
        let left_rows = left_rows.clone();
        let right_rows = right_rows.clone();
        let provider_choices = provider_choices.clone();
        let left_list = left_list;
        let right_list = right_list;
        timer.on_tick(move |_| {
            if let Some(result) = fetch_result.lock().ok().and_then(|mut slot| slot.take()) {
                in_flight.store(false, Ordering::SeqCst);
                match result {
                    Ok(response) => {
                        *provider_choices.borrow_mut() = build_provider_choices(
                            &api,
                            response.providers.clone(),
                            &response.threads,
                        );
                        refresh_rows(
                            response.threads,
                            &left_rows,
                            &left_list,
                            &right_rows,
                            &right_list,
                        );
                        frame.set_title(&format!(
                            "{} ({} / {})",
                            text.session_history_title(),
                            left_rows.borrow().len() + right_rows.borrow().len(),
                            response.total
                        ));
                    }
                    Err(err) => show_error(&frame, &err),
                }
            }
            if let Some(result) = move_result.lock().ok().and_then(|mut slot| slot.take()) {
                in_flight.store(false, Ordering::SeqCst);
                match result {
                    Ok(count) => {
                        show_info(&frame, &text.session_move_done(count));
                        start_fetch(&api, &fetch_result, &in_flight);
                    }
                    Err(err) => show_error(&frame, &err),
                }
            }
            let _keep_timer_alive = &timer_store;
        });
    }
    timer.start(150, false);
    timer_store.borrow_mut().replace(timer);
    frame.show(true);
}

fn create_session_list<W: WxWidget>(parent: &W, text: GuiText) -> ListCtrl {
    let list = ListCtrl::builder(parent)
        .with_style(listctrl_report_style())
        .with_size(Size::new(-1, 470))
        .build();
    list.insert_column(0, text.session_col_provider(), ListColumnFormat::Left, 130);
    list.insert_column(1, text.session_col_preview(), ListColumnFormat::Left, 330);
    list.insert_column(2, text.session_col_workspace(), ListColumnFormat::Left, 260);
    apply_listctrl_theme(&list);
    list
}

fn bind_refresh(
    button: &Button,
    api: ApiClient,
    fetch_result: SessionFetchResult,
    in_flight: Arc<AtomicBool>,
) {
    let button = *button;
    button.on_click(move |_| {
        start_fetch(&api, &fetch_result, &in_flight);
    });
}

#[allow(clippy::too_many_arguments)]
fn bind_move_actions(
    frame: &Frame,
    move_to_button: Button,
    move_back_button: Button,
    text: GuiText,
    api: ApiClient,
    left_list: ListCtrl,
    left_rows: SessionRows,
    right_list: ListCtrl,
    right_rows: SessionRows,
    provider_choices: ProviderChoices,
    move_result: SessionMoveResult,
    in_flight: Arc<AtomicBool>,
) {
    {
        let left_list = left_list;
        left_list.on_item_right_click(move |event| {
            ensure_context_row_selected(left_list, event.get_item_index());
            let mut menu = Menu::builder()
                .append_item(
                    ID_SESSION_MOVE_TO_GATEWAY,
                    text.move_to_ai_gateway(),
                    text.move_to_ai_gateway_help(),
                )
                .build();
            left_list.popup_menu(&mut menu, None);
        });
    }
    {
        let right_list = right_list;
        right_list.on_item_right_click(move |event| {
            ensure_context_row_selected(right_list, event.get_item_index());
            let mut menu = Menu::builder()
                .append_item(
                    ID_SESSION_MOVE_TO_PROVIDER,
                    text.move_back_provider(),
                    text.move_back_provider_help(),
                )
                .build();
            right_list.popup_menu(&mut menu, None);
        });
    }
    let frame = *frame;
    let move_selected = Rc::new(move |command| {
        let (sessions, destination, empty_message) = match command {
            ID_SESSION_MOVE_TO_GATEWAY => (
                selected_sessions(left_list, &left_rows),
                MoveDestination::Gateway,
                text.session_select_left_first(),
            ),
            ID_SESSION_MOVE_TO_PROVIDER => (
                selected_sessions(right_list, &right_rows),
                MoveDestination::Providers(provider_choices.borrow().clone()),
                text.session_select_right_first(),
            ),
            _ => return,
        };
        if sessions.is_empty() {
            show_error(&frame, empty_message);
            return;
        }
        if matches!(&destination, MoveDestination::Providers(choices) if choices.is_empty()) {
            show_error(&frame, text.session_no_target_provider());
            return;
        }
        let (dialog, choice) = move_dialog(&frame, text, &destination, sessions.len());
        let result = dialog.show_modal();
        let target = destination.confirmed(result, choice.and_then(|c| c.get_selection()));
        dialog.destroy();
        if let Some(target) = target {
            start_move_many(&api, sessions, target, &move_result, &in_flight);
        }
    });
    for (button, command) in [
        (move_to_button, ID_SESSION_MOVE_TO_GATEWAY),
        (move_back_button, ID_SESSION_MOVE_TO_PROVIDER),
    ] {
        let move_selected = move_selected.clone();
        button.on_click(move |_| move_selected(command));
    }
    frame.on_menu_selected(move |event| move_selected(event.get_id()));
}

enum MoveDestination {
    Gateway,
    Providers(Vec<String>),
}

impl MoveDestination {
    fn confirmed(&self, result: i32, selection: Option<u32>) -> Option<String> {
        if result != ID_OK {
            return None;
        }
        match self {
            Self::Gateway => Some(AI_GATEWAY_PROVIDER.to_owned()),
            Self::Providers(choices) => choices
                .get(selection? as usize)
                .map(|provider| provider.trim())
                .filter(|provider| !provider.is_empty())
                .map(str::to_owned),
        }
    }
}

fn move_dialog(
    frame: &Frame,
    text: GuiText,
    destination: &MoveDestination,
    selected_count: usize,
) -> (Dialog, Option<Choice>) {
    let dialog = Dialog::builder(frame, text.session_target_provider_title()).build();
    super::motion::install_dialog(dialog);
    dialog.set_background_color(theme::theme().bg_card);
    dialog.set_font(&theme::font(theme::TextRole::Body));
    dialog.set_escape_id(ID_CANCEL);
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let gap = dialog.from_dip(12);
    let prompt = StaticText::builder(&dialog)
        .with_label(&text.session_target_provider_prompt(selected_count))
        .build();
    prompt.set_foreground_color(theme::theme().ink_primary);
    sizer.add(&prompt, 0, SizerFlag::Expand | SizerFlag::Bottom, gap);
    let choice = match destination {
        MoveDestination::Gateway => {
            let target = StaticText::builder(&dialog)
                .with_label("AI Gateway")
                .build();
            target.set_foreground_color(theme::theme().ink_primary);
            sizer.add(&target, 0, SizerFlag::Expand | SizerFlag::Bottom, gap);
            None
        }
        MoveDestination::Providers(choices) => {
            let choice = Choice::builder(&dialog)
                .with_choices(choices.iter().map(|p| provider_display_name(p)).collect())
                .with_size(Size::new(320, 34))
                .with_selection(0)
                .build();
            sizer.add(&choice, 0, SizerFlag::Expand | SizerFlag::Bottom, gap);
            Some(choice)
        }
    };

    let buttons = BoxSizer::builder(Orientation::Horizontal).build();
    let cancel_button = Button::builder(&dialog)
        .with_id(ID_CANCEL)
        .with_label(text.cancel())
        .build();
    let ok_button = Button::builder(&dialog)
        .with_id(ID_OK)
        .with_label(text.move_sessions_confirm())
        .build();
    ok_button.set_default();
    buttons.add_stretch_spacer(1);
    buttons.add(&cancel_button, 0, SizerFlag::Right, dialog.from_dip(8));
    buttons.add(&ok_button, 0, SizerFlag::default(), 0);
    sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
    let outer = BoxSizer::builder(Orientation::Vertical).build();
    outer.add_sizer(
        &sizer,
        0,
        SizerFlag::Expand | SizerFlag::All,
        dialog.from_dip(18),
    );
    dialog.set_sizer_and_fit(outer, true);
    dialog.center();
    cancel_button.on_click(move |_| dialog.end_modal(ID_CANCEL));
    ok_button.on_click(move |_| dialog.end_modal(ID_OK));
    // wxDialog maps Escape to a wxButton. Hub's keyboard-focusable command
    // controls derive from wxControl, so handle Escape on each focus target.
    fn dismiss_on_escape(widget: &impl WindowEvents, dialog: Dialog) {
        widget.on_key_down(move |event| {
            if matches!(&event, WindowEventData::Keyboard(key) if key.get_key_code() == Some(27)) {
                dialog.end_modal(ID_CANCEL);
            } else {
                event.skip(true);
            }
        });
    }
    dismiss_on_escape(&cancel_button, dialog);
    dismiss_on_escape(&ok_button, dialog);
    if let Some(choice) = &choice {
        dismiss_on_escape(choice, dialog);
    }
    (dialog, choice)
}

fn start_fetch(api: &ApiClient, fetch_result: &SessionFetchResult, in_flight: &Arc<AtomicBool>) {
    if in_flight.swap(true, Ordering::SeqCst) {
        return;
    }
    let api = api.clone();
    let fetch_result = fetch_result.clone();
    thread::spawn(move || {
        let result = api.codex_app_sessions();
        if let Ok(mut slot) = fetch_result.lock() {
            slot.replace(result);
        }
    });
}

fn start_move_many(
    api: &ApiClient,
    sessions: Vec<SessionRow>,
    target_provider: String,
    move_result: &SessionMoveResult,
    in_flight: &Arc<AtomicBool>,
) {
    if in_flight.swap(true, Ordering::SeqCst) {
        return;
    }
    let api = api.clone();
    let move_result = move_result.clone();
    thread::spawn(move || {
        let mut moved = 0usize;
        let result = sessions.into_iter().try_for_each(|session| {
            let request = MoveCodexAppSessionProviderRequest {
                thread_id: session.thread_id,
                rollout_path: session.rollout_path,
                target_provider: target_provider.clone(),
            };
            api.move_codex_app_session_provider(&request)?;
            moved += 1;
            Ok::<(), String>(())
        });
        if let Ok(mut slot) = move_result.lock() {
            slot.replace(result.map(|_| moved));
        }
    });
}

fn refresh_rows(
    threads: Vec<CodexAppThread>,
    left_rows: &SessionRows,
    left_list: &ListCtrl,
    right_rows: &SessionRows,
    right_list: &ListCtrl,
) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    for thread in threads {
        let row = SessionRow {
            thread_id: thread.id,
            provider: thread.model_provider,
            preview: thread
                .name
                .filter(|value| !value.trim().is_empty())
                .or_else(|| {
                    let preview = thread.preview.trim().to_string();
                    (!preview.is_empty()).then_some(preview)
                })
                .unwrap_or_else(|| "(untitled)".to_string()),
            updated_at: thread.updated_at,
            workspace: thread_workspace(thread.cwd.as_ref()),
            rollout_path: thread.path,
        };
        if row.provider == AI_GATEWAY_PROVIDER {
            right.push(row);
        } else {
            left.push(row);
        }
    }
    left.sort_by(|a, b| {
        a.provider
            .cmp(&b.provider)
            .then_with(|| b.updated_at.cmp(&a.updated_at))
    });
    right.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    replace_rows(left_rows, left_list, left);
    replace_rows(right_rows, right_list, right);
}

fn build_provider_choices(
    api: &ApiClient,
    fallback_providers: Vec<String>,
    threads: &[CodexAppThread],
) -> Vec<String> {
    let mut providers = api
        .codex_app_status()
        .ok()
        .map(|status| {
            status
                .providers
                .into_iter()
                .map(|provider| provider.name)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if providers.is_empty() {
        providers.extend(fallback_providers);
        providers.extend(threads.iter().map(|thread| thread.model_provider.clone()));
    }
    providers.push(OPENAI_PROVIDER.to_string());

    providers.retain(|provider| {
        let provider = provider.trim();
        !provider.is_empty() && provider != AI_GATEWAY_PROVIDER
    });
    providers.sort();
    providers.dedup();
    providers
}

fn replace_rows(rows: &SessionRows, list: &ListCtrl, next: Vec<SessionRow>) {
    *rows.borrow_mut() = next;
    populate_session_list(list, &rows.borrow());
}

fn populate_session_list(list: &ListCtrl, rows: &[SessionRow]) {
    list.delete_all_items();
    for (index, row) in rows.iter().enumerate() {
        let index = index as i64;
        list.insert_item(index, &provider_display_name(&row.provider), None);
        list.set_item_text_by_column(index, 1, &row.preview);
        list.set_item_text_by_column(index, 2, &row.workspace);
        apply_listctrl_row_theme(list, index);
    }
}

fn selected_sessions(list: ListCtrl, rows: &SessionRows) -> Vec<SessionRow> {
    let rows = rows.borrow();
    let mut sessions = Vec::new();
    let mut item = -1i32;
    loop {
        item = list.get_next_item(item.into(), ListNextItemFlag::All, ListItemState::Selected);
        if item < 0 {
            break;
        }
        if let Some(row) = rows.get(item as usize) {
            sessions.push(row.clone());
        }
    }
    sessions
}

fn ensure_context_row_selected(list: ListCtrl, item_index: i32) {
    if item_index < 0 {
        return;
    }
    if !list.get_item_state(item_index.into(), ListItemState::Selected) {
        list.set_item_state(
            item_index.into(),
            ListItemState::Selected,
            ListItemState::Selected,
        );
    }
}

fn provider_display_name(provider: &str) -> String {
    if provider == OPENAI_PROVIDER {
        "openai (ChatGPT 登录)".to_string()
    } else {
        provider.to_string()
    }
}

fn thread_workspace(cwd: Option<&serde_json::Value>) -> String {
    let Some(cwd) = cwd else {
        return String::new();
    };
    if let Some(value) = cwd.as_str() {
        return value.to_string();
    }
    for key in ["path", "value", "text", "uri"] {
        if let Some(value) = cwd.get(key).and_then(serde_json::Value::as_str) {
            return value.to_string();
        }
    }
    String::new()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::gui::text::GuiLocale;

    #[test]
    #[ignore = "isolated hidden session move confirmation dialog"]
    fn move_confirmation_keeps_actions_inside_dialog_and_requires_acceptance() {
        wxdragon::app::render_offscreen(|| {
            theme::init(theme::ThemeMode::Light);
            let frame = Frame::builder().with_title("Session move fixture").build();
            for (destination, count, locale, expected) in [
                (
                    MoveDestination::Gateway,
                    1,
                    GuiLocale::ZhCn,
                    AI_GATEWAY_PROVIDER,
                ),
                (
                    MoveDestination::Providers(vec!["openai".into(), "custom".into()]),
                    23,
                    GuiLocale::EnUs,
                    "custom",
                ),
            ] {
                let text = GuiText::new(locale);
                let (dialog, choice) = move_dialog(&frame, text, &destination, count);
                dialog.layout();
                let client = dialog.get_client_size();
                let mut last_right = 0;
                for id in [ID_CANCEL, ID_OK] {
                    let button = dialog.find_window_by_id(id).unwrap();
                    let pos = button.get_position();
                    let size = button.get_size();
                    assert!(button.is_shown() && size.width > 0 && size.height > 0);
                    assert!(pos.x >= last_right && pos.x + size.width <= client.width);
                    assert!(pos.y > 0 && pos.y + size.height <= client.height);
                    last_right = pos.x + size.width;
                    if let Some(choice) = choice {
                        assert!(pos.y >= choice.get_position().y + choice.get_size().height);
                    }
                }
                let selection = choice.and_then(|c| {
                    c.set_selection(1);
                    c.get_selection()
                });
                assert_eq!(destination.confirmed(ID_CANCEL, selection), None);
                assert_eq!(destination.confirmed(wxdragon::id::ID_NONE, selection), None);
                assert_eq!(
                    destination.confirmed(ID_OK, selection).as_deref(),
                    Some(expected)
                );
                dialog.destroy();
            }
            frame.destroy();
        })
        .unwrap();
    }
}
