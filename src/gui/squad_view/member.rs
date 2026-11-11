//! One member detail surface with embedded editable sections, shared with native review.
use super::*;
type Change = Rc<RefCell<Option<Rc<dyn Fn(usize)>>>>;
#[derive(Clone)]
pub(crate) struct Member {
    pub panel: Panel,
    pub back: Button,
    pub save: Button,
    pub status: StaticText,
    pub reset: Button,
    pub unlock: Button,
    pub sections: Vec<(&'static str, &'static str)>,
    current: Rc<Cell<usize>>,
    tabs: Vec<Button>,
    host: Panel,
    editors: Rc<RefCell<Vec<Option<super::super::squad_form::FieldEditor>>>>,
    select: Change,
    editable: bool,
}
impl Member {
    pub fn new(parent: &dyn WxWidget, row: &Value, manage: bool, editable: bool) -> Self {
        let panel = Panel::builder(parent).build();
        panel.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let name = row["username"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("成员详情");
        let title = StaticText::builder(&panel).with_label(name).build();
        title.set_font(&theme::font(theme::TextRole::PageTitle));
        title.set_foreground_color(theme::theme().ink_primary);
        col.add(&title, 0, SizerFlag::Bottom, panel.from_dip(6));
        let info = StaticText::builder(&panel)
            .with_label(&format!(
                "{}  ·  {}",
                row["user_email"]
                    .as_str()
                    .or(row["email"].as_str())
                    .unwrap_or("—"),
                row["department_name"].as_str().unwrap_or("组织成员")
            ))
            .with_style(StaticTextStyle::EllipsizeEnd)
            .build();
        info.set_min_size(Size::new(0, -1));
        info.set_foreground_color(theme::theme().ink_secondary);
        col.add(
            &info,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(18),
        );
        let mut sections = vec![("profile", "成员资料"), ("balance", "成员余额")];
        if manage {
            sections.push(("limits", "成员限额"));
        }
        sections.push(("menu-access", "菜单权限"));
        if manage {
            sections.push(("billing-context", "扣费身份"));
        }
        let select: Change = Default::default();
        let mut tabs = Vec::new();
        let tab_row = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        for (i, (_, label)) in sections.iter().enumerate() {
            let b = Button::builder(&panel)
                .with_label(label)
                .with_kind(ButtonKind::Tab)
                .with_size(Size::new(108, 34))
                .build();
            let cb = select.clone();
            b.on_click(move |_| {
                if let Some(f) = cb.borrow().clone() {
                    f(i);
                }
            });
            tab_row.add(
                &b,
                0,
                SizerFlag::Right | SizerFlag::Bottom,
                panel.from_dip(5),
            );
            tabs.push(b);
        }
        col.add_sizer(
            &tab_row,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(10),
        );
        for b in &tabs {
            b.show(editable);
        }
        let host = Panel::builder(&panel).build();
        host.set_background_color(theme::theme().bg_card);
        host.set_min_size(Size::new(0, 0));
        host.set_sizer(BoxSizer::builder(Orientation::Vertical).build(), false);
        if !editable {
            let detail = detail::Detail::new(&host);
            detail.render("成员资料", row, Default::default());
            if let Some(s) = host.get_sizer() {
                s.add(&detail.panel, 1, SizerFlag::Expand, 0);
            }
        }
        col.add(&host, 1, SizerFlag::Expand, 0);
        let status = StaticText::builder(&panel).build();
        status.set_foreground_color(theme::theme().ink_secondary);
        col.add(
            &status,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            panel.from_dip(12),
        );
        let footer = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        let reset = Button::builder(&panel)
            .with_label("重置本期用量")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(150, 36))
            .build();
        let unlock = Button::builder(&panel)
            .with_label("解除扣费锁定")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(150, 36))
            .build();
        let back = Button::builder(&panel)
            .with_label("返回")
            .with_icon(Icon::Back)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(96, 36))
            .build();
        let save = Button::builder(&panel)
            .with_label("保存资料")
            .with_kind(ButtonKind::Primary)
            .with_size(Size::new(150, 36))
            .build();
        for b in [back, save, reset, unlock] {
            footer.add(
                &b,
                0,
                SizerFlag::Right | SizerFlag::Bottom,
                panel.from_dip(8),
            );
        }
        reset.hide();
        unlock.hide();
        save.show(editable);
        save.enable(false);
        col.add_sizer(
            &footer,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            panel.from_dip(12),
        );
        panel.set_sizer(col, false);
        let count = sections.len();
        Self {
            panel,
            back,
            save,
            status,
            reset,
            unlock,
            sections,
            current: Default::default(),
            tabs,
            host,
            editors: Rc::new(RefCell::new(vec![None; count])),
            select,
            editable,
        }
    }
    pub fn on_select(&self, f: impl Fn(usize) + 'static) {
        self.select.replace(Some(Rc::new(f)));
    }
    pub fn has(&self, i: usize) -> bool {
        self.editors.borrow().get(i).is_some_and(Option::is_some)
    }
    pub fn current(&self) -> usize {
        self.current.get()
    }
    pub fn set_loading(&self, loading: bool) {
        for tab in &self.tabs {
            tab.enable(!loading);
        }
        self.reset.enable(!loading);
        self.unlock.enable(!loading);
        if loading {
            self.save.enable(false);
            self.status.set_label("正在读取成员资料…");
        } else {
            self.select(self.current());
        }
    }
    pub fn select(&self, i: usize) {
        if i >= self.sections.len() {
            return;
        }
        self.current.set(i);
        for (index, b) in self.tabs.iter().enumerate() {
            b.set_selected(i == index);
        }
        for (index, e) in self.editors.borrow().iter().enumerate() {
            if let Some(e) = e {
                e.page.show(index == i);
            }
        }
        self.save.enable(self.editable && self.has(i));
        self.save.set_label(&format!("保存{}", self.sections[i].1));
        self.reset
            .show(self.editable && self.sections[i].0 == "limits");
        self.unlock
            .show(self.editable && self.sections[i].0 == "billing-context");
        self.panel.layout();
        self.host.layout();
        if let Some(e) = &self.editors.borrow()[i] {
            e.page.layout();
            e.page.fit_inside();
        }
    }
    pub fn populate(
        &self,
        i: usize,
        path: &str,
        fields: &Value,
        options: &std::collections::BTreeMap<String, Vec<(String, Value)>>,
    ) {
        if let Some(old) = self.editors.borrow_mut()[i].take() {
            old.page.destroy();
        }
        let e = super::super::squad_form::FieldEditor::new(
            &self.host,
            path,
            fields,
            options,
            Default::default(),
        );
        if let Some(s) = self.host.get_sizer() {
            s.add(&e.page, 1, SizerFlag::Expand, 0);
        }
        self.editors.borrow_mut()[i] = Some(e);
        self.select(i);
    }
    pub fn body(&self) -> Result<Value, String> {
        self.editors.borrow()[self.current.get()]
            .as_ref()
            .ok_or("成员资料未读取完成，请重试")?
            .body()
    }
}
