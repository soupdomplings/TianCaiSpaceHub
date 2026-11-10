//! Actual editable member allocation; percentages and current-period balances stay distinct.
use super::*;
type Fields = Rc<RefCell<Vec<(Panel, TextCtrl, CheckBox, StaticText)>>>;
#[derive(Clone)]
pub(crate) struct Allocation {
    pub panel: Panel,
    pub save: Button,
    pub back: Button,
    pub status: StaticText,
    data: Value,
    rows: Fields,
    custom: Rc<Cell<bool>>,
    apply: CheckBox,
    total: StaticText,
    modes: [Button; 2],
    distribute: Button,
}
impl Allocation {
    pub fn new(parent: &dyn WxWidget, data: &Value) -> Self {
        let panel = Panel::builder(parent).build();
        panel.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let heading = StaticText::builder(&panel)
            .with_label("成员额度分配")
            .build();
        heading.set_font(&theme::font(theme::TextRole::PageTitle));
        heading.set_foreground_color(theme::theme().ink_primary);
        col.add(&heading, 0, SizerFlag::Bottom, panel.from_dip(16));
        let mode_row = BoxSizer::builder(Orientation::Horizontal).build();
        let modes = ["不分配", "按比例分配"].map(|text| {
            Button::builder(&panel)
                .with_label(text)
                .with_kind(ButtonKind::Segment)
                .with_size(Size::new(126, 34))
                .build()
        });
        for b in modes {
            mode_row.add(&b, 0, SizerFlag::Right, panel.from_dip(6));
        }
        mode_row.add_stretch_spacer(1);
        let distribute = Button::builder(&panel)
            .with_label("均分未锁定成员")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(172, 34))
            .build();
        mode_row.add(&distribute, 0, SizerFlag::default(), 0);
        col.add_sizer(
            &mode_row,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(14),
        );
        let total = StaticText::builder(&panel).build();
        total.set_foreground_color(theme::theme().ink_secondary);
        col.add(
            &total,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(12),
        );
        let scroll = ScrolledWindow::builder(&panel)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        scroll.set_min_size(Size::new(0, 0));
        scroll.set_scroll_rate(0, scroll.from_dip(10));
        scroll.show_scroll_indicators(false);
        scroll.set_background_color(theme::theme().bg_app);
        let members_col = BoxSizer::builder(Orientation::Vertical).build();
        let rows: Fields = Default::default();
        let money = crate::desktop::squad_money::Money::default();
        for (i, member) in data["members"].as_array().into_iter().flatten().enumerate() {
            let host = Panel::builder(&scroll).build();
            super::super::controls::paint_surface(host, 10);
            let body = BoxSizer::builder(Orientation::Vertical).build();
            let top = BoxSizer::builder(Orientation::Horizontal).build();
            let name = member["username"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or(member["email"].as_str())
                .unwrap_or("成员");
            let caption = StaticText::builder(&host)
                .with_label(name)
                .with_style(StaticTextStyle::EllipsizeEnd)
                .build();
            caption.set_min_size(Size::new(0, -1));
            caption.set_font(&theme::font(theme::TextRole::Title));
            caption.set_foreground_color(theme::theme().ink_primary);
            caption.set_tooltip(member["email"].as_str().unwrap_or(name));
            top.add(
                &caption,
                1,
                SizerFlag::AlignCenterVertical | SizerFlag::Right,
                host.from_dip(10),
            );
            let n = member[if data["allocation_mode"] == "equal" {
                "share_basis_points"
            } else {
                "custom_share_basis_points"
            }]
            .as_u64()
            .unwrap_or(0);
            let ratio = TextCtrl::builder(&host)
                .with_value(&format!("{:.2}", n as f64 / 100.))
                .with_size(Size::new(86, 34))
                .build();
            ratio.set_name(&format!("allocation-percent-{i}"));
            ratio.set_tooltip("配置比例，最多两位小数；保存后按所选生效范围应用");
            top.add(&ratio, 0, SizerFlag::AlignCenterVertical, 0);
            let percent = StaticText::builder(&host).with_label("%").build();
            percent.set_foreground_color(theme::theme().ink_secondary);
            top.add(
                &percent,
                0,
                SizerFlag::AlignCenterVertical | SizerFlag::Left | SizerFlag::Right,
                host.from_dip(8),
            );
            let locked = CheckBox::builder(&host).with_label("锁定比例").build();
            locked.set_foreground_color(theme::theme().ink_secondary);
            locked.set_tooltip("只影响本次均分，不修改服务端权限");
            top.add(&locked, 0, SizerFlag::AlignCenterVertical, 0);
            body.add_sizer(
                &top,
                0,
                SizerFlag::Expand | SizerFlag::All,
                host.from_dip(14),
            );
            let amounts = StaticText::builder(&host)
                .with_label(&format!(
                    "本期已分配 {}    已用 {}    剩余 {}",
                    money.usd(&member["current_period_allocated_amount"]),
                    money.usd(&member["current_period_used_amount"]),
                    money.usd(&member["current_period_remaining_amount"])
                ))
                .with_style(StaticTextStyle::EllipsizeEnd)
                .build();
            amounts.set_min_size(Size::new(0, -1));
            amounts.set_foreground_color(theme::theme().ink_secondary);
            body.add(
                &amounts,
                0,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
                host.from_dip(14),
            );
            let preview = StaticText::builder(&host).build();
            preview.set_foreground_color(theme::theme().accent);
            body.add(
                &preview,
                0,
                SizerFlag::Expand | SizerFlag::All,
                host.from_dip(14),
            );
            host.set_sizer(body, false);
            members_col.add(
                &host,
                0,
                SizerFlag::Expand | SizerFlag::Bottom,
                scroll.from_dip(10),
            );
            rows.borrow_mut().push((host, ratio, locked, preview));
        }
        if rows.borrow().is_empty() {
            let empty = StaticText::builder(&scroll)
                .with_label("暂无可分配的成员")
                .build();
            empty.set_foreground_color(theme::theme().ink_secondary);
            members_col.add(&empty, 0, SizerFlag::All, scroll.from_dip(20));
        }
        scroll.set_sizer(members_col, false);
        col.add(&scroll, 1, SizerFlag::Expand, 0);
        let apply = CheckBox::builder(&panel)
            .with_label("同时应用到本期")
            .build();
        apply.set_foreground_color(theme::theme().ink_primary);
        col.add(
            &apply,
            0,
            SizerFlag::Top | SizerFlag::Bottom,
            panel.from_dip(14),
        );
        let status = StaticText::builder(&panel).build();
        status.set_foreground_color(theme::theme().error);
        col.add(&status, 0, SizerFlag::Expand, 0);
        let footer = BoxSizer::builder(Orientation::Horizontal).build();
        footer.add_stretch_spacer(1);
        let back = Button::builder(&panel)
            .with_label("返回")
            .with_icon(Icon::Back)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(96, 36))
            .build();
        let save = Button::builder(&panel)
            .with_label("保存分配")
            .with_kind(ButtonKind::Primary)
            .with_size(Size::new(112, 36))
            .build();
        footer.add(&back, 0, SizerFlag::Right, panel.from_dip(8));
        footer.add(&save, 0, SizerFlag::default(), 0);
        col.add_sizer(
            &footer,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            panel.from_dip(12),
        );
        panel.set_sizer(col, false);
        let ui = Self {
            panel,
            save,
            back,
            status,
            data: data.clone(),
            rows,
            custom: Rc::new(Cell::new(data["allocation_mode"] != "none")),
            apply,
            total,
            modes,
            distribute,
        };
        for (i, b) in ui.modes.iter().enumerate() {
            let u = ui.clone();
            b.on_click(move |_| {
                u.custom.set(i == 1);
                u.update();
            });
        }
        for (_, ratio, locked, _) in ui.rows.borrow().iter() {
            let u = ui.clone();
            ratio.on_text_changed(move |_| u.update());
            let u = ui.clone();
            locked.on_toggled(move |_| u.update());
        }
        let u = ui.clone();
        ui.distribute.on_click(move |_| u.distribute());
        ui.update();
        scroll.fit_inside();
        ui
    }
    pub fn body(&self) -> Result<Value, String> {
        super::organization_data::allocation_body(
            &self.data,
            self.custom.get(),
            &self
                .rows
                .borrow()
                .iter()
                .map(|(_, v, _, _)| v.get_value())
                .collect::<Vec<_>>(),
            self.apply.get_value(),
        )
    }
    fn update(&self) {
        self.modes[0].set_selected(!self.custom.get());
        self.modes[1].set_selected(self.custom.get());
        let rows = self.rows.borrow();
        let mut total = 0.;
        for (_, ratio, locked, preview) in rows.iter() {
            ratio.enable(self.custom.get() && !locked.get_value());
            locked.enable(self.custom.get());
            let n = if self.custom.get() {
                ratio
                    .get_value()
                    .parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite() && (0. ..=100.).contains(n))
            } else {
                Some(0.)
            };
            total += n.unwrap_or(0.);
            let amount = self.data["budget_limit_amount"]
                .as_f64()
                .zip(n)
                .map(|(budget, p)| crate::desktop::squad_money::cny(budget * p / 100.))
                .unwrap_or("—".into());
            preview.set_label(&format!("配置预计分配 {amount}"));
        }
        drop(rows);
        self.total.set_label(&format!(
            "已分配 {total:.2}%    预留 {:.2}%",
            (100. - total).max(0.)
        ));
        self.distribute.enable(self.custom.get());
        let result = self.body();
        self.save.enable(result.is_ok());
        self.status
            .set_label(result.as_ref().err().map(String::as_str).unwrap_or(""));
    }
    fn distribute(&self) {
        let rows = self.rows.borrow();
        let unlocked = rows.iter().filter(|r| !r.2.get_value()).count();
        if unlocked == 0 {
            return;
        }
        let Some(locked) = rows
            .iter()
            .filter(|r| r.2.get_value())
            .try_fold(0u64, |sum, r| {
                let n =
                    r.1.get_value()
                        .parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite() && (0. ..=100.).contains(v))?;
                sum.checked_add((n * 100.).round() as u64)
            })
        else {
            self.status.set_label("请先修正已锁定成员的比例");
            return;
        };
        if locked > 10000 {
            self.status.set_label("已锁定比例超过 100%");
            return;
        }
        let remaining = 10000 - locked;
        let mut changes = Vec::new();
        let mut index = 0;
        for (_, input, lock, _) in rows.iter() {
            if !lock.get_value() {
                let bp =
                    remaining / unlocked as u64 + u64::from(index < remaining % unlocked as u64);
                changes.push((*input, format!("{:.2}", bp as f64 / 100.)));
                index += 1;
            }
        }
        drop(rows);
        for (input, text) in changes {
            input.set_value(&text);
        }
        self.update();
    }
}
