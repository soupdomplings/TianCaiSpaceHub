//! Opt-in personal device networking. All operations use the trusted local API.
mod mobile;
use super::{
    api::ApiClient,
    controls::{Button, ButtonKind, Choice, TextCtrl},
    dialog_job::InlineJob,
    feedback,
    shell::{PageId, Shell, SlidingStrip},
    text::GuiText,
    theme,
    widgets::{apply_listctrl_theme, listctrl_report_style},
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use wxdragon::{prelude::*, timer::Timer};

type ReadResult = Arc<Mutex<Option<(u64, Result<Value, String>)>>>;

#[derive(Clone)]
pub(super) struct RemotePage {
    frame: Frame,
    page: Panel,
    scroll: ScrolledWindow,
    tabs: SlidingStrip<3>,
    sections: [Panel; 3],
    selected_tab: Rc<Cell<usize>>,
    review_request: Button,
    api: ApiClient,
    text: GuiText,
    enabled: CheckBox,
    display_content: [CheckBox; 4],
    device_name: TextCtrl,
    relay: TextCtrl,
    state_label: StaticText,
    network_label: StaticText,
    code: TextCtrl,
    join_code: TextCtrl,
    directories: Choice,
    remove_directory: Button,
    request_card: Panel,
    mobile: mobile::MobilePanel,
    pairing_hint: StaticText,
    devices: ListCtrl,
    requests: ListCtrl,
    create: Button,
    join: Button,
    open: Button,
    revoke: Button,
    approve: Button,
    reject: Button,
    save: Button,
    state: Rc<RefCell<Value>>,
    device_rows: Rc<RefCell<Vec<Value>>>,
    request_rows: Rc<RefCell<Vec<Value>>>,
    job: Rc<InlineJob>,
    pending: ReadResult,
    reading: Arc<AtomicBool>,
    read_generation: Arc<AtomicU64>,
    active: Rc<Cell<bool>>,
    loaded: Rc<Cell<bool>>,
    last_read: Rc<Cell<Instant>>,
    last_error: Rc<RefCell<String>>,
    timer: Rc<RefCell<Option<Timer<Panel>>>>,
}

impl RemotePage {
    pub fn new(frame: &Frame, shell: &Shell, api: ApiClient, text: GuiText) -> Self {
        let page = Panel::builder(&shell.book).build();
        page.set_background_color(theme::theme().bg_app);
        let root = BoxSizer::builder(Orientation::Vertical).build();
        let nav = Panel::builder(&page).build();
        nav.set_background_color(theme::theme().bg_app);
        let toolbar = BoxSizer::builder(Orientation::Horizontal).build();
        let tabs = SlidingStrip::new(nav, [("设备", 70), ("配对", 70), ("设置", 70)], 34, false);
        for key in tabs.keys() {
            toolbar.add(&key, 0, SizerFlag::AlignCenterVertical, 0);
        }
        toolbar.add_stretch_spacer(1);
        let review_request = button(&nav, "查看连接申请", ButtonKind::Secondary);
        toolbar.add(&review_request, 0, SizerFlag::AlignCenterVertical, 0);
        review_request.hide();
        nav.set_sizer(toolbar, false);
        root.add(
            &nav,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
            page.from_dip(20),
        );
        let scroll = ScrolledWindow::builder(&page)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        scroll.set_scroll_rate(0, scroll.from_dip(12));
        scroll.set_background_color(theme::theme().bg_app);
        let column = BoxSizer::builder(Orientation::Vertical).build();
        let views = std::array::from_fn(|_| {
            let panel = Panel::builder(&scroll).build();
            panel.set_background_color(theme::theme().bg_app);
            column.add(&panel, 0, SizerFlag::Expand, 0);
            panel
        });
        let [device_card, network, settings] = views;
        network.hide();
        settings.hide();
        let device_fields = BoxSizer::builder(Orientation::Vertical).build();
        let network_fields = BoxSizer::builder(Orientation::Vertical).build();
        let fields = BoxSizer::builder(Orientation::Vertical).build();
        let pairing_hint = label(&network, "", theme::TextRole::Body);
        network_fields.add(
            &pairing_hint,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            network.from_dip(10),
        );
        let request_card = Panel::builder(&network).build();
        request_card.set_background_color(theme::theme().bg_app);
        let request_fields = BoxSizer::builder(Orientation::Vertical).build();
        let requests = table(&request_card, &[("设备", 300), ("状态", 230)], 96);
        request_fields.add(
            &requests,
            0,
            SizerFlag::Expand | SizerFlag::All,
            request_card.from_dip(10),
        );
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let approve = button(&request_card, "允许连接", ButtonKind::Primary);
        let reject = button(&request_card, "拒绝", ButtonKind::Ghost);
        row.add(&approve, 0, SizerFlag::Right, request_card.from_dip(10));
        row.add(&reject, 0, SizerFlag::default(), 0);
        request_fields.add_sizer(&row, 0, SizerFlag::All, request_card.from_dip(10));
        request_card.set_sizer(request_fields, false);
        network_fields.add(
            &request_card,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            network.from_dip(8),
        );
        request_card.hide();
        let enabled = CheckBox::builder(&settings)
            .with_label("启用远程连接")
            .build();
        enabled.set_foreground_color(theme::theme().ink_primary);
        enabled.set_background_color(theme::theme().bg_app);
        enabled.enable(false);
        fields.add(&enabled, 0, SizerFlag::All, settings.from_dip(10));
        let state_label = label(&device_card, "正在读取设备状态…", theme::TextRole::Body);
        device_fields.add(
            &state_label,
            0,
            SizerFlag::Left | SizerFlag::Bottom,
            settings.from_dip(10),
        );
        let grid = FlexGridSizer::builder(0, 2)
            .with_hgap(14)
            .with_vgap(10)
            .build();
        grid.add_growable_col(1, 1);
        let device_name = TextCtrl::builder(&settings)
            .with_size(Size::new(-1, 34))
            .build();
        device_name.set_max_length(80);
        let relay = TextCtrl::builder(&settings)
            .with_size(Size::new(-1, 34))
            .build();
        relay.set_hint("留空使用默认公共节点");
        for (name, input) in [("设备名称", device_name), ("连接节点", relay)] {
            grid.add(
                &label(&settings, name, theme::TextRole::Body),
                0,
                SizerFlag::AlignCenterVertical,
                0,
            );
            grid.add(&input, 1, SizerFlag::Expand, 0);
        }
        fields.add_sizer(
            &grid,
            0,
            SizerFlag::Expand | SizerFlag::All,
            settings.from_dip(10),
        );
        let actions = BoxSizer::builder(Orientation::Horizontal).build();
        let save = button(&settings, "保存设置", ButtonKind::Secondary);
        actions.add(&save, 0, SizerFlag::Right, settings.from_dip(10));
        fields.add_sizer(&actions, 0, SizerFlag::All, settings.from_dip(10));
        fields.add(
            &label(&settings, "显示内容（本机发送）", theme::TextRole::Title),
            0,
            SizerFlag::Left | SizerFlag::Top,
            settings.from_dip(10),
        );
        let display_hint = label(
            &settings,
            "本机向网页和小程序发送会话内容时，统一使用这里的选择。访问另一台电脑时，由对方 Hub 控制。勾选后自动保存，只影响后续实时过程和新产物。",
            theme::TextRole::Caption,
        );
        display_hint.wrap(settings.from_dip(560));
        fields.add(&display_hint, 0, SizerFlag::All, settings.from_dip(10));
        let display_row = WrapSizer::builder(Orientation::Horizontal).build();
        let display_content = ["思考过程", "工具执行", "文件变更", "产物"].map(|name| {
            let checkbox = CheckBox::builder(&settings).with_label(name).build();
            checkbox.set_foreground_color(theme::theme().ink_primary);
            checkbox.set_background_color(theme::theme().bg_app);
            checkbox.enable(false);
            display_row.add(&checkbox, 0, SizerFlag::Right | SizerFlag::Bottom, settings.from_dip(16));
            checkbox
        });
        fields.add_sizer(&display_row, 0, SizerFlag::Left | SizerFlag::Right, settings.from_dip(10));
        let network_label = label(
            &network,
            "在一台电脑获取连接码，在另一台粘贴。只需一个方向。",
            theme::TextRole::Body,
        );
        network_fields.add(&network_label, 0, SizerFlag::All, network.from_dip(10));
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let create = button(&network, "获取本机连接码", ButtonKind::Primary);
        let copy = button(&network, "复制连接码", ButtonKind::Ghost);
        row.add(&create, 0, SizerFlag::Right, network.from_dip(10));
        row.add(&copy, 0, SizerFlag::default(), 0);
        network_fields.add_sizer(&row, 0, SizerFlag::All, network.from_dip(10));
        let code = TextCtrl::builder(&network)
            .with_style(TextCtrlStyle::ReadOnly)
            .with_size(Size::new(-1, network.from_dip(34)))
            .build();
        code.set_hint("获取后发送给你自己的另一台设备，连接码限时有效");
        network_fields.add(
            &code,
            0,
            SizerFlag::Expand | SizerFlag::All,
            network.from_dip(10),
        );
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let join_code = TextCtrl::builder(&network)
            .with_size(Size::new(-1, 36))
            .build();
        join_code.set_hint("粘贴另一台 Hub 的连接码");
        let join = button(&network, "加入并请求连接", ButtonKind::Secondary);
        row.add(
            &join_code,
            1,
            SizerFlag::Expand | SizerFlag::Right,
            network.from_dip(10),
        );
        row.add(&join, 0, SizerFlag::default(), 0);
        network_fields.add_sizer(
            &row,
            0,
            SizerFlag::Expand | SizerFlag::All,
            network.from_dip(10),
        );
        let devices = table(
            &device_card,
            &[("设备", 250), ("连接", 150), ("授权", 130)],
            155,
        );
        device_fields.add(
            &devices,
            0,
            SizerFlag::Expand | SizerFlag::All,
            device_card.from_dip(10),
        );
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let open = button(&device_card, "打开会话网页", ButtonKind::Primary);
        let revoke = button(&device_card, "删除已授权设备…", ButtonKind::Ghost);
        let refresh = button(&device_card, "刷新", ButtonKind::Ghost);
        for item in [open, revoke, refresh] {
            row.add(&item, 0, SizerFlag::Right, device_card.from_dip(10));
        }
        device_fields.add_sizer(&row, 0, SizerFlag::All, device_card.from_dip(10));
        device_card.set_sizer(device_fields, false);
        let mobile = mobile::MobilePanel::new(&network, &network_fields);
        network.set_sizer(network_fields, false);
        fields.add(
            &label(&settings, "Codex 工作目录", theme::TextRole::Body),
            0,
            SizerFlag::Left | SizerFlag::Top,
            settings.from_dip(10),
        );
        let directories = Choice::builder(&settings).build();
        directories.set_name("已添加的本机目录");
        directories.set_tooltip("这里添加的目录可在已配对设备的新建会话窗口中选择。");
        fields.add(
            &directories,
            0,
            SizerFlag::Expand | SizerFlag::All,
            scroll.from_dip(10),
        );
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let add_directory = button(&settings, "添加工作目录", ButtonKind::Secondary);
        let remove_directory = button(&settings, "从列表移除", ButtonKind::Ghost);
        for b in [add_directory, remove_directory] {
            buttons.add(&b, 0, SizerFlag::Right, scroll.from_dip(8));
        }
        fields.add_sizer(&buttons, 0, SizerFlag::All, scroll.from_dip(10));
        settings.set_sizer(fields, false);
        let status = label(&scroll, "", theme::TextRole::Caption);
        status.hide();
        let inside = BoxSizer::builder(Orientation::Vertical).build();
        inside.add_sizer(
            &column,
            0,
            SizerFlag::Expand | SizerFlag::All,
            scroll.from_dip(20),
        );
        scroll.set_sizer(inside, false);
        root.add(&scroll, 1, SizerFlag::Expand, 0);
        page.set_sizer(root, false);
        shell.register_viewport(PageId::Remote, &page);

        let ui = Self {
            frame: *frame,
            page,
            scroll,
            tabs,
            sections: views,
            selected_tab: Default::default(),
            review_request,
            api,
            text,
            enabled,
            display_content,
            device_name,
            relay,
            state_label,
            network_label,
            code,
            join_code,
            request_card,
            mobile,
            pairing_hint,
            directories,
            remove_directory,
            devices,
            requests,
            create,
            join,
            open,
            revoke,
            approve,
            reject,
            save,
            state: Default::default(),
            device_rows: Default::default(),
            request_rows: Default::default(),
            job: Rc::new(InlineJob::new(page, status)),
            pending: Default::default(),
            reading: Default::default(),
            read_generation: Default::default(),
            active: Default::default(),
            loaded: Default::default(),
            last_read: Rc::new(Cell::new(Instant::now() - Duration::from_secs(30))),
            last_error: Default::default(),
            timer: Default::default(),
        };
        let u = ui.clone();
        add_directory.on_click(move |_| {
            let dialog = DirDialog::builder(&u.frame, "选择在远程会话中使用的本机目录", "")
                .with_style(DirDialogStyle::MustExist.bits())
                .build();
            let selected = (dialog.show_modal() == ID_OK)
                .then(|| dialog.get_path())
                .flatten();
            dialog.destroy();
            if let Some(path) = selected {
                u.post("workspace", json!({"path":path}), "添加工作目录", |_| {});
            }
        });
        let u = ui.clone();
        remove_directory.on_click(move |_| {
            let path = u.directories.get_selection().and_then(|i| {
                u.state.borrow()["workspaceDirs"]
                    .get(i as usize)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
            if let Some(path) = path {
                u.post(
                    "workspace",
                    json!({"path":path,"remove":true}),
                    "从列表移除工作目录",
                    |_| {},
                );
            }
        });
        ui.bind(copy, refresh);
        ui.controls();
        ui
    }

    fn bind(&self, copy: Button, refresh: Button) {
        self.mobile.bind(self);
        for checkbox in self.display_content {
            let ui = self.clone();
            checkbox.on_toggled(move |_| {
                let [reasoning, tool, file_change, artifact] = ui.display_content.map(|c| c.is_checked());
                let updated = ui.clone();
                ui.post(
                    "display-content",
                    json!({"reasoning":reasoning,"tool":tool,"fileChange":file_change,"artifact":artifact}),
                    "保存显示内容",
                    move |value| {
                        updated.state.borrow_mut()["displayContent"] = value["displayContent"].clone();
                        updated.render();
                    },
                );
            });
        }
        for (index, key) in self.tabs.keys().into_iter().enumerate() {
            let ui = self.clone();
            key.on_click(move |_| ui.select_tab(index));
        }
        let ui = self.clone();
        self.review_request.on_click(move |_| ui.select_tab(1));
        let ui = self.clone();
        self.enabled.on_toggled(move |_| {
            let enabled = ui.enabled.is_checked();
            ui.post(
                "settings",
                json!({"enabled":enabled}),
                "更新远程连接",
                |_| {},
            );
        });
        let ui = self.clone();
        self.save.on_click(move |_| {
            ui.post(
                "settings",
                json!({"enabled":ui.state.borrow()["enabled"] == true,"deviceName":ui.device_name.get_value(),"relayUrl":ui.relay.get_value()}),
                "保存连接设置",
                |_| {},
            );
        });
        let ui = self.clone();
        self.create.on_click(move |_| {
            let output = ui.code;
            ui.post("create", json!({}), "获取连接码", move |result| {
                if let Some(code) = result["code"].as_str() {
                    output.change_value(code);
                }
            });
        });
        let ui = self.clone();
        copy.on_click(move |_| {
            let code = ui.code.get_value();
            if code.is_empty() {
                feedback::error(&ui.page, "请先获取连接码");
                return;
            }
            if wxdragon::clipboard::Clipboard::get().set_text(&code) {
                feedback::success(&ui.page, "连接码已复制，请发给你自己的设备");
            } else {
                feedback::error(&ui.page, "无法复制，请直接选中连接码复制");
            }
        });
        let ui = self.clone();
        self.join.on_click(move |_| {
            let code = ui.join_code.get_value();
            if code.trim().is_empty() {
                feedback::error(&ui.page, "请粘贴另一台 Hub 的连接码");
                return;
            }
            let input = ui.join_code;
            ui.post(
                "join",
                json!({"code":code.trim()}),
                "请求连接",
                move |_| input.clear(),
            );
        });
        let ui = self.clone();
        self.open.on_click(move |_| {
            let device = ui.selected_device();
            let frame = ui.frame;
            let text = ui.text;
            ui.post(
                "open",
                json!({"deviceId":device.as_ref().and_then(|d|d["id"].as_str())}),
                "打开会话网页",
                move |result| {
                    if let Some(url) = result["url"].as_str() {
                        if let Err(error) = super::browser::open_url_in_browser(text, url) {
                            feedback::error(&frame, &error);
                        }
                    } else {
                        feedback::error(&frame, "后台未返回会话网页地址");
                    }
                },
            );
        });
        let ui = self.clone();
        self.revoke.on_click(move |_| {
            // Snapshot both labels and stable IDs. Polling continues in a modal dialog.
            let devices = authorized_devices(&ui.state.borrow());
            if devices.is_empty() {
                feedback::error(&ui.page, "没有可以删除的已授权设备");
                return;
            }
            let labels: Vec<_> = devices
                .iter()
                .map(|(id, name, online)| {
                    let link = if *online { "在线" } else { "离线" };
                    if devices.iter().filter(|(_, other, _)| other == name).count() > 1 {
                        format!("{name} · {link} · {id}")
                    } else {
                        format!("{name} · {link}")
                    }
                })
                .collect();
            let dialog = SingleChoiceDialog::builder(
                &ui.frame,
                "请选择要删除的设备。确认后将断开它的远程访问，再次连接需要重新授权。",
                "删除已授权设备",
                &labels.iter().map(String::as_str).collect::<Vec<_>>(),
            )
            .build();
            let selected_id = ui
                .selected_device()
                .and_then(|v| v["id"].as_str().map(String::from));
            if let Some(index) = devices
                .iter()
                .position(|(id, _, _)| Some(id) == selected_id.as_ref())
            {
                dialog.set_selection(index as i32);
            }
            let index = (dialog.show_modal() == ID_OK).then(|| dialog.get_selection() as usize);
            dialog.destroy();
            if let Some((id, _, _)) = index.and_then(|i| devices.get(i)) {
                let after = ui.clone();
                ui.post(
                    "revoke",
                    json!({"deviceId":id}),
                    "删除设备授权",
                    move |new_status| {
                        if new_status["devices"].is_array() {
                            *after.state.borrow_mut() = new_status;
                            after.render();
                        }
                    },
                );
            }
        });
        for (button, action, title) in [
            (self.approve, "pair/approve", "允许设备连接"),
            (self.reject, "pair/reject", "拒绝设备连接"),
        ] {
            let ui = self.clone();
            button.on_click(move |_| {
                if let Some(index) = selected(ui.requests) {
                    if let Some(request) = ui.request_rows.borrow().get(index) {
                        ui.post(
                            action,
                            json!({"requestId":request["requestId"]}),
                            title,
                            |_| {},
                        );
                    }
                } else {
                    feedback::error(&ui.page, "请先选择待确认设备");
                }
            });
        }
        let ui = self.clone();
        refresh.on_click(move |_| ui.fetch());
        for list in [self.devices, self.requests] {
            let ui = self.clone();
            list.on_item_selected(move |_| ui.controls());
            let ui = self.clone();
            list.on_item_deselected(move |_| ui.controls());
        }
        let ui = self.clone();
        let timer = Timer::new(&self.page);
        timer.on_tick(move |_| ui.poll());
        timer.start(200, false);
        self.timer.borrow_mut().replace(timer);
    }

    fn select_tab(&self, tab: usize) {
        if self.selected_tab.replace(tab) == tab {
            return;
        }
        self.tabs.select(tab);
        for (index, panel) in self.sections.iter().enumerate() {
            panel.show(index == tab);
        }
        self.review_request
            .show(tab != 1 && !self.request_rows.borrow().is_empty());
        self.page.layout();
        self.scroll.layout();
        self.scroll.fit_inside();
        self.scroll.scroll_coords(-1, 0);
    }

    pub fn enter(&self, page: PageId) {
        self.active.set(page == PageId::Remote);
        if self.active.get() {
            self.fetch();
        }
    }

    fn post(&self, action: &str, body: Value, title: &str, complete: impl FnOnce(Value) + 'static) {
        self.read_generation.fetch_add(1, Ordering::SeqCst);
        let ui = self.clone();
        self.job.start(
            self.api.clone(),
            title,
            &format!("/api/remote-network/{action}"),
            body,
            move |result| {
                if let Ok(value) = result {
                    complete(value);
                }
                ui.fetch();
                ui.controls();
            },
        );
        self.controls();
    }

    fn fetch(&self) {
        if self.reading.swap(true, Ordering::SeqCst) {
            return;
        }
        self.last_read.set(Instant::now());
        let generation = self.read_generation.load(Ordering::SeqCst);
        let (api, pending, reading) =
            (self.api.clone(), self.pending.clone(), self.reading.clone());
        std::thread::spawn(move || {
            let result = api
                .request_text(
                    api.http
                        .get(format!("{}/api/remote-network/status", api.base_url))
                        .header("x-tiancai-desktop", "1")
                        .timeout(Duration::from_secs(10)),
                )
                .map_err(|error| friendly_error(&error))
                .and_then(|text| {
                    serde_json::from_str(&text).map_err(|_| "远程连接状态无效".into())
                });
            if let Ok(mut slot) = pending.lock() {
                *slot = Some((generation, result));
            }
            reading.store(false, Ordering::SeqCst);
            wxdragon::wake_up_idle();
        });
    }

    fn poll(&self) {
        let result = self.pending.lock().ok().and_then(|mut slot| slot.take());
        if let Some((generation, result)) = result
            && generation == self.read_generation.load(Ordering::SeqCst)
        {
            match result {
                Ok(value) => {
                    if !self.loaded.replace(true) {
                        self.device_name
                            .change_value(value["deviceName"].as_str().unwrap_or(""));
                        self.relay
                            .change_value(value["relayUrl"].as_str().unwrap_or(""));
                    }
                    if self.state.borrow()["workspaceDirs"] != value["workspaceDirs"] {
                        let old_path = self.directories.get_selection().and_then(|i| {
                            self.state.borrow()["workspaceDirs"]
                                .get(i as usize)
                                .cloned()
                        });
                        self.directories.clear();
                        if let Some(dirs) = value["workspaceDirs"].as_array() {
                            for path in dirs.iter().filter_map(Value::as_str) {
                                self.directories.append(path);
                            }
                            if !dirs.is_empty() {
                                self.directories.set_selection(
                                    dirs.iter()
                                        .position(|v| Some(v) == old_path.as_ref())
                                        .unwrap_or(0) as u32,
                                );
                            }
                        }
                    }
                    *self.state.borrow_mut() = value;
                    self.render();
                }
                Err(error) if self.active.get() => self.report_error(&error),
                Err(_) => {}
            }
        }
        if self.active.get()
            && !self.job.is_running()
            && self.last_read.get().elapsed() >= Duration::from_secs(3)
        {
            self.fetch();
        }
    }

    fn report_error(&self, error: &str) {
        if !error.is_empty() && self.last_error.borrow().as_str() != error {
            *self.last_error.borrow_mut() = error.to_owned();
            feedback::error(&self.page, error);
        }
    }

    fn render(&self) {
        let state = self.state.borrow();
        let enabled = state["enabled"] == true;
        self.enabled.set_value(enabled);
        for (checkbox, key) in self.display_content.into_iter().zip(["reasoning", "tool", "fileChange", "artifact"]) {
            checkbox.set_value(state["displayContent"][key] == true);
        }
        self.state_label
            .set_label(if state["runtimeReady"] != true {
                "内置组网暂不可用 · 请查看错误提示"
            } else if !enabled {
                "远程连接未启用。请在「设置」中开启。"
            } else if !state["networkName"].as_str().is_some_and(|s| !s.is_empty()) {
                "打开「配对」，连接自己的另一台电脑。"
            } else if state["connected"] == true {
                "网络已连接 · 可查看设备状态并打开会话"
            } else if state["lastError"].as_str().is_some_and(|s| !s.is_empty()) {
                "连接失败 · 请检查错误提示或更换节点"
            } else if state["networkReady"] == true {
                "正在连接网络 · 等待组网节点和自己的设备"
            } else {
                "网络尚未启动 · 请检查连接设置或重新启用"
            });
        self.network_label.set_label(
            &state["networkName"]
                .as_str()
                .filter(|name| !name.is_empty())
                .map(|name| format!("当前网络：{name}"))
                .unwrap_or_else(|| "还没有个人网络".into()),
        );
        self.pairing_hint.set_label(&pairing_guidance(&state));
        self.pairing_hint.wrap(
            (self.scroll.get_client_size().width - self.scroll.from_dip(52))
                .max(self.scroll.from_dip(260)),
        );
        let rows = state["devices"].as_array().cloned().unwrap_or_default();
        if *self.device_rows.borrow() != rows {
            let selected_id = self.selected_device().map(|d| d["id"].clone());
            self.devices.delete_all_items();
            for (index, row) in rows.iter().enumerate() {
                let index = index as i64;
                self.devices
                    .insert_item(index, row["name"].as_str().unwrap_or("未命名设备"), None);
                self.devices
                    .set_item_text_by_column(index, 1, connection_text(row));
                self.devices
                    .set_item_text_by_column(index, 2, authorization_text(row));
                if selected_id.as_ref() == Some(&row["id"]) {
                    self.devices.set_item_state(
                        index,
                        ListItemState::Selected,
                        ListItemState::Selected,
                    );
                }
            }
            *self.device_rows.borrow_mut() = rows;
        }
        let mut rows = state["pendingPairings"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        rows.sort_by(|a, b| a["requestId"].as_str().cmp(&b["requestId"].as_str()));
        if *self.request_rows.borrow() != rows {
            let previous_id = selected(self.requests).and_then(|i| {
                self.request_rows
                    .borrow()
                    .get(i)
                    .map(|r| r["requestId"].clone())
            });
            let was_empty = self.request_rows.borrow().is_empty();
            let selection = rows
                .iter()
                .position(|r| Some(&r["requestId"]) == previous_id.as_ref())
                .or_else(|| (rows.len() == 1).then_some(0));
            self.requests.delete_all_items();
            for (index, row) in rows.iter().enumerate() {
                self.requests.insert_item(
                    index as i64,
                    row["deviceName"].as_str().unwrap_or("新设备"),
                    None,
                );
                self.requests.set_item_text_by_column(
                    index as i64,
                    1,
                    "请求访问本机 Codex / 天工会话",
                );
            }
            self.request_card.show(!rows.is_empty());
            self.review_request
                .set_label(&format!("查看连接申请（{}）", rows.len()));
            self.review_request
                .show(!rows.is_empty() && self.selected_tab.get() != 1);
            self.page.layout();
            if let Some(i) = selection {
                self.requests.set_item_state(
                    i as i64,
                    ListItemState::Selected,
                    ListItemState::Selected,
                );
            }
            if was_empty && !rows.is_empty() && self.active.get() && self.selected_tab.get() == 1 {
                self.scroll.scroll_coords(-1, 0);
            }
            *self.request_rows.borrow_mut() = rows;
        }
        if let Some(error) = state["lastError"].as_str() {
            if self.active.get() {
                self.report_error(error);
            }
        } else {
            self.last_error.borrow_mut().clear();
        }
        drop(state);
        self.controls();
        self.scroll.layout();
        self.scroll.fit_inside();
    }

    fn selected_device(&self) -> Option<Value> {
        selected(self.devices).and_then(|index| self.device_rows.borrow().get(index).cloned())
    }

    fn controls(&self) {
        let state = self.state.borrow();
        let available = self.loaded.get() && !self.job.is_running();
        self.mobile.update(&state, available);
        let enabled = state["enabled"] == true;
        let ready = state["runtimeReady"] == true;
        self.enabled.enable(available);
        for checkbox in self.display_content {
            checkbox.enable(available);
        }
        self.save.enable(available);
        self.create.enable(available && enabled && ready);
        self.join.enable(available && enabled && ready);
        self.remove_directory
            .enable(available && self.directories.get_selection().is_some());
        self.open.enable(
            available
                && enabled
                && ready
                && self
                    .selected_device()
                    .is_none_or(|d| d["paired"] == true || d["id"] == state["deviceId"]),
        );
        self.revoke
            .enable(available && !authorized_devices(&state).is_empty());
        self.approve
            .enable(available && selected(self.requests).is_some());
        self.reject
            .enable(available && selected(self.requests).is_some());
    }
}

pub(super) fn friendly_error(error: &str) -> String {
    error
        .find('{')
        .and_then(|start| serde_json::from_str::<Value>(&error[start..]).ok())
        .and_then(|body| {
            body.pointer("/error/message")
                .and_then(Value::as_str)
                .filter(|message| !message.trim().is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| error.to_owned())
}

fn authorized_devices(state: &Value) -> Vec<(String, String, bool)> {
    state["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let id = row["id"].as_str()?;
            (row["paired"] == true
                && Some(id) != state["deviceId"].as_str()
                && row["connectionType"] != "local")
                .then(|| {
                    (
                        id.to_owned(),
                        row["name"].as_str().unwrap_or("未命名设备").to_owned(),
                        row["online"] == true,
                    )
                })
        })
        .collect()
}

#[cfg(test)]
#[test]
fn delete_choices_include_offline_pair_not_local_or_discovered_peer() {
    let state = json!({"deviceId":"here","devices":[
        {"id":"here","name":"本机","paired":true},{"id":"offline","name":"工作电脑","paired":true,"online":false},
        {"id":"online","name":"工作电脑","paired":true,"online":true},{"id":"discovered","paired":false}
    ]});
    assert_eq!(
        authorized_devices(&state),
        vec![
            ("offline".into(), "工作电脑".into(), false),
            ("online".into(), "工作电脑".into(), true)
        ]
    );
}

fn pairing_guidance(state: &Value) -> String {
    if state["pendingPairings"]
        .as_array()
        .is_some_and(|requests| !requests.is_empty())
    {
        return "有设备申请连接。核对名称后在本机允许，两台电脑即可互相访问。".into();
    }
    let outgoing = &state["outgoingPairing"];
    if let Some(name) = outgoing["deviceName"].as_str() {
        return if outgoing["delivered"] == true {
            format!("请求已送达「{name}」。请在那台电脑打开「远程连接 → 配对」，点「允许连接」。")
        } else {
            format!("正在联系「{name}」，请求尚未送达。请保持两端 Hub 在线。")
        };
    }
    if let Some(error) = state["pairingError"].as_str().filter(|s| !s.is_empty()) {
        return error.into();
    }
    if state["enabled"] != true {
        return "先在「设置」启用远程连接。".into();
    }
    "只需一台电脑提供连接码，另一台输入；提供码的电脑在此允许。".into()
}

#[cfg(test)]
#[test]
fn pairing_rows_and_instructions_follow_actual_delivery() {
    let mut status =
        json!({"enabled":true,"outgoingPairing":{"deviceName":"办公室","delivered":false}});
    assert!(pairing_guidance(&status).contains("请求尚未送达"));
    status["outgoingPairing"]["delivered"] = json!(true);
    assert!(pairing_guidance(&status).contains("在那台电脑"));
    status["pendingPairings"] = json!([{"deviceName":"办公室","requestId":"incoming"}]);
    assert!(pairing_guidance(&status).contains("在本机允许"));
    assert_eq!(
        authorization_text(&json!({"online":true,"paired":false,"connectionType":"direct"})),
        "已发现，未配对"
    );
}

fn authorization_text(device: &Value) -> &'static str {
    if device["paired"] == true {
        return if device["connectionType"] == "local" {
            "—"
        } else {
            "已授权"
        };
    }
    match device["connectionType"].as_str() {
        Some("incoming") => "请在本机允许",
        Some("pairing") => "等对方允许",
        Some("sending") => "请求未送达",
        _ => "已发现，未配对",
    }
}

fn connection_text(device: &Value) -> &'static str {
    if device["online"] != true {
        "离线"
    } else {
        match device["connectionType"].as_str() {
            Some("local") => "本机",
            Some("direct") => "直连",
            Some("relay") => "中继",

            _ => "在线",
        }
    }
}
fn selected(list: ListCtrl) -> Option<usize> {
    let row = list.get_next_item(-1, ListNextItemFlag::All, ListItemState::Selected);
    (row >= 0).then_some(row as usize)
}

#[cfg(all(test, windows))]
#[test]
fn pairing_tabs_preserve_draft_and_offer_review_without_auto_approval() {
    wxdragon::app::render_offscreen(|| {
        theme::init(theme::ThemeMode::Light);
        let frame = Frame::builder().build();
        let host = Panel::builder(&frame).build();
        let shell = Shell::new(&host, &frame);
        let text = GuiText::new(super::text::GuiLocale::ZhCn);
        let ui = RemotePage::new(&frame, &shell, ApiClient::new(String::new(), text), text);
        ui.loaded.set(true);
        let arrivals = |ids: &[&str]| {
            *ui.state.borrow_mut() = json!({"runtimeReady":true,"enabled":true,"pendingPairings":ids.iter().map(|id| json!({"deviceName":id,"requestId":id})).collect::<Vec<_>>()});
            ui.render();
        };
        arrivals(&[]);
        assert!(!ui.request_card.is_shown());
        ui.join_code.change_value("草稿");
        arrivals(&["device-b"]);
        assert_eq!(ui.selected_tab.get(), 0);
        assert!(ui.review_request.is_shown());
        ui.select_tab(1);
        assert!(!ui.review_request.is_shown());
        assert_eq!(ui.join_code.get_value(), "草稿");
        assert!(ui.request_card.is_shown() && ui.approve.is_enabled());
        assert_eq!(selected(ui.requests), Some(0));
        arrivals(&["device-b", "device-a"]);
        assert_eq!(selected(ui.requests), Some(1));
        assert_eq!(ui.request_rows.borrow().len(), 2);
        assert!(!ui.job.is_running());
        arrivals(&[]);
        arrivals(&["device-b", "device-c"]);
        assert!(selected(ui.requests).is_none() && !ui.approve.is_enabled());
        frame.destroy();
    }).unwrap();
}
fn label(parent: &impl WxWidget, text: &str, role: theme::TextRole) -> StaticText {
    let label = StaticText::builder(parent).with_label(text).build();
    label.set_foreground_color(if matches!(role, theme::TextRole::Caption) {
        theme::theme().ink_secondary
    } else {
        theme::theme().ink_primary
    });
    label.set_font(&theme::font(role));
    label
}
fn button(parent: &impl WxWidget, text: &str, kind: ButtonKind) -> Button {
    Button::builder(parent)
        .with_label(text)
        .with_kind(kind)
        .build()
}
fn table(parent: &impl WxWidget, columns: &[(&str, i32)], height: i32) -> ListCtrl {
    let list = ListCtrl::builder(parent)
        .with_style(listctrl_report_style())
        .with_size(Size::new(-1, parent.from_dip(height)))
        .build();
    for (index, (name, width)) in columns.iter().enumerate() {
        list.insert_column(
            index as i64,
            name,
            ListColumnFormat::Left,
            parent.from_dip(*width),
        );
    }
    apply_listctrl_theme(&list);
    list
}
