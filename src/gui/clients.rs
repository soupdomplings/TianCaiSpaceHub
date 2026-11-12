//! Native client workspace. Legacy editors keep their protected save services.
use super::{
    api::ApiClient,
    dialog_job,
    shell::{PageId, Shell},
    theme,
};
use super::{
    controls::Button,
    visual::{self, Canvas, ClientDiagram},
};
use crate::desktop::clients::ClientId;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use wxdragon::dc::GraphicsDC;
use wxdragon::{prelude::*, timer::Timer, widgets::SimpleBook};
enum ClientUpdate {
    Models(Result<Value, String>),
    Activity(Result<Value, String>),
    Library(Result<Value, String>),
}
type Slot = Arc<Mutex<Vec<ClientUpdate>>>;

#[derive(Clone)]
pub(super) struct ClientPage {
    pub page: Panel,
    pub launch: Button,
    pub reload: Button,
    pub model_host: Panel,
    pub settings_host: Panel,
    pub skills_host: Panel,
    pub mcp_host: Panel,
    settings: super::client_header::ClientSettings,
    workbuddy_share: Option<super::workbuddy_share_settings::ShareSettings>,
    id: ClientId,
    graph: Panel,
    state_label: StaticText,
    error: StaticText,
    action_job: Rc<dialog_job::InlineJob>,
    model_loading: super::loading::LoadingIndicator,
    model_list: super::model_list::ModelList,
    model_fields: Option<super::model_fields::ModelFields>,
    model_view: super::model_fields::ModelPage,
    editing_id: Rc<RefCell<Option<String>>>,
    draft: Rc<RefCell<Option<(Value, Value)>>>,
    editing_snapshot: Rc<RefCell<Option<(Value, Value)>>>,
    model_refresher: Rc<RefCell<Option<Rc<dyn Fn(bool)>>>>,
    model_adder: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    model_focuser: Rc<RefCell<Option<Rc<dyn Fn(&str)>>>>,
    model_saved: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    model_remover: Rc<RefCell<Option<Rc<dyn Fn(&str)>>>>,
    codex_selection: Rc<RefCell<Option<(Vec<String>, Vec<String>)>>>,
    models_render_pending: Rc<Cell<bool>>,
    models: Rc<RefCell<Value>>,
    activity: Rc<RefCell<Value>>,
    library: Rc<RefCell<Value>>,
    rows: Rc<RefCell<Vec<Value>>>,
    api: ApiClient,
    frame: Frame,
    active: Rc<Cell<bool>>,
    busy: Rc<Cell<bool>>,
    refresh_pending: Rc<Cell<bool>>,
    activity_busy: Arc<AtomicBool>,
    library_busy: Arc<AtomicBool>,
    pending: Slot,
    timer: Rc<RefCell<Option<Timer<Panel>>>>,
    last: Rc<Cell<Instant>>,
    book: SimpleBook,
    tabs: Vec<Button>,
    hits: Rc<RefCell<Vec<(Rect, usize)>>>,
    fresh: Rc<Cell<Instant>>,
}
impl ClientPage {
    pub fn new(shell: &Shell, id: ClientId, frame: Frame, api: ApiClient) -> Self {
        let t = theme::theme();
        let page = Panel::builder(&shell.book).build();
        page.set_background_color(t.bg_app);
        let column = BoxSizer::builder(Orientation::Vertical).build();
        let state_label = shell.status;
        state_label.set_foreground_color(t.ink_secondary);
        let header = super::client_header::ClientHeader::new(&page, id);
        let reload = header.reload;
        let open = header.launch;
        let buttons = header.tabs;
        column.add(
            &header.panel,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(12),
        );
        let book = SimpleBook::builder(&page).build();
        let status_page = ScrolledWindow::builder(&book)
            .with_style(ScrolledWindowStyle::VScroll | ScrolledWindowStyle::HScroll)
            .build();
        status_page.set_scroll_rate(status_page.from_dip(8), status_page.from_dip(12));
        status_page.set_background_color(t.bg_app);
        let status_sizer = BoxSizer::builder(Orientation::Vertical).build();
        let graph = Panel::builder(&status_page).build();
        graph.set_min_size(visual::dip_size(&graph, 620, 430));
        graph.set_background_style(BackgroundStyle::Paint);
        visual::redraw_on_resize(graph);
        graph.set_background_color(t.bg_app);
        status_sizer.add(&graph, 0, SizerFlag::Expand, 0);
        status_page.set_sizer(status_sizer, true);
        book.add_page(&status_page, "状态概览", true, None);
        let model_view = super::model_fields::ModelPage::new(&book, id);
        let add = model_view.add;
        add.enable(false);
        let generic = !matches!(
            id,
            ClientId::Codex | ClientId::WorkBuddy | ClientId::Tiangong
        );
        let (model_list, model_fields) = (
            model_view.workspace.list.clone(),
            Some(model_view.workspace.fields.clone()),
        );
        let model_host = model_view.legacy_host;
        book.add_page(&model_view.page, "模型", false, None);
        let hosts = std::array::from_fn::<_, 3, _>(|i| {
            let viewport = ScrolledWindow::builder(&book)
                .with_style(ScrolledWindowStyle::VScroll)
                .build();
            viewport.set_scroll_rate(0, viewport.from_dip(10));
            viewport.set_background_color(t.bg_app);
            let host = Panel::builder(&viewport).build();
            host.set_background_color(t.bg_app);
            host.set_sizer(BoxSizer::builder(Orientation::Vertical).build(), true);
            let layout = BoxSizer::builder(Orientation::Vertical).build();
            layout.add(&host, 1, SizerFlag::Expand, 0);
            viewport.set_sizer(layout, true);
            book.add_page(&viewport, ["Skill", "MCP", "接入设置"][i], false, None);
            host
        });
        let settings = super::client_header::ClientSettings::new(&hosts[2], id);
        if let Some(col) = hosts[2].get_sizer() {
            col.add(
                &settings.panel,
                0,
                SizerFlag::Expand | SizerFlag::All,
                page.from_dip(18),
            );
        }
        let workbuddy_share = (id == ClientId::WorkBuddy).then(|| {
            super::workbuddy_share_settings::ShareSettings::new(&hosts[2], frame, api.clone())
        });
        column.add(&book, 1, SizerFlag::Expand, 0);
        let error = StaticText::builder(&page).with_label("").build();
        error.set_foreground_color(t.error);
        error.hide();
        column.add(&error, 0, SizerFlag::Expand | SizerFlag::All, 10);
        let action_status = StaticText::builder(&page).with_label("").build();
        action_status.set_name("client-action-status");
        action_status.hide();
        column.add(
            &action_status,
            0,
            SizerFlag::Expand | SizerFlag::All,
            page.from_dip(10),
        );
        let action_job = Rc::new(dialog_job::InlineJob::new(page, action_status));
        let model_loading = super::loading::LoadingIndicator::for_button(&page, reload);
        page.set_sizer(column, true);
        shell.register(PageId::Client(id), &page);
        let ui = Self {
            page,
            launch: open,
            reload,
            model_host,
            skills_host: hosts[0],
            mcp_host: hosts[1],
            settings_host: hosts[2],
            settings,
            workbuddy_share,
            id,
            graph,
            state_label,
            error,
            action_job,
            model_loading,
            model_list,
            model_fields,
            model_view,
            editing_id: Default::default(),
            draft: Default::default(),
            editing_snapshot: Default::default(),
            model_refresher: Default::default(),
            model_adder: Default::default(),
            model_focuser: Default::default(),
            model_saved: Default::default(),
            model_remover: Default::default(),
            codex_selection: Default::default(),
            models_render_pending: Default::default(),
            models: Default::default(),
            activity: Default::default(),
            library: Default::default(),
            rows: Default::default(),
            api,
            frame,
            active: Default::default(),
            busy: Default::default(),
            refresh_pending: Default::default(),
            activity_busy: Default::default(),
            library_busy: Default::default(),
            pending: Default::default(),
            timer: Default::default(),
            last: Rc::new(Cell::new(Instant::now() - Duration::from_secs(30))),
            book,
            tabs: buttons.clone(),
            hits: Default::default(),
            fresh: Rc::new(Cell::new(Instant::now() - Duration::from_secs(60))),
        };
        ui.build_settings();
        for (index, button) in buttons.into_iter().enumerate() {
            let u = ui.clone();
            button.on_click(move |_| {
                u.select(index);
                u.fetch();
            });
        }
        if !matches!(id, ClientId::WorkBuddy | ClientId::Tiangong) {
            let u = ui.clone();
            reload.on_click(move |_| {
                u.refresh_pending.set(true);
                u.refresh_legacy_models(true);
                u.fetch();
            });
        }
        let u = ui.clone();
        graph.on_paint(move |_| u.paint());
        let (u, nav) = (ui.clone(), shell.clone());
        graph.on_mouse_left_up(move |event| {
            let WindowEventData::MouseButton(e) = event else {
                return;
            };
            let Some(p) = e.get_position() else {
                return;
            };
            let hit = u
                .hits
                .borrow()
                .iter()
                .find(|(r, _)| {
                    p.x >= r.x && p.x < r.x + r.width && p.y >= r.y && p.y < r.y + r.height
                })
                .map(|(_, i)| *i);
            match hit {
                Some(0) => {
                    u.select(4);
                }
                Some(1) => {
                    u.select(1);
                }
                Some(2) => {
                    let sources = u.models.borrow();
                    let routes = sources["copies"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|r| r["client"] == u.id.key())
                        .collect::<Vec<_>>();
                    if routes.len() == 1 {
                        if let Some(route) = routes[0]["routeId"].as_str() {
                            nav.focus_source(route.into());
                        }
                    } else {
                        nav.navigate(PageId::Sources);
                    }
                }
                Some(5) => nav.navigate(PageId::Chat),
                _ => {}
            }
        });
        if generic {
            let u = ui.clone();
            open.on_click(move |_| {
                u.action(
                    "打开客户端",
                    "/api/desktop/client/open",
                    json!({"client":u.id}),
                    |_, _| {},
                );
            });
        }
        let u = ui.clone();
        add.on_click(move |_| u.add());
        let u = ui.clone();
        ui.model_view.remove_saved.on_click(move |_| {
            if u.selected().is_some_and(|r| r["draft"] == true) {
                u.remove();
                return;
            }
            let model = u
                .selected()
                .and_then(|r| r["model"].as_str().map(str::to_string));
            if let (Some(model), Some(remove)) = (model, u.model_remover.borrow().clone()) {
                remove(&model);
            }
        });
        let u = ui.clone();
        ui.model_view
            .back
            .on_click(move |_| u.model_view.show_draft(false));
        if let Some(fields) = &ui.model_fields {
            let u = ui.clone();
            ui.model_list.on_selection_changed(move |_| u.fill_model());
            let u = ui.clone();
            fields.connection.on_click(move |_| u.edit());
            let u = ui.clone();
            fields.save.on_click(move |_| u.save_model());
            let u = ui.clone();
            fields.remove.on_click(move |_| u.remove());
            let u = ui.clone();
            fields.restore.on_click(move |_| u.restore());
            let u = ui.clone();
            fields.reload.on_click(move |_| u.reload_model());
        }
        let timer = Timer::new(&page);
        let u = ui.clone();
        timer.on_tick(move |_| {
            if !u.active.get() {
                return;
            }
            if u.models_render_pending.replace(false) {
                u.render();
            }
            u.apply_pending();
            if let Some(share) = &u.workbuddy_share {
                share.tick(u.book.selection() == 4 && u.frame.is_shown() && !u.frame.is_iconized());
            }
            if u.last.get().elapsed() > Duration::from_secs(5) {
                u.fetch();
            }
            u.graph.refresh(false, None);
        });
        ui.timer.borrow_mut().replace(timer);
        ui
    }
    pub fn attach_model<W: WxWidget>(&self, page: &W) {
        if let Some(sizer) = self.model_host.get_sizer() {
            sizer.add(page, 1, SizerFlag::Expand, 0);
        }
        self.model_host.layout();
    }
    pub fn on_model_refresh(&self, refresh: impl Fn(bool) + 'static) {
        *self.model_refresher.borrow_mut() = Some(Rc::new(refresh));
    }
    pub fn on_model_add(&self, add: impl Fn() + 'static) {
        *self.model_adder.borrow_mut() = Some(Rc::new(add));
    }
    pub fn on_model_focus(&self, focus: impl Fn(&str) + 'static) {
        *self.model_focuser.borrow_mut() = Some(Rc::new(focus));
    }
    pub fn on_model_remove(&self, remove: impl Fn(&str) + 'static) {
        *self.model_remover.borrow_mut() = Some(Rc::new(remove));
    }
    pub fn set_codex_selection(&self, models: Vec<String>, base: Vec<String>) {
        *self.codex_selection.borrow_mut() = Some((models, base));
        // A checkbox command is still running. Updating list rows in place is
        // safe, but defer focusing/rebuilding its originating checkbox panel.
        self.models_render_pending.set(true);
    }
    pub fn finish_codex_models(&self) {
        self.codex_selection.borrow_mut().take();
        self.draft.borrow_mut().take();
        self.editing_snapshot.borrow_mut().take();
        self.editing_id.borrow_mut().take();
        self.models_render_pending.set(true);
        if self.active.get() {
            self.fetch();
        }
    }
    fn refresh_legacy_models(&self, rebase_draft: bool) {
        if let Some(refresh) = self.model_refresher.borrow().clone() {
            refresh(rebase_draft);
        }
    }
    fn build_settings(&self) {
        let host = self.settings_host;
        let Some(column) = host.get_sizer() else {
            return;
        };
        if self.id == ClientId::Tiangong {
            let assistance = Button::builder(&host)
                .with_label("本机协助")
                .with_kind(super::controls::ButtonKind::Secondary)
                .build();
            assistance.set_name("tiangong-local-assistance");
            let api = self.api.clone();
            let frame = self.frame;
            assistance.on_click(move |_| super::local_assistance::show(&frame, api.clone()));
            column.add(
                &assistance,
                0,
                SizerFlag::Left | SizerFlag::Bottom,
                host.from_dip(16),
            );
        }
        if self.id == ClientId::WebStorm {
            let export = Button::builder(&host)
                .with_label("导出 AI Assistant 辅助插件")
                .build();
            column.add(&export, 0, SizerFlag::Left | SizerFlag::Bottom, 18);
            let u = self.clone();
            export.on_click(move |_| {
                let picker = FileDialog::builder(&u.frame)
                    .with_message("保存 Hub 辅助插件")
                    .with_default_file("tiancai-hub-bridge.jar")
                    .with_wildcard("JetBrains 插件 (*.jar)|*.jar")
                    .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
                    .build();
                let selected = (picker.show_modal() == ID_OK).then(|| picker.get_path()).flatten();
                picker.destroy();
                if let Some(path) = selected {
                    u.action("导出辅助插件", "/api/desktop/webstorm/plugin", json!({"path":path}), |u, _| {
                        u.action_job.feedback("插件已导出。在 WebStorm 设置 → 插件 → 从磁盘安装中选择此文件，重开 IDE 并打开项目；适配 AI Assistant 262.11879 系列。", false);
                    });
                }
            });
            let install = Button::builder(&host)
                .with_label("安装 / 修复 Air / ACP 组件")
                .build();
            column.add(&install, 0, SizerFlag::Left | SizerFlag::Bottom, 18);
            let u = self.clone();
            install.on_click(move |_| {
                u.action(
                    "安装 ACP 组件",
                    "/api/desktop/components/install",
                    json!({}),
                    |u, value| {
                        if value["opencode"]["installed"] == true {
                            u.action_job.feedback(
                                "ACP 组件已安装。到模型页选择 Air / ACP，再使用模型。",
                                false,
                            );
                        }
                    },
                );
            });
        }
        if let Some(download) = self.settings.download {
            super::tiangong_models::bind_download(download);
        }
        let executable = self.settings.program;
        let u = self.clone();
        executable.on_click(move |_| {
            #[cfg(target_os = "macos")]
            let selected = {
                let picker =
                    DirDialog::builder(&u.frame, "选择客户端 .app 程序包", "/Applications")
                        .with_style(DirDialogStyle::MustExist.bits())
                        .build();
                let selected = (picker.show_modal() == ID_OK)
                    .then(|| picker.get_path())
                    .flatten();
                picker.destroy();
                selected
            };
            #[cfg(not(target_os = "macos"))]
            let selected = {
                let picker = FileDialog::builder(&u.frame)
                    .with_message("选择客户端程序")
                    .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
                    .build();
                let selected = (picker.show_modal() == ID_OK)
                    .then(|| picker.get_path())
                    .flatten();
                picker.destroy();
                selected
            };
            if let Some(path) = selected {
                u.action(
                    "保存程序位置",
                    "/api/desktop/client/open",
                    json!({"client":u.id,"path":path}),
                    |_, _| {},
                );
            }
        });
        let u = self.clone();
        self.settings.folder.on_click(move |_| {
            if let Some(path) = u.settings.directory() {
                if let Err(e) = super::resource_detail::open_directory(&path) {
                    super::show_error(&u.frame, &e);
                }
            }
        });
        host.layout();
    }
    fn action(
        &self,
        title: &str,
        path: &str,
        body: Value,
        saved: impl FnOnce(&Self, Value) + 'static,
    ) {
        if self.action_job.is_running() {
            return;
        }
        let previous = (
            self.model_view.page.is_enabled(),
            self.settings_host.is_enabled(),
            self.launch.is_enabled(),
            self.reload.is_enabled(),
        );
        let u = self.clone();
        if self
            .action_job
            .start(self.api.clone(), title, path, body, move |outcome| {
                u.model_view.page.enable(previous.0);
                u.settings_host.enable(previous.1);
                u.launch.enable(previous.2);
                u.reload.enable(previous.3);
                if let Ok(value) = outcome {
                    saved(&u, value);
                }
                u.fetch();
            })
        {
            self.model_view.page.enable(false);
            self.settings_host.enable(false);
            self.launch.enable(false);
            self.reload.enable(false);
        }
    }
    pub fn enter(&self, page: PageId) {
        let active = page == PageId::Client(self.id);
        let entering = active && !self.active.get();
        self.active.set(active);
        if let Some(timer) = self.timer.borrow().as_ref() {
            if active {
                // Consume a read completed while hidden before requesting its
                // replacement; a hidden page must not keep its busy flag set.
                self.apply_pending();
                if entering {
                    self.refresh_pending.set(true);
                    self.refresh_legacy_models(false);
                    if let Some(share) = &self.workbuddy_share {
                        share.read();
                    }
                }
                timer.start(160, false);
                self.fetch();
            } else {
                timer.stop();
            }
        }
    }
    pub fn configuration_changed(&self) {
        self.refresh_pending.set(true);
        if self.active.get() {
            self.refresh_legacy_models(false);
            self.fetch_models();
        }
    }
    fn fetch(&self) {
        self.last.set(Instant::now());
        self.fetch_models();
        // Model lists must not wait for process/resource discovery. Those
        // snapshots are only needed by the client's visible overview.
        if self.book.get_page(0).is_some_and(|page| page.is_shown()) {
            self.fetch_auxiliary(false);
            self.fetch_auxiliary(true);
        }
    }
    fn fetch_models(&self) {
        if self.busy.replace(true) {
            return;
        }
        if self.refresh_pending.get() || self.models.borrow()["revision"].is_null() {
            self.model_loading.start("读取模型中…");
        }
        self.refresh_pending.set(false);
        let (api, slot) = (self.api.clone(), self.pending.clone());
        std::thread::spawn(move || {
            let result = api.get_with_timeout("/api/desktop/models", Duration::from_secs(8));
            if let Ok(mut messages) = slot.lock() {
                messages.push(ClientUpdate::Models(result));
            }
            wxdragon::wake_up_idle();
        });
    }
    fn fetch_auxiliary(&self, library: bool) {
        let flag = if library {
            self.library_busy.clone()
        } else {
            self.activity_busy.clone()
        };
        if flag.swap(true, Ordering::SeqCst) {
            return;
        }
        let (api, slot) = (self.api.clone(), self.pending.clone());
        std::thread::spawn(move || {
            let update = if library {
                ClientUpdate::Library(
                    api.get_with_timeout("/api/desktop/library", Duration::from_secs(10)),
                )
            } else {
                ClientUpdate::Activity(
                    api.get_with_timeout("/api/desktop/activity", Duration::from_secs(5)),
                )
            };
            if let Ok(mut messages) = slot.lock() {
                messages.push(update);
            }
            flag.store(false, Ordering::SeqCst);
            wxdragon::wake_up_idle();
        });
    }
    fn apply_pending(&self) {
        let messages = self
            .pending
            .lock()
            .map(|mut messages| std::mem::take(&mut *messages))
            .unwrap_or_default();
        let mut render = false;
        for message in messages {
            match message {
                ClientUpdate::Models(result) => {
                    self.model_loading.finish();
                    self.busy.set(false);
                    if self.refresh_pending.replace(false) {
                        // A save happened during this read. Discard its stale
                        // model view and read once more, without losing drafts.
                        self.fetch_models();
                        continue;
                    }
                    match result {
                        Ok(value) => {
                            render |= *self.models.borrow() != value;
                            *self.models.borrow_mut() = value;
                            self.model_view
                                .add
                                .enable(!self.models.borrow()["revision"].is_null());
                            self.error.hide();
                        }
                        Err(error) => {
                            self.error.set_label(&error);
                            self.error.wrap(700);
                            self.error.show(true);
                        }
                    }
                }
                ClientUpdate::Activity(result) => {
                    if let Ok(value) = result {
                        *self.activity.borrow_mut() = value;
                        self.fresh.set(Instant::now());
                        render = true;
                    }
                }
                ClientUpdate::Library(result) => {
                    if let Ok(value) = result {
                        *self.library.borrow_mut() = value;
                    }
                }
            }
        }
        if render {
            self.render();
        }
    }
    fn render(&self) {
        self.settings.set_targets(&self.models.borrow(), self.id);
        let selected = self.model_list.get_selection().and_then(|i| {
            self.rows
                .borrow()
                .get(i as usize)
                .and_then(|r| r["id"].as_str())
                .map(str::to_string)
        });
        let mut rows = self.models.borrow()["copies"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|r| r["client"] == self.id.key())
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if self.id == ClientId::Codex {
            // Keep unsaved choices, but let assistant/API saves appear as soon
            // as the client snapshot arrives, independently of dashboard timing.
            let selected = self
                .codex_selection
                .borrow()
                .clone()
                .and_then(|(models, base)| {
                    (!super::codex_tab::same_model_set(&models, &base)
                        || self.models.borrow().get("codexVisibleModels").is_none())
                    .then_some(models)
                })
                .unwrap_or_else(|| {
                    self.models.borrow()["codexVisibleModels"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                });
            rows.extend(selected.into_iter().map(|id|json!({"id":format!("codex-visible:{id}"),"model":id,"label":id,"codexCatalog":true,"using":true})));
        }
        for row in &mut rows {
            row["sourceName"] = self.models.borrow()["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|s| s["routeId"] == row["sourceRoute"])
                .map(|s| Value::String(source_label(s)))
                .unwrap_or_else(|| "独立配置".into());
        }
        if let Some((row, _)) = self.draft.borrow().as_ref() {
            rows.push(row.clone());
        }
        let labels = rows
            .iter()
            .map(|row| {
                format!(
                    "{}{}",
                    row["label"].as_str().unwrap_or("模型"),
                    if row["using"] == false {
                        " · 未使用"
                    } else if row["using"].is_null() && row["draft"] != true {
                        " · 待核对"
                    } else {
                        ""
                    }
                )
            })
            .collect::<Vec<_>>();
        self.model_list.replace(&labels);
        let index = selected
            .and_then(|id| rows.iter().position(|r| r["id"] == id))
            .unwrap_or(0);
        if !rows.is_empty() {
            self.model_list.set_selection(index as u32, true);
        }
        *self.rows.borrow_mut() = rows;
        self.model_view
            .remove_saved
            .enable(!self.rows.borrow().is_empty());
        let id = self
            .selected()
            .and_then(|r| r["id"].as_str().map(str::to_string));
        if *self.editing_id.borrow() != id {
            self.fill_model();
        }
        if let Some(fields) = &self.model_fields {
            let has_recovery = self.models.borrow()["operations"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|o| {
                    self.models.borrow()["targets"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|t| t["id"] == o["targetId"] && t["client"] == self.id.key())
                });
            fields.restore.show(has_recovery);
            fields.reload.show(
                self.editing_snapshot
                    .borrow()
                    .as_ref()
                    .is_some_and(|(_, view)| view["revision"] != self.models.borrow()["revision"]),
            );
        }
        let data = self.activity.borrow();
        let state = data["clients"]
            .as_array()
            .and_then(|a| a.iter().find(|r| r["client"] == self.id.key()));
        let opened = state.and_then(|r| r["opened"].as_bool());
        let requests = state
            .and_then(|r| r["requests"].as_array())
            .map(Vec::len)
            .unwrap_or(0);
        self.state_label.set_label(&format!(
            "{}{}",
            match opened {
                Some(true) => "已打开",
                Some(false) => "未打开",
                None => "运行状态未知",
            },
            if requests > 0 {
                format!(" · {requests} 个请求进行中")
            } else {
                String::new()
            }
        ));
        self.graph.refresh(false, None);
        self.page.layout();
    }
    fn selected(&self) -> Option<Value> {
        self.model_list
            .get_selection()
            .and_then(|i| self.rows.borrow().get(i as usize).cloned())
    }
    fn fill_model(&self) {
        if let (Some((mut previous, view)), Some(fields)) =
            (self.editing_snapshot.borrow().clone(), &self.model_fields)
        {
            if previous["draft"] == true {
                previous["draftFields"] = fields.draft_snapshot();
                *self.draft.borrow_mut() = Some((previous.clone(), view));
                if let Some(row) = self
                    .rows
                    .borrow_mut()
                    .iter_mut()
                    .find(|r| r["draft"] == true)
                {
                    *row = previous;
                }
            }
        }
        let row = self.selected();
        *self.editing_snapshot.borrow_mut() = if row.as_ref().is_some_and(|r| r["draft"] == true) {
            self.draft.borrow().clone()
        } else {
            row.as_ref()
                .map(|r| (r.clone(), self.models.borrow().clone()))
        };
        *self.editing_id.borrow_mut() = row
            .as_ref()
            .and_then(|r| r["id"].as_str().map(str::to_string));
        if let Some(fields) = &self.model_fields
            && self.id != ClientId::Codex
        {
            let native = row.as_ref().is_some_and(|r| {
                self.models.borrow()["targets"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|t| t["id"] == r["targetId"] && t["modelMode"] == "native")
            });
            fields.fill(row.as_ref(), native);
            if let Some(row) = row.as_ref().filter(|r| r["draft"] == true) {
                fields.restore_draft(&row["draftFields"]);
            }
            fields.reload.hide();
        }
        if self.id == ClientId::Codex {
            self.model_view
                .show_draft(row.as_ref().is_some_and(|r| r["draft"] == true));
            if let Some(row) = row.as_ref().filter(|r| r["codexCatalog"] == true) {
                if let (Some(focus), Some(model)) =
                    (self.model_focuser.borrow().clone(), row["model"].as_str())
                {
                    focus(model);
                }
            }
        }
    }
    fn reload_model(&self) {
        // This is an explicit discard-and-reload action. Background refreshes
        // keep the draft's original revision so a later save cannot silently
        // overwrite configuration changed elsewhere.
        if let Some(mut row) = self.selected().filter(|r| r["draft"] == true) {
            let view = self.models.borrow().clone();
            row.as_object_mut().unwrap().remove("draftFields");
            if let Some(fields) = &self.model_fields {
                fields.prepare_draft(&view, self.id);
            }
            if let Some(current) = self
                .rows
                .borrow_mut()
                .iter_mut()
                .find(|r| r["draft"] == true)
            {
                *current = row.clone();
            }
            self.editing_snapshot.borrow_mut().take();
            *self.draft.borrow_mut() = Some((row, view));
        }
        self.fill_model();
    }
    fn save_model(&self) {
        let (Some((row, view)), Some(fields)) =
            (self.editing_snapshot.borrow().clone(), &self.model_fields)
        else {
            return;
        };
        let mut body = match fields.body(&row, &view, self.id) {
            Ok(body) => body,
            Err(error) => {
                self.action_job.feedback(&error, true);
                return;
            }
        };
        let legacy = matches!(
            self.id,
            ClientId::Codex | ClientId::WorkBuddy | ClientId::Tiangong
        );
        body["client"] = serde_json::to_value(self.id).unwrap();
        self.action(
            "保存客户端模型",
            if legacy {
                "/api/desktop/models/copy"
            } else {
                "/api/desktop/models/use"
            },
            body,
            move |u, _| {
                u.draft.borrow_mut().take();
                u.editing_snapshot.borrow_mut().take();
                if let Some(saved) = u.model_saved.borrow().clone() {
                    saved();
                }
                if legacy {
                    u.model_view.show_draft(false);
                    u.refresh_legacy_models(true);
                }
                u.editing_id.borrow_mut().take();
            },
        );
    }
    fn begin_model_draft(&self) {
        if self.models.borrow()["revision"].is_null() {
            self.fetch();
            return;
        }
        self.model_view.show_draft(true);
        if self.draft.borrow().is_some() {
            let last = self.rows.borrow().len().saturating_sub(1);
            self.model_list.set_selection(last as u32, true);
            if self.editing_id.borrow().as_deref() != Some("__draft__") {
                self.fill_model();
            }
            return;
        }
        let view = self.models.borrow().clone();
        let row = json!({"id":"__draft__","draft":true,"label":"新建模型","model":"新建模型","capabilities":{}});
        if let Some(fields) = &self.model_fields
            && self.id != ClientId::Codex
        {
            fields.prepare_draft(&view, self.id);
        }
        *self.draft.borrow_mut() = Some((row, view));
        self.render();
        self.model_list
            .set_selection((self.rows.borrow().len() - 1) as u32, true);
        self.fill_model();
    }
    pub fn on_model_saved(&self, saved: impl Fn() + 'static) {
        *self.model_saved.borrow_mut() = Some(Rc::new(saved));
    }
    fn add(&self) {
        self.select(1);
        if let Some(add) = self.model_adder.borrow().clone() {
            add();
            self.model_view.show_draft(false);
        } else {
            self.begin_model_draft();
        }
    }
    fn edit(&self) {
        let Some(row) = self.selected() else {
            return;
        };
        self.action(
            "读取模型配置",
            "/api/desktop/models/read",
            json!({"id":row["id"]}),
            move |u, value| {
                // Reading in the background must not open an editor over a
                // different page the user has moved to in the meantime.
                if !u.active.get() { return; }
                let Ok(mut provider) = serde_json::from_value::<crate::ai_gateway::config::ProviderConfig>(value) else {
                    u.action_job.feedback("模型配置格式不兼容，请重新读取", true);
                    return;
                };
                provider.name = row["label"].as_str().unwrap_or("模型").into();
                provider.desktop_binding = None;
                if let Some(provider) = super::show_ai_gw_channel_dialog(
                    &u.frame,
                    super::text::GuiText::new(super::load_gui_locale()),
                    &u.api,
                    Some(&provider),
                ) {
                    let view = u.models.borrow();
                    let body = json!({"revision":view["revision"],"targetId":row["targetId"],"id":row["id"],"label":provider.name,"provider":provider,"using":true});
                    drop(view);
                    u.action("保存客户端模型", "/api/desktop/models/use", body, |_, _| {});
                }
            }
        );
    }
    fn remove(&self) {
        if self.selected().is_some_and(|r| r["draft"] == true) {
            self.draft.borrow_mut().take();
            self.editing_snapshot.borrow_mut().take();
            self.editing_id.borrow_mut().take();
            if matches!(
                self.id,
                ClientId::Codex | ClientId::WorkBuddy | ClientId::Tiangong
            ) {
                self.model_view.show_draft(false);
            }
            self.render();
            return;
        }
        let Some(row) = self.selected() else {
            return;
        };
        if !super::external_import::confirm(
            &self.frame,
            &format!(
                "从 {} 移除模型“{}”？",
                self.id.name(),
                row["label"].as_str().unwrap_or("此模型")
            ),
        ) {
            return;
        }
        let view = self.models.borrow().clone();
        let body = json!({"revision":view["revision"],"targetId":row["targetId"],"id":row["id"],"using":false});
        self.action(
            "移除此客户端模型",
            "/api/desktop/models/use",
            body,
            |_, _| {},
        );
    }
    fn restore(&self) {
        let view = self.models.borrow();
        let operations = view["operations"]
            .as_array()
            .map(|a| {
                a.iter()
                    .rev()
                    .filter(|o| {
                        view["targets"].as_array().is_some_and(|t| {
                            t.iter()
                                .any(|t| t["id"] == o["targetId"] && t["client"] == self.id.key())
                        })
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        drop(view);
        if operations.is_empty() {
            self.action_job.feedback("没有此客户端的可撤销更改", false);
            return;
        }
        if operations.len() == 1 {
            if super::external_import::confirm(&self.frame, "恢复此客户端最近一次模型配置更改？")
            {
                self.action(
                    "恢复模型配置",
                    "/api/desktop/models/restore",
                    json!({"undo":operations[0]["id"]}),
                    |_, _| {},
                );
            }
            return;
        }
        let labels = operations
            .iter()
            .enumerate()
            .map(|(i, o)| {
                format!(
                    "最近第 {} 次更改 · {}",
                    i + 1,
                    match o["phase"].as_str().unwrap_or("") {
                        "applied" => "已应用",
                        "prepared" => "已准备",
                        "native-error" => "写入未完成",
                        "needs-recovery" => "待恢复",
                        _ => "待核对",
                    }
                )
            })
            .collect::<Vec<_>>();
        let dialog = SingleChoiceDialog::builder(
            &self.frame,
            "选择要恢复的更改",
            "撤销更改",
            &labels.iter().map(String::as_str).collect::<Vec<_>>(),
        )
        .build();
        let selected = (dialog.show_modal() == ID_OK)
            .then(|| dialog.get_selection())
            .filter(|i| *i >= 0);
        dialog.destroy();
        if let Some(i) = selected {
            self.action(
                "恢复模型配置",
                "/api/desktop/models/restore",
                json!({"undo":operations[i as usize]["id"]}),
                |_, _| {},
            );
        }
    }
    fn paint(&self) {
        let base = AutoBufferedPaintDC::new(&self.graph);
        let dc = GraphicsDC::new(&base);
        let t = theme::theme();
        dc.set_background(t.bg_card);
        dc.clear();
        dc.set_font(&theme::font(theme::TextRole::Body));
        self.hits.borrow_mut().clear();
        let size = self.graph.get_client_size();

        let runtime = self.activity.borrow();
        let fresh = self.fresh.get().elapsed() < Duration::from_secs(12);
        let client = runtime["clients"]
            .as_array()
            .and_then(|a| a.iter().find(|v| v["client"] == self.id.key()));
        let opened = if fresh {
            client.and_then(|c| c["opened"].as_bool())
        } else {
            None
        };
        let configured = if fresh {
            client.and_then(|c| c["configured"].as_bool())
        } else {
            None
        };
        let empty = Vec::new();
        let requests = client
            .and_then(|c| c["requests"].as_array())
            .unwrap_or(&empty);
        let now =
            runtime["at"].as_i64().unwrap_or(0) + self.fresh.get().elapsed().as_millis() as i64;
        let flowing = fresh
            && theme::motion_enabled()
            && requests.iter().any(|r| {
                r["phase"] != "waiting"
                    && now.saturating_sub(r["lastEvidenceAt"].as_i64().unwrap_or(0)) < 5000
            });
        let phase = (self.fresh.get().elapsed().as_secs_f64() * 0.55) % 1.;
        let lib = self.library.borrow();
        let resources = lib["resources"].as_array();
        let count = |kind: &str| -> (usize, bool) {
            let Some(resources) = resources else {
                return (0, false);
            };
            let mut count = 0;
            let mut known = true;
            for r in resources.iter().filter(|r| r["resource"]["kind"] == kind) {
                let states = r["targets"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|v| v["client"] == self.id.key())
                    .collect::<Vec<_>>();
                if states.iter().any(|s| s["using"] == true) {
                    count += 1;
                } else if states.iter().any(|s| s["using"].is_null()) {
                    known = false;
                }
            }
            (count, known)
        };
        let resource_label = |kind: &str| {
            let (n, known) = count(kind);
            (
                if n > 0 {
                    format!("{n} 个使用中")
                } else if known {
                    "未使用".into()
                } else {
                    "待读取客户端".into()
                },
                if n > 0 {
                    Some(true)
                } else if known {
                    Some(false)
                } else {
                    None
                },
            )
        };
        let (skill, skill_state) = resource_label("skill");
        let (mcp, mcp_state) = resource_label("mcp");
        let execution = if !fresh {
            "执行状态未知"
        } else {
            match self.id {
                ClientId::Codex => {
                    let r = &runtime["execution"]["codex"];
                    if r["connected"] == true
                        && r["initialized"] == true
                        && r["healthy"] == true
                        && r["stale"] != true
                    {
                        "执行就绪"
                    } else {
                        "执行未就绪"
                    }
                }
                ClientId::Tiangong => {
                    match runtime["overview"]["gmclaw"]["bridge"]["state"].as_str() {
                        Some("connected") => "执行就绪",
                        Some("not_configured") => "未启用外部执行",
                        Some("auth_failed" | "error") => "执行需处理",
                        _ => "执行待连接",
                    }
                }
                ClientId::WorkBuddy => "支持模型接入",
                _ => "在客户端中使用",
            }
        };
        let model_label = match configured {
            Some(true) => {
                if requests.is_empty() {
                    "使用中".into()
                } else {
                    format!("{} 个请求中", requests.len())
                }
            }
            Some(false) => "未使用".into(),
            None => "状态未知".into(),
        };
        let channel_label = if fresh && !requests.is_empty() {
            requests
                .first()
                .and_then(|r| r["channel"].as_str())
                .unwrap_or("正在路由")
                .to_string()
        } else {
            "查看模型渠道".into()
        };
        let chat_count = runtime["chat"]["accounts"].as_array().map(|a| {
            a.iter()
                .filter(|r| r["connected"] == true && r["enabled"] == true)
                .count()
        });
        let chat_label = match self.id {
            ClientId::WorkBuddy => "外部执行待接通".into(),
            ClientId::Codex | ClientId::Tiangong => chat_count
                .map(|n| {
                    if n > 0 {
                        format!("{n} 个账号在线")
                    } else {
                        "未接入".into()
                    }
                })
                .unwrap_or("待读取".into()),
            _ => "由客户端管理".into(),
        };
        let labels: [(String, String, Option<bool>); 6] = [
            (
                match opened {
                    Some(true) => "已打开",
                    Some(false) => "未打开",
                    None => "运行状态未知",
                }
                .into(),
                execution.to_string(),
                opened,
            ),
            ("模型".into(), model_label, configured),
            ("渠道".into(), channel_label, None),
            ("Skill".into(), skill, skill_state),
            ("MCP".into(), mcp, mcp_state),
            ("聊天接入".into(), chat_label, None),
        ];
        let scale = visual::scale(&self.graph);
        let c = Canvas::new(&dc, scale);
        let view = ClientDiagram {
            id: self.id,
            opened: &labels[0].0,
            execution,
            model: &labels[1].1,
            channel: &labels[2].1,
            skill: &labels[3].1,
            mcp: &labels[4].1,
            chat: &labels[5].1,
            states: std::array::from_fn(|i| labels[i].2),
            hub: if fresh { Some(true) } else { None },
            flowing,
        };
        *self.hits.borrow_mut() = visual::draw_client(
            &c,
            Size::new(
                (size.width as f64 / scale) as i32,
                (size.height as f64 / scale) as i32,
            ),
            &view,
            phase,
        );
    }
    fn select(&self, index: usize) {
        if matches!(index, 2 | 3) {
            return;
        }
        let previous = self.book.selection();
        self.book.change_selection(index);
        if index == 1 {
            let draft = self.selected().is_some_and(|r| r["draft"] == true);
            self.model_view.show_draft(draft);
        } else if index == 4 {
            self.settings.set_targets(&self.models.borrow(), self.id);
            if let Some(share) = &self.workbuddy_share {
                share.read();
            }
        }
        for (i, tab) in self.tabs.iter().enumerate() {
            tab.set_selected(i == index);
        }
        self.launch.set_kind(if index == 0 {
            super::controls::ButtonKind::Primary
        } else {
            super::controls::ButtonKind::Secondary
        });
        super::motion::page_changed(self.book, previous);
    }
}

#[cfg(all(test, windows))]
pub(super) fn review_inline_draft(frame: &Frame, shell: &Shell, id: ClientId) {
    let api = ApiClient::new(
        "http://127.0.0.1:1".into(),
        super::text::GuiText::new(super::text::GuiLocale::ZhCn),
    );
    let ui = ClientPage::new(shell, id, *frame, api);
    let view = json!({"revision":"original-revision","targets":[{"id":"synthetic-target","client":id.key(),"label":"合成位置","modelMode":if id==ClientId::WebStorm {"acp"}else{"gateway"}}],"sources":[{"routeId":"source-a","name":"合成渠道","enabled":true,"models":["gpt-6","gpt-5.6"]}],"codexVisibleModels":["gpt-6"],"copies":if id == ClientId::Codex {json!([])} else {json!([{"id":"saved-entry","client":id.key(),"model":"gpt-6","label":"已有模型","sourceRoute":"source-a","targetId":"synthetic-target","using":true,"capabilities":{}}])}});
    *ui.models.borrow_mut() = view;
    if id == ClientId::Codex {
        let tab = super::codex_tab::create_with_settings(
            &ui.model_host,
            super::text::GuiText::new(super::text::GuiLocale::ZhCn),
            Some(&ui.settings_host),
            Some((ui.launch, ui.reload)),
        );
        ui.attach_model(&tab.page);
        super::codex_tab::bind_model_list(&tab, &ui);
        let focus = tab.clone();
        ui.on_model_focus(move |model| super::codex_tab::focus_model(&focus, model));
        let mut config = crate::ai_gateway::config::AiGatewayConfig::default();
        config.codex_visible_models = vec!["gpt-6".into()];
        config.providers = vec![crate::ai_gateway::config::ProviderConfig {
            name: "合成渠道".into(),
            models: vec!["gpt-6".into(), "gpt-5.6".into()],
            ..Default::default()
        }];
        super::codex_tab::initialize_visible_model_checks(&tab, &config);
        super::codex_tab::set_actions_enabled(&tab, true);
        let picker = super::codex_tab::review_picker(&tab);
        super::codex_tab::review_remote_model_updates(&tab, &mut config);
        ui.render();
        ui.select(1);
        ui.model_view.add.enable(true);
        shell.navigate(PageId::Client(id));
        frame.layout();
        shell.panel.layout();
        ui.page.layout();
        let candidate_size = picker.page.get_client_size();
        assert!(candidate_size.width >= ui.page.from_dip(240));
        assert!(candidate_size.height >= ui.page.from_dip(120));
        for _ in 0..4 {
            super::native_model_review::click(&ui.model_view.add);
            assert_eq!(ui.selected().unwrap()["draft"], true);
            assert!(ui.model_host.is_shown());
            assert!(!ui.model_view.workspace.fields.page.is_shown());
            picker.show_discovered(1, &["discovered-model".into()]);
            assert_eq!(picker.selected_models(), vec!["gpt-6"]);
            picker.select_models(&["discovered-model".into()]);
            ui.render();
            assert_eq!(ui.rows.borrow().len(), 3);
            let index = ui
                .rows
                .borrow()
                .iter()
                .position(|r| r["model"] == "discovered-model")
                .unwrap();
            ui.model_list.set_selection(index as u32, true);
            ui.fill_model();
            assert!(
                picker
                    .page
                    .find_window_by_name("codex-visible-gpt-5.6")
                    .is_some()
            );
            super::native_model_review::click(&ui.model_view.remove_saved);
            ui.render();
            assert_eq!(picker.selected_models(), vec!["gpt-6"]);
            ui.model_list
                .set_selection((ui.rows.borrow().len() - 1) as u32, true);
            ui.fill_model();
            ui.remove();
            assert!(ui.draft.borrow().is_none());
            assert!(ui.model_host.is_shown());
        }
        picker.remove_model("gpt-6");
        ui.render();
        assert!(ui.rows.borrow().is_empty());
        assert!(picker.selected_models().is_empty());
        assert!(ui.model_host.is_shown());
        super::native_model_review::capture(
            &ui.page,
            &format!(
                "codex-candidates-empty-{}",
                if theme::theme().is_dark {
                    "dark"
                } else {
                    "light"
                }
            ),
        );
        picker.select_models(&["discovered-model".into()]);
        ui.render();
        super::native_model_review::capture(
            &ui.page,
            &format!(
                "codex-candidates-ready-{}",
                if theme::theme().is_dark {
                    "dark"
                } else {
                    "light"
                }
            ),
        );
        return;
    }
    ui.render();
    ui.model_view.add.enable(true);
    shell.navigate(PageId::Client(id));
    if id == ClientId::Codex {
        let label = StaticText::builder(&ui.model_host)
            .with_label("已有模型 · 原渠道可见列表（合成）")
            .build();
        ui.model_host
            .get_sizer()
            .unwrap()
            .add(&label, 0, SizerFlag::All, 10);
    }
    let fields = ui.model_fields.as_ref().unwrap();
    for _ in 0..4 {
        super::native_model_review::click(&ui.model_view.add);
        super::native_model_review::click(&ui.model_view.add);
        assert_eq!(ui.rows.borrow().len(), 2);
        assert_eq!(ui.selected().unwrap()["draft"], true);
        fields.restore_draft(&json!({"source":0,"model":0,"label":"我的模型","context":"200000","output":"16000","efforts":"max","image":1,"protocol":2,"parameters":false}));
        super::native_model_review::click(
            &ui.model_list.find_window_by_name("client-model-0").unwrap(),
        );
        assert_ne!(ui.selected().unwrap()["draft"], true);
        if id == ClientId::Codex {
            assert!(ui.model_host.is_shown());
        }
        super::native_model_review::click(&ui.model_view.add);
        assert_eq!(fields.draft_snapshot()["label"], "我的模型");
        assert_eq!(fields.draft_snapshot()["model"], 0);
        assert_eq!(fields.draft_snapshot()["context"], "200000");
        ui.models.borrow_mut()["revision"] = "external-change".into();
        ui.render();
        let (row, snapshot) = ui.editing_snapshot.borrow().clone().unwrap();
        let body = fields.body(&row, &snapshot, id).unwrap();
        assert_eq!(body["revision"], "original-revision");
        assert_eq!(body["model"], "gpt-5.6");
        ui.reload_model();
        let (row, snapshot) = ui.editing_snapshot.borrow().clone().unwrap();
        assert_eq!(
            fields.body(&row, &snapshot, id).unwrap()["revision"],
            "external-change"
        );
        assert_eq!(fields.draft_snapshot()["label"], "");
        ui.remove();
        assert!(ui.draft.borrow().is_none());
        assert_eq!(ui.rows.borrow().len(), 1);
        ui.models.borrow_mut()["revision"] = "original-revision".into();
    }
    super::native_model_review::click(&ui.model_view.add);
    frame.layout();
    ui.model_view.show_draft(true);
    assert!(!ui.model_view.back.is_shown());
    assert!(
        ui.model_list
            .find_window_by_name("client-model-0")
            .is_some()
    );
    assert!(
        ui.model_list
            .find_window_by_name("client-model-1")
            .is_some()
    );
    let viewport = fields.page.get_client_size();
    assert!(fields.page.get_virtual_size().height <= viewport.height + fields.page.from_dip(2));
    // No timer or network action is started: this uses synthetic in-memory state.
}
fn source_label(source: &Value) -> String {
    let name = source["name"].as_str().unwrap_or("来源");
    if let Some(client) = ClientId::ALL.iter().find(|c| source["client"] == c.key()) {
        format!("{} · {name}", visual::client_short(*client))
    } else {
        name.into()
    }
}

pub(super) fn add_field<W: WxWidget, P: WxWidget>(
    parent: &P,
    grid: &FlexGridSizer,
    label: &str,
    field: &W,
) {
    let text = StaticText::builder(parent).with_label(label).build();
    text.set_foreground_color(theme::theme().ink_secondary);
    grid.add(&text, 0, SizerFlag::AlignCenterVertical, 0);
    grid.add(field, 1, SizerFlag::Expand, 0);
}
