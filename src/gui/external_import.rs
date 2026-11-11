use super::{
    GuiTimers,
    api::ApiClient,
    controls::{Button, ButtonKind, Choice, TextCtrl},
    squad_dialog::{SquadDialog, field_label, note},
};
use crate::{
    config::AppConfig,
    desktop::targets::ClientTarget,
    external_import::{self as import, CommitImport, ImportDraft, ImportLink, UpdateTarget},
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeSet, VecDeque},
    rc::Rc,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use wxdragon::{prelude::*, timer::Timer};

// The event bridge uses portable wxDragon APIs, so Windows test compilation can
// type-check it too. Native delivery and Unix IPC still require macOS acceptance.
#[cfg(any(target_os = "macos", all(test, windows)))]
pub(super) mod macos;

mod batch;

enum Work {
    Resolved(Result<ImportDraft, String>),
    Ready(ImportDraft),
}
enum Action {
    Overflow,
    Preview(ImportDraft),
    ImportFailed(String),
    Error(String),
}

pub(super) fn confirm(parent: &dyn WxWidget, message: &str) -> bool {
    super::feedback::confirm(parent, "确认操作", message, "继续")
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
    app: App,
    frame: &Frame,
    api: &ApiClient,
    timers: &GuiTimers,
    inbox: Result<import::ipc::Inbox, String>,
    initial: Option<ImportLink>,
) {
    #[cfg(target_os = "macos")]
    let native = macos::NativeEvents::attach(app);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
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
        #[cfg(target_os = "macos")]
        {
            while let Ok(result) = native.receiver.try_recv() {
                match result {
                    Ok(link) => pending_links.borrow_mut().push_back(link),
                    Err(error) => {
                        if actions.borrow().len() < 16 {
                            actions.borrow_mut().push_back(Action::Error(error));
                        }
                    }
                }
            }
            if native.take_overflow()
                && !actions
                    .borrow()
                    .iter()
                    .any(|a| matches!(a, Action::Overflow))
            {
                actions.borrow_mut().push_back(Action::Overflow);
            }
        }
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
                Work::Resolved(Err(error)) => {
                    actions.borrow_mut().push_back(Action::ImportFailed(error));
                }
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
        // Receiver/startup errors have not reserved an import slot.
        let finished = matches!(&action, Action::Preview(_) | Action::ImportFailed(_));
        match action {
            Action::Preview(draft) => {
                preview(&frame, &api, draft);
            }
            Action::ImportFailed(error) | Action::Error(error) => {
                super::show_error(&frame, &error);
            }
            Action::Overflow => {
                super::show_info(
                    &frame,
                    "待处理导入过多，请完成当前导入后从网页重试 / Too many pending imports",
                );
            }
        }
        if finished {
            active.set(active.get().saturating_sub(1));
        }
        timer.start(250, false);
    });
    timer.start(250, false);
    presenter.start(250, false);
    timers.panel_timers.borrow_mut().extend([timer, presenter]);
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

pub(super) fn preview(parent: &Frame, api: &ApiClient, draft: ImportDraft) {
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

#[derive(Clone)]
struct ImportModels {
    list: CheckListBox,
    search: TextCtrl,
    count: StaticText,
    models: Rc<Vec<String>>,
    selected: Rc<RefCell<BTreeSet<String>>>,
}

impl ImportModels {
    fn new(parent: &Panel, content: &BoxSizer, models: &[String]) -> Self {
        let search = TextCtrl::builder(parent).build();
        search.set_name("import-model-search");
        search.set_hint("搜索模型");
        content.add(
            &search,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            parent.from_dip(10),
        );
        let tools = BoxSizer::builder(Orientation::Horizontal).build();
        let count = note(parent, "");
        tools.add(&count, 1, SizerFlag::AlignCenterVertical, 0);
        let all = Button::builder(parent)
            .with_label("选择当前结果")
            .with_kind(ButtonKind::Ghost)
            .build();
        let clear = Button::builder(parent)
            .with_label("清空选择")
            .with_kind(ButtonKind::Ghost)
            .build();
        tools.add(&all, 0, SizerFlag::Right, parent.from_dip(8));
        tools.add(&clear, 0, SizerFlag::default(), 0);
        content.add_sizer(
            &tools,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            parent.from_dip(8),
        );
        let list = CheckListBox::builder(parent)
            .with_size(Size::new(-1, parent.from_dip(152)))
            .build();
        list.set_min_size(Size::new(0, parent.from_dip(152)));
        list.set_name("import-models");
        list.set_background_color(super::theme::theme().bg_card);
        list.set_foreground_color(super::theme::theme().ink_primary);
        content.add(&list, 0, SizerFlag::Expand, 0);
        let ui = Self {
            list,
            search,
            count,
            models: Rc::new(models.to_vec()),
            selected: Rc::new(RefCell::new(models.iter().cloned().collect())),
        };
        let u = ui.clone();
        search.on_text_updated(move |_| u.render());
        let u = ui.clone();
        list.on_toggled(move |event| {
            if let Some(index) = event.get_selection() {
                u.update_selection(index);
            }
        });
        let u = ui.clone();
        all.on_click(move |_| {
            for i in 0..u.list.get_count() {
                if let Some(model) = u.list.get_string(i as usize) {
                    u.selected.borrow_mut().insert(model);
                    u.list.check(i, true);
                }
            }
            u.update_count();
        });
        let u = ui.clone();
        clear.on_click(move |_| {
            u.selected.borrow_mut().clear();
            u.render();
        });
        ui.render();
        ui
    }
    fn selected_models(&self) -> Vec<String> {
        let selected = self.selected.borrow();
        self.models
            .iter()
            .filter(|m| selected.contains(*m))
            .cloned()
            .collect()
    }
    fn update_selection(&self, index: u32) {
        if let Some(model) = self.list.get_string(index as usize) {
            if self.list.is_checked(index) {
                self.selected.borrow_mut().insert(model);
            } else {
                self.selected.borrow_mut().remove(&model);
            }
            self.update_count();
        }
    }
    fn render(&self) {
        let query = self.search.get_value().trim().to_lowercase();
        let selected = self.selected.borrow();
        self.list.freeze();
        self.list.clear();
        for model in self
            .models
            .iter()
            .filter(|m| m.to_lowercase().contains(&query))
        {
            self.list.append(model);
            self.list
                .check(self.list.get_count() - 1, selected.contains(model));
        }
        self.list.thaw();
        self.update_count();
    }
    fn update_count(&self) {
        self.count.set_label(&format!(
            "已选 {} 个 · 当前 {} 个",
            self.selected.borrow().len(),
            self.list.get_count()
        ));
    }
}

#[derive(Clone)]
struct ImportResults {
    table: ListCtrl,
    detail: TextCtrl,
    details: Rc<RefCell<Vec<String>>>,
}

impl ImportResults {
    fn new(parent: &Panel, content: &BoxSizer) -> Self {
        let table = ListCtrl::builder(parent)
            .with_style(
                ListCtrlStyle::Report | ListCtrlStyle::SingleSel | ListCtrlStyle::BorderNone,
            )
            .with_size(Size::new(-1, parent.from_dip(220)))
            .build();
        table.set_name("import-results");
        table.set_background_color(super::theme::theme().bg_card);
        table.set_foreground_color(super::theme::theme().ink_primary);
        for (column, title) in ["客户端", "模型", "结果"].iter().enumerate() {
            table.insert_column(
                column as i64,
                title,
                ListColumnFormat::Left,
                parent.from_dip(160),
            );
        }
        table.on_size(move |event| {
            let width = (table.get_client_size().width - table.from_dip(18)).max(1);
            let state = table.from_dip(92).min(width / 3);
            let client = (width - state) * 2 / 5;
            table.set_column_width(0, client);
            table.set_column_width(1, width - state - client);
            table.set_column_width(2, state);
            event.skip(true);
        });
        content.add(&table, 1, SizerFlag::Expand, 0);
        let detail = TextCtrl::builder(parent)
            .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly)
            .with_size(Size::new(-1, 90))
            .with_value("选择一项查看详情。")
            .build();
        detail.set_name("import-result-detail");
        content.add(
            &detail,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            parent.from_dip(10),
        );
        let ui = Self {
            table,
            detail,
            details: Default::default(),
        };
        let u = ui.clone();
        table.on_item_selected(move |event| {
            u.select(event.get_item_index() as usize);
        });
        ui
    }
    fn select(&self, index: usize) {
        if let Some(detail) = self.details.borrow().get(index) {
            self.detail.change_value(detail);
        }
    }
    fn add(&self, target: &str, model: &str, result: &str, detail: &str, failed: bool) {
        let row = self
            .table
            .insert_item(self.table.get_item_count() as i64, target, None) as i64;
        self.table.set_item_text_by_column(row, 1, model);
        self.table.set_item_text_by_column(row, 2, result);
        self.table.set_item_text_colour(
            row,
            &if failed {
                super::theme::theme().error
            } else {
                super::theme::theme().ink_primary
            },
        );
        self.details
            .borrow_mut()
            .push(format!("{target} · {model}\n{detail}"));
    }
    fn start(&self) {
        self.table.delete_all_items();
        self.details.borrow_mut().clear();
        self.add("来源渠道", "—", "保存中", "正在保存来源渠道。", false);
        self.detail.change_value("正在保存来源渠道。");
    }
    fn saved(&self) {
        self.table.set_item_text_by_column(0, 2, "已保存");
        self.details.borrow_mut()[0] = "来源渠道已保存；已选客户端的模型正在逐项接入。".into();
        self.select(0);
    }
    fn outcome(&self, outcome: &batch::Outcome) {
        let (state, detail, failed) = match &outcome.result {
            Ok(message) => ("已接入", message.as_str(), false),
            Err(error) => ("未完成", error.as_str(), true),
        };
        self.add(&outcome.target, &outcome.model, state, detail, failed);
    }
    fn save_error(&self, error: &str) {
        self.table.set_item_text_by_column(0, 2, "未确认");
        self.table
            .set_item_text_colour(0, &super::theme::theme().error);
        self.details.borrow_mut()[0] =
            format!("渠道保存未确认：{error}\n请在渠道页核对，本批不会自动再次提交。");
        self.select(0);
    }
}

struct ImportPreview {
    chrome: SquadDialog,
    cards: [Panel; 4],
    operation: Choice,
    name: TextCtrl,
    models: ImportModels,
    target_card: Panel,
    target_fields: BoxSizer,
    target_status: StaticText,
    target_rows: Rc<RefCell<Vec<(ClientTarget, CheckBox)>>>,
    enabled: CheckBox,
    aliases: CheckBox,
    result_card: Panel,
    results: ImportResults,
    save: Button,
    close: Button,
    channels: Vec<crate::ai_gateway::config::ProviderConfig>,
    draft: ImportDraft,
    config: AppConfig,
}

impl ImportPreview {
    fn new(parent: &Frame, draft: ImportDraft, config: AppConfig) -> Self {
        let channels = config
            .ai_gateway
            .providers
            .iter()
            .filter(|p| !p.is_client_reserved())
            .cloned()
            .collect::<Vec<_>>();
        let matching = channels.iter().position(|p| {
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

        let ui = SquadDialog::new(
            parent,
            "导入模型",
            "选择要导入的模型和使用它们的客户端。不选客户端时，只保存到 Hub。",
            Size::new(760, 840),
        );
        let (dialog, body) = (ui.dialog, ui.body);
        let wrap_width = (dialog.get_client_size().width - dialog.from_dip(112)).max(100);
        let mut wrapped = Vec::new();
        dialog.set_name("hub-import-dialog");
        let (source_card, source_fields) = ui.section("保存到 Hub");
        let source = note(
            &source_card,
            &format!(
                "{} · {}    密钥：••••••••",
                draft.source.site_name, draft.source.key_name
            ),
        );
        wrapped.push((source, source.get_label()));
        source.wrap(wrap_width);
        source_fields.add(
            &source,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            source_card.from_dip(12),
        );
        let operation_label = field_label(&source_card, "保存为");
        source_fields.add(
            &operation_label,
            0,
            SizerFlag::Bottom,
            source_card.from_dip(6),
        );
        let operation = Choice::builder(&source_card).build();
        operation.append("添加新渠道");
        for channel in &channels {
            operation.append(&format!("更新已有渠道：{}", channel.name));
        }
        operation.set_selection(matching.map(|i| i as u32 + 1).unwrap_or(0));
        source_fields.add(
            &operation,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            source_card.from_dip(12),
        );
        let name_label = field_label(&source_card, "渠道名称");
        source_fields.add(&name_label, 0, SizerFlag::Bottom, source_card.from_dip(6));
        let name = TextCtrl::builder(&source_card)
            .with_value(
                matching
                    .map(|i| channels[i].name.as_str())
                    .unwrap_or(&new_name),
            )
            .build();
        name.set_name("import-channel-name");
        source_fields.add(&name, 0, SizerFlag::Expand, 0);
        {
            let channels = channels.clone();
            operation.on_selection_changed(move |_| {
                let index = operation.get_selection().unwrap_or(0) as usize;
                name.change_value(
                    index
                        .checked_sub(1)
                        .map(|i| channels[i].name.as_str())
                        .unwrap_or(&new_name),
                );
            });
        }

        let (model_card, model_fields) = ui.section("要导入的模型");
        if let Some(warning) = &draft.model_warning {
            let warning = note(&model_card, warning);
            wrapped.push((warning, warning.get_label()));
            warning.wrap(wrap_width);
            model_fields.add(
                &warning,
                0,
                SizerFlag::Expand | SizerFlag::Bottom,
                model_card.from_dip(8),
            );
        }
        if draft.provider.models.is_empty() {
            let empty = note(
                &model_card,
                "暂未获取到模型，可先保存渠道，再到渠道页补充。",
            );
            wrapped.push((empty, empty.get_label()));
            empty.wrap(wrap_width);
            model_fields.add(
                &empty,
                0,
                SizerFlag::Expand | SizerFlag::Bottom,
                model_card.from_dip(8),
            );
        }
        let models = ImportModels::new(&model_card, &model_fields, &draft.provider.models);

        let (target_card, target_fields) = ui.section("在哪些客户端使用");
        let target_status = note(&target_card, "正在查找本机客户端…");
        target_fields.add(
            &target_status,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            target_card.from_dip(8),
        );
        let target_rows: Rc<RefCell<Vec<(ClientTarget, CheckBox)>>> = Default::default();

        let (advanced_card, advanced_fields) = ui.section("");
        let advanced = CollapsiblePane::builder(&advanced_card)
            .with_label("更多设置")
            .with_style(CollapsiblePaneStyle::NoTlwResize)
            .build();
        let advanced_panel = advanced.get_pane().expect("import advanced pane");
        let advanced_col = BoxSizer::builder(Orientation::Vertical).build();
        let addresses = note(
            &advanced_panel,
            &format!(
                "站点：{}\n模型服务：{}\n协议：{}",
                draft.source.origin,
                draft.provider.base_url,
                super::provider_protocol_display(
                    &draft.provider.provider_type,
                    draft.provider.compatibility.as_deref()
                )
            ),
        );
        wrapped.push((addresses, addresses.get_label()));
        addresses.wrap(wrap_width);
        advanced_col.add(
            &addresses,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            advanced_card.from_dip(12),
        );
        let enabled = CheckBox::builder(&advanced_panel)
            .with_label("保存后启用渠道")
            .build();
        enabled.set_tooltip(
            "启用后，Hub 可以使用这个渠道里的模型。选择客户端时会自动勾选；暂不使用时，可以只保存。",
        );
        let aliases = CheckBox::builder(&advanced_panel)
            .with_label("用本次的模型别名替换旧别名")
            .build();
        aliases.set_tooltip(
            "更新已有渠道时，勾选会用本次导入的模型别名替换旧别名，未勾选则保留。新建渠道使用本次导入的别名。",
        );
        advanced_col.add(&enabled, 0, SizerFlag::Bottom, advanced_card.from_dip(10));
        advanced_col.add(&aliases, 0, SizerFlag::default(), 0);
        advanced_panel.set_sizer(advanced_col, false);
        advanced_fields.add(&advanced, 0, SizerFlag::Expand, 0);
        advanced.on_changed(move |_| {
            body.layout();
            body.fit_inside();
            dialog.layout();
        });

        let (result_card, result_fields) = ui.section("接入结果");
        let results = ImportResults::new(&result_card, &result_fields);
        result_card.show(false);

        let close = Button::builder(&dialog)
            .with_id(ID_CANCEL)
            .with_label("取消")
            .build();
        let save = Button::builder(&dialog)
            .with_label("保存到 Hub")
            .with_kind(ButtonKind::Primary)
            .build();
        close.set_name("import-close");
        save.set_name("import-submit");
        save.enable(false);
        ui.actions
            .add(&close, 0, SizerFlag::Right, dialog.from_dip(10));
        ui.actions.add(&save, 0, SizerFlag::default(), 0);
        dialog.on_size(move |event| {
            let width = (dialog.get_client_size().width - dialog.from_dip(112)).max(100);
            for (label, text) in &wrapped {
                label.set_label(text);
                label.wrap(width);
            }
            event.skip(true);
        });

        ui.finish();
        Self {
            chrome: ui,
            cards: [source_card, model_card, target_card, advanced_card],
            operation,
            name,
            models,
            target_card,
            target_fields,
            target_status,
            target_rows,
            enabled,
            aliases,
            result_card,
            results,
            save,
            close,
            channels,
            draft,
            config,
        }
    }
    fn load_targets(&self, result: Result<Vec<ClientTarget>, String>) {
        match result {
            Ok(targets) => {
                self.target_status.set_label(if targets.is_empty() {
                    "暂未找到可用客户端，可以先保存到 Hub，稍后再配置。"
                } else {
                    "可以多选；不选则只保存到 Hub。ChatGPT 首次使用还需完成接入设置。"
                });
                let mut rows = self.target_rows.borrow_mut();
                let manual_enabled = Rc::new(Cell::new(self.enabled.is_checked()));
                let enabled = self.enabled;
                let preference = manual_enabled.clone();
                enabled.on_toggled(move |_| preference.set(enabled.is_checked()));
                for target in targets {
                    let same_client = rows.iter().any(|(t, _)| t.client == target.client);
                    let label = if target.label == "默认" && !same_client {
                        target.client.name().to_string()
                    } else {
                        format!("{} · {}", target.client.name(), target.label)
                    };
                    let check = CheckBox::builder(&self.target_card)
                        .with_label(&label)
                        .build();
                    check.set_min_size(Size::new(0, check.get_best_size().height));
                    check.set_name(&format!("import-target-{}", target.id));
                    check.set_tooltip(&format!("{label}\n{}", target.models_root().display()));
                    self.target_fields.add(
                        &check,
                        0,
                        SizerFlag::Expand | SizerFlag::Bottom,
                        self.target_card.from_dip(10),
                    );
                    let row_store = self.target_rows.clone();
                    let save = self.save;
                    let manual_enabled = manual_enabled.clone();
                    check.on_toggled(move |_| {
                        let has_targets = row_store.borrow().iter().any(|(_, c)| c.is_checked());
                        enabled.set_value(has_targets || manual_enabled.get());
                        enabled.enable(!has_targets);
                        save.set_label(if has_targets {
                            "导入并配置客户端"
                        } else {
                            "保存到 Hub"
                        });
                    });
                    rows.push((target, check));
                }
            }
            Err(error) => {
                self.target_status
                    .set_label("暂时无法读取客户端，本次可先保存到 Hub。");
                super::feedback::error(
                    &self.chrome.dialog,
                    &format!("客户端列表读取失败：{error}"),
                );
            }
        }
        let width = self.chrome.dialog.get_client_size().width - self.chrome.dialog.from_dip(96);
        self.target_status.wrap(width.max(100));
        self.save.enable(true);
        self.layout();
    }
    fn request(&self) -> Result<(CommitImport, Vec<ClientTarget>), String> {
        let models = self.models.selected_models();
        let targets = self
            .target_rows
            .borrow()
            .iter()
            .filter(|(_, check)| check.is_checked())
            .map(|(target, _)| target.clone())
            .collect::<Vec<_>>();
        if !targets.is_empty() && models.is_empty() {
            return Err("请至少选择一个要接入的模型。".into());
        }
        let selected = self.operation.get_selection().unwrap_or(0) as usize;
        let mut draft = self.draft.clone();
        draft
            .provider
            .model_aliases
            .retain(|_, upstream| models.contains(upstream));
        let request = CommitImport {
            draft,
            name: self.name.get_value(),
            update: selected.checked_sub(1).map(|i| UpdateTarget {
                name: self.channels[i].name.clone(),
                fingerprint: import::provider_fingerprint(&self.channels[i]),
            }),
            models,
            enabled: !targets.is_empty() || self.enabled.is_checked(),
            visible_models: false,
            replace_aliases: self.aliases.is_checked(),
        };
        let mut trial = self.config.clone();
        import::merge_import(&mut trial, &request)?;
        Ok((request, targets))
    }
    fn start(&self) {
        self.save.enable(false);
        self.close.enable(false);
        for card in self.cards {
            card.hide();
        }
        self.result_card.show(true);
        self.results.start();
        self.progress("正在保存并接入…");
        self.chrome.body.scroll_coords(0, 0);
        self.layout();
    }
    fn progress(&self, message: &str) {
        self.chrome.status.set_label(message);
        self.chrome
            .status
            .set_foreground_color(super::theme::theme().ink_secondary);
        self.chrome.status.wrap(
            (self.chrome.dialog.get_client_size().width - self.chrome.dialog.from_dip(48)).max(100),
        );
        self.chrome.status.show(true);
        self.chrome.dialog.layout();
    }
    fn complete(&self, report: &batch::Report) {
        self.close.enable(true);
        self.close.set_label("完成");
        self.save.hide();
        if let Some(error) = &report.save_error {
            self.results.save_error(error);
        }
        self.chrome.status.set_label("");
        self.chrome.status.hide();
        if report.save_error.is_some() || report.outcomes.iter().any(|o| o.result.is_err()) {
            super::feedback::error(&self.chrome.dialog, &report.message());
        } else {
            super::feedback::success(&self.chrome.dialog, &report.message());
        }
        if report.save_error.is_none() && report.outcomes.is_empty() {
            self.results
                .detail
                .change_value("来源渠道已保存，可在渠道页继续管理。");
        }
        self.layout();
    }
    fn layout(&self) {
        self.chrome.dialog.layout();
        self.chrome.body.layout();
        self.chrome.body.fit_inside();
    }
}

enum ImportEvent {
    Progress(batch::Progress),
    Complete(batch::Report),
}

fn preview_with_config(parent: &Frame, api: &ApiClient, draft: ImportDraft, config: AppConfig) {
    let ui = Rc::new(ImportPreview::new(parent, draft, config));
    let dialog = ui.chrome.dialog;
    let (target_tx, target_rx) = mpsc::channel::<Result<Vec<ClientTarget>, String>>();
    let (event_tx, event_rx) = mpsc::channel::<ImportEvent>();
    let saving = Rc::new(Cell::new(false));
    let total = Rc::new(Cell::new(0usize));
    {
        let api = api.clone();
        let saving = saving.clone();
        let total = total.clone();
        let u = ui.clone();
        ui.save.on_click(move |_| {
            if saving.get() {
                return;
            }
            let (request, targets) = match u.request() {
                Ok(value) => value,
                Err(error) => {
                    super::feedback::error(&u.chrome.dialog, &error);
                    return;
                }
            };
            total.set(targets.len() * request.models.len());
            saving.set(true);
            u.start();
            let api = api.clone();
            let sender = event_tx.clone();
            thread::spawn(move || {
                let report = batch::run(&api, request, targets, |progress| {
                    let _ = sender.send(ImportEvent::Progress(progress));
                });
                let _ = sender.send(ImportEvent::Complete(report));
                wxdragon::wake_up_idle();
            });
        });
    }
    {
        let saving = saving.clone();
        dialog.on_close(move |event| {
            if saving.get() {
                if let WindowEventData::General(raw) = &event
                    && raw.can_veto()
                {
                    raw.veto();
                }
            } else {
                dialog.end_modal(ID_CANCEL);
            }
        });
    }
    let timer = Timer::new(&dialog);
    {
        let saving = saving.clone();
        let total = total.clone();
        let u = ui.clone();
        let finished = Cell::new(0usize);
        timer.on_tick(move |_| {
            if let Ok(result) = target_rx.try_recv() {
                u.load_targets(result);
            }
            while let Ok(event) = event_rx.try_recv() {
                match event {
                    ImportEvent::Progress(batch::Progress::Saved) => {
                        u.results.saved();
                        u.progress("渠道已保存，正在接入所选客户端…");
                    }
                    ImportEvent::Progress(batch::Progress::Item(outcome)) => {
                        finished.set(finished.get() + 1);
                        u.results.outcome(&outcome);
                        u.progress(&format!("已处理 {} / {} 项", finished.get(), total.get()));
                    }
                    ImportEvent::Complete(report) => {
                        saving.set(false);
                        u.complete(&report);
                    }
                }
            }
        });
    }
    {
        let api = api.clone();
        thread::spawn(move || {
            // Deep links may have just started Hub. Read only until the user submits.
            let deadline = Instant::now() + Duration::from_secs(30);
            while !api.is_online() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(300));
            }
            let _ = target_tx.send(batch::load_targets(&api));
            wxdragon::wake_up_idle();
        });
    }
    timer.start(100, false);
    dialog.show_modal();
    timer.stop();
    dialog.destroy();
}

#[cfg(all(test, windows))]
mod native_import_review {
    use super::*;
    use crate::{
        ai_gateway::config::ProviderConfig,
        desktop::{clients::ClientId, targets::McpFormat},
    };

    fn draft() -> ImportDraft {
        let source = import::ImportSource {
            origin: "https://synthetic.example".into(),
            key_id: "synthetic-key".into(),
            site_name: "合成站点的较长说明名称（用于窄窗口排版）".into(),
            key_name: "合成密钥的较长名称（仅本地示例）".into(),
        };
        ImportDraft {
            source: source.clone(),
            provider: ProviderConfig {
                name: format!("synthetic-channel-{}", "long-name-".repeat(14)),
                base_url: format!("https://synthetic.example/{}", "long-path/".repeat(12)),
                api_key: "synthetic-only-never-sent".into(),
                enabled: false,
                models: vec![
                    "model-a".into(),
                    "model-needle".into(),
                    format!("long-model-{}", "unbroken_".repeat(24)),
                ],
                import_source: Some(source),
                ..Default::default()
            },
            model_warning: None,
        }
    }
    fn targets() -> Vec<ClientTarget> {
        ClientId::VISIBLE
            .iter()
            .map(|client| ClientTarget {
                id: format!("synthetic-{}", client.key()),
                client: *client,
                label: format!("合成目标{}", "长名称".repeat(20)),
                root: std::path::PathBuf::from(format!("C:/synthetic/{}", client.key())),
                skills: Default::default(),
                mcp: Default::default(),
                mcp_format: McpFormat::Codex,
                present: true,
                model_mode: client.key().into(),
                model_root: None,
                models_inherited: false,
            })
            .collect()
    }
    fn footer_inside(ui: &ImportPreview) {
        ui.layout();
        let size = ui.chrome.dialog.get_client_size();
        for button in [ui.save, ui.close] {
            if !button.is_shown() {
                continue;
            }
            let point = ui
                .chrome
                .dialog
                .screen_to_client(button.client_to_screen(Point::new(0, 0)));
            let rect = button.get_size();
            assert!(
                point.x >= 0
                    && point.y >= 0
                    && point.x + rect.width <= size.width + 1
                    && point.y + rect.height <= size.height + 1,
                "import footer must remain inside the dialog: {point:?} {rect:?} {size:?}"
            );
        }
        let body = ui.chrome.body.get_client_size();
        assert!(body.width > 0 && body.height > 0);
        assert!(
            !ui.chrome.dialog.is_shown(),
            "fixture must never show a modal window"
        );
    }
    fn capture(ui: &ImportPreview, name: &str) {
        footer_inside(ui);
        crate::gui::native_model_review::capture(&ui.chrome.dialog, name);
    }

    #[test]
    #[ignore = "isolated hidden import form and result layout; run with --test-threads=1"]
    fn native_import_picker_keeps_selection_footer_and_structured_results_without_backend() {
        wxdragon::app::render_offscreen(|| {
            for (mode, suffix) in [
                (super::super::theme::ThemeMode::Light, "light"),
                (super::super::theme::ThemeMode::Dark, "dark"),
            ] {
                super::super::theme::init(mode);
                for (width, height) in [(760, 560), (1168, 800)] {
                    let parent = Frame::builder()
                        .with_title("hidden synthetic import preview")
                        .build();
                    parent.set_client_size(crate::gui::visual::dip_size(&parent, width, height));
                    let draft = draft();
                    let mut config = AppConfig::default();
                    config.ai_gateway.providers.push(draft.provider.clone());
                    let ui = ImportPreview::new(&parent, draft.clone(), config);
                    ui.load_targets(Ok(targets()));
                    assert_eq!(
                        ui.operation.get_selection(),
                        Some(1),
                        "same source/key defaults to update"
                    );
                    assert_eq!(ui.models.selected_models(), draft.provider.models);
                    assert!(
                        ui.target_rows
                            .borrow()
                            .iter()
                            .all(|(_, check)| !check.is_checked())
                    );
                    let (channel_only, targets) = ui.request().unwrap();
                    assert!(targets.is_empty());
                    assert!(!channel_only.enabled && !channel_only.visible_models);
                    ui.models.search.change_value("needle");
                    ui.models.render();
                    assert_eq!(ui.models.list.get_count(), 1);
                    ui.models.list.check(0, false);
                    ui.models.update_selection(0);
                    ui.models.search.change_value("");
                    ui.models.render();
                    assert_eq!(ui.models.list.get_count(), 3);
                    assert_eq!(
                        ui.models.selected_models().len(),
                        2,
                        "filtering must preserve other choices"
                    );
                    assert!(
                        !ui.models
                            .selected_models()
                            .iter()
                            .any(|m| m == "model-needle")
                    );
                    for (_, check) in ui.target_rows.borrow().iter() {
                        check.set_value(true);
                    }
                    let (request, chosen) = ui.request().unwrap();
                    assert!(request.enabled);
                    assert!(!request.visible_models);
                    assert_eq!(chosen.len(), 5);
                    assert_eq!(request.models.len(), 2);
                    footer_inside(&ui);
                    let body_width = ui.chrome.body.get_client_size().width;
                    for control in [
                        &ui.name as &dyn WxWidget,
                        &ui.operation,
                        &ui.models.search,
                        &ui.models.list,
                    ] {
                        let point = ui
                            .chrome
                            .body
                            .screen_to_client(control.client_to_screen(Point::new(0, 0)));
                        assert!(
                            point.x >= 0 && point.x + control.get_size().width <= body_width + 1,
                            "import field overflowed body width: {point:?} {:?}",
                            control.get_size()
                        );
                    }
                    assert!(
                        ui.chrome.body.get_virtual_size().height
                            > ui.chrome.body.get_client_size().height,
                        "long import content should scroll inside the body"
                    );
                    capture(&ui, &format!("import-selected-{width}-{suffix}"));
                    ui.start();
                    assert!(ui.cards.iter().all(|card| !card.is_shown()));
                    ui.results.saved();
                    let outcomes = vec![
                        batch::Outcome {
                            target: "ChatGPT / Codex".into(),
                            model: "model-a".into(),
                            result: Ok("已接入合成模型".into()),
                        },
                        batch::Outcome {
                            target: "DeepSeek Harness".into(),
                            model: "model-b".into(),
                            result: Err("合成写入超时，结果未知；请在客户端页核对。".into()),
                        },
                    ];
                    for outcome in &outcomes {
                        ui.results.outcome(outcome);
                    }
                    ui.complete(&batch::Report {
                        save_error: None,
                        outcomes,
                    });
                    assert_eq!(ui.results.table.get_item_count(), 3);
                    assert_eq!(ui.results.table.get_item_text(1, 2), "已接入");
                    assert_eq!(ui.results.table.get_item_text(2, 2), "未完成");
                    ui.results.select(2);
                    assert!(ui.results.detail.get_value().contains("结果未知"));
                    assert!(!ui.save.is_shown());
                    assert!(ui.close.is_enabled());
                    capture(&ui, &format!("import-partial-results-{width}-{suffix}"));
                    ui.start();
                    ui.complete(&batch::Report {
                        save_error: Some("合成保存超时，结果未知".into()),
                        outcomes: vec![],
                    });
                    assert_eq!(ui.results.table.get_item_text(0, 2), "未确认");
                    capture(&ui, &format!("import-unknown-save-{width}-{suffix}"));
                    ui.chrome.dialog.destroy();

                    let mut empty_draft = draft;
                    empty_draft.provider.models.clear();
                    empty_draft.model_warning = Some("合成目录为空，允许先保存渠道。".into());
                    let empty = ImportPreview::new(&parent, empty_draft, AppConfig::default());
                    empty.load_targets(Ok(vec![]));
                    let (request, chosen) = empty.request().unwrap();
                    assert!(request.models.is_empty() && chosen.is_empty() && !request.enabled);
                    assert!(empty.target_rows.borrow().is_empty());
                    capture(&empty, &format!("import-empty-catalog-{width}-{suffix}"));
                    empty.chrome.dialog.destroy();
                    assert!(!parent.is_shown());
                    parent.destroy();
                }
            }
        })
        .unwrap();
    }
}
