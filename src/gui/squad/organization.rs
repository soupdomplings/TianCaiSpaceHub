//! Account-scoped organization workbench; all writes retain server permissions.
use super::super::controls::Button;
use super::*;
use crate::gui::controls::TextCtrl;

use super::super::squad_view::organization_data::{Section, sections};
type OrgSlot = Arc<Mutex<Option<(u64, Result<Value, String>)>>>;
#[derive(Clone)]
struct Workspace {
    frame: Frame,
    dialog: Dialog,
    api: ApiClient,
    unit: Value,
    menu: String,
    sections: Vec<Section>,
    selection: Rc<Cell<usize>>,
    table: ListCtrl,
    rows: Rc<RefCell<Vec<Value>>>,
    data: Rc<RefCell<Value>>,
    status: StaticText,
    serial: Rc<Cell<u64>>,
    slot: OrgSlot,
    add: Button,
    edit: Button,
    remove: Button,
    more: Button,
    search: TextCtrl,
    start: super::super::squad_view::filters::DateField,
    end: super::super::squad_view::filters::DateField,
    page: Rc<Cell<usize>>,
    next: Button,
    previous: Button,
    batch: Button,
    view: super::super::squad_view::organization::Organization,
    money: crate::desktop::squad_money::Money,
    ready: Rc<Cell<bool>>,
}
pub(super) fn show(
    frame: &Frame,
    api: ApiClient,
    menu: &str,
    unit: &Value,
    money: crate::desktop::squad_money::Money,
) {
    let title = format!("{} · 组织工作台", unit["name"].as_str().unwrap_or("组织"));
    let size = super::super::ai_gw_channel_dialog_size(frame, Size::new(1120, 740));
    let dialog = Dialog::builder(frame, &title)
        .with_size(size.width, size.height)
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    dialog.set_background_color(theme::theme().bg_app);
    dialog.set_font(&theme::font(theme::TextRole::Body));
    let entries = sections(menu);
    if entries.is_empty() {
        dialog.destroy();
        return;
    }
    let view = super::super::squad_view::organization::Organization::new(
        &dialog,
        unit["name"].as_str().unwrap_or("组织"),
        entries
            .iter()
            .map(|s| (s.name.into(), s.path.into()))
            .collect(),
    );
    let (
        table,
        status,
        add,
        edit,
        remove,
        more,
        batch,
        search,
        start,
        end,
        next,
        previous,
        details,
        reload,
    ) = (
        view.table,
        view.status,
        view.add,
        view.edit,
        view.remove,
        view.more,
        view.batch,
        view.search,
        view.start,
        view.end,
        view.next,
        view.previous,
        view.details,
        view.reload,
    );
    view.close.on_click(move |_| dialog.end_modal(ID_CANCEL));
    let outer = BoxSizer::builder(Orientation::Vertical).build();
    outer.add(
        &view.panel,
        1,
        SizerFlag::Expand | SizerFlag::All,
        dialog.from_dip(20),
    );
    dialog.set_sizer(outer, false);
    let ui = Workspace {
        frame: *frame,
        dialog,
        api,
        unit: unit.clone(),
        menu: menu.into(),
        sections: entries,
        selection: Default::default(),
        table,
        rows: Default::default(),
        data: Default::default(),
        status,
        serial: Default::default(),
        slot: Default::default(),
        add,
        edit,
        remove,
        more,
        search,
        start,
        end,
        page: Default::default(),
        next,
        previous,
        batch,
        view: view.clone(),
        money,
        ready: Default::default(),
    };
    let u = ui.clone();
    view.on_select(move |index| {
        u.selection.set(index);
        u.page.set(0);
        u.load();
    });
    let u = ui.clone();
    table.on_size(move |e| {
        u.view.fit_columns();
        e.skip(true);
    });
    let u = ui.clone();
    table.on_item_selected(move |_| u.enable_actions());
    let u = ui.clone();
    table.on_item_deselected(move |_| u.enable_actions());
    for (action, b) in [
        ("add", add),
        ("edit", edit),
        ("remove", remove),
        ("more", more),
        ("batch", batch),
    ] {
        let u = ui.clone();
        b.on_click(move |_| {
            u.action(action);
            u.load();
        });
    }
    let u = ui.clone();
    details.on_click(move |_| {
        u.details();
    });
    let u = ui.clone();
    table.on_item_activated(move |_| {
        u.details();
    });
    let u = ui.clone();
    reload.on_click(move |_| u.load());
    let u = ui.clone();
    view.filter_bar.on_apply(move || {
        u.page.set(0);
        u.load();
    });
    let u = ui.clone();
    next.on_click(move |_| {
        u.page.set(u.page.get() + 1);
        u.load();
    });
    let u = ui.clone();
    previous.on_click(move |_| {
        u.page.set(u.page.get().saturating_sub(1));
        u.load();
    });
    let timer = Timer::new(&dialog);
    let u = ui.clone();
    timer.on_tick(move |_| {
        let result = u.slot.lock().ok().and_then(|mut s| s.take());
        if let Some((serial, result)) = result {
            if serial == u.serial.get() {
                match result {
                    Ok(data) => u.render(data),
                    Err(error) => {
                        u.status
                            .set_label(&super::super::squad_browser::friendly_error(&error));
                        u.status.set_foreground_color(theme::theme().error);
                    }
                }
            }
        }
    });
    timer.start(100, false);
    ui.load();
    dialog.show_modal();
    timer.stop();
    dialog.destroy();
}
impl Workspace {
    fn section(&self) -> Section {
        self.sections[self.selection.get()]
    }
    fn prefix(&self) -> String {
        format!("/organization-admin/business-units/{}", self.unit["id"])
    }
    fn path(&self) -> String {
        format!("{}/{}", self.prefix(), self.section().path)
    }
    fn selected(&self) -> Option<Value> {
        let i = self
            .table
            .get_next_item(-1, ListNextItemFlag::All, ListItemState::Selected);
        if i < 0 {
            None
        } else {
            self.rows.borrow().get(i as usize).cloned()
        }
    }
    fn load(&self) {
        self.view.select(self.selection.get());
        if self.start.is_shown() {
            if let Err(error) = super::super::squad_view::filters::validate(
                &self.start.get_value(),
                &self.end.get_value(),
            ) {
                self.status.set_label(error);
                self.status.set_foreground_color(theme::theme().error);
                return;
            }
        }
        self.ready.set(false);
        self.view.set_metrics(&[]);
        self.view.select(self.selection.get());
        self.rows.borrow_mut().clear();
        self.table.delete_all_items();
        self.table.hide();
        self.view.summary.panel.show(true);
        self.view.summary.render(
            self.section().name,
            &json!({"message":"正在读取…"}),
            self.money,
        );
        self.previous.hide();
        self.next.hide();
        self.serial.set(self.serial.get() + 1);
        for b in [
            self.add,
            self.edit,
            self.remove,
            self.more,
            self.batch,
            self.view.details,
        ] {
            b.enable(false);
        }
        self.status.set_label("读取中…");
        self.status
            .set_foreground_color(theme::theme().ink_secondary);
        self.view.panel.layout();
        let (api, slot, serial, path, page, search, start, end) = (
            self.api.clone(),
            self.slot.clone(),
            self.serial.get(),
            self.path(),
            self.page.get() + 1,
            if self.search.is_shown() {
                self.search.get_value()
            } else {
                String::new()
            },
            if self.start.is_shown() {
                self.start.get_value()
            } else {
                String::new()
            },
            if self.end.is_shown() {
                self.end.get_value()
            } else {
                String::new()
            },
        );
        std::thread::spawn(move || {
            let mut query = vec![
                ("page", page.to_string()),
                ("page_size", "50".into()),
                ("search", search),
            ];
            for (key, value, suffix) in [
                ("start_time", start, "T00:00:00+08:00"),
                ("end_time", end, "T23:59:59+08:00"),
            ] {
                if !value.trim().is_empty() {
                    query.push((key, format!("{}{suffix}", value.trim())));
                }
            }
            let result = api.desktop_post(
                "/api/desktop/squad/request",
                &json!({"method":"GET","path":path,"query":query}),
            );
            if let Ok(mut slot) = slot.lock() {
                if slot.as_ref().is_none_or(|(old, _)| *old <= serial) {
                    *slot = Some((serial, result));
                }
            }
            wxdragon::wake_up_idle();
        });
    }
    fn render(&self, data: Value) {
        let section = self.section();
        let payload = if section.rows.is_empty() {
            &data
        } else {
            &data[section.rows]
        };
        let is_list = payload.is_array() || payload["items"].is_array();
        let rows = if section.path == "allocation-policy" {
            super::super::squad_view::organization_data::allocation_rows(&data)
        } else {
            payload
                .as_array()
                .or_else(|| payload["items"].as_array())
                .cloned()
                .unwrap_or_default()
        };
        self.view
            .set_metrics(&super::super::squad_view::organization_data::metrics(
                section.path,
                &data,
            ));
        self.table.show(is_list && !rows.is_empty());
        self.view.summary.panel.show(!is_list || rows.is_empty());
        if !is_list {
            self.view.summary.render(section.name, payload, self.money);
        } else if rows.is_empty() {
            let message = if section.path == "allocation-policy" && data["configured"] != true {
                "请先在主站配置组织额度，再分配成员额度"
            } else {
                "暂无记录"
            };
            self.view
                .summary
                .render(section.name, &json!({"message":message}), self.money);
        }
        self.table.freeze();
        self.table.clear_all();
        for (i, key) in section.columns.iter().enumerate() {
            self.table.insert_column(
                i as i64,
                forms::label(key),
                ListColumnFormat::Left,
                if i == 0 { 200 } else { 160 },
            );
        }
        for (i, row) in rows.iter().enumerate() {
            for (col, key) in section.columns.iter().enumerate() {
                let value = if matches!(*key, "starts_at" | "ends_at") {
                    row[*key]
                        .as_str()
                        .map(|s| s.chars().take(10).collect::<String>())
                        .unwrap_or("—".into())
                } else {
                    super::super::squad_view::detail::formatted(key, &row[*key], self.money)
                };
                if col == 0 {
                    self.table.insert_item(i as i64, &value, None);
                } else {
                    self.table
                        .set_item_text_by_column(i as i64, col as i32, &value);
                }
            }
            if i % 2 == 1 {
                self.table
                    .set_item_background_colour(i as i64, &theme::theme().table_row_alt);
            }
        }
        self.table.thaw();
        let manage = self.menu == "organization-admin" && self.unit["can_manage"] == true;
        let members = self.menu == "organization-admin"
            && (manage || self.unit["can_manage_members"] == true);
        self.add.show(
            members && section.path == "memberships"
                || manage && matches!(section.path, "departments" | "department-admins"),
        );
        self.edit.show(
            manage && (section.path == "departments" || section.path == "allocation-policy")
                || section.path == "settlement/prepayment" && data["can_adjust"] == true
                || manage && section.path == "settlement" && section.rows == "policies",
        );
        self.remove.show(
            members && section.path == "memberships"
                || manage && section.path == "department-admins",
        );
        self.more.show(
            section.path == "settlement/prepayment"
                || section.path == "settlement" && section.rows == "lines",
        );
        self.batch.show(
            members && section.path == "memberships"
                || manage && section.path == "member-limit-details"
                || manage && section.path == "departments",
        );
        self.add.set_label(if section.path == "memberships" {
            "添加成员"
        } else if section.path == "departments" {
            "新建部门"
        } else {
            "添加管理员"
        });
        self.batch.set_label(if section.path == "departments" {
            "调整排序"
        } else {
            "批量设置"
        });
        for b in [self.add, self.edit, self.remove, self.more, self.batch] {
            b.enable(true);
        }
        self.edit.set_label(if section.path == "allocation-policy" {
            "设置分配"
        } else if section.path == "settlement" {
            "编辑清账规则"
        } else {
            "编辑"
        });
        self.more
            .set_label(if section.path == "settlement/prepayment" {
                "充值"
            } else {
                "账单与清账"
            });
        self.ready.set(true);
        self.enable_actions();
        self.view.fit_columns();
        self.previous.enable(self.page.get() > 0);
        let total = data["total"].as_u64();
        self.next.enable(
            total
                .map(|n| n > ((self.page.get() + 1) * 50) as u64)
                .unwrap_or(rows.len() == 50),
        );
        let paginated = is_list
            && (self.page.get() > 0
                || total.is_some_and(|n| n > 50)
                || total.is_none() && rows.len() == 50);
        self.previous.show(paginated);
        self.next.show(paginated);
        self.status
            .set_foreground_color(theme::theme().ink_secondary);
        self.status
            .set_label(if paginated { "每页 50 条" } else { "" });
        *self.rows.borrow_mut() = rows;
        self.enable_actions();
        *self.data.borrow_mut() = data;
        self.dialog.layout();
    }
    fn enable_actions(&self) {
        let section = self.section();
        let selected = self.selected().is_some();
        let ready = self.ready.get();
        self.add.enable(ready);
        self.batch.enable(ready);
        self.remove.enable(ready && selected);
        self.edit.enable(
            ready
                && (!matches!(
                    section.path,
                    "departments" | "memberships" | "member-limit-details"
                ) && !(section.path == "settlement" && section.rows == "policies")
                    || selected),
        );
        self.more.enable(
            ready
                && (!matches!(
                    section.path,
                    "departments" | "groups" | "settlement" | "settlement/prepayment"
                ) || selected),
        );
        self.view.details.show(self.table.is_shown());
        self.view.details.set_label(
            if matches!(section.path, "memberships" | "member-limit-details") {
                "查看成员"
            } else {
                "查看详情"
            },
        );
        self.view.details.enable(ready && selected);
    }
    fn details(&self) {
        let Some(row) = self.selected() else {
            return;
        };
        if matches!(self.section().path, "memberships" | "member-limit-details") {
            member(
                &self.frame,
                self.api.clone(),
                &self.prefix(),
                &row,
                self.unit["can_manage"] == true,
                self.unit["can_manage"] == true || self.unit["can_manage_members"] == true,
                self.money,
            );
            self.load();
        } else {
            forms::inspect_money(&self.frame, self.section().name, &row, self.money);
        }
    }
    fn save(&self, title: &str, method: &str, path: &str, fields: Value) -> Option<Value> {
        let mut options = enum_options();
        if fields.get("recipient_user_ids").is_some() {
            let members = query(
                &self.frame,
                self.api.clone(),
                "读取接收成员",
                &format!("{}/memberships", self.prefix()),
            )?;
            let mut choices = Vec::new();
            for row in members
                .as_array()
                .or_else(|| members["items"].as_array())
                .into_iter()
                .flatten()
            {
                let id = &row["user_id"];
                if id.is_number() && !choices.iter().any(|(_, v)| v == id) {
                    choices.push((
                        format!(
                            "{} · {}",
                            row["user_email"]
                                .as_str()
                                .or_else(|| row["email"].as_str())
                                .unwrap_or("成员"),
                            row["username"].as_str().unwrap_or("")
                        ),
                        id.clone(),
                    ));
                }
            }
            options.insert("recipient_user_ids".into(), choices);
        }
        if path.ends_with("/settlement/policy") {
            options.insert(
                "period_type".into(),
                vec![
                    ("每周".into(), "weekly".into()),
                    ("每月".into(), "monthly".into()),
                ],
            );
        }
        forms::edit_money(
            &self.frame,
            self.api.clone(),
            title,
            method,
            path,
            fields,
            options,
            self.money,
        )
    }
    fn send(&self, title: &str, method: &str, path: &str, body: Value) -> Option<Value> {
        dialog_job::run(
            &self.frame,
            self.api.clone(),
            title,
            "/api/desktop/squad/request",
            json!({"method":method,"path":path,"body":body}),
        )
    }
    fn action(&self, action: &str) {
        let section = self.section();
        let row = self.selected().unwrap_or(Value::Null);
        let prefix = self.prefix();
        let data = self.data.borrow().clone();
        let path = self.path();
        match (section.path, action) {
            ("departments", "add") => {
                self.save(
                    "添加部门",
                    "POST",
                    &path,
                    json!({"name":"","code":"","description":"","sort_order":0}),
                );
            }
            ("departments", "edit") if !row.is_null() => {
                self.save(
                    "编辑部门",
                    "PUT",
                    &format!("{path}/{}", row["id"]),
                    pick(
                        &row,
                        &["name", "code", "description", "sort_order", "status"],
                    ),
                );
            }
            ("departments", "batch") => {
                reorder_departments(&self.frame, self.api.clone(), &prefix, &self.rows.borrow())
            }
            ("memberships" | "member-limit-details", "batch") => batch_members(
                &self.frame,
                self.api.clone(),
                &prefix,
                self.unit["can_manage"] == true,
                self.money,
            ),
            ("memberships" | "department-admins", "add") => {
                let Some(user) = choose_user(&self.frame, self.api.clone(), &prefix) else {
                    return;
                };
                let Some(department) = choose_department(&self.frame, self.api.clone(), &prefix)
                else {
                    return;
                };
                self.send(
                    "加入组织",
                    "POST",
                    &path,
                    json!({"user_id":user,"department_id":department}),
                );
            }
            ("memberships", "remove") if !row.is_null() => {
                if super::super::external_import::confirm(&self.frame, "移除此组织成员？") {
                    self.send(
                        "移除成员",
                        "DELETE",
                        &format!("{path}/{}", row["id"]),
                        Value::Null,
                    );
                }
            }
            ("department-admins", "remove") if !row.is_null() => {
                if super::super::external_import::confirm(&self.frame, "撤销所选部门管理员？")
                {
                    self.send(
                        "撤销管理员",
                        "DELETE",
                        &format!("{path}/{}/{}", row["department_id"], row["user_id"]),
                        Value::Null,
                    );
                }
            }
            ("memberships" | "member-limit-details", "edit") if !row.is_null() => member(
                &self.frame,
                self.api.clone(),
                &prefix,
                &row,
                self.unit["can_manage"] == true,
                self.unit["can_manage"] == true || self.unit["can_manage_members"] == true,
                self.money,
            ),
            ("allocation-policy", "edit") => {
                edit_allocation(&self.frame, self.api.clone(), &path, &data);
            }
            ("settlement", "edit") => {
                self.save("结算规则","PUT",&format!("{prefix}/settlement/policy"),json!({"period_type":row["period_type"].as_str().unwrap_or("monthly"),"boundary_day":row["boundary_day"].as_i64().unwrap_or(1),"timezone":row["timezone"].as_str().unwrap_or("Asia/Shanghai"),"grace_days":row["grace_days"].as_i64().unwrap_or(7),"enforcement_enabled":row["enforcement_enabled"].as_bool().unwrap_or(false)}));
            }
            ("settlement", "more") if !row.is_null() && section.rows == "lines" => {
                statement(&self.frame, self.api.clone(), &self.unit, &row)
            }
            ("settlement/prepayment", "edit") if !row.is_null() && data["can_adjust"] == true => {
                self.save("调整组织财务","POST",&format!("{path}/adjust"),json!({"kind":"balance","department_id":row["department_id"],"amount_cents":0,"reason":"","request_key":uuid::Uuid::new_v4().to_string()}));
            }
            ("settlement/prepayment", "more") if !row.is_null() => {
                payment::with_base(
                    &self.frame,
                    self.api.clone(),
                    json!({"order_type":"organization_prepayment","business_unit_id":self.unit["id"],"department_id":row["department_id"]}),
                );
            }
            _ => {}
        }
    }
}
pub(super) fn pick(data: &Value, keys: &[&str]) -> Value {
    keys.iter()
        .filter_map(|k| data.get(*k).map(|v| ((*k).into(), v.clone())))
        .collect::<serde_json::Map<String, Value>>()
        .into()
}
fn statement(frame: &Frame, api: ApiClient, unit: &Value, row: &Value) {
    let Some(id) = row["statement_id"].as_u64() else {
        return;
    };
    let prefix = format!(
        "/organization-admin/business-units/{}/settlement",
        unit["id"]
    );
    let options = [
        json!({"name":"账单明细","id":0}),
        json!({"name":"下载账单 PDF","id":1}),
        json!({"name":"支付未结算金额","id":2}),
        json!({"name":"使用组织预存余额抵扣","id":3}),
        json!({"name":"发票与交付文件","id":4}),
    ];
    let Some(action) = choose(frame, "所选账单", &options, "name") else {
        return;
    };
    match action["id"].as_u64() {
        Some(0) => {
            if let Some(value) = query(frame, api, "账单明细", &format!("{prefix}/statements/{id}"))
            {
                forms::inspect(frame, "账单明细", &value);
            }
        }
        Some(1) => finance::download(
            frame,
            api,
            &format!("{prefix}/statements/{id}/pdf"),
            &format!("周期账单-{id}.pdf"),
        ),
        Some(2) => {
            if row["outstanding_cents"].as_i64().unwrap_or(0) > 0 {
                payment::with_base(
                    frame,
                    api,
                    json!({"order_type":"organization_settlement","business_unit_id":unit["id"],"department_id":row["department_id"],"settlement_items":[{"statement_id":id,"department_id":row["department_id"],"amount_cents":row["outstanding_cents"]}],"amount":row["outstanding_cents"].as_f64().unwrap_or(0.)/100.}),
                );
            }
        }
        Some(3) => {
            let Some(wallets) = query(
                frame,
                api.clone(),
                "读取预存余额",
                &format!("{prefix}/prepayment"),
            ) else {
                return;
            };
            let rows = wallets["wallets"].as_array().cloned().unwrap_or_default();
            let Some(wallet) = choose(frame, "选择抵扣资金的部门", &rows, "department_name")
            else {
                return;
            };
            let amount = row["outstanding_cents"]
                .as_i64()
                .unwrap_or(0)
                .min(wallet["available_cents"].as_i64().unwrap_or(0));
            if amount <= 0 {
                super::super::show_info(frame, "此账单没有待结算金额，或所选部门预存余额不足");
                return;
            }
            let body = json!({"source_department_id":wallet["department_id"],"items":[{"statement_id":id,"department_id":row["department_id"],"amount_cents":amount}],"request_key":uuid::Uuid::new_v4().to_string()});
            if super::super::external_import::confirm(
                frame,
                &format!(
                    "使用 {} 的预存余额抵扣 {:.2} 元？",
                    wallet["department_name"].as_str().unwrap_or("所选部门"),
                    amount as f64 / 100.
                ),
            ) {
                dialog_job::run(
                    frame,
                    api,
                    "预存抵扣",
                    "/api/desktop/squad/request",
                    json!({"method":"POST","path":format!("{prefix}/prepayment/repay"),"body":body}),
                );
            }
        }
        Some(4) => finance::invoices(
            frame,
            api,
            json!({"business_unit_id":unit["id"],"department_id":row["department_id"]}),
        ),
        _ => {}
    }
}
fn edit_allocation(frame: &Frame, api: ApiClient, path: &str, data: &Value) {
    let size = super::super::ai_gw_channel_dialog_size(frame, Size::new(840, 700));
    let dialog = Dialog::builder(frame, "成员额度分配")
        .with_size(size.width, size.height)
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    dialog.set_background_color(theme::theme().bg_app);
    dialog.set_font(&theme::font(theme::TextRole::Body));
    let view = super::super::squad_view::allocation::Allocation::new(&dialog, data);
    let col = BoxSizer::builder(Orientation::Vertical).build();
    col.add(
        &view.panel,
        1,
        SizerFlag::Expand | SizerFlag::All,
        dialog.from_dip(22),
    );
    dialog.set_sizer(col, false);
    view.back.on_click(move |_| dialog.end_modal(ID_CANCEL));
    let (v, path) = (view.clone(), path.to_string());
    let job = dialog_job::bind(dialog, view.save, view.status, api, move || {
        Ok((
            "/api/desktop/squad/request".into(),
            json!({"method":"PUT","path":path,"body":v.body()?}),
        ))
    });
    dialog.show_modal();
    drop(job);
    dialog.destroy();
}
pub(super) fn enum_options() -> std::collections::BTreeMap<String, Vec<(String, Value)>> {
    [
        ("status", vec![("启用", "active"), ("停用", "disabled")]),
        (
            "allocation_mode",
            vec![
                ("不预分配", "none"),
                ("按比例分配", "custom"),
                ("平均分配", "equal"),
            ],
        ),
        (
            "group_by",
            vec![
                ("不分组", "none"),
                ("按天", "day"),
                ("按周", "week"),
                ("按月", "month"),
                ("按模型", "model"),
            ],
        ),
        (
            "budget_mode",
            vec![("组织共享", "business_unit"), ("按部门", "department")],
        ),
        (
            "effective_mode",
            vec![
                ("下期生效", "next"),
                ("本期生效", "current"),
                ("取消待生效调整", "cancel"),
            ],
        ),
        (
            "period_type",
            vec![("按周", "week"), ("按月", "month"), ("累计", "total")],
        ),
        (
            "frequency",
            vec![("每天", "daily"), ("每周", "weekly"), ("每月", "monthly")],
        ),
        ("operation", vec![("增加", "add"), ("扣减", "subtract")]),
        (
            "kind",
            vec![("预存余额", "balance"), ("开票额度", "invoice_quota")],
        ),
    ]
    .into_iter()
    .map(|(k, rows)| {
        (
            k.into(),
            rows.into_iter()
                .map(|(a, b)| (a.into(), b.into()))
                .collect(),
        )
    })
    .collect()
}
fn choose_department(frame: &Frame, api: ApiClient, prefix: &str) -> Option<Value> {
    let data = query(frame, api, "读取部门", &format!("{prefix}/departments"))?;
    choose(
        frame,
        "选择部门",
        data.as_array().or_else(|| data["items"].as_array())?,
        "name",
    )
    .map(|r| r["id"].clone())
}
fn choose_user(frame: &Frame, api: ApiClient, prefix: &str) -> Option<Value> {
    let query_text = forms::prompt(frame, "查找成员", "query", "")?;
    let data = dialog_job::run(
        frame,
        api,
        "查找成员",
        "/api/desktop/squad/request",
        json!({"method":"GET","path":format!("{prefix}/member-candidates"),"query":[["query",query_text],["limit","500"]]}),
    )?;
    let rows = data.as_array().or_else(|| data["items"].as_array())?;
    choose(frame, "选择成员", rows, "email").map(|r| r["user_id"].clone())
}
pub(super) fn choose(frame: &Frame, title: &str, rows: &[Value], name: &str) -> Option<Value> {
    if rows.is_empty() {
        super::super::show_info(frame, "没有可选条目");
        return None;
    }
    let labels = rows
        .iter()
        .map(|v| v[name].as_str().unwrap_or("条目").to_string())
        .collect::<Vec<_>>();
    forms::select(frame, title, labels, false)
        .first()
        .and_then(|i| rows.get(*i))
        .cloned()
}

fn member(
    frame: &Frame,
    api: ApiClient,
    prefix: &str,
    row: &Value,
    manage: bool,
    editable: bool,
    _money: crate::desktop::squad_money::Money,
) {
    if row["user_id"].as_u64().is_none_or(|n| n == 0) {
        super::super::show_error(frame, "成员身份不完整，请重新读取");
        return;
    }
    let size = super::super::ai_gw_channel_dialog_size(frame, Size::new(820, 660));
    let dialog = Dialog::builder(frame, "成员详情")
        .with_size(size.width, size.height)
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    dialog.set_background_color(theme::theme().bg_app);
    dialog.set_font(&theme::font(theme::TextRole::Body));
    let view = super::super::squad_view::member::Member::new(&dialog, row, manage, editable);
    let col = BoxSizer::builder(Orientation::Vertical).build();
    col.add(
        &view.panel,
        1,
        SizerFlag::Expand | SizerFlag::All,
        dialog.from_dip(22),
    );
    dialog.set_sizer(col, false);
    view.back.on_click(move |_| dialog.end_modal(ID_CANCEL));
    if !editable {
        dialog.show_modal();
        dialog.destroy();
        return;
    }
    let base = format!("{prefix}/members/{}", row["user_id"]);
    let (v, a, f, b, r) = (view.clone(), api.clone(), *frame, base.clone(), row.clone());
    let load: Rc<dyn Fn(usize, bool)> = Rc::new(move |index, force| {
        if v.has(index) && !force {
            v.select(index);
            v.status.set_label("");
            return;
        }
        let Some((suffix, _)) = v.sections.get(index) else {
            return;
        };
        let path = format!("{b}/{suffix}");
        let data = if *suffix == "profile" {
            r.clone()
        } else {
            let Some(data) = query(&f, a.clone(), "读取成员资料", &path) else {
                v.save.enable(false);
                return;
            };
            data
        };
        let mut options = enum_options();
        let fields = match *suffix {
            "profile" => json!({"username":data["username"]}),
            "balance" => json!({"balance":data["balance"]}),
            "limits" => {
                let mut fields = pick(
                    &data,
                    &[
                        "daily_enabled",
                        "daily_limit",
                        "weekly_enabled",
                        "weekly_limit",
                        "monthly_enabled",
                        "monthly_limit",
                        "gradient",
                    ],
                );
                if !data["updated_at"].is_null() {
                    fields["expected_updated_at"] = data["updated_at"].clone();
                }
                fields
            }
            "menu-access" => pick(
                &data,
                &[
                    "can_view_structure",
                    "can_view_billing",
                    "can_view_accounts",
                ],
            ),
            _ => {
                options.insert(
                    "membership_id".into(),
                    data["memberships"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|m| {
                            (
                                m["department_name"].as_str().unwrap_or("组织部门").into(),
                                m["id"].clone(),
                            )
                        })
                        .collect(),
                );
                json!({"membership_id":data["active_department_membership_id"],"locked":data["control_mode"] == "admin_locked","reason":data["lock_reason"].as_str().unwrap_or("")})
            }
        };
        v.populate(index, &path, &fields, &options);
        v.status.set_label(if *suffix == "balance" {
            "金额单位：人民币"
        } else {
            ""
        });
    });
    let l = load.clone();
    view.on_select(move |i| l(i, false));
    let (v, b) = (view.clone(), base.clone());
    let (saved, refresh) = (view.clone(), load.clone());
    let job = dialog_job::bind_open(
        dialog,
        view.save,
        view.status,
        api.clone(),
        move || {
            let suffix = v.sections[v.current()].0;
            Ok((
                "/api/desktop/squad/request".into(),
                json!({"method":if suffix == "profile" {"PATCH"} else {"PUT"},"path":format!("{b}/{suffix}"),"body":v.body()?}),
            ))
        },
        move |_| {
            if saved.sections[saved.current()].0 != "profile" {
                refresh(saved.current(), true);
            }
            saved.status.set_label("已保存");
            saved.status.set_foreground_color(theme::theme().ok);
        },
    );
    for (button, suffix, method, message) in [
        (
            view.reset,
            "limits/reset",
            "POST",
            "重置此成员本期已用额度？",
        ),
        (
            view.unlock,
            "billing-context-lock",
            "DELETE",
            "解除管理员设置的扣费身份锁定？",
        ),
    ] {
        let (f, a, b, l, v) = (
            *frame,
            api.clone(),
            base.clone(),
            load.clone(),
            view.clone(),
        );
        button.on_click(move |_| {
            if super::super::external_import::confirm(&f, message) {
                if dialog_job::run(
                    &f,
                    a.clone(),
                    "更新成员资料",
                    "/api/desktop/squad/request",
                    json!({"method":method,"path":format!("{b}/{suffix}"),"body":{}}),
                )
                .is_some()
                {
                    l(v.current(), true);
                }
            }
        });
    }
    load(0, false);
    dialog.show_modal();
    drop(job);
    dialog.destroy();
}

fn reorder_departments(frame: &Frame, api: ApiClient, prefix: &str, rows: &[Value]) {
    if rows.len() < 2 {
        super::super::show_info(frame, "至少需要两个部门才能调整排序");
        return;
    }
    let items = Rc::new(RefCell::new(rows.to_vec()));
    let size = super::super::ai_gw_channel_dialog_size(frame, Size::new(540, 460));
    let dialog = Dialog::builder(frame, "部门排序")
        .with_size(size.width, size.height)
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    let column = BoxSizer::builder(Orientation::Vertical).build();
    let list = ListBox::builder(&dialog)
        .with_choices(
            rows.iter()
                .map(|r| r["name"].as_str().unwrap_or("部门").into())
                .collect(),
        )
        .build();
    list.set_selection(0, true);
    column.add(&list, 1, SizerFlag::Expand | SizerFlag::All, 16);
    let controls = BoxSizer::builder(Orientation::Horizontal).build();
    for (label, delta) in [("上移", -1isize), ("下移", 1)] {
        let button = Button::builder(&dialog).with_label(label).build();
        controls.add(&button, 0, SizerFlag::Right, 8);
        let items = items.clone();
        button.on_click(move |_| {
            let Some(index) = list.get_selection().map(|n| n as usize) else {
                return;
            };
            let target = index as isize + delta;
            let mut rows = items.borrow_mut();
            if target < 0 || target >= rows.len() as isize {
                return;
            }
            rows.swap(index, target as usize);
            list.clear();
            for row in rows.iter() {
                list.append(row["name"].as_str().unwrap_or("部门"));
            }
            list.set_selection(target as u32, true);
        });
    }
    let save = Button::builder(&dialog).with_label("保存排序").build();
    controls.add(&save, 0, SizerFlag::default(), 0);
    column.add_sizer(&controls, 0, SizerFlag::All, 16);
    let status = StaticText::builder(&dialog).build();
    column.add(&status, 0, SizerFlag::Expand | SizerFlag::All, 16);
    dialog.set_sizer(column, true);
    let path = format!("{prefix}/departments/reorder");
    let job = dialog_job::bind(dialog, save, status, api, move || {
        Ok((
            "/api/desktop/squad/request".into(),
            json!({"method":"PUT","path":path,"body":{"ordered_ids":items.borrow().iter().map(|r|r["id"].clone()).collect::<Vec<_>>()}}),
        ))
    });
    dialog.show_modal();
    drop(job);
    dialog.destroy();
}
fn batch_members(
    frame: &Frame,
    api: ApiClient,
    prefix: &str,
    manage: bool,
    money: crate::desktop::squad_money::Money,
) {
    let actions = if manage {
        vec![
            "批量加入部门",
            "批量设置限额",
            "批量重置用量",
            "批量调整余额",
            "批量菜单权限",
        ]
    } else {
        vec!["批量加入部门", "批量调整余额", "批量菜单权限"]
    };
    let options = actions
        .iter()
        .enumerate()
        .map(|(i, name)| json!({"name":name,"id":i}))
        .collect::<Vec<_>>();
    let Some(action) = choose(frame, "成员批量设置", &options, "name") else {
        return;
    };
    let action = action["name"].as_str().unwrap_or("");
    let rows = if action == "批量加入部门" {
        let term = forms::prompt(frame, "查找要加入的成员", "query", "");
        let Some(term) = term else {
            return;
        };
        let Some(data) = dialog_job::run(
            frame,
            api.clone(),
            "查找成员",
            "/api/desktop/squad/request",
            json!({"method":"GET","path":format!("{prefix}/member-candidates"),"query":[["query",term],["limit","500"]]}),
        ) else {
            return;
        };
        data.as_array()
            .or_else(|| data["items"].as_array())
            .cloned()
            .unwrap_or_default()
    } else {
        let Some(data) = query(
            frame,
            api.clone(),
            "读取成员",
            &format!("{prefix}/memberships"),
        ) else {
            return;
        };
        let mut rows = Vec::new();
        for row in data
            .as_array()
            .or_else(|| data["items"].as_array())
            .into_iter()
            .flatten()
        {
            if !rows.iter().any(|r: &Value| r["user_id"] == row["user_id"]) {
                rows.push(row.clone());
            }
        }
        rows
    };
    if rows.is_empty() {
        super::super::show_info(frame, "没有可选成员");
        return;
    }
    let labels = rows
        .iter()
        .map(|r| {
            format!(
                "{} · {}",
                r["user_email"]
                    .as_str()
                    .or_else(|| r["email"].as_str())
                    .unwrap_or("成员"),
                r["username"].as_str().unwrap_or("")
            )
        })
        .collect::<Vec<_>>();
    let ids = forms::select(frame, action, labels, true)
        .into_iter()
        .filter_map(|i| rows.get(i).map(|r| r["user_id"].clone()))
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return;
    }
    if action == "批量加入部门" {
        let Some(department) = choose_department(frame, api.clone(), prefix) else {
            return;
        };
        let body = json!({"items":ids.iter().map(|id|json!({"user_id":id,"department_id":department})).collect::<Vec<_>>()});
        if super::super::external_import::confirm(
            frame,
            &format!("将 {} 位成员加入所选部门？", ids.len()),
        ) {
            if let Some(result) = dialog_job::run(
                frame,
                api,
                "批量加入部门",
                "/api/desktop/squad/request",
                json!({"method":"POST","path":format!("{prefix}/memberships/batch"),"body":body}),
            ) {
                forms::inspect(frame, "批量加入结果", &result);
            }
        }
        return;
    }
    if action == "批量重置用量" {
        if super::super::external_import::confirm(
            frame,
            &format!("重置所选 {} 位成员本期已用额度？", ids.len()),
        ) {
            dialog_job::run(
                frame,
                api,
                action,
                "/api/desktop/squad/request",
                json!({"method":"POST","path":format!("{prefix}/members/limits/reset-batch"),"body":{"user_ids":ids}}),
            );
        }
        return;
    }
    let (suffix, method, mut body) = match action {
        "批量设置限额" => (
            "limits/batch",
            "PUT",
            json!({"daily_enabled":false,"daily_limit":0.0,"weekly_enabled":false,"weekly_limit":0.0,"monthly_enabled":false,"monthly_limit":0.0}),
        ),
        "批量调整余额" => (
            "balance/adjust-batch",
            "POST",
            json!({"operation":"add","amount":0.0}),
        ),
        _ => (
            "menu-access/batch",
            "PUT",
            json!({"can_view_structure":false,"can_view_billing":false,"can_view_accounts":false}),
        ),
    };
    body["user_ids"] = ids.clone().into();
    let f = *frame;
    let title = action.to_string();
    forms::edit_checked_money(
        frame,
        api,
        action,
        method,
        &format!("{prefix}/members/{suffix}"),
        body,
        enum_options(),
        money,
        move |_| {
            if super::super::external_import::confirm(
                &f,
                &format!("{}将应用到所选 {} 位成员，确认保存？", title, ids.len()),
            ) {
                Ok(())
            } else {
                Err("尚未提交，可继续编辑".into())
            }
        },
    );
}
