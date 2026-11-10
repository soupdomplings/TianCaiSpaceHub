use super::controls::{Button, ButtonKind};
use super::{
    GuiMessage,
    api::ApiClient,
    dialog_job, mcp_editor,
    resource_detail::{IssueItem, IssueList, TargetOption, TargetPicker},
    resource_gallery::{Gallery, ResourceCard},
    shell::{PageId, Shell},
    theme,
};
use crate::desktop::{
    clients::ClientId,
    mcp::McpServer,
    resources::{DiscoveryIssue, LibraryView, ResourceKind, ResourceView},
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeSet, HashSet},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use wxdragon::prelude::*;
use wxdragon::widgets::SimpleBook;

pub(super) enum ResourceEvent {
    Result(Result<Value, String>),
    Edit {
        resource_id: String,
        result: Result<Value, String>,
    },
}
#[derive(Clone)]
struct Tab {
    page: Panel,
    list_page: Panel,
    book: SimpleBook,
    picker: TargetPicker,
    selection_revision: Rc<RefCell<String>>,
    issues_button: Button,
    issue_list: IssueList,
    gallery: Gallery,
    used: Button,
    available: Button,
    browsing: Rc<Cell<bool>>,
    status: StaticText,
    add: Button,
    retry: Button,
    scan: Button,
    resolve: Button,
    kind: ResourceKind,
    client_filter: Option<ClientId>,
    shown: Rc<RefCell<Vec<ResourceView>>>,
    selected: Rc<RefCell<Option<String>>>,
}
#[derive(Clone)]
pub(super) struct ResourceUi {
    skills: Tab,
    market: super::skill_market::SkillMarket,
    market_visible: Rc<Cell<bool>>,
    mcp: Tab,
    client_tabs: Rc<RefCell<Vec<Tab>>>,
    view: Rc<RefCell<Option<LibraryView>>>,
    busy: Arc<AtomicBool>,
    loaded: Rc<Cell<bool>>,
    scanned_at: Rc<Cell<Option<Instant>>>,
    recovery: Rc<RefCell<Option<Button>>>,
    api: ApiClient,
    frame: Frame,
    tx: tokio::sync::mpsc::UnboundedSender<GuiMessage>,
    last_undo: Rc<RefCell<Option<String>>>,
    last_request: Rc<RefCell<Option<(String, Value)>>>,
    failed: Rc<RefCell<Option<Value>>>,
    dumpling: Button,
    issues: Rc<RefCell<Vec<DiscoveryIssue>>>,
}

impl ResourceUi {
    pub fn set_dumpling_visible(&self, visible: bool) {
        self.dumpling.show(visible);
        self.mcp.list_page.layout();
    }
    pub fn new(
        shell: &Shell,
        frame: Frame,
        api: ApiClient,
        tx: tokio::sync::mpsc::UnboundedSender<GuiMessage>,
    ) -> Self {
        let skills_root = Panel::builder(&shell.book).build();
        skills_root.set_background_color(theme::theme().bg_app);
        let skills_col = BoxSizer::builder(Orientation::Vertical).build();
        let nav = BoxSizer::builder(Orientation::Horizontal).build();
        let mine = Button::builder(&skills_root)
            .with_label("我的 Skill")
            .with_kind(ButtonKind::Tab)
            .build();
        let plaza = Button::builder(&skills_root)
            .with_label("Skill 广场")
            .with_kind(ButtonKind::Tab)
            .build();
        mine.set_selected(true);
        for b in [mine, plaza] {
            nav.add(&b, 0, SizerFlag::Right, skills_root.from_dip(12));
        }
        skills_col.add_sizer(&nav, 0, SizerFlag::Bottom, skills_root.from_dip(8));
        let skills_book = SimpleBook::builder(&skills_root).build();
        let skills = Tab::new(&skills_book, ResourceKind::Skill);
        let market =
            super::skill_market::SkillMarket::new(&skills_book, frame, api.clone(), tx.clone());
        skills_book.add_page(&skills.page, "我的 Skill", true, None);
        skills_book.add_page(&market.page, "Skill 广场", false, None);
        skills_col.add(&skills_book, 1, SizerFlag::Expand, 0);
        skills_root.set_sizer(skills_col, true);
        let market_visible = Rc::new(Cell::new(false));
        for (button, show) in [(mine, false), (plaza, true)] {
            let (market, visible) = (market.clone(), market_visible.clone());
            button.on_click(move |_| {
                visible.set(show);
                skills_book.change_selection(usize::from(show));
                mine.set_selected(!show);
                plaza.set_selected(show);
                market.enter(show);
            });
        }
        let mcp = Tab::new(&shell.book, ResourceKind::Mcp);
        shell.register(PageId::Skills, &skills_root);
        shell.register(PageId::Mcp, &mcp.page);
        let dumpling = Button::builder(&mcp.list_page)
            .with_label("Dumpling-MCP 环境与认证")
            .build();
        // Put the optional specialised manager alongside the general service list.
        if let Some(sizer) = mcp.list_page.get_sizer() {
            sizer.add(&dumpling, 0, SizerFlag::All, 12);
        }
        let nav = shell.clone();
        dumpling.on_click(move |_| nav.navigate(PageId::Dumpling));
        let ui = Self {
            skills,
            market,
            market_visible,
            mcp,
            client_tabs: Default::default(),
            view: Default::default(),
            busy: Default::default(),
            loaded: Default::default(),
            scanned_at: Default::default(),
            recovery: Default::default(),
            api,
            frame,
            tx,
            last_undo: Default::default(),
            last_request: Default::default(),
            failed: Default::default(),
            dumpling,
            issues: Default::default(),
        };
        ui.set_dumpling_visible(super::load_dumpling_mcp_visibility());
        for tab in [&ui.skills, &ui.mcp] {
            ui.wire(tab);
        }
        ui
    }
    fn wire(&self, tab: &Tab) {
        let ui = self;
        let (ui, tab) = (ui.clone(), tab.clone());
        for (button, browse) in [(tab.used, false), (tab.available, true)] {
            let (c, t) = (ui.clone(), tab.clone());
            button.on_click(move |_| {
                t.browsing.set(browse);
                t.used.set_selected(!browse);
                t.available.set_selected(browse);
                c.rebuild();
            });
        }
        let c = ui.clone();
        let t = tab.clone();
        tab.retry.on_click(move |_| {
            let failed = c.failed.borrow().clone();
            if let Some(body) = failed {
                let path = if body.get("changes").is_some() {
                    "/api/desktop/resource/selection"
                } else {
                    "/api/desktop/resource/use"
                };
                c.post(path, body, &t);
            } else {
                c.refresh(true, &t);
            }
        });
        let c = ui.clone();
        let t = tab.clone();
        tab.scan.on_click(move |_| c.refresh(true, &t));
        let t = tab.clone();
        tab.picker.back.on_click(move |_| {
            t.book.change_selection(0);
            t.page.layout();
        });
        let (c, t) = (ui.clone(), tab.clone());
        tab.picker.reload.on_click(move |_| {
            t.book.change_selection(0);
            c.refresh(true, &t);
        });
        let (c, t) = (ui.clone(), tab.clone());
        tab.picker.apply.on_click(move |_| c.apply_selection(&t));
        let (c, t) = (ui.clone(), tab.clone());
        tab.issues_button.on_click(move |_| {
            c.populate_issues(&t);
            t.book.change_selection(2);
            t.page.layout();
        });
        let t = tab.clone();
        tab.issue_list.back.on_click(move |_| {
            t.book.change_selection(0);
            t.page.layout();
        });
        let (c, t) = (ui.clone(), tab.clone());
        tab.issue_list.retry.on_click(move |_| c.refresh(true, &t));
        let c = ui.clone();
        let t = tab.clone();
        tab.resolve.on_click(move |_| {
            let failed = c.failed.borrow().clone();
            if let Some(mut body) = failed {
                let decision = MessageDialog::builder(
                    &c.frame,
                    "已使用的客户端会保留。用 Hub 配置替换失败端的同名条目或后改内容？",
                    "处理配置冲突",
                )
                .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
                .build();
                if decision.show_modal() == ID_YES {
                    body["replaceConflict"] = true.into();
                    let path = if body.get("changes").is_some() {
                        "/api/desktop/resource/selection"
                    } else {
                        "/api/desktop/resource/use"
                    };
                    c.post(path, body, &t);
                }
            }
        });
        let c = ui.clone();
        let t = tab.clone();
        tab.add.on_click(move |_| c.add(&t));
    }
    pub fn add_recovery_control(&self, parent: &dyn WxWidget, column: &BoxSizer) {
        let button = Button::builder(parent)
            .with_label("恢复 Skill / MCP 配置")
            .with_kind(ButtonKind::Ghost)
            .build();
        button.enable(false);
        column.add(
            &button,
            0,
            SizerFlag::Left | SizerFlag::Bottom,
            parent.from_dip(20),
        );
        *self.recovery.borrow_mut() = Some(button);
        let c = self.clone();
        button.on_click(move |_| c.restore(&c.skills));
    }
    fn restore(&self, tab: &Tab) {
        let c = self;
        let t = tab;
        let rows = c
            .view
            .borrow()
            .as_ref()
            .map(|v| v.recoveries.clone())
            .unwrap_or_default();
        if !rows.is_empty() {
            let labels = rows
                .iter()
                .map(|v| {
                    format!(
                        "{} · {} · {}",
                        v["name"].as_str().unwrap_or("配置更改"),
                        v["client"].as_str().unwrap_or("客户端"),
                        if v["using"] == true {
                            "使用"
                        } else {
                            "停用"
                        }
                    )
                })
                .collect::<Vec<_>>();
            let picker = SingleChoiceDialog::builder(
                &c.frame,
                "选择要撤销的操作；会先核对后续修改",
                "恢复更改",
                &labels.iter().map(String::as_str).collect::<Vec<_>>(),
            )
            .build();
            if picker.show_modal() == ID_OK {
                let index = picker.get_selection();
                if index >= 0 {
                    c.post(
                        "/api/desktop/resource/restore",
                        json!({"undo":rows[index as usize]["id"]}),
                        &t,
                    );
                }
            }
            picker.destroy();
        } else if let Some(undo) = c.last_undo.borrow().clone() {
            c.post("/api/desktop/resource/restore", json!({"undo":undo}), &t);
        }
    }
    fn all_tabs(&self) -> Vec<Tab> {
        let mut tabs = vec![self.skills.clone(), self.mcp.clone()];
        tabs.extend(self.client_tabs.borrow().iter().cloned());
        tabs
    }
    pub fn mount_client(&self, parent: &Panel, client: ClientId, kind: ResourceKind) {
        let mut tab = Tab::new(parent, kind);
        tab.client_filter = Some(client);
        tab.used.show(true);
        tab.available.show(true);
        tab.used.set_selected(true);
        if let Some(sizer) = parent.get_sizer() {
            sizer.add(&tab.page, 1, SizerFlag::Expand, 0);
        }
        self.wire(&tab);
        self.client_tabs.borrow_mut().push(tab);
    }
    pub fn enter(&self, id: PageId) {
        self.market
            .enter(id == PageId::Skills && self.market_visible.get());
        if matches!(
            id,
            PageId::Skills | PageId::Mcp | PageId::Client(_) | PageId::Settings
        ) {
            self.refresh(
                false,
                if id == PageId::Skills {
                    &self.skills
                } else {
                    &self.mcp
                },
            );
        }
    }
    fn refresh(&self, force: bool, tab: &Tab) {
        if !force
            && self.loaded.get()
            && self
                .scanned_at
                .get()
                .is_some_and(|at| at.elapsed() < Duration::from_secs(10))
        {
            return;
        }
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        tab.status.set_label("正在读取客户端配置…");
        self.set_enabled(false);
        self.scanned_at.set(Some(Instant::now()));
        let (api, tx) = (self.api.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let result = api.desktop_post("/api/desktop/library/discover", &json!({}));
            let _ = tx.send(GuiMessage::Resources(ResourceEvent::Result(result)));
            wxdragon::wake_up_idle();
        });
    }
    fn post(&self, path: &str, body: Value, tab: &Tab) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        if path == "/api/desktop/resource/selection" {
            *self.failed.borrow_mut() = None;
        }
        *self.last_request.borrow_mut() = Some((path.into(), body.clone()));
        self.set_enabled(false);
        tab.status.set_label("正在更新配置…");
        tab.picker.summary.set_label("正在应用更改…");
        let (api, tx, path) = (self.api.clone(), self.tx.clone(), path.to_string());
        std::thread::spawn(move || {
            let result = api.desktop_post(&path, &body);
            let _ = tx.send(GuiMessage::Resources(ResourceEvent::Result(result)));
            wxdragon::wake_up_idle();
        });
    }
    pub fn apply(&self, event: ResourceEvent) {
        self.busy.store(false, Ordering::SeqCst);
        self.set_enabled(true);
        let result = match event {
            ResourceEvent::Result(result) => result,
            ResourceEvent::Edit {
                resource_id,
                result,
            } => {
                match result {
                    Ok(value) => {
                        let server = serde_json::from_value::<McpServer>(value);
                        let view = self.view.borrow().clone();
                        if let (Ok(server), Some(view)) = (server, view) {
                            if let Some(resource) =
                                view.resources.iter().find(|r| r.resource.id == resource_id)
                            {
                                if let Some(value) = mcp_editor::show(
                                    &self.frame,
                                    self.api.clone(),
                                    &view,
                                    Some((&resource.resource, server)),
                                ) {
                                    self.apply(ResourceEvent::Result(Ok(value)));
                                }
                            }
                        }
                    }
                    Err(error) => super::show_error(&self.frame, &error),
                }
                return;
            }
        };
        match result {
            Ok(value) => {
                let view_value = value.get("view").cloned().unwrap_or_else(|| value.clone());
                match serde_json::from_value::<LibraryView>(view_value) {
                    Ok(view) => {
                        if let Some(issues) = value.get("issues") {
                            if let Ok(issues) =
                                serde_json::from_value::<Vec<DiscoveryIssue>>(issues.clone())
                            {
                                *self.issues.borrow_mut() = issues;
                            }
                        }
                        *self.view.borrow_mut() = Some(view);
                        self.loaded.set(true);
                        self.rebuild();
                        let mut messages = Vec::new();
                        let mut failed_targets = Vec::new();
                        if let Some(outcomes) = value.get("outcomes").and_then(Value::as_array) {
                            for outcome in outcomes {
                                if let Some(undo) = outcome.get("undo").and_then(Value::as_str) {
                                    *self.last_undo.borrow_mut() = Some(undo.into());
                                }
                                if outcome.get("success").and_then(Value::as_bool) == Some(false) {
                                    let target = outcome
                                        .get("targetId")
                                        .and_then(Value::as_str)
                                        .unwrap_or("");
                                    failed_targets.push(target.to_string());
                                    let label = self
                                        .view
                                        .borrow()
                                        .as_ref()
                                        .and_then(|v| {
                                            v.targets
                                                .iter()
                                                .find(|t| t.id == target)
                                                .map(|t| t.client.name())
                                        })
                                        .unwrap_or("客户端");
                                    messages.push(format!(
                                        "{}：{}",
                                        label,
                                        outcome
                                            .get("message")
                                            .and_then(Value::as_str)
                                            .unwrap_or("更新失败")
                                    ));
                                }
                            }
                        }
                        if value.get("outcomes").is_some() {
                            let request = self
                                .last_request
                                .borrow()
                                .as_ref()
                                .map(|(_, v)| v.clone())
                                .unwrap_or(Value::Null);
                            if !failed_targets.is_empty() && request.get("changes").is_none() {
                                *self.failed.borrow_mut() = Some(
                                    json!({"resourceId":value.get("resourceId").cloned().unwrap_or(request["resourceId"].clone()),"targets":failed_targets,"using":request.get("using").cloned().unwrap_or(true.into())}),
                                );
                            } else {
                                *self.failed.borrow_mut() = None;
                                if !failed_targets.is_empty() {
                                    messages.push("部分更改未完成。请重新读取后进入管理客户端，已成功的配置会保留。".into());
                                }
                            }
                        }
                        if value.get("issues").is_none() {
                            if let Some(warnings) = value["warnings"].as_array() {
                                messages.extend(
                                    warnings
                                        .iter()
                                        .filter_map(Value::as_str)
                                        .map(str::to_string),
                                );
                            }
                        }
                        let message = if messages.is_empty() {
                            String::new()
                        } else {
                            messages.join("\n")
                        };
                        for tab in &self.all_tabs() {
                            tab.status.set_label(&message);
                            tab.status.show(!message.is_empty());
                            tab.status.wrap(580);
                            if value.get("outcomes").is_some() && tab.picker.page.is_shown() {
                                tab.book.change_selection(0);
                            }
                            tab.picker.summary.set_label(&message);
                            tab.retry.set_label(if self.failed.borrow().is_some() {
                                "重试失败端"
                            } else {
                                "重试"
                            });
                            let partial_selection =
                                value.get("outcomes").and_then(Value::as_array).is_some_and(
                                    |items| items.iter().any(|o| o["success"] == false),
                                ) && self
                                    .last_request
                                    .borrow()
                                    .as_ref()
                                    .is_some_and(|(_, body)| body.get("changes").is_some());
                            if partial_selection {
                                tab.retry.set_label("重新读取");
                            }
                            tab.retry
                                .show(self.failed.borrow().is_some() || partial_selection);
                            tab.resolve.show(self.failed.borrow().is_some());
                            tab.page.layout();
                        }
                    }
                    Err(_) => {
                        for tab in &self.all_tabs() {
                            tab.status
                                .set_label("管理数据格式不匹配，请更新后台服务后重试");
                            tab.status.show(true);
                            tab.retry.show(true);
                            tab.page.layout();
                        }
                    }
                }
            }
            Err(error) => {
                for tab in &self.all_tabs() {
                    tab.status.set_label(&error);
                    tab.status.show(true);
                    tab.retry.show(true);
                    tab.status.wrap(580);
                    tab.picker.invalidate(&format!(
                        "未能确认更改结果：{error}。请重新读取后再选择；此处不会自动重复提交。"
                    ));
                    tab.page.layout();
                }
            }
        }
        if let Some(button) = *self.recovery.borrow() {
            button.enable(
                self.view
                    .borrow()
                    .as_ref()
                    .is_some_and(|v| !v.recoveries.is_empty()),
            );
        }
    }
    fn set_enabled(&self, enabled: bool) {
        if let Some(button) = *self.recovery.borrow() {
            button.enable(
                enabled
                    && self
                        .view
                        .borrow()
                        .as_ref()
                        .is_some_and(|v| !v.recoveries.is_empty()),
            );
        }
        for tab in &self.all_tabs() {
            for b in [tab.add, tab.retry, tab.scan, tab.resolve] {
                b.enable(enabled);
            }
            tab.picker.set_enabled(enabled);
            tab.issues_button.enable(enabled);
            tab.issue_list.back.enable(enabled);
            tab.issue_list.retry.enable(enabled);
            for card in tab.gallery.cards.borrow().iter() {
                card.enable(enabled);
            }
        }
    }
    fn rebuild(&self) {
        let view = self.view.borrow().clone();
        let Some(view) = view.as_ref() else {
            return;
        };
        for tab in &self.all_tabs() {
            let mut rows: Vec<_> = view
                .resources
                .iter()
                .filter(|r| r.resource.kind == tab.kind)
                .filter(|r| {
                    tab.client_filter
                        .is_none_or(|client| r.visible_for_client(client, tab.browsing.get()))
                })
                .cloned()
                .collect();
            rows.sort_by_key(|r| {
                (
                    !r.targets.iter().any(|t| {
                        t.using == Some(true) && tab.client_filter.is_none_or(|id| t.client == id)
                    }),
                    r.resource.name.to_lowercase(),
                )
            });
            *tab.shown.borrow_mut() = rows;
            self.render_tab(tab);
        }
    }
    fn selected(&self, tab: &Tab) -> Option<ResourceView> {
        let id = tab.selected.borrow();
        self.view
            .borrow()
            .as_ref()?
            .resources
            .iter()
            .find(|r| id.as_deref() == Some(&r.resource.id))
            .cloned()
    }
    fn render_tab(&self, tab: &Tab) {
        self.populate_issues(tab);
        tab.gallery.clear();
        let rows = tab.shown.borrow().clone();
        for row in rows {
            let mut targets = row
                .targets
                .iter()
                .filter(|t| tab.client_filter.is_none_or(|id| id == t.client))
                .filter(|t| t.compatible || t.managed || t.using == Some(true))
                .filter(|t| {
                    tab.client_filter.is_none()
                        || if tab.browsing.get() {
                            t.compatible && t.using != Some(true) && !t.managed
                        } else {
                            t.using == Some(true) || t.managed
                        }
                })
                .cloned()
                .collect::<Vec<_>>();
            targets.sort_by_key(|t| t.using != Some(true));
            if tab.client_filter.is_none() {
                let mut seen = HashSet::new();
                targets.retain(|t| {
                    seen.insert((
                        t.client,
                        if t.client == ClientId::WebStorm {
                            t.label.clone()
                        } else {
                            String::new()
                        },
                    ))
                });
            }
            let labels = targets
                .iter()
                .take(12)
                .map(|t| {
                    (
                        t.client,
                        if tab.client_filter.is_some() || t.client == ClientId::WebStorm {
                            t.label.clone()
                        } else {
                            super::visual::client_short(t.client).into()
                        },
                        t.using,
                    )
                })
                .collect::<Vec<_>>();
            let card = ResourceCard::new(
                &tab.gallery.page,
                &row.resource.name,
                &row.resource.description,
                tab.kind == ResourceKind::Skill,
                &labels,
                tab.client_filter.is_some(),
            );
            card.delete.show(tab.client_filter.is_none() || row.targets.iter().any(|t|tab.client_filter == Some(t.client) && (t.managed || t.using == Some(true))));
            let (c, t, id) = (self.clone(), tab.clone(), row.resource.id.clone());
            card.edit.on_click(move |_| {
                *t.selected.borrow_mut() = Some(id.clone());
                c.edit(&t);
            });
            let (c, t, id) = (self.clone(), tab.clone(), row.resource.id.clone());
            card.delete.on_click(move |_| {
                *t.selected.borrow_mut() = Some(id.clone());
                c.delete(&t);
            });
            let (c, t, id) = (self.clone(), tab.clone(), row.resource.id.clone());
            card.manage.on_click(move |_| {
                *t.selected.borrow_mut() = Some(id.clone());
                c.choose_target(&t);
            });
            card.enable(!self.busy.load(Ordering::SeqCst));
            tab.gallery.push(card);
        }
        tab.gallery.finish(if tab.client_filter.is_none() {
            "还没有添加内容"
        } else if tab.browsing.get() {
            "没有可添加的兼容内容，可导入或新建"
        } else {
            "当前客户端尚未使用内容，可从“可添加”中选择"
        });
        tab.page.layout();
    }
    fn populate_issues(&self, tab: &Tab) {
        let issues = self
            .issues
            .borrow()
            .iter()
            .filter(|i| i.kind == tab.kind && tab.client_filter.is_none_or(|id| i.client == id))
            .map(|i| IssueItem {
                title: format!("{} · {}", i.client.name(), i.name),
                reason: i.reason.clone(),
                guidance: i.guidance.clone(),
                location: i.location.clone(),
            })
            .collect::<Vec<_>>();
        tab.issues_button
            .set_label(&format!("{} 项未读取 · 查看处理方法", issues.len()));
        tab.issues_button.show(!issues.is_empty());
        tab.issue_list.populate(&issues);
    }
    fn choose_target(&self, tab: &Tab) {
        let Some(row) = self.selected(tab) else {
            return;
        };
        let view = self.view.borrow().clone();
        let Some(view) = view else {
            return;
        };
        let mut targets = row
            .targets
            .iter()
            .filter(|t| tab.client_filter.is_none_or(|id| id == t.client))
            .filter_map(|t| {
                let native = view.targets.iter().find(|n| n.id == t.target_id)?;
                let location = if tab.kind == ResourceKind::Skill {
                    native.skills.clone()
                } else {
                    native.mcp.clone()
                };
                Some(TargetOption {
                    id: t.target_id.clone(),
                    client: t.client,
                    label: t.label.clone(),
                    using: t.using,
                    compatible: t.compatible,
                    error: t.error.clone().unwrap_or_default(),
                    shared_key: if tab.kind == ResourceKind::Skill {
                        location.to_string_lossy().to_string()
                    } else {
                        String::new()
                    },
                    shared_with: t.shared_with.clone(),
                    location,
                })
            })
            .collect::<Vec<_>>();
        targets.sort_by_key(|t| t.using != Some(true));
        *tab.selection_revision.borrow_mut() = view.revision;
        tab.picker.populate(&row.resource.name, targets);
        tab.book.change_selection(1);
        tab.page.layout();
    }
    fn apply_selection(&self, tab: &Tab) {
        let Some(row) = self.selected(tab) else {
            return;
        };
        let changes = tab.picker.changes();
        if changes.is_empty() {
            return;
        }
        let sharing = tab.picker.sharing();
        if !sharing.is_empty()
            && MessageDialog::builder(
                &self.frame,
                &format!(
                    "这些客户端共用同一个 Skill 文件夹，本次更改会同时影响：{}。继续应用？",
                    sharing.join("、")
                ),
                "共享文件夹",
            )
            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
            .build()
            .show_modal()
                != ID_YES
        {
            return;
        }
        self.post("/api/desktop/resource/selection",json!({"resourceId":row.resource.id,"revision":*tab.selection_revision.borrow(),"changes":changes.into_iter().map(|(target_id,using)|json!({"targetId":target_id,"using":using})).collect::<Vec<_>>()}),tab);
    }
    fn add(&self, tab: &Tab) {
        *self.last_request.borrow_mut() = None;
        let view = self.view.borrow().clone();
        let Some(mut view) = view else {
            self.refresh(true, tab);
            return;
        };
        if let Some(client) = tab.client_filter {
            view.targets.retain(|t| t.client == client);
        }
        let value = if tab.kind == ResourceKind::Mcp {
            mcp_editor::show(&self.frame, self.api.clone(), &view, None)
        } else {
            skill_dialog(&self.frame, self.api.clone(), &view)
        };
        if let Some(value) = value {
            self.apply(ResourceEvent::Result(Ok(value)));
        }
    }
    fn edit(&self, tab: &Tab) {
        *self.last_request.borrow_mut() = None;
        let Some(row) = self.selected(tab) else {
            return;
        };
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        self.set_enabled(false);
        let (api, tx, id) = (self.api.clone(), self.tx.clone(), row.resource.id);
        std::thread::spawn(move || {
            let result = api.desktop_post("/api/desktop/mcp/read", &json!({"resourceId":id}));
            let _ = tx.send(GuiMessage::Resources(ResourceEvent::Edit {
                resource_id: id,
                result,
            }));
            wxdragon::wake_up_idle();
        });
    }
    fn delete(&self, tab: &Tab) {
        let Some(row) = self.selected(tab) else {
            return;
        };
        let revision = self
            .view
            .borrow()
            .as_ref()
            .map(|v| v.revision.clone())
            .unwrap_or_default();
        if let Some(client) = tab.client_filter {
            let targets = row.targets.iter().filter(|t|t.client == client && (t.managed || t.using != Some(false))).collect::<Vec<_>>();
            if targets.is_empty() { return; }
            if targets.iter().any(|t|t.using.is_none()) {
                self.choose_target(tab);
                tab.picker.summary.set_label("此项已被客户端修改或暂时无法读取。请先核对状态，再决定是否移除。");
                return;
            }
            let shared = targets.iter().flat_map(|t|t.shared_with.iter().cloned()).collect::<BTreeSet<_>>();
            let sharing = if shared.is_empty() { String::new() } else { format!("\n共用同一文件夹的客户端也会受影响：{}。",shared.into_iter().collect::<Vec<_>>().join("、")) };
            if MessageDialog::builder(&self.frame,
                &format!("从 {} 移除 {}？Hub 中的内容及其他独立客户端配置会保留。{}",client.name(),row.resource.name,sharing),
                "移除客户端配置").with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion).build().show_modal() != ID_YES { return; }
            self.post("/api/desktop/resource/use",json!({"resourceId":row.resource.id,
                "targets":targets.iter().map(|t|t.target_id.clone()).collect::<Vec<_>>(),"using":false}),tab);
            return;
        }
        if MessageDialog::builder(
            &self.frame,
            &format!("删除 {}？", row.resource.name),
            "删除",
        )
        .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
        .build()
        .show_modal()
            != ID_YES
        {
            return;
        }
        self.post(
            "/api/desktop/resource/delete",
            json!({"resourceId":row.resource.id,"revision":revision}),
            tab,
        );
    }
}

impl Tab {
    fn new(parent: &dyn WxWidget, kind: ResourceKind) -> Self {
        let root = Panel::builder(parent).build();
        root.set_background_color(theme::theme().bg_app);
        let book = SimpleBook::builder(&root).build();
        let page = Panel::builder(&book).build();
        page.set_background_color(theme::theme().bg_app);
        let column = BoxSizer::builder(Orientation::Vertical).build();
        let toolbar = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        let add = Button::builder(&page)
            .with_label(if kind == ResourceKind::Skill {
                "导入 Skill"
            } else {
                "添加 MCP"
            })
            .build();
        add.set_kind(ButtonKind::Primary);
        add.set_foreground_color(theme::theme().on_accent);
        let retry = Button::builder(&page)
            .with_label("重试")
            .with_kind(ButtonKind::Ghost)
            .build();
        retry.hide();
        let scan = Button::builder(&page)
            .with_label("")
            .with_icon(super::visual::Icon::Refresh)
            .with_size(Size::new(34, 34))
            .build();
        scan.set_tooltip("刷新，自动读取客户端已有配置");
        let used = Button::builder(&page)
            .with_label("使用中")
            .with_kind(ButtonKind::Tab)
            .with_size(Size::new(82, 34))
            .build();
        let available = Button::builder(&page)
            .with_label("可添加")
            .with_kind(ButtonKind::Tab)
            .with_size(Size::new(82, 34))
            .build();
        used.hide();
        available.hide();
        toolbar.add(&used, 0, SizerFlag::Right, page.from_dip(8));
        toolbar.add(&available, 0, SizerFlag::Right, page.from_dip(12));
        scan.set_kind(ButtonKind::Ghost);
        toolbar.add(&add, 0, SizerFlag::Right, 10);
        toolbar.add(&scan, 0, SizerFlag::Right, 8);
        let issues_button = Button::builder(&page)
            .with_label("未读取项目")
            .with_kind(ButtonKind::Ghost)
            .build();
        issues_button.set_foreground_color(theme::theme().warn);
        issues_button.hide();
        toolbar.add(&issues_button, 0, SizerFlag::Right, page.from_dip(8));
        toolbar.add(&retry, 0, SizerFlag::Right, 10);
        column.add_sizer(&toolbar, 0, SizerFlag::Expand | SizerFlag::All, 16);
        let gallery = Gallery::new(&page);
        column.add(
            &gallery.page,
            1,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            page.from_dip(12),
        );
        let resolve = Button::builder(&page).with_label("核对并解决冲突").build();
        resolve.hide();
        column.add(
            &resolve,
            0,
            SizerFlag::Left | SizerFlag::Bottom,
            page.from_dip(12),
        );
        let status = StaticText::builder(&page).with_label("").build();
        status.hide();
        status.set_foreground_color(theme::theme().error);
        column.add(
            &status,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            16,
        );
        page.set_sizer(column, true);
        book.add_page(&page, "列表", true, None);
        let picker = TargetPicker::new(&book);
        book.add_page(&picker.page, "管理客户端", false, None);
        let issue_list = IssueList::new(&book);
        book.add_page(&issue_list.page, "未读取的项目", false, None);
        let root_col = BoxSizer::builder(Orientation::Vertical).build();
        root_col.add(&book, 1, SizerFlag::Expand, 0);
        root.set_sizer(root_col, true);
        Self {
            page: root,
            list_page: page,
            book,
            picker,
            selection_revision: Default::default(),
            issues_button,
            issue_list,
            gallery,
            used,
            available,
            browsing: Default::default(),
            status,
            add,
            retry,
            scan,
            resolve,
            kind,
            client_filter: None,
            shown: Default::default(),
            selected: Default::default(),
        }
    }
}

fn skill_dialog(parent: &Frame, api: ApiClient, view: &LibraryView) -> Option<Value> {
    let size = super::ai_gw_channel_dialog_size(parent, Size::new(730, 520));
    let dialog = Dialog::builder(parent, "导入 Skill")
        .with_size(size.width, size.height)
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    dialog.set_background_color(theme::theme().bg_card);
    let column = BoxSizer::builder(Orientation::Vertical).build();
    let fields = FlexGridSizer::builder(0, 2)
        .with_hgap(12)
        .with_vgap(12)
        .build();
    fields.add_growable_col(1, 1);
    let path = mcp_editor::field(&dialog, &fields, "目录、ZIP 或仓库地址", "", false, 0);
    let subdirectory = mcp_editor::field(&dialog, &fields, "包内目录（可选）", "", false, 0);
    column.add_sizer(&fields, 0, SizerFlag::Expand | SizerFlag::All, 16);
    let selected = Rc::new(RefCell::new(BTreeSet::new()));
    let targets = mcp_editor::target_selector(&dialog, &view.targets, selected.clone());
    column.add(&targets, 1, SizerFlag::Expand | SizerFlag::All, 16);
    let status = StaticText::builder(&dialog).with_label("").build();
    column.add(
        &status,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        16,
    );
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    row.add_stretch_spacer(1);
    let cancel = Button::builder(&dialog).with_label("取消").build();
    let save = Button::builder(&dialog).with_label("导入并使用").build();
    row.add(&cancel, 0, SizerFlag::Right, 10);
    row.add(&save, 0, SizerFlag::default(), 0);
    column.add_sizer(&row, 0, SizerFlag::Expand | SizerFlag::All, 16);
    dialog.set_sizer(column, true);
    dialog.centre();
    cancel.on_click(move |_| dialog.end_modal(ID_CANCEL));
    let revision = view.revision.clone();
    let job = dialog_job::bind(dialog, save, status, api, move || {
        if path.get_value().trim().is_empty() {
            return Err("请选择 Skill 目录、ZIP 文件，或填写公开仓库地址".into());
        }
        Ok((
            "/api/desktop/skill/import".into(),
            json!({"path":path.get_value().trim(),"subdirectory":subdirectory.get_value(),"revision":revision,"targets":selected.borrow().iter().cloned().collect::<Vec<_>>()}),
        ))
    });
    dialog.show_modal();
    let result = job.result.borrow_mut().take();
    drop(job);
    dialog.destroy();
    result
}
