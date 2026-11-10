use super::controls::Button;
use super::request_view::{RequestView, fit_columns};
use super::{
    api::{ApiClient, RequestLogDetail},
    text::GuiText,
    theme,
};
use crate::gui::controls::{Choice, TextCtrl};
use crate::{ai_gateway::request_log::desktop::LogFilter, desktop::clients::ClientId};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use wxdragon::{prelude::*, timer::Timer};
type Slot = Arc<Mutex<Option<(u64, Result<Value, String>)>>>;
#[derive(Clone)]
pub(super) struct RequestBrowser {
    pub page: Panel,
    filter: Rc<RefCell<LogFilter>>,
    rows: Rc<RefCell<Vec<Value>>>,
    table: ListCtrl,
    status: StaticText,
    previous: Button,
    next: Button,
    api: ApiClient,
    pending: Slot,
    busy: Arc<AtomicBool>,
    pub active: Rc<Cell<bool>>,
    timer: Rc<RefCell<Option<Timer<Panel>>>>,
    serial: Rc<Cell<u64>>,
    queued: Rc<Cell<bool>>,
    last: Rc<Cell<std::time::Instant>>,
    technical: Rc<Cell<bool>>,
    total: Rc<Cell<u64>>,
    search: TextCtrl,
    client: Choice,
    result: Choice,
    channel: TextCtrl,
    start: TextCtrl,
    end: TextCtrl,
}
impl RequestBrowser {
    pub fn new<W: WxWidget>(
        parent: &W,
        frame: Frame,
        api: ApiClient,
        text: GuiText,
        filter: LogFilter,
    ) -> Self {
        let RequestView {
            page,
            search,
            client,
            apply,
            reset,
            technical,
            result,
            channel,
            start,
            end,
            table,
            status,
            previous,
            next,
        } = RequestView::new(
            parent,
            [
                filter.query.as_deref().unwrap_or(""),
                filter.channel.as_deref().unwrap_or(""),
                filter.start_date.as_deref().unwrap_or(""),
                filter.end_date.as_deref().unwrap_or(""),
            ],
            filter
                .client
                .as_deref()
                .and_then(|key| ClientId::ALL.iter().position(|c| c.key() == key))
                .map_or(0, |i| i as u32 + 1),
            match filter.status.as_deref() {
                Some("completed" | "success") => 1,
                Some("failed" | "error") => 2,
                Some("running") => 3,
                Some("cancelled" | "canceled") => 4,
                Some("incomplete") => 5,
                _ => 0,
            },
        );
        let view = Self {
            page,
            filter: Rc::new(RefCell::new(filter)),
            rows: Default::default(),
            table,
            status,
            previous,
            next,
            api,
            pending: Default::default(),
            busy: Default::default(),
            active: Default::default(),
            timer: Default::default(),
            serial: Default::default(),
            queued: Default::default(),
            last: Rc::new(Cell::new(std::time::Instant::now())),
            technical: Default::default(),
            total: Default::default(),
            search,
            client,
            result,
            channel,
            start,
            end,
        };
        let v = view.clone();
        apply.on_click(move |_| {
            let mut filter = v.filter.borrow().clone();
            filter.offset = 0;
            filter.query = Some(search.get_value());
            filter.client = client
                .get_selection()
                .and_then(|s| s.checked_sub(1))
                .and_then(|i| ClientId::ALL.get(i as usize))
                .map(|c| c.key().into());
            filter.status = [
                None,
                Some("completed"),
                Some("failed"),
                Some("running"),
                Some("cancelled"),
                Some("incomplete"),
            ][result.get_selection().unwrap_or(0) as usize]
                .map(str::to_string);
            filter.channel = Some(channel.get_value());
            filter.start_date = Some(start.get_value());
            filter.end_date = Some(end.get_value());
            if let Err(error) = filter.validate() {
                v.status.set_label(&error);
                return;
            }
            *v.filter.borrow_mut() = filter;
            v.refresh();
        });
        let v = view.clone();
        reset.on_click(move |_| v.set_filter(LogFilter::default()));
        let v = view.clone();
        technical.on_toggled(move |_| {
            v.technical.set(technical.get_value());
            let rows = v.rows.borrow().clone();
            let offset = v.filter.borrow().offset;
            v.render(serde_json::json!({"rows":rows,"offset":offset,"total":v.total.get()}));
        });
        let v = view.clone();
        previous.on_click(move |_| {
            let mut f = v.filter.borrow_mut();
            f.offset = f.offset.saturating_sub(50);
            drop(f);
            v.refresh();
        });
        let v = view.clone();
        next.on_click(move |_| {
            v.filter.borrow_mut().offset += 50;
            v.refresh();
        });
        let detail: Arc<Mutex<Option<Result<RequestLogDetail, String>>>> = Default::default();
        let detail_busy = Arc::new(AtomicBool::new(false));
        let (rows, worker_api, slot, in_flight) = (
            view.rows.clone(),
            view.api.clone(),
            detail.clone(),
            detail_busy.clone(),
        );
        table.on_item_activated(move |e| {
            let Some(id) = rows
                .borrow()
                .get(e.get_item_index().max(0) as usize)
                .and_then(|r| r["id"].as_i64())
            else {
                return;
            };
            if in_flight.swap(true, Ordering::SeqCst) {
                return;
            }
            let (api, slot) = (worker_api.clone(), slot.clone());
            std::thread::spawn(move || {
                let result = api.get_with_timeout(
                    &format!("/ai-gateway/request-logs/{id}"),
                    Duration::from_secs(15),
                );
                if let Ok(mut s) = slot.lock() {
                    *s = Some(result);
                }
                wxdragon::wake_up_idle();
            });
        });
        let timer = Timer::new(&page);
        let v = view.clone();
        timer.on_tick(move |_| {
            let pending = v.pending.lock().ok().and_then(|mut s| s.take());
            if let Some((serial, result)) = pending {
                v.busy.store(false, Ordering::SeqCst);
                if serial == v.serial.get() {
                    match result {
                        Ok(value) => v.render(value),
                        Err(error) => v.status.set_label(&error),
                    }
                }
                if v.queued.replace(false) {
                    v.load();
                }
            }
            let result = detail.lock().ok().and_then(|mut s| s.take());
            if let Some(result) = result {
                detail_busy.store(false, Ordering::SeqCst);
                match result {
                    Ok(detail) => super::request_log_detail::show(&frame, text, &detail),
                    Err(error) => super::show_error(&frame, &error),
                }
            }
            if v.active.get()
                && v.filter.borrow().offset == 0
                && v.last.get().elapsed() > Duration::from_secs(5)
                && !v.busy.load(Ordering::SeqCst)
            {
                v.refresh();
            }
        });
        view.timer.borrow_mut().replace(timer);
        view
    }
    pub fn enter(&self, active: bool) {
        self.active.set(active);
        if let Some(timer) = self.timer.borrow().as_ref() {
            if active {
                timer.start(100, false);
                self.refresh();
            } else {
                timer.stop();
            }
        }
    }
    pub fn set_filter(&self, filter: LogFilter) {
        self.search.set_value(filter.query.as_deref().unwrap_or(""));
        self.client.set_selection(
            filter
                .client
                .as_deref()
                .and_then(|key| ClientId::ALL.iter().position(|c| c.key() == key))
                .map(|i| i + 1)
                .unwrap_or(0) as u32,
        );
        self.result.set_selection(match filter.status.as_deref() {
            Some("completed" | "success") => 1,
            Some("failed" | "error") => 2,
            Some("running") => 3,
            Some("cancelled" | "canceled") => 4,
            Some("incomplete") => 5,
            _ => 0,
        });
        self.channel
            .set_value(filter.channel.as_deref().unwrap_or(""));
        self.start
            .set_value(filter.start_date.as_deref().unwrap_or(""));
        self.end.set_value(filter.end_date.as_deref().unwrap_or(""));
        *self.filter.borrow_mut() = filter;
        self.refresh();
    }
    pub fn refresh(&self) {
        self.serial.set(self.serial.get() + 1);
        if self.busy.load(Ordering::SeqCst) {
            self.queued.set(true);
            return;
        }
        self.load();
    }
    fn load(&self) {
        if self.busy.swap(true, Ordering::SeqCst) {
            self.queued.set(true);
            return;
        }
        self.last.set(std::time::Instant::now());
        self.status.set_label("读取中…");
        let (api, slot, filter, serial) = (
            self.api.clone(),
            self.pending.clone(),
            self.filter.borrow().clone(),
            self.serial.get(),
        );
        std::thread::spawn(move || {
            let url = url::Url::parse_with_params(
                &format!("{}/api/desktop/requests", api.base_url),
                serde_json::to_value(filter)
                    .unwrap_or_default()
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter(|(_, v)| !v.is_null())
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            if v.is_string() {
                                v.as_str().unwrap().into()
                            } else {
                                v.to_string()
                            },
                        )
                    }),
            );
            let result = url.map_err(|_| "筛选条件无效".to_string()).and_then(|url| {
                api.request_text(api.http.get(url))
                    .and_then(|s| serde_json::from_str(&s).map_err(|_| "请求列表格式不匹配".into()))
            });
            if let Ok(mut s) = slot.lock() {
                *s = Some((serial, result));
            }
            wxdragon::wake_up_idle();
        });
    }
    fn render(&self, value: Value) {
        let rows = value["rows"].as_array().cloned().unwrap_or_default();
        self.table.freeze();
        self.table.clear_all();
        for (i, (title, width)) in [
            ("时间", 150),
            ("客户端", 125),
            ("模型", 170),
            ("渠道", 155),
            ("状态", 90),
            ("输入", 90),
            ("输出", 90),
            ("耗时", 85),
        ]
        .into_iter()
        .enumerate()
        {
            self.table
                .insert_column(i as i64, title, ListColumnFormat::Left, width);
        }
        if self.technical.get() {
            for (i, title) in ["客户端流式", "上游流式", "请求编号", "路由"]
                .into_iter()
                .enumerate()
            {
                self.table.insert_column(
                    i as i64 + 8,
                    title,
                    ListColumnFormat::Left,
                    if i < 2 { 100 } else { 230 },
                );
            }
        }
        for (i, row) in rows.iter().enumerate() {
            let client = row["clientId"]
                .as_str()
                .and_then(|key| ClientId::ALL.into_iter().find(|c| c.key() == key))
                .map(ClientId::name)
                .unwrap_or("未标记来源");
            let status = match row["status"].as_str().unwrap_or("") {
                "running" => "进行中",
                "completed" | "success" => "成功",
                "failed" | "error" => "失败",
                "cancelled" => "已取消",
                "incomplete" => "未完整返回",
                _ => "未知",
            };
            let mut values = vec![
                row["createdAt"].as_str().unwrap_or("—").into(),
                client.into(),
                row["modelId"].as_str().unwrap_or("").into(),
                row["channel"].as_str().unwrap_or("").into(),
                status.into(),
                number(&row["inputTokens"]),
                number(&row["outputTokens"]),
                row["latencyMs"]
                    .as_i64()
                    .map(|n| format!("{:.2}s", n as f64 / 1000.))
                    .unwrap_or("—".into()),
            ];
            if self.technical.get() {
                values.extend([
                    boolean(&row["stream"]),
                    boolean(&row["upstreamStream"]),
                    row["requestId"].as_str().unwrap_or("—").into(),
                    row["routeId"].as_str().unwrap_or("—").into(),
                ]);
            }
            self.table.insert_item(i as i64, &values[0], None);
            for (col, text) in values.iter().enumerate().skip(1) {
                self.table
                    .set_item_text_by_column(i as i64, col as i32, text);
            }
            if i % 2 == 1 {
                self.table
                    .set_item_background_colour(i as i64, &theme::theme().table_row_alt);
            }
        }
        self.table.thaw();
        fit_columns(self.table, self.technical.get());
        let total = value["total"].as_u64().unwrap_or(0);
        self.total.set(total);
        let offset = self.filter.borrow().offset;
        self.status
            .set_label(&format!("{} 条 · 第 {} 页", total, offset / 50 + 1));
        self.previous.enable(offset > 0);
        self.next.enable((offset + rows.len()) < (total as usize));
        *self.rows.borrow_mut() = rows;
    }
}
pub(super) fn show(frame: Frame, api: ApiClient, text: GuiText, filter: LogFilter) {
    let size = super::ai_gw_channel_dialog_size(&frame, Size::new(1160, 720));
    let dialog = Dialog::builder(&frame, "请求记录")
        .with_size(size.width, size.height)
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    let browser = RequestBrowser::new(&dialog, frame, api, text, filter);
    let s = BoxSizer::builder(Orientation::Vertical).build();
    s.add(&browser.page, 1, SizerFlag::Expand, 0);
    dialog.set_sizer(s, true);
    dialog.centre();
    browser.enter(true);
    dialog.show_modal();
    browser.enter(false);
    dialog.destroy();
}
fn number(value: &Value) -> String {
    value.as_i64().map(|n| n.to_string()).unwrap_or("—".into())
}

fn boolean(value: &Value) -> String {
    value
        .as_bool()
        .map(|v| if v { "是" } else { "否" }.into())
        .unwrap_or("—".into())
}
