use super::{GuiTimers, api::ApiClient};
use crate::{
    config::AppConfig,
    external_import::{self as import, CommitImport, ImportDraft, ImportLink, UpdateTarget},
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use wxdragon::{prelude::*, timer::Timer};

enum Work {
    Resolved(Result<ImportDraft, String>),
    Ready(ImportDraft),
}
enum Action {
    Overflow,
    Preview(ImportDraft),
    Error(String),
}

pub(super) fn confirm(parent: &dyn WxWidget, message: &str) -> bool {
    MessageDialog::builder(parent, message, "TianCaiSpace Hub")
        .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
        .build()
        .show_modal()
        == ID_YES
}

fn config_path() -> std::path::PathBuf {
    super::daemon_config_path().unwrap_or_else(super::app_support_config_path)
}

fn run_network<T>(
    job: impl AsyncFnOnce(reqwest::Client) -> Result<T, String>,
) -> Result<T, String> {
    let config = AppConfig::load_or_default(&config_path())
        .map_err(|_| "无法读取网络配置 / Cannot read network settings")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "无法启动导入任务 / Cannot start import")?;
    runtime.block_on(async {
        let client =
            import::client::build_client(&config.outbound_proxy, config.local_listen_port())?;
        job(client).await
    })
}

fn resolve(link: ImportLink, sender: mpsc::Sender<Work>) {
    thread::spawn(move || {
        let outcome =
            run_network(async move |client| import::client::resolve(&client, &link).await);
        let _ = sender.send(Work::Resolved(outcome));
    });
}

fn fetch(mut draft: ImportDraft, sender: mpsc::Sender<Work>) {
    thread::spawn(move || {
        let outcome =
            run_network(async |client| import::client::fetch_models(&client, &mut draft).await);
        if let Err(error) = outcome {
            draft.provider.models.clear();
            draft.model_warning = Some(error);
        }
        let _ = sender.send(Work::Ready(draft));
    });
}

pub(super) fn install(
    frame: &Frame,
    api: &ApiClient,
    timers: &GuiTimers,
    inbox: Result<import::ipc::Inbox, String>,
    initial: Option<ImportLink>,
) {
    let (tx, rx) = mpsc::channel::<Work>();
    let (inbox, startup_error) = match inbox {
        Ok(i) => (Some(i), None),
        Err(e) => (None, Some(e)),
    };
    let pending_links = Rc::new(RefCell::new(VecDeque::from_iter(initial)));
    let actions = Rc::new(RefCell::new(VecDeque::new()));
    if let Some(error) = startup_error {
        actions.borrow_mut().push_back(Action::Error(error));
    }
    let seen = Rc::new(RefCell::new(VecDeque::<String>::new()));
    let active = Rc::new(Cell::new(0usize));
    let frame = *frame;
    let api = api.clone();
    let timers_clone = timers.clone();
    // wxdragon timers bind to their owner: dedicated panels isolate these ticks
    // from dashboard events. Receiving continues while a modal preview is open.
    let receiver_owner = Panel::builder(&frame).build();
    receiver_owner.show(false);
    let presenter_owner = Panel::builder(&frame).build();
    presenter_owner.show(false);
    let timer = Rc::new(Timer::new(&receiver_owner));
    let presenter = Rc::new(Timer::new(&presenter_owner));
    let receive_actions = actions.clone();
    let receive_active = active.clone();
    let receive_tx = tx;
    timer.on_tick(move |_| {
        let actions = &receive_actions;
        let active = &receive_active;
        let tx = &receive_tx;
        if let Some(inbox) = &inbox {
            while let Ok(link) = inbox.receiver.try_recv() {
                pending_links.borrow_mut().push_back(link);
            }
        }
        loop {
            let link = pending_links.borrow_mut().pop_front();
            let Some(link) = link else {
                break;
            };
            let id = link.identity();
            if seen.borrow().contains(&id) {
                continue;
            }
            if active.get() >= 8 {
                if !actions
                    .borrow()
                    .iter()
                    .any(|a| matches!(a, Action::Overflow))
                {
                    actions.borrow_mut().push_back(Action::Overflow);
                }
                continue;
            }
            active.set(active.get() + 1);
            {
                let mut seen = seen.borrow_mut();
                seen.push_back(id);
                if seen.len() > 256 {
                    seen.pop_front();
                }
            }
            super::tray::show_main_window(&frame, &timers_clone);
            resolve(link, tx.clone());
        }
        while let Ok(result) = rx.try_recv() {
            match result {
                Work::Resolved(Ok(draft)) => fetch(draft, tx.clone()),
                Work::Resolved(Err(error)) => actions.borrow_mut().push_back(Action::Error(error)),
                Work::Ready(draft) => actions.borrow_mut().push_back(Action::Preview(draft)),
            }
        }
    });
    let present_timer = Rc::downgrade(&presenter);
    presenter.on_tick(move |_| {
        let action = actions.borrow_mut().pop_front();
        let Some(action) = action else {
            return;
        };
        let Some(timer) = present_timer.upgrade() else {
            return;
        };
        timer.stop(); // FnMut callbacks must not be re-entered by modal wx loops.
        let finished = match action {
            Action::Preview(draft) => {
                preview(&frame, &api, draft);
                true
            }
            Action::Error(error) => {
                super::show_error(&frame, &error);
                true
            }
            Action::Overflow => {
                super::show_info(
                    &frame,
                    "待处理导入过多，请完成当前导入后从网页重试 / Too many pending imports",
                );
                false
            }
        };
        if finished {
            active.set(active.get().saturating_sub(1));
        }
        timer.start(250, false);
    });
    timer.start(250, false);
    presenter.start(250, false);
    timers.panel_timers.borrow_mut().extend([timer, presenter]);
}

fn add_label(parent: &impl WxWidget, root: &BoxSizer, value: &str) {
    let label = StaticText::builder(parent).with_label(value).build();
    label.wrap(580);
    root.add(&label, 0, SizerFlag::Expand | SizerFlag::All, 8);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Explicitly invoked UI acceptance harness. It uses an isolated in-process
    /// backend, no real daemon, Codex configuration changes, or registry writes.
    #[test]
    #[ignore = "interactive Windows preview acceptance"]
    fn preview_smoke() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let mut config = AppConfig::load_or_default(&path).unwrap();
        config.state_path = temp.path().join("state.json");
        config.save(&path).unwrap();
        let state = crate::app_state::AppState::new(path.clone(), config.clone(), None, None);
        let (ready, address) = mpsc::channel();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let worker = thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    ready
                        .send(format!("http://{}", listener.local_addr().unwrap()))
                        .unwrap();
                    axum::serve(listener, crate::web::router(state))
                        .with_graceful_shutdown(async {
                            let _ = stopped.await;
                        })
                        .await
                        .unwrap();
                });
        });
        let source = import::ImportSource {
            origin: import::OFFICIAL_ORIGIN.into(),
            key_id: "smoke-123".into(),
            key_name: "开发密钥（测试）".into(),
            site_name: "天才空间".into(),
        };
        let draft = ImportDraft {
            source: source.clone(),
            provider: crate::ai_gateway::config::ProviderConfig {
                name: "天才空间 · 开发密钥".into(),
                api_key: "mock-preview-key".into(),
                base_url: format!("{}/antigravity/v1", import::OFFICIAL_ORIGIN),
                enabled: false,
                models: vec!["mock-model-a".into(), "mock-model-b".into()],
                import_source: Some(source),
                ..Default::default()
            },
            model_warning: None,
        };
        let api = ApiClient::new(
            address.recv().unwrap(),
            super::super::text::GuiText::new(super::super::text::GuiLocale::ZhCn),
        );
        wxdragon::main(move |app| {
            let _ = wxdragon::set_appearance(super::super::theme::ThemeMode::System.appearance());
            super::super::theme::init(super::super::theme::ThemeMode::System);
            let frame = Frame::builder()
                .with_title("Hub import acceptance")
                .with_size(Size::new(780, 820))
                .build();
            app.set_top_window(&frame);
            frame.show(true);
            let owner = Panel::builder(&frame).build();
            owner.show(false);
            let timer = Rc::new(Timer::new(&owner));
            let keeper = timer.clone();
            timer.on_tick(move |_| {
                keeper.stop();
                preview_with_config(&frame, &api, draft.clone(), config.clone());
                frame.destroy();
                app.exit_main_loop();
            });
            timer.start(300, true);
            // The registered closure keeps the timer alive until window teardown.
        })
        .unwrap();
        let _ = stop.send(());
        worker.join().unwrap();
        let saved = AppConfig::load_or_default(&path).unwrap();
        assert_eq!(saved.ai_gateway.providers.len(), 1);
        assert!(!saved.ai_gateway.providers[0].enabled);
        assert_eq!(saved.ai_gateway.providers[0].models.len(), 2);
        assert!(saved.ai_gateway.codex_visible_models.is_empty());
    }
}

fn preview(parent: &Frame, api: &ApiClient, draft: ImportDraft) {
    let config = match AppConfig::load_or_default(&config_path()) {
        Ok(config) => config,
        Err(_) => {
            super::show_error(
                parent,
                "无法读取当前配置，请修复配置后重新导入 / Cannot read configuration",
            );
            return;
        }
    };
    preview_with_config(parent, api, draft, config);
}

fn preview_with_config(parent: &Frame, api: &ApiClient, draft: ImportDraft, config: AppConfig) {
    let targets: Vec<_> = config
        .ai_gateway
        .providers
        .iter()
        .filter(|p| !p.is_workbuddy())
        .cloned()
        .collect();
    let matching = targets.iter().position(|p| {
        p.import_source
            .as_ref()
            .is_some_and(|s| s.same_key(&draft.source))
    });
    let mut new_name = draft.provider.name.clone();
    let mut suffix = 2;
    while config
        .ai_gateway
        .providers
        .iter()
        .any(|p| p.name.eq_ignore_ascii_case(&new_name))
    {
        new_name = format!("{} ({suffix})", draft.provider.name);
        suffix += 1;
    }
    let dialog = Dialog::builder(parent, "导入渠道 / Import channel")
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .with_size(680, 740)
        .build();
    dialog.set_min_size(Size::new(560, 520));
    let panel = ScrolledWindow::builder(&dialog)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    panel.set_scroll_rate(0, 12);
    let root = BoxSizer::builder(Orientation::Vertical).build();
    add_label(
        &panel,
        &root,
        &format!(
            "来源 / Source: {}\n站点：{}    Key：{}\n协议 / Protocol: {}\n模型服务 / Endpoint: {}\nAPI Key: ••••••••",
            draft.source.origin,
            draft.source.site_name,
            draft.source.key_name,
            super::provider_protocol_display(
                &draft.provider.provider_type,
                draft.provider.compatibility.as_deref()
            ),
            draft.provider.base_url
        ),
    );
    add_label(&panel, &root, "保存方式 / Save as");
    if matching.is_some() {
        add_label(
            &panel,
            &root,
            "已发现相同站点及 Key 的渠道，可更新已有渠道或选择新建。\nA channel for this source and key already exists.",
        );
    } else if config
        .ai_gateway
        .providers
        .iter()
        .any(|p| p.name.eq_ignore_ascii_case(&draft.provider.name))
    {
        add_label(
            &panel,
            &root,
            "已有同名渠道：默认新建并添加编号，也可明确选择更新目标。\nA channel with this name exists; choose create or update.",
        );
    }
    let operation = Choice::builder(&panel).build();
    operation.append("新建渠道 / Create new channel");
    for target in &targets {
        operation.append(&format!("更新渠道 / Update: {}", target.name));
    }
    operation.set_selection(matching.map(|i| i as u32 + 1).unwrap_or(0));
    root.add(&operation, 0, SizerFlag::Expand | SizerFlag::All, 8);
    add_label(&panel, &root, "渠道名称 / Channel name");
    let name = TextCtrl::builder(&panel)
        .with_value(
            matching
                .map(|i| targets[i].name.as_str())
                .unwrap_or(&new_name),
        )
        .build();
    root.add(&name, 0, SizerFlag::Expand | SizerFlag::All, 8);
    {
        let targets = targets.clone();
        let new_name = new_name.clone();
        operation.on_selection_changed(move |_| {
            let index = operation.get_selection().unwrap_or(0) as usize;
            name.change_value(if index == 0 {
                &new_name
            } else {
                &targets[index - 1].name
            });
        });
    }
    add_label(
        &panel,
        &root,
        "保留要导入的模型，每行一个；可以手动补充 / Models to import, one per line",
    );
    let models = TextCtrl::builder(&panel)
        .with_value(&draft.provider.models.join("\n"))
        .with_style(TextCtrlStyle::MultiLine)
        .with_size(Size::new(-1, 140))
        .build();
    root.add(&models, 1, SizerFlag::Expand | SizerFlag::All, 8);
    let enabled = CheckBox::builder(&panel)
        .with_label("启用此渠道（相同模型将按现有优先级路由） / Enable this channel")
        .build();
    let visible = CheckBox::builder(&panel)
        .with_label("将以上模型追加到 Codex 可见模型 / Add models to Codex list")
        .build();
    let aliases = CheckBox::builder(&panel)
        .with_label("更新时替换原有模型映射 / Replace existing aliases on update")
        .build();
    root.add(&enabled, 0, SizerFlag::All, 8);
    root.add(&visible, 0, SizerFlag::All, 8);
    root.add(&aliases, 0, SizerFlag::All, 8);
    add_label(
        &panel,
        &root,
        "默认禁用保存。空模型渠道需补全后启用。Codex 首次接入仍需手动初始化。\nSaved disabled by default. Set up Codex separately if needed.",
    );
    let status = StaticText::builder(&panel)
        .with_label(
            draft
                .model_warning
                .as_deref()
                .unwrap_or("模型列表已获取 / Model list loaded"),
        )
        .build();
    status.wrap(580);
    root.add(&status, 0, SizerFlag::Expand | SizerFlag::All, 8);
    panel.set_sizer(root, true);
    let buttons = BoxSizer::builder(Orientation::Horizontal).build();
    let cancel = Button::builder(&dialog)
        .with_id(ID_CANCEL)
        .with_label("取消 / Cancel")
        .build();
    let save = Button::builder(&dialog)
        .with_label("确认保存 / Save")
        .build();
    buttons.add_stretch_spacer(1);
    buttons.add(&cancel, 0, SizerFlag::All, 10);
    buttons.add(&save, 0, SizerFlag::All, 10);
    let outer = BoxSizer::builder(Orientation::Vertical).build();
    outer.add(&panel, 1, SizerFlag::Expand, 0);
    outer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
    dialog.set_sizer(outer, true);
    let (result_tx, result_rx) = mpsc::channel::<Result<(), String>>();
    let saving = Rc::new(Cell::new(false));
    {
        let api = api.clone();
        let saving = saving.clone();
        save.on_click(move |_| {
            if saving.get() {
                return;
            }
            let models: Vec<String> = models
                .get_value()
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            let selected = operation.get_selection().unwrap_or(0) as usize;
            let request = CommitImport {
                draft: draft.clone(),
                name: name.get_value(),
                update: selected.checked_sub(1).map(|i| UpdateTarget {
                    name: targets[i].name.clone(),
                    fingerprint: import::provider_fingerprint(&targets[i]),
                }),
                models,
                enabled: enabled.is_checked(),
                visible_models: visible.is_checked(),
                replace_aliases: aliases.is_checked(),
            };
            let mut trial = config.clone();
            if let Err(error) = import::merge_import(&mut trial, &request) {
                status.set_label(&error);
                status.wrap(580);
                panel.layout();
                return;
            }
            saving.set(true);
            save.enable(false);
            cancel.enable(false);
            status.set_label("正在保存，等待本地服务就绪… / Saving; waiting for local service…");
            let api = api.clone();
            let sender = result_tx.clone();
            thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(30);
                while !api.is_online() && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(300));
                }
                let result = api
                    .post_json_with_timeout::<_, serde_json::Value>(
                        "/api/external-import/commit",
                        &request,
                        Duration::from_secs(15),
                    )
                    .map(|_| ());
                let _ = sender.send(result);
            });
        });
    }
    {
        let saving = saving.clone();
        dialog.on_close(move |event| {
            if saving.get() {
                if let WindowEventData::General(raw) = &event {
                    if raw.can_veto() {
                        raw.veto();
                    }
                }
            } else {
                dialog.end_modal(ID_CANCEL);
            }
        });
    }
    let timer = Timer::new(&dialog);
    {
        let saving = saving.clone();
        timer.on_tick(move |_| {
        if let Ok(result) = result_rx.try_recv() {
            saving.set(false);
            match result {
                Ok(()) => dialog.end_modal(ID_OK),
                Err(error) => { save.enable(true); cancel.enable(true); status.set_label(&format!("{error}\n请刷新核对渠道后重试 / Refresh and check channels before retrying")); status.wrap(580); panel.layout(); },
            }
        }
    });
    }
    timer.start(150, false);
    dialog.center();
    let saved = dialog.show_modal() == ID_OK;
    timer.stop();
    dialog.destroy();
    if saved {
        super::show_info(
            parent,
            "渠道已保存。可在“大模型接入”中查看和启用；新增可见模型后按需重新打开 Codex。\nChannel saved. Manage it in AI Gateway; reopen Codex to refresh its model list.",
        );
    }
}
