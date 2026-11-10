//! Resource assignment is a draft until the user applies it.
use super::{
    controls::{Button, ButtonKind, paint_surface},
    theme,
    visual::{self, Icon},
};
use crate::desktop::clients::ClientId;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    path::PathBuf,
    rc::Rc,
};
use wxdragon::prelude::*;

#[derive(Clone)]
pub(super) struct TargetOption {
    pub id: String,
    pub client: ClientId,
    pub label: String,
    pub using: Option<bool>,
    pub compatible: bool,
    pub error: String,
    pub shared_key: String,
    pub shared_with: Vec<String>,
    pub location: PathBuf,
}
#[derive(Clone)]
pub(super) struct TargetPicker {
    pub page: Panel,
    pub back: Button,
    pub apply: Button,
    pub reload: Button,
    pub summary: StaticText,
    title: StaticText,
    list: ScrolledWindow,
    grid: GridSizer,
    advanced: Button,
    files: ScrolledWindow,
    file_rows: BoxSizer,
    entries: Rc<RefCell<Vec<(TargetOption, Button)>>>,
    selected: Rc<RefCell<BTreeSet<String>>>,
    file_cards: Rc<RefCell<Vec<Panel>>>,
    enabled: Rc<Cell<bool>>,
    valid: Rc<Cell<bool>>,
}
impl TargetPicker {
    pub fn new(parent: &dyn WxWidget) -> Self {
        let page = Panel::builder(parent).build();
        page.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let heading = BoxSizer::builder(Orientation::Horizontal).build();
        let back = Button::builder(&page)
            .with_icon(Icon::Back)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(34, 34))
            .build();
        back.set_tooltip("返回列表，放弃未应用的选择");
        let title = StaticText::builder(&page).with_label("管理客户端").build();
        title.set_foreground_color(theme::theme().ink_primary);
        title.set_font(&theme::font(theme::TextRole::Title));
        heading.add(
            &back,
            0,
            SizerFlag::Right | SizerFlag::AlignCenterVertical,
            page.from_dip(8),
        );
        heading.add(&title, 1, SizerFlag::AlignCenterVertical, 0);
        col.add_sizer(
            &heading,
            0,
            SizerFlag::Expand | SizerFlag::All,
            page.from_dip(12),
        );
        let hint = StaticText::builder(&page)
            .with_label("选择使用的客户端，再应用更改。取消选择表示停用。")
            .build();
        hint.set_foreground_color(theme::theme().ink_secondary);
        hint.wrap(page.from_dip(500));
        col.add(
            &hint,
            0,
            SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            page.from_dip(16),
        );
        let list = ScrolledWindow::builder(&page)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        list.set_background_color(theme::theme().bg_app);
        list.set_scroll_rate(0, list.from_dip(12));
        list.set_min_size(visual::dip_size(&list, 0, 120));
        list.set_max_size(visual::dip_size(&list, 600, -1));
        let grid = GridSizer::builder(0, 3)
            .with_hgap(list.from_dip(10))
            .with_vgap(list.from_dip(10))
            .build();
        let list_column = BoxSizer::builder(Orientation::Vertical).build();
        list_column.add_sizer(&grid, 0, SizerFlag::Expand, 0);
        list_column.add_stretch_spacer(1);
        list.set_sizer(list_column, true);
        col.add(
            &list,
            1,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            page.from_dip(16),
        );
        list.on_size(move |e| {
            grid.set_cols((list.get_client_size().width / list.from_dip(150)).clamp(2, 4));
            list.layout();
            list.fit_inside();
            e.skip(true);
        });
        let advanced = Button::builder(&page)
            .with_label("查看配置文件")
            .with_kind(ButtonKind::Ghost)
            .build();
        advanced.set_name("resource-config-files");
        advanced.set_tooltip("只查看文件位置，不改变客户端配置");
        col.add(
            &advanced,
            0,
            SizerFlag::Left | SizerFlag::Top,
            page.from_dip(12),
        );
        let files = ScrolledWindow::builder(&page)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        files.set_name("resource-config-details");
        files.set_background_color(theme::theme().bg_app);
        files.set_min_size(visual::dip_size(&files, 0, 120));
        files.set_scroll_rate(0, files.from_dip(12));
        let file_rows = BoxSizer::builder(Orientation::Vertical).build();
        files.set_sizer(file_rows.clone(), true);
        files.hide();
        col.add(
            &files,
            1,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            page.from_dip(16),
        );
        let summary = StaticText::builder(&page).with_label("").build();
        summary.set_foreground_color(theme::theme().ink_secondary);
        col.add(
            &summary,
            0,
            SizerFlag::Expand | SizerFlag::All,
            page.from_dip(16),
        );
        let actions = BoxSizer::builder(Orientation::Horizontal).build();
        let reload = Button::builder(&page)
            .with_label("重新读取")
            .with_kind(ButtonKind::Ghost)
            .build();
        reload.set_tooltip("放弃当前选择并读取最新状态");
        let apply = Button::builder(&page)
            .with_label("应用更改")
            .with_kind(ButtonKind::Primary)
            .build();
        apply.enable(false);
        actions.add(&reload, 0, SizerFlag::Right, page.from_dip(8));
        actions.add_stretch_spacer(1);
        actions.add(&apply, 0, SizerFlag::default(), 0);
        col.add_sizer(
            &actions,
            0,
            SizerFlag::Expand | SizerFlag::All,
            page.from_dip(16),
        );
        page.set_sizer(col, true);
        let view = Self {
            page,
            back,
            apply,
            reload,
            summary,
            title,
            list,
            grid,
            advanced,
            files,
            file_rows,
            entries: Default::default(),
            selected: Default::default(),
            file_cards: Default::default(),
            enabled: Rc::new(Cell::new(true)),
            valid: Rc::new(Cell::new(true)),
        };
        let v = view.clone();
        advanced.on_click(move |_| {
            let show_files = !v.files.is_shown();
            v.files.show(show_files);
            v.list.show(!show_files);
            v.advanced.set_label(if show_files {
                "返回客户端选择"
            } else {
                "查看配置文件"
            });
            v.advanced.set_name("resource-config-files");
            v.page.layout();
        });
        view
    }
    pub fn populate(&self, name: &str, options: Vec<TargetOption>) {
        self.title.set_label(&format!("{name} · 管理客户端"));
        self.title.set_tooltip(name);
        self.title.wrap(self.page.from_dip(520));
        self.list.freeze();
        for (_, button) in self.entries.borrow_mut().drain(..) {
            button.destroy();
        }
        for card in self.file_cards.borrow_mut().drain(..) {
            card.destroy();
        }
        self.selected.borrow_mut().clear();
        self.valid.set(true);
        for option in options {
            let label = if option.label == "默认" {
                visual::client_short(option.client).to_string()
            } else {
                format!("{} · {}", visual::client_short(option.client), option.label)
            };
            let button = Button::builder(&self.list)
                .with_label(&label)
                .with_icon(Icon::client(option.client))
                .with_kind(ButtonKind::Tile)
                .with_size(Size::new(132, 66))
                .build();
            button.set_name(&format!("target-{}", option.id));
            button.set_tooltip(&format!(
                "{}\n{}",
                label,
                if !option.compatible {
                    "当前客户端不支持此项"
                } else if option.using.is_none() {
                    &option.error
                } else if option.using == Some(true) {
                    "使用中"
                } else {
                    "未使用"
                }
            ));
            button.enable(self.enabled.get() && option.compatible && option.using.is_some());
            if option.using == Some(true) {
                self.selected.borrow_mut().insert(option.id.clone());
            }
            button.set_selected(option.using == Some(true));
            let v = self.clone();
            let id = option.id.clone();
            let shared = option.shared_key.clone();
            button.on_click(move |_| {
                let using = !v.selected.borrow().contains(&id);
                for (target, b) in v.entries.borrow().iter() {
                    if target.compatible
                        && target.using.is_some()
                        && (target.id == id || !shared.is_empty() && target.shared_key == shared)
                    {
                        if using {
                            v.selected.borrow_mut().insert(target.id.clone());
                        } else {
                            v.selected.borrow_mut().remove(&target.id);
                        }
                        b.set_selected(using);
                    }
                }
                v.update_summary();
            });
            self.grid.add(&button, 0, SizerFlag::Expand, 0);
            let card = Panel::builder(&self.files).build();
            paint_surface(card, 8);
            let column = BoxSizer::builder(Orientation::Vertical).build();
            let title = StaticText::builder(&card)
                .with_label(&format!(
                    "{label}{}",
                    if option.using.is_none() {
                        " · 需核对"
                    } else {
                        ""
                    }
                ))
                .build();
            title.set_foreground_color(theme::theme().ink_primary);
            column.add(&title, 0, SizerFlag::All, card.from_dip(6));
            let path = StaticText::builder(&card)
                .with_label(&option.location.to_string_lossy())
                .with_style(StaticTextStyle::EllipsizeEnd)
                .build();
            path.set_foreground_color(theme::theme().ink_secondary);
            path.set_tooltip(&option.location.to_string_lossy());
            path.set_min_size(Size::new(0, -1));
            column.add(
                &path,
                0,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
                card.from_dip(6),
            );
            if !option.error.is_empty() {
                let error = StaticText::builder(&card).with_label(&option.error).build();
                error.set_foreground_color(theme::theme().warn);
                error.wrap(card.from_dip(440));
                column.add(
                    &error,
                    0,
                    SizerFlag::Expand | SizerFlag::All,
                    card.from_dip(6),
                );
            }
            let open = Button::builder(&card)
                .with_label("打开所在文件夹")
                .with_kind(ButtonKind::Ghost)
                .build();
            let location = option.location.clone();
            open.enable(location.is_absolute());
            open.on_click(move |_| {
                if let Err(error) = open_directory(&location) {
                    path.set_label(&error);
                }
            });
            column.add(&open, 0, SizerFlag::All, card.from_dip(6));
            card.set_sizer(column, true);
            self.file_rows.add(
                &card,
                0,
                SizerFlag::Expand | SizerFlag::Bottom,
                self.files.from_dip(8),
            );
            self.file_cards.borrow_mut().push(card);
            self.entries.borrow_mut().push((option, button));
        }
        self.files.hide();
        self.list.show(true);
        self.advanced.set_label("查看配置文件");
        self.advanced.set_name("resource-config-files");
        self.list.layout();
        self.list.fit_inside();
        self.list.thaw();
        self.files.layout();
        self.files.fit_inside();
        self.update_summary();
        self.page.layout();
    }
    pub fn changes(&self) -> Vec<(String, bool)> {
        self.entries
            .borrow()
            .iter()
            .filter_map(|(t, _)| {
                let baseline = t.using?;
                let desired = self.selected.borrow().contains(&t.id);
                (t.compatible && baseline != desired).then(|| (t.id.clone(), desired))
            })
            .collect()
    }
    pub fn sharing(&self) -> Vec<String> {
        let changed = self.changes();
        self.entries
            .borrow()
            .iter()
            .filter(|(t, _)| changed.iter().any(|c| c.0 == t.id))
            .flat_map(|(t, _)| t.shared_with.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    fn update_summary(&self) {
        let changes = self.changes();
        let blocked = self
            .entries
            .borrow()
            .iter()
            .filter(|(t, _)| t.using.is_none())
            .count();
        self.summary.set_label(&if blocked > 0 {
            format!(
                "{} 项更改 · {blocked} 个位置需核对，可展开配置文件查看原因",
                changes.len()
            )
        } else if changes.is_empty() {
            "尚无更改".into()
        } else {
            format!(
                "{} 个位置将使用，{} 个位置将停用",
                changes.iter().filter(|t| t.1).count(),
                changes.iter().filter(|t| !t.1).count()
            )
        });
        self.summary.wrap(self.page.from_dip(520));
        self.apply
            .enable(self.enabled.get() && self.valid.get() && !changes.is_empty());
    }
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.set(enabled);
        for button in [self.back, self.reload, self.advanced] {
            button.enable(enabled);
        }
        for (option, button) in self.entries.borrow().iter() {
            button
                .enable(enabled && self.valid.get() && option.compatible && option.using.is_some());
        }
        self.apply
            .enable(enabled && self.valid.get() && !self.changes().is_empty());
    }
    pub fn invalidate(&self, reason: &str) {
        self.valid.set(false);
        self.set_enabled(true);
        self.summary.set_label(reason);
        self.summary.wrap(self.page.from_dip(520));
        self.page.layout();
    }
}

pub(super) fn open_directory(path: &std::path::Path) -> Result<(), String> {
    let mut directory = path.to_path_buf();
    while !directory.is_dir() {
        if !directory.pop() {
            return Err("找不到所在文件夹".into());
        }
    }
    #[cfg(windows)]
    let result = std::process::Command::new("explorer.exe")
        .arg(&directory)
        .spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(&directory).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let result: std::io::Result<std::process::Child> =
        Err(std::io::Error::other("unsupported platform"));
    result
        .map(|_| ())
        .map_err(|_| "无法打开文件夹，请核对路径是否仍存在".into())
}

#[derive(Clone)]
pub(super) struct IssueItem {
    pub title: String,
    pub reason: String,
    pub guidance: String,
    pub location: PathBuf,
}
#[derive(Clone)]
pub(super) struct IssueList {
    pub page: Panel,
    pub back: Button,
    pub retry: Button,
    scroll: ScrolledWindow,
    rows: BoxSizer,
    cards: Rc<RefCell<Vec<Panel>>>,
}
impl IssueList {
    pub fn new(parent: &dyn WxWidget) -> Self {
        let page = Panel::builder(parent).build();
        page.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let heading = BoxSizer::builder(Orientation::Horizontal).build();
        let back = Button::builder(&page)
            .with_icon(Icon::Back)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(34, 34))
            .build();
        back.set_tooltip("返回列表");
        let title = StaticText::builder(&page)
            .with_label("未读取的项目")
            .build();
        title.set_font(&theme::font(theme::TextRole::Title));
        title.set_foreground_color(theme::theme().ink_primary);
        let retry = Button::builder(&page)
            .with_label("重新读取")
            .with_kind(ButtonKind::Ghost)
            .build();
        heading.add(
            &back,
            0,
            SizerFlag::Right | SizerFlag::AlignCenterVertical,
            page.from_dip(8),
        );
        heading.add(&title, 1, SizerFlag::AlignCenterVertical, 0);
        heading.add(&retry, 0, SizerFlag::AlignCenterVertical, 0);
        col.add_sizer(
            &heading,
            0,
            SizerFlag::Expand | SizerFlag::All,
            page.from_dip(12),
        );
        let hint = StaticText::builder(&page)
            .with_label("以下项目暂未纳入 Hub 管理，原客户端文件未被修改。")
            .build();
        hint.set_foreground_color(theme::theme().ink_secondary);
        hint.wrap(page.from_dip(500));
        col.add(
            &hint,
            0,
            SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            page.from_dip(16),
        );
        let scroll = ScrolledWindow::builder(&page)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        scroll.set_background_color(theme::theme().bg_app);
        scroll.set_scroll_rate(0, scroll.from_dip(12));
        scroll.set_min_size(Size::new(0, 0));
        let rows = BoxSizer::builder(Orientation::Vertical).build();
        scroll.set_sizer(rows.clone(), true);
        col.add(
            &scroll,
            1,
            SizerFlag::Expand | SizerFlag::All,
            page.from_dip(12),
        );
        page.set_sizer(col, true);
        Self {
            page,
            back,
            retry,
            scroll,
            rows,
            cards: Default::default(),
        }
    }
    pub fn populate(&self, items: &[IssueItem]) {
        self.scroll.freeze();
        for card in self.cards.borrow_mut().drain(..) {
            card.destroy();
        }
        if items.is_empty() {
            let card = Panel::builder(&self.scroll).build();
            card.set_background_color(theme::theme().bg_app);
            let col = BoxSizer::builder(Orientation::Vertical).build();
            let text = StaticText::builder(&card)
                .with_label("未读取项目已处理，可返回列表继续使用")
                .build();
            text.set_foreground_color(theme::theme().ink_primary);
            col.add(&text, 0, SizerFlag::All, card.from_dip(16));
            card.set_sizer(col, true);
            self.rows.add(&card, 0, SizerFlag::Expand, 0);
            self.cards.borrow_mut().push(card);
        }
        for item in items {
            let card = Panel::builder(&self.scroll).build();
            paint_surface(card, 10);
            let col = BoxSizer::builder(Orientation::Vertical).build();
            for (text, role) in [
                (&item.title, theme::TextRole::Title),
                (&item.reason, theme::TextRole::Body),
                (&item.guidance, theme::TextRole::Body),
            ] {
                let label = StaticText::builder(&card).with_label(text).build();
                label.set_foreground_color(theme::theme().ink_primary);
                label.set_font(&theme::font(role));
                label.wrap(card.from_dip(500));
                col.add(
                    &label,
                    0,
                    SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
                    card.from_dip(14),
                );
            }
            let details = StaticText::builder(&card)
                .with_label(&item.location.to_string_lossy())
                .with_style(StaticTextStyle::EllipsizeEnd)
                .build();
            details.set_tooltip(&item.location.to_string_lossy());
            details.set_foreground_color(theme::theme().ink_secondary);
            details.set_min_size(Size::new(0, -1));
            col.add(
                &details,
                0,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
                card.from_dip(14),
            );
            let open = Button::builder(&card)
                .with_label("打开所在文件夹")
                .with_kind(ButtonKind::Ghost)
                .build();
            open.enable(item.location.is_absolute());
            let location = item.location.clone();
            let hint = details;
            open.on_click(move |_| {
                if let Err(error) = open_directory(&location) {
                    hint.set_label(&error);
                }
            });
            col.add(&open, 0, SizerFlag::All, card.from_dip(10));
            card.set_sizer(col, true);
            self.rows.add(
                &card,
                0,
                SizerFlag::Expand | SizerFlag::Bottom,
                self.scroll.from_dip(12),
            );
            self.cards.borrow_mut().push(card);
        }
        self.scroll.layout();
        self.scroll.fit_inside();
        self.scroll.thaw();
        self.page.layout();
    }
}
