use super::{
    api::ApiClient,
    shell::{PageId, Shell},
    text::GuiText,
    theme,
};
use super::{
    controls::{Button, ButtonKind},
    runtime_scene::RuntimeScene,
    visual::{self, Canvas},
};
use crate::gui::controls::Choice;
use crate::{
    ai_gateway::request_log::desktop::LogFilter,
    desktop::{clients::ClientId, request_source},
};
use serde_json::Value;
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
#[derive(Clone)]
enum Action {
    Client(ClientId),
    Requests(LogFilter),
    Channel(String),
    Mcp(bool),
}
#[derive(Clone)]
struct Hit {
    rect: Rect,
    action: Action,
}
type Slot = Arc<Mutex<Option<Result<(String, Value, Option<Value>), String>>>>;

fn visible_activity(mut activity: Value) -> Value {
    for field in ["clients", "requests", "recentRequests", "mcpCalls"] {
        if let Some(rows) = activity[field].as_array_mut() {
            rows.retain(|row| {
                ClientId::ALL
                    .iter()
                    .find(|id| row["client"] == id.key())
                    .is_none_or(|id| id.is_visible())
            });
        }
    }
    activity
}

#[derive(Clone)]
pub(super) struct Overview {
    pub page: Panel,
    graph: Panel,
    scroll: ScrolledWindow,
    chart: Panel,
    stats: [StaticText; 4],
    status: StaticText,
    mode: Rc<Cell<usize>>,
    days: Rc<Cell<i64>>,
    data: Rc<RefCell<Value>>,
    scene: Rc<RefCell<RuntimeScene>>,
    usage: Rc<RefCell<Value>>,
    hits: Rc<RefCell<Vec<Hit>>>,
    phase: Rc<Cell<f64>>,
    active: Rc<Cell<bool>>,
    timer: Rc<RefCell<Option<Timer<Panel>>>>,
    pending: Slot,
    busy: Arc<AtomicBool>,
    api: ApiClient,
    last_fetch: Rc<Cell<Instant>>,
    last_usage: Rc<Cell<Instant>>,
    usage_dirty: Rc<Cell<bool>>,
    fresh: Rc<Cell<Instant>>,
    filters: Rc<RefCell<LogFilter>>,
    breakdown: ListCtrl,
    dimension: Choice,
    breakdown_rows: Rc<RefCell<Vec<Value>>>,
    chart_hover: Rc<Cell<Option<usize>>>,
}
impl Overview {
    pub fn new(shell: &Shell, frame: Frame, api: ApiClient, text: GuiText) -> Self {
        let page = Panel::builder(&shell.book).build();
        page.set_background_color(theme::theme().bg_app);
        let column = BoxSizer::builder(Orientation::Vertical).build();
        let toolbar = BoxSizer::builder(Orientation::Horizontal).build();
        let run = Button::builder(&page)
            .with_label("运行状态")
            .with_kind(ButtonKind::Tab)
            .with_size(Size::new(100, 42))
            .build();
        let stats_button = Button::builder(&page)
            .with_label("使用统计")
            .with_kind(ButtonKind::Tab)
            .with_size(Size::new(100, 42))
            .build();
        run.set_selected(true);
        toolbar.add(&run, 0, SizerFlag::Right, 12);
        toolbar.add(&stats_button, 0, SizerFlag::default(), 0);
        column.add_sizer(
            &toolbar,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(12),
        );
        let book = SimpleBook::builder(&page).build();
        let running = Panel::builder(&book).build();
        let rs = BoxSizer::builder(Orientation::Vertical).build();
        let (scroll, graph) = visual::runtime_surface(&running);
        rs.add(&scroll, 1, SizerFlag::Expand, 0);
        running.set_background_color(theme::theme().bg_app);
        scroll.set_background_color(theme::theme().bg_app);
        running.set_sizer(rs, true);
        book.add_page(&running, "运行状态", true, None);
        let usage_view = super::usage_view::UsageView::new(&book);
        let usage_page = usage_view.page;
        let days = Rc::new(Cell::new(super::usage_view::DEFAULT_DAYS));
        let usage_dirty = Rc::new(Cell::new(true));
        for (index, b) in usage_view.periods.iter().enumerate() {
            let (days, dirty, view) = (days.clone(), usage_dirty.clone(), usage_view.clone());
            b.on_click(move |_| {
                days.set(super::usage_view::PERIOD_DAYS[index]);
                dirty.set(true);
                for (i, b) in view.periods.iter().enumerate() {
                    b.set_selected(i == index);
                }
            });
        }
        let client = usage_view.client;
        let reset = usage_view.reset;
        let stats = usage_view.stats;
        let chart = usage_view.chart;
        let dimension = usage_view.dimension;
        let breakdown = usage_view.breakdown;
        book.add_page(&usage_page, "使用统计", false, None);
        column.add(&book, 1, SizerFlag::Expand, 0);
        let status = StaticText::builder(&page).with_label("").build();
        status.set_foreground_color(theme::theme().error);
        status.wrap(900);
        status.hide();
        column.add(&status, 0, SizerFlag::Expand | SizerFlag::All, 10);
        page.set_sizer(column, true);
        shell.register(PageId::Overview, &page);
        let ui = Self {
            page,
            graph,
            scroll,
            chart,
            stats,
            status,
            mode: Rc::new(Cell::new(0)),
            days,
            data: Default::default(),
            scene: Default::default(),
            usage: Default::default(),
            hits: Default::default(),
            phase: Default::default(),
            active: Default::default(),
            timer: Default::default(),
            pending: Default::default(),
            busy: Default::default(),
            api,
            last_fetch: Rc::new(Cell::new(Instant::now() - Duration::from_secs(5))),
            last_usage: Rc::new(Cell::new(Instant::now() - Duration::from_secs(60))),
            usage_dirty,
            fresh: Rc::new(Cell::new(Instant::now() - Duration::from_secs(20))),
            filters: Default::default(),
            breakdown,
            dimension,
            breakdown_rows: Default::default(),
            chart_hover: Default::default(),
        };
        let (u, view) = (ui.clone(), usage_view.clone());
        client.on_selection_changed(move |_| {
            u.filters.borrow_mut().client = client
                .get_selection()
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| request_source::keys().nth(i as usize))
                .map(Into::into);
            let f = u.filters.borrow().clone();
            view.update_filter_label(
                f.model.as_deref().unwrap_or(""),
                f.channel.as_deref().unwrap_or(""),
            );
            u.usage_dirty.set(true);
            u.fetch();
        });
        let (u, view) = (ui.clone(), usage_view.clone());
        usage_view.filters.on_click(move |_| {
            let f = u.filters.borrow().clone();
            let options = u.usage.borrow()["filters"].clone();
            if let Some((model, channel)) = super::usage_view::choose_filters(
                &frame,
                f.model.as_deref().unwrap_or(""),
                f.channel.as_deref().unwrap_or(""),
                &options,
            ) {
                let mut f = u.filters.borrow_mut();
                f.model = (!model.is_empty()).then_some(model.clone());
                f.channel = (!channel.is_empty()).then_some(channel.clone());
                drop(f);
                view.update_filter_label(&model, &channel);
                u.usage_dirty.set(true);
                u.fetch();
            }
        });
        let (u, view) = (ui.clone(), usage_view.clone());
        reset.on_click(move |_| {
            client.set_selection(0);
            view.update_filter_label("", "");
            *u.filters.borrow_mut() = LogFilter::default();
            u.usage_dirty.set(true);
            u.fetch();
        });
        let u = ui.clone();
        dimension.on_selection_changed(move |_| u.render_breakdown());
        let u = ui.clone();
        let action_api = ui.api.clone();
        breakdown.on_item_activated(move |e| {
            let Some(row) = u
                .breakdown_rows
                .borrow()
                .get(e.get_item_index().max(0) as usize)
                .cloned()
            else {
                return;
            };
            let mut filter = u.filter();
            let name = row["name"].as_str().map(str::to_string);
            match u.dimension.get_selection() {
                Some(1) => filter.model = name,
                Some(2) => {
                    filter.route = row["route"].as_str().map(str::to_string);
                    if filter.route.is_none() {
                        filter.channel = name;
                    }
                }
                _ => filter.client = Some(name.unwrap_or("__unknown__".into())),
            }
            super::request_browser::show(frame, action_api.clone(), text, filter);
        });
        for (i, b) in [run, stats_button].into_iter().enumerate() {
            let ui = ui.clone();
            b.on_click(move |_| {
                if i == 0 && ui.mode.get() != 0 {
                    ui.reset_scene();
                }
                ui.mode.set(i);
                book.change_selection(i);
                run.set_selected(i == 0);
                stats_button.set_selected(i == 1);
                run.set_foreground_color(if i == 0 {
                    theme::theme().accent
                } else {
                    theme::theme().ink_secondary
                });
                stats_button.set_foreground_color(if i == 1 {
                    theme::theme().accent
                } else {
                    theme::theme().ink_secondary
                });
                ui.fetch();
            });
        }
        let u = ui.clone();
        graph.on_paint(move |_| u.paint_graph());
        let u = ui.clone();
        chart.on_paint(move |_| u.paint_chart());
        let u = ui.clone();
        chart.on_mouse_motion(move |event| {
            if let WindowEventData::MouseMotion(ref event) = event {
                let count = u.usage.borrow()["trend"].as_array().map_or(0, Vec::len);
                let index = event.get_position().and_then(|p| {
                    visual::usage_chart_index(
                        chart.get_client_size(),
                        p,
                        visual::scale(&chart),
                        count,
                    )
                });
                if u.chart_hover.replace(index) != index {
                    chart.refresh(false, None);
                }
            }
            event.skip(true);
        });
        let u = ui.clone();
        chart.on_mouse_leave(move |event| {
            u.chart_hover.set(None);
            chart.refresh(false, None);
            event.skip(true);
        });
        let (u, nav) = (ui.clone(), shell.clone());
        let action_api = ui.api.clone();
        graph.on_mouse_left_up(move |event| {
            let WindowEventData::MouseButton(event) = event else {
                return;
            };
            let Some(point) = event.get_position() else {
                return;
            };
            let hit = u
                .hits
                .borrow()
                .iter()
                .find(|h| {
                    point.x >= h.rect.x
                        && point.x < h.rect.x + h.rect.width
                        && point.y >= h.rect.y
                        && point.y < h.rect.y + h.rect.height
                })
                .cloned();
            if let Some(hit) = hit {
                match hit.action {
                    Action::Client(id) => nav.navigate(PageId::Client(id)),
                    Action::Requests(filter) => {
                        super::request_browser::show(frame, action_api.clone(), text, filter)
                    }
                    Action::Channel(route) => nav.focus_source(route),
                    Action::Mcp(dumpling) => nav.navigate(if dumpling {
                        PageId::Dumpling
                    } else {
                        PageId::Logs
                    }),
                }
            }
        });
        let u = ui.clone();
        let api = ui.api.clone();
        chart.on_mouse_left_up(move |event| {
            let WindowEventData::MouseButton(event) = event else {
                return;
            };
            let Some(point) = event.get_position() else {
                return;
            };
            let usage = u.usage.borrow();
            let Some(trend) = usage["trend"].as_array() else {
                return;
            };
            let Some(index) = visual::usage_chart_index(
                chart.get_client_size(),
                point,
                visual::scale(&chart),
                trend.len(),
            ) else {
                return;
            };
            let at = trend[index]["at"].as_i64().unwrap_or(0);
            let bucket = usage["bucketMs"].as_i64().unwrap_or(86400000);
            drop(usage);
            super::request_browser::show(
                frame,
                api.clone(),
                text,
                LogFilter {
                    from: at,
                    to: at + bucket,
                    calendar_days: 0,
                    ..u.filter()
                },
            );
        });
        let timer = Timer::new(&page);
        let u = ui.clone();
        timer.on_tick(move |_| {
            if !u.active.get() {
                return;
            }
            let result = u.pending.lock().ok().and_then(|mut s| s.take());
            if let Some(result) = result {
                u.busy.store(false, Ordering::SeqCst);
                match result {
                    Ok((key, activity, usage)) => {
                        u.scene.borrow_mut().observe(&activity);
                        let view = u.scene.borrow().view(
                            crate::ai_gateway::request_log::now_ms(),
                            theme::motion_enabled(),
                        );
                        let min = visual::dip_size(
                            &u.graph,
                            620,
                            visual::runtime_height(
                                view.clients.len(),
                                view.routes.len(),
                                view.mcps.len(),
                            ),
                        );
                        if u.graph.get_min_size() != min {
                            u.graph.set_min_size(min);
                            u.scroll.layout();
                            u.scroll.fit_inside();
                        }
                        *u.data.borrow_mut() = activity;
                        u.fresh.set(Instant::now());
                        if let Some(usage) = usage.filter(|_| key == u.filter_key()) {
                            u.render_stats(&usage);
                            *u.usage.borrow_mut() = usage;
                            u.render_breakdown();
                            u.usage_dirty.set(false);
                            u.last_usage.set(Instant::now());
                            u.chart.refresh(false, None);
                        }
                        u.status.set_label("");
                    }
                    Err(error) => u.status.set_label(&error),
                }
                u.status.show(!u.status.get_label().is_empty());
                u.page.layout();
            }
            if u.mode.get() == 1 && u.last_usage.get().elapsed() > Duration::from_secs(15) {
                u.usage_dirty.set(true);
            }
            if u.last_fetch.get().elapsed()
                > Duration::from_millis(if u.mode.get() == 0 { 300 } else { 1200 })
                || (u.mode.get() == 1 && u.usage_dirty.get())
            {
                u.fetch();
            }
            if u.mode.get() == 0 {
                if theme::motion_enabled() {
                    u.phase.set((u.phase.get() + 0.024) % 1.);
                }
                u.graph.refresh(false, None);
            }
        });
        ui.timer.borrow_mut().replace(timer);
        ui
    }
    pub fn enter(&self, id: PageId) {
        let active = id == PageId::Overview;
        if active && !self.active.get() {
            self.reset_scene();
        }
        self.active.set(active);
        if let Some(timer) = self.timer.borrow().as_ref() {
            if active {
                timer.start(60, false);
                self.fetch();
            } else {
                timer.stop();
            }
        }
    }
    fn reset_scene(&self) {
        self.scene
            .borrow_mut()
            .reset(crate::ai_gateway::request_log::now_ms());
        self.fresh.set(Instant::now() - Duration::from_secs(20));
        self.graph
            .set_min_size(visual::dip_size(&self.graph, 620, 300));
        self.scroll.layout();
        self.scroll.fit_inside();
        self.graph.refresh(false, None);
    }
    fn filter(&self) -> LogFilter {
        LogFilter {
            calendar_days: self.days.get(),
            bucket_minutes: if self.days.get() == 1 { 60 } else { 1440 },
            ..self.filters.borrow().clone()
        }
    }
    fn filter_key(&self) -> String {
        serde_json::to_string(&self.filter()).unwrap_or_default()
    }
    fn fetch(&self) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        self.last_fetch.set(Instant::now());
        let (api, slot, filter, include_usage) = (
            self.api.clone(),
            self.pending.clone(),
            self.filter(),
            self.mode.get() == 1 && self.usage_dirty.get(),
        );
        std::thread::spawn(move || {
            let result = (|| {
                let activity =
                    visible_activity(api.get_with_timeout(
                        "/api/desktop/activity?live=true",
                        Duration::from_secs(4),
                    )?);
                let usage = if include_usage {
                    let fields = serde_json::to_value(&filter).unwrap_or_default();
                    let url = url::Url::parse_with_params(
                        &format!("{}/api/desktop/usage", api.base_url),
                        fields
                            .as_object()
                            .into_iter()
                            .flatten()
                            .filter(|(_, v)| !v.is_null())
                            .map(|(k, v)| {
                                (
                                    k.as_str(),
                                    v.as_str()
                                        .map(str::to_string)
                                        .unwrap_or_else(|| v.to_string()),
                                )
                            }),
                    )
                    .map_err(|e| e.to_string())?;
                    let path = format!("{}?{}", url.path(), url.query().unwrap_or(""));
                    Some(api.get_with_timeout(&path, Duration::from_secs(15))?)
                } else {
                    None
                };
                Ok((
                    serde_json::to_string(&filter).unwrap_or_default(),
                    activity,
                    usage,
                ))
            })();
            if let Ok(mut s) = slot.lock() {
                *s = Some(result);
            }
            wxdragon::wake_up_idle();
        });
    }
    fn render_stats(&self, usage: &Value) {
        let summary = &usage["summary"];
        let requests = summary["requests"].as_i64().unwrap_or(0);
        let success = summary["successful"].as_i64().unwrap_or(0);
        for (i, value) in [
            requests.to_string(),
            if requests > 0 {
                format!("{:.1}%", success as f64 * 100. / requests as f64)
            } else {
                "—".into()
            },
            short_number(&summary["input"]),
            short_number(&summary["output"]),
        ]
        .into_iter()
        .enumerate()
        {
            self.stats[i].set_label(&value);
        }
        self.page.layout();
    }
    fn render_breakdown(&self) {
        let index = self.dimension.get_selection().unwrap_or(0) as usize;
        let rows = self.usage.borrow()["breakdown"][["clients", "models", "channels"][index]]
            .as_array()
            .cloned()
            .unwrap_or_default();
        self.breakdown.delete_all_items();
        for (i, row) in rows.iter().enumerate() {
            let name = row["name"].as_str().unwrap_or("历史未标记");
            let label = if index == 0 {
                request_source::name(name).unwrap_or(name)
            } else {
                name
            };
            self.breakdown.insert_item(i as i64, label, None);
            for (j, value) in [
                row["requests"].to_string(),
                short_number(&row["input"]),
                short_number(&row["output"]),
            ]
            .iter()
            .enumerate()
            {
                self.breakdown
                    .set_item_text_by_column(i as i64, j as i32 + 1, value);
            }
        }
        *self.breakdown_rows.borrow_mut() = rows;
    }
    fn paint_graph(&self) {
        let base = AutoBufferedPaintDC::new(&self.graph);
        let dc = GraphicsDC::new(&base);
        let scale = visual::scale(&self.graph);
        let c = Canvas::new(&dc, scale);
        let physical = self.graph.get_client_size();
        let size = Size::new(
            (physical.width as f64 / scale) as i32,
            (physical.height as f64 / scale) as i32,
        );
        let view = self.scene.borrow().view(
            crate::ai_gateway::request_log::now_ms(),
            theme::motion_enabled(),
        );
        let routes = view
            .routes
            .iter()
            .map(|(_, r)| r.clone())
            .collect::<Vec<_>>();
        let mcps = view
            .mcps
            .iter()
            .map(|(_, _, r)| r.clone())
            .collect::<Vec<_>>();
        let result = visual::draw_runtime(
            &c,
            size,
            &view.clients,
            &routes,
            &mcps,
            if self.fresh.get().elapsed() < Duration::from_secs(8) {
                Some(true)
            } else {
                None
            },
            self.phase.get(),
        );
        let mut hits = self.hits.borrow_mut();
        hits.clear();
        for (rect, client) in result.clients.into_iter().zip(&view.clients) {
            hits.push(Hit {
                rect,
                action: Action::Client(client.id),
            });
        }
        for ((route, model), rect) in view.routes.iter().zip(result.models) {
            hits.push(Hit {
                rect,
                action: Action::Requests(LogFilter {
                    route: Some(route.clone()),
                    model: Some(model.model.clone()),
                    ..Default::default()
                }),
            });
        }
        for ((route, _), rect) in view.routes.iter().zip(result.channels) {
            hits.push(Hit {
                rect,
                action: Action::Channel(route.clone()),
            });
        }
        for ((id, dumpling, _), rect) in view.mcps.iter().zip(result.mcps) {
            hits.push(Hit {
                rect,
                action: if id == request_source::LOCAL_ASSISTANCE_MCP_RESOURCE {
                    Action::Requests(LogFilter {
                        client: Some(request_source::LOCAL_ASSISTANCE_MCP.into()),
                        ..Default::default()
                    })
                } else {
                    Action::Mcp(*dumpling)
                },
            });
        }
    }
    fn paint_chart(&self) {
        let base = AutoBufferedPaintDC::new(&self.chart);
        let dc = GraphicsDC::new(&base);
        let scale = visual::scale(&self.chart);
        let size = self.chart.get_client_size();
        let usage = self.usage.borrow();
        let rows = usage["trend"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| {
                let label = r["label"].as_str().unwrap_or("");
                let label = if self.days.get() == 1 {
                    label.split(' ').last().unwrap_or(label)
                } else {
                    label.split(' ').next().unwrap_or(label)
                };
                (label.to_string(), r["input"].as_i64(), r["output"].as_i64())
            })
            .collect::<Vec<_>>();
        visual::draw_usage_chart(
            &Canvas::new(&dc, scale),
            Size::new(
                (size.width as f64 / scale) as i32,
                (size.height as f64 / scale) as i32,
            ),
            &rows,
        );
        if let Some(index) = self.chart_hover.get().filter(|i| *i < rows.len()) {
            let row = &usage["trend"][index];
            visual::draw_usage_hover(
                &Canvas::new(&dc, scale),
                Size::new(
                    (size.width as f64 / scale) as i32,
                    (size.height as f64 / scale) as i32,
                ),
                &rows,
                index,
                row["label"].as_str().unwrap_or(""),
                row["requests"].as_i64().unwrap_or(0),
            );
        }
    }
}
fn short_number(value: &Value) -> String {
    value.as_i64().map(compact).unwrap_or("—".into())
}
fn compact(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1_000_000.)
    } else if n >= 10_000 {
        format!("{:.1}K", n as f64 / 1000.)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod visibility_tests {
    use super::*;

    #[test]
    fn overview_hides_clients_and_their_live_edges_without_changing_other_sources() {
        let rows = serde_json::json!([
            {"client":"vscode","route":"hidden-code"},
            {"client":"webstorm","route":"hidden-webstorm"},
            {"client":"zcode","route":"visible"},
            {"source":"local-assistance-mcp","route":"assistance"},
            {"client":"genius-buddy","route":"buddy"}
        ]);
        let mut input = serde_json::json!({"at":42,"metadata":"keep"});
        for field in ["clients", "requests", "recentRequests", "mcpCalls"] {
            input[field] = rows.clone();
        }
        let filtered = visible_activity(input);
        assert_eq!(filtered["at"], 42);
        assert_eq!(filtered["metadata"], "keep");
        for field in ["clients", "requests", "recentRequests", "mcpCalls"] {
            assert_eq!(
                filtered[field],
                serde_json::json!([
                    {"client":"zcode","route":"visible"},
                    {"source":"local-assistance-mcp","route":"assistance"},
                    {"client":"genius-buddy","route":"buddy"}
                ])
            );
        }
    }
}
