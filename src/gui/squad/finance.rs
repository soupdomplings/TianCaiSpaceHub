use super::super::controls::Button;
use super::*;

pub(super) fn invoices(frame: &Frame, api: ApiClient, scope: Value) {
    let size = super::super::ai_gw_channel_dialog_size(frame, Size::new(940, 650));
    let dialog = Dialog::builder(frame, "发票与交付文件")
        .with_size(size.width, size.height)
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    dialog.set_background_color(theme::theme().bg_app);
    dialog.set_font(&theme::font(theme::TextRole::Body));
    let column = BoxSizer::builder(Orientation::Vertical).build();
    let summary = StaticText::builder(&dialog).build();
    column.add(&summary, 0, SizerFlag::Expand | SizerFlag::All, 16);
    let table = ListCtrl::builder(&dialog)
        .with_style(ListCtrlStyle::Report | ListCtrlStyle::SingleSel | ListCtrlStyle::BorderNone)
        .build();
    table.set_background_color(theme::theme().bg_card);
    table.set_foreground_color(theme::theme().ink_primary);
    table.set_font(&theme::font(theme::TextRole::Body));
    table.on_size(move |e| {
        let width = (table.get_client_size().width - 4).max(4);
        let mut used = 0;
        for i in 0..4 {
            let w = if i == 3 {
                width - used
            } else {
                width * [36, 20, 18, 26][i as usize] / 100
            };
            table.set_column_width(i, w);
            used += w;
        }
        e.skip(true);
    });
    for (i, (label, width)) in [
        ("抬头", 250),
        ("金额（元）", 120),
        ("状态", 120),
        ("申请时间", 190),
    ]
    .iter()
    .enumerate()
    {
        table.insert_column(i as i64, label, ListColumnFormat::Left, *width);
    }
    column.add(
        &table,
        1,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        16,
    );
    let controls = WrapSizer::builder(Orientation::Horizontal)
        .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
        .build();
    let add = Button::builder(&dialog).with_label("申请开票").build();
    let details = Button::builder(&dialog).with_label("查看详情").build();
    let cancel = Button::builder(&dialog).with_label("撤销申请").build();
    let remove = Button::builder(&dialog).with_label("删除退回申请").build();
    let files = Button::builder(&dialog).with_label("交付文件").build();
    let previous = Button::builder(&dialog).with_label("上一页").build();
    let next = Button::builder(&dialog).with_label("下一页").build();
    for b in [add, details, cancel, remove, files, previous, next] {
        controls.add(&b, 0, SizerFlag::Right, 8);
    }
    column.add_sizer(&controls, 0, SizerFlag::Expand | SizerFlag::All, 16);
    let close = Button::builder(&dialog)
        .with_label("返回")
        .with_icon(super::super::visual::Icon::Back)
        .with_kind(super::super::controls::ButtonKind::Ghost)
        .build();
    close.on_click(move |_| dialog.end_modal(ID_CANCEL));
    column.add(&close, 0, SizerFlag::AlignRight | SizerFlag::All, 12);
    dialog.set_sizer(column, true);
    let data: Rc<RefCell<Value>> = Default::default();
    let rows: Rc<RefCell<Vec<Value>>> = Default::default();
    let page = Rc::new(Cell::new(1usize));
    let (a, f, d, r, s, p) = (
        api.clone(),
        *frame,
        data.clone(),
        rows.clone(),
        scope.clone(),
        page.clone(),
    );
    let load = Rc::new(move || {
        let query = vec![
            ("business_unit_id", s["business_unit_id"].to_string()),
            ("department_id", s["department_id"].to_string()),
        ];
        let Some(mut value) = dialog_job::run(
            &f,
            a.clone(),
            "读取发票",
            "/api/desktop/squad/request",
            json!({"method":"GET","path":"/invoices/overview","query":query}),
        ) else {
            return;
        };
        if p.get() > 1 {
            let Some(items) = dialog_job::run(
                &f,
                a.clone(),
                "读取申请记录",
                "/api/desktop/squad/request",
                json!({"method":"GET","path":"/invoices","query":[["business_unit_id",s["business_unit_id"].to_string()],["department_id",s["department_id"].to_string()],["page",p.get().to_string()]]}),
            ) else {
                return;
            };
            value["applications"] = items;
        }
        let sources = value["sources"].as_array().cloned().unwrap_or_default();
        let sum =
            |key: &str| sources.iter().filter_map(|v| v[key].as_i64()).sum::<i64>() as f64 / 100.;
        summary.set_label(&format!(
            "可开票 {:.2} 元    申请中 {:.2} 元    已开票 {:.2} 元",
            sum("available_cents"),
            sum("pending_cents"),
            sum("issued_cents")
        ));
        let items = value["applications"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        table.delete_all_items();
        for (i, row) in items.iter().enumerate() {
            table.insert_item(
                i as i64,
                row["profile"]["title"].as_str().unwrap_or(""),
                None,
            );
            for (j, text) in [
                format!("{:.2}", row["amount_cents"].as_f64().unwrap_or(0.) / 100.),
                invoice_status(row["status"].as_str().unwrap_or("")).to_string(),
                row["created_at"].as_str().unwrap_or("").into(),
            ]
            .iter()
            .enumerate()
            {
                table.set_item_text_by_column(i as i64, j as i32 + 1, text);
            }
        }
        previous.enable(p.get() > 1);
        next.enable(items.len() >= 50);
        *r.borrow_mut() = items;
        *d.borrow_mut() = value;
    });
    let r = rows.clone();
    let selected = Rc::new(move || {
        let i = table.get_next_item(-1, ListNextItemFlag::All, ListItemState::Selected);
        if i < 0 {
            None
        } else {
            r.borrow().get(i as usize).cloned()
        }
    });
    let (a, f, d, s, l) = (
        api.clone(),
        *frame,
        data.clone(),
        scope.clone(),
        load.clone(),
    );
    add.on_click(move |_| {
        apply_invoice(&f, a.clone(), &s, &d.borrow());
        l();
    });
    let (s, f) = (selected.clone(), *frame);
    details.on_click(move |_| {
        if let Some(row) = s() {
            forms::inspect(&f, "发票详情", &row);
        }
    });
    for (operation, b) in [("cancel", cancel), ("remove", remove)] {
        let (s, a, f, l) = (selected.clone(), api.clone(), *frame, load.clone());
        b.on_click(move |_|{let Some(row)=s()else{return;};let required=if operation=="cancel"{"pending"}else{"rejected"};if row["status"]!=required{super::super::show_info(&f,if operation=="cancel"{"只能撤销待处理申请"}else{"只能删除已退回的申请"});return;}if super::super::external_import::confirm(&f,if operation=="cancel"{"撤销此开票申请？"}else{"删除此退回申请？"}){dialog_job::run(&f,a.clone(),"更新开票申请","/api/desktop/squad/request",json!({"method":if operation=="cancel"{"PATCH"}else{"DELETE"},"path":format!("/invoices/{}",row["id"]),"body":{"status":"cancelled"}}));l();}});
    }
    let (s, a, f) = (selected.clone(), api.clone(), *frame);
    files.on_click(move |_| {
        let Some(row) = s() else {
            return;
        };
        let Some(file) = organization::choose(
            &f,
            "选择交付文件",
            row["deliveries"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            "name",
        ) else {
            return;
        };
        if file["kind"] == "link" {
            let raw = file["url"].as_str().unwrap_or("");
            if url::Url::parse(raw).is_ok_and(|u| {
                u.scheme() == "https" && u.username().is_empty() && u.password().is_none()
            }) {
                let _ = super::super::browser::open_url_in_browser(
                    super::super::text::GuiText::new(super::super::load_gui_locale()),
                    raw,
                );
            }
        } else {
            download(
                &f,
                a.clone(),
                &format!("/invoices/deliveries/{}", file["id"]),
                file["name"].as_str().unwrap_or("发票文件"),
            );
        }
    });
    for (delta, b) in [(-1i32, previous), (1, next)] {
        let (p, l) = (page.clone(), load.clone());
        b.on_click(move |_| {
            p.set((p.get() as i32 + delta).max(1) as usize);
            l();
        });
    }
    load();
    dialog.show_modal();
    dialog.destroy();
}
fn apply_invoice(frame: &Frame, api: ApiClient, scope: &Value, overview: &Value) {
    let profiles = overview["profiles"].as_array().cloned().unwrap_or_default();
    let profile = if !profiles.is_empty() {
        organization::choose(
            frame,
            "选择已保存的抬头（取消可填写新抬头）",
            &profiles
                .iter()
                .map(|p| json!({"name":p["details"]["title"],"details":p["details"]}))
                .collect::<Vec<_>>(),
            "name",
        )
        .map(|p| p["details"].clone())
    } else {
        None
    };
    let mut fields = scope.clone();
    fields["amount_cents"] = 0.into();
    fields["profile"]=profile.unwrap_or_else(||json!({"title_type":"company","title":"","tax_number":"","email":"","phone":"","address":"","bank":"","bank_account":"","remark":""}));
    fields["save_profile"] = true.into();
    fields["tax_method"] = if scope["business_unit_id"].as_u64().unwrap_or(0) > 0 {
        "next_statement"
    } else {
        "balance"
    }
    .into();
    fields["request_key"] = uuid::Uuid::new_v4().to_string().into();
    for (target, source) in [
        ("expected_calculation_version", "calculation_version"),
        ("expected_tax_rate_bps", "tax_rate_bps"),
        ("expected_item_name", "item_name"),
        ("expected_seller_profile", "seller_profile"),
    ] {
        fields[target] = overview["tax_settings"][source].clone();
    }
    let org = scope["business_unit_id"].as_u64().unwrap_or(0) > 0;
    let mut options: std::collections::BTreeMap<_, _> = [
        (
            "title_type".into(),
            vec![
                ("企业".into(), "company".into()),
                ("个人".into(), "personal".into()),
            ],
        ),
        (
            "tax_method".into(),
            vec![
                ("个人余额".into(), "balance".into()),
                ("下一期结算".into(), "next_statement".into()),
                ("组织预存余额".into(), "organization_balance".into()),
            ],
        ),
    ]
    .into_iter()
    .collect();
    if org {
        options
            .get_mut("tax_method")
            .unwrap()
            .retain(|(_, v)| v != "balance");
    } else {
        options
            .get_mut("tax_method")
            .unwrap()
            .retain(|(_, v)| v == "balance");
    }
    let available = overview["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["available_cents"].as_i64())
        .sum::<i64>();
    let rate = overview["tax_settings"]["tax_rate_bps"]
        .as_i64()
        .unwrap_or(0);
    let seller = overview["tax_settings"]["seller_profile"]["name"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let item = overview["tax_settings"]["item_name"]
        .as_str()
        .unwrap_or("技术服务费")
        .to_string();
    let f = *frame;
    forms::edit_checked(
        frame,
        api,
        "申请开票",
        "POST",
        "/invoices",
        fields,
        options,
        move |body| {
            let cents = body["amount_cents"].as_i64().unwrap_or(0);
            if cents <= 0 || cents > available {
                return Err(format!(
                    "开票金额需大于零，且不超过 {:.2} 元",
                    available as f64 / 100.
                ));
            }
            let p = &body["profile"];
            if p["title"].as_str().is_none_or(|s| s.trim().is_empty())
                || p["tax_number"].as_str().is_none_or(|s| s.trim().len() < 15)
                || p["email"].as_str().is_none_or(|s| {
                    !s.contains('@') || !s.rsplit('@').next().unwrap_or("").contains('.')
                })
            {
                return Err("请填写抬头、有效税号及接收邮箱".into());
            }
            let tax =
                crate::desktop::squad_quote::invoice_tax(cents, rate).map_err(|e| e.to_string())?;
            let method = match body["tax_method"].as_str() {
                Some("balance") => "扣个人余额",
                Some("next_statement") => "计入下一期未清账",
                Some("organization_balance") => "扣组织预存余额",
                _ => return Err("请选择税费结算方式".into()),
            };
            if !super::super::external_import::confirm(
                &f,
                &format!(
                    "抬头：{}\n销售方：{seller}\n项目：{item}\n开票金额：{:.2} 元\n税费：{:.2} 元（{method}）\n\n确认申请开票？",
                    p["title"].as_str().unwrap_or(""),
                    cents as f64 / 100.,
                    tax as f64 / 100.
                ),
            ) {
                return Err("尚未提交，可继续编辑".into());
            }
            Ok(())
        },
    );
}
fn invoice_status(raw: &str) -> &str {
    match raw {
        "pending" => "待处理",
        "processing" => "处理中",
        "issued" => "已开票",
        "rejected" => "已退回",
        "cancelled" => "已撤销",
        _ => raw,
    }
}
pub(super) fn download(frame: &Frame, api: ApiClient, path: &str, name: &str) {
    let name = std::path::Path::new(name)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("下载文件");
    let picker = FileDialog::builder(frame)
        .with_message("保存文件")
        .with_default_file(name)
        .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
        .build();
    if picker.show_modal() == ID_OK {
        if let Some(destination) = picker.get_path() {
            dialog_job::run(
                frame,
                api,
                "保存文件",
                "/api/desktop/squad/download",
                json!({"path":path,"destination":destination}),
            );
        }
    }
    picker.destroy();
}
