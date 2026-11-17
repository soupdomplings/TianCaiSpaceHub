//! Opt-in personal device networking. All operations use the trusted local API.
use super::{
    api::ApiClient,
    controls::{Button, ButtonKind, TextCtrl},
    dialog_job::InlineJob,
    feedback,
    shell::{PageId, Shell},
    text::GuiText,
    theme,
    widgets::{apply_listctrl_theme, card_section, listctrl_report_style},
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
    api: ApiClient,
    text: GuiText,
    enabled: CheckBox,
    device_name: TextCtrl,
    relay: TextCtrl,
    state_label: StaticText,
    network_label: StaticText,
    code: TextCtrl,
    join_code: TextCtrl,
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
        let scroll = ScrolledWindow::builder(&page)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        scroll.set_scroll_rate(0, scroll.from_dip(12));
        scroll.set_background_color(theme::theme().bg_app);
        let column = BoxSizer::builder(Orientation::Vertical).build();
        let title = label(&scroll, "在自己的设备之间继续对话", theme::TextRole::Title);
        column.add(&title, 0, SizerFlag::Bottom, scroll.from_dip(8));
        let hint = label(
            &scroll,
            "两端打开 Hub，目标端保持 Codex 或天工运行。连接后，在浏览器继续原来的会话。",
            theme::TextRole::Caption,
        );
        column.add(&hint, 0, SizerFlag::Bottom, scroll.from_dip(20));

        let (settings, fields) = card_section(&scroll, "本机与网络");
        let enabled = CheckBox::builder(&settings)
            .with_label("启用远程连接")
            .build();
        enabled.set_foreground_color(theme::theme().ink_primary);
        enabled.set_background_color(theme::theme().bg_card);
        enabled.enable(false);
        fields.add(&enabled, 0, SizerFlag::All, settings.from_dip(10));
        let state_label = label(&settings, "打开此页后读取状态", theme::TextRole::Body);
        fields.add(
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
        column.add(
            &settings,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            scroll.from_dip(16),
        );

        let (network, network_fields) = card_section(&scroll, "连接另一台电脑");
        let network_label = label(
            &network,
            "创建网络，或粘贴另一台 Hub 的连接码加入。",
            theme::TextRole::Body,
        );
        network_fields.add(&network_label, 0, SizerFlag::All, network.from_dip(10));
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let create = button(&network, "创建网络 / 获取连接码", ButtonKind::Primary);
        let copy = button(&network, "复制连接码", ButtonKind::Ghost);
        row.add(&create, 0, SizerFlag::Right, network.from_dip(10));
        row.add(&copy, 0, SizerFlag::default(), 0);
        network_fields.add_sizer(&row, 0, SizerFlag::All, network.from_dip(10));
        let code = TextCtrl::builder(&network)
            .with_style(TextCtrlStyle::ReadOnly | TextCtrlStyle::MultiLine)
            .with_size(Size::new(-1, network.from_dip(72)))
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
        column.add(
            &network,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            scroll.from_dip(16),
        );

        let (device_card, device_fields) = card_section(&scroll, "我的设备");
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
        let revoke = button(&device_card, "撤销所选设备", ButtonKind::Ghost);
        let refresh = button(&device_card, "刷新", ButtonKind::Ghost);
        for item in [open, revoke, refresh] {
            row.add(&item, 0, SizerFlag::Right, device_card.from_dip(10));
        }
        device_fields.add_sizer(&row, 0, SizerFlag::All, device_card.from_dip(10));
        column.add(
            &device_card,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            scroll.from_dip(16),
        );

        let (request_card, request_fields) = card_section(&scroll, "待确认的设备");
        let requests = table(&request_card, &[("设备", 300), ("状态", 230)], 96);
        request_fields.add(
            &requests,
            0,
            SizerFlag::Expand | SizerFlag::All,
            request_card.from_dip(10),
        );
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let approve = button(&request_card, "允许连接", ButtonKind::Secondary);
        let reject = button(&request_card, "拒绝", ButtonKind::Ghost);
        row.add(&approve, 0, SizerFlag::Right, request_card.from_dip(10));
        row.add(&reject, 0, SizerFlag::default(), 0);
        request_fields.add_sizer(&row, 0, SizerFlag::All, request_card.from_dip(10));
        column.add(&request_card, 0, SizerFlag::Expand, 0);
        let status = label(&scroll, "", theme::TextRole::Caption);
        status.hide();
        scroll.set_sizer(column, false);
        root.add(
            &scroll,
            1,
            SizerFlag::Expand | SizerFlag::All,
            page.from_dip(24),
        );
        page.set_sizer(root, false);
        shell.register(PageId::Remote, &page);

        let ui = Self {
            frame: *frame,
            page,
            scroll,
            api,
            text,
            enabled,
            device_name,
            relay,
            state_label,
            network_label,
            code,
            join_code,
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
        ui.bind(copy, refresh);
        ui.controls();
        ui
    }

    fn bind(&self, copy: Button, refresh: Button) {
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
            if let Some(device) = ui.selected_device() {
                ui.post(
                    "revoke",
                    json!({"deviceId":device["id"]}),
                    "撤销设备授权",
                    |_| {},
                );
            } else {
                feedback::error(&ui.page, "请先选择设备");
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
        self.state_label
            .set_label(if state["runtimeReady"] != true {
                "内置组网暂不可用 · 请查看错误提示"
            } else if !enabled {
                "未启用 · 内置组网已就绪"
            } else if !state["networkName"].as_str().is_some_and(|s| !s.is_empty()) {
                "内置组网已就绪 · 请创建或加入网络"
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
                .unwrap_or_else(|| "创建网络，或粘贴另一台 Hub 的连接码加入。".into()),
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
                self.devices.set_item_text_by_column(
                    index,
                    2,
                    if row["paired"] == true {
                        "已授权"
                    } else {
                        "未授权"
                    },
                );
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
        let rows = state["pendingPairings"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if *self.request_rows.borrow() != rows {
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
        let enabled = state["enabled"] == true;
        let ready = state["runtimeReady"] == true;
        self.enabled.enable(available);
        self.save.enable(available);
        self.create.enable(available && enabled && ready);
        self.join.enable(available && enabled && ready);
        self.open.enable(available && enabled && ready);
        self.revoke.enable(
            available
                && self.selected_device().is_some_and(|device| {
                    device["paired"] == true && device["id"] != state["deviceId"]
                }),
        );
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

fn connection_text(device: &Value) -> &'static str {
    if device["online"] != true {
        "离线"
    } else {
        match device["connectionType"].as_str() {
            Some("local") => "本机",
            Some("direct") => "直连",
            Some("relay") => "中继",
            Some("pairing") => "等待授权",
            _ => "在线",
        }
    }
}
fn selected(list: ListCtrl) -> Option<usize> {
    let row = list.get_next_item(-1, ListNextItemFlag::All, ListItemState::Selected);
    (row >= 0).then_some(row as usize)
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
