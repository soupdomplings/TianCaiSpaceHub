//! Channel-scoped editing of Codex's existing, global visible-model list.
use super::{
    controls::{Choice, TextCtrl},
    theme,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    rc::Rc,
};
use wxdragon::prelude::*;

#[derive(Clone)]
pub(super) struct ChannelModels {
    pub page: Panel,
    pub channel: Choice,
    search: TextCtrl,
    summary: StaticText,
    list: ScrolledWindow,
    grid: GridSizer,
    groups: Rc<RefCell<Vec<Vec<String>>>>,
    selected: Rc<RefCell<BTreeSet<String>>>,
    checks: Rc<RefCell<Vec<CheckBox>>>,
    enabled: Rc<Cell<bool>>,
    rendered_group: Rc<Cell<Option<u32>>>,
    changed: Rc<RefCell<Option<Rc<dyn Fn(Vec<String>)>>>>,
}

#[cfg(all(test, windows))]
#[test]
fn codex_discovery_is_visible_and_deletion_is_staged_without_filtering_other_models() {
    wxdragon::app::render_offscreen(|| {
        super::theme::init(super::theme::ThemeMode::Light);
        let frame = Frame::builder()
            .with_title("hidden synthetic Codex candidates")
            .build();
        let picker = ChannelModels::new(&frame);
        let changed: Rc<RefCell<Vec<String>>> = Default::default();
        let record = changed.clone();
        picker.on_changed(move |models| *record.borrow_mut() = models);
        picker.channel.append("合成渠道");
        picker.update(
            vec![vec![], vec!["saved".into(), "another".into()]],
            Some(&["saved".into()]),
        );
        picker.focus_model("saved");
        assert_eq!(picker.checks.borrow().len(), 2);
        picker.show_discovered(1, &["new-a".into(), "new-b".into()]);
        assert_eq!(picker.checks.borrow().len(), 4);
        assert_eq!(picker.selected_models(), vec!["saved"]); // Fetching never enables every model.
        picker.remove_model("saved");
        assert!(picker.selected_models().is_empty());
        assert!(changed.borrow().is_empty());
        assert_eq!(picker.checks.borrow().len(), 4);
        frame.destroy();
    })
    .unwrap();
}
impl ChannelModels {
    pub fn new(parent: &dyn WxWidget) -> Self {
        let page = Panel::builder(parent).build();
        page.set_background_color(theme::theme().bg_card);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let tools = BoxSizer::builder(Orientation::Horizontal).build();
        let channel = Choice::builder(&page).with_size(Size::new(240, 36)).build();
        channel.append("内置与其他模型");
        channel.set_selection(0);
        let search = TextCtrl::builder(&page).build();
        search.set_hint("搜索当前渠道模型");
        search.set_tooltip("搜索当前渠道模型");
        tools.add(&channel, 1, SizerFlag::Right, page.from_dip(10));
        tools.add(&search, 1, SizerFlag::default(), 0);
        col.add_sizer(
            &tools,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(8),
        );
        let summary = StaticText::builder(&page).build();
        summary.set_foreground_color(theme::theme().ink_secondary);
        summary.set_background_color(page.get_background_color());
        col.add(&summary, 0, SizerFlag::Bottom, page.from_dip(6));
        let list = ScrolledWindow::builder(&page)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        list.set_name("codex-channel-models-list");
        list.set_background_color(page.get_background_color());
        list.set_scroll_rate(0, list.from_dip(12));
        list.set_min_size(Size::new(0, list.from_dip(96)));
        let grid = GridSizer::builder(0, 2)
            .with_hgap(list.from_dip(16))
            .with_vgap(list.from_dip(8))
            .build();
        let list_col = BoxSizer::builder(Orientation::Vertical).build();
        list_col.add_sizer(&grid, 0, SizerFlag::Expand, 0);
        list.set_sizer(list_col, true);
        list.on_size(move |event| {
            let columns = (list.get_client_size().width / list.from_dip(280)).clamp(1, 3);
            if grid.get_cols() != columns {
                grid.set_cols(columns);
                list.layout();
                list.fit_inside();
            }
            event.skip(true);
        });
        col.add(&list, 1, SizerFlag::Expand, 0);
        page.set_sizer(col, true);
        let ui = Self {
            page,
            channel,
            search,
            summary,
            list,
            grid,
            groups: Default::default(),
            selected: Default::default(),
            checks: Default::default(),
            enabled: Default::default(),
            rendered_group: Default::default(),
            changed: Default::default(),
        };
        let u = ui.clone();
        channel.on_selection_changed(move |_| u.render());
        let u = ui.clone();
        search.on_text_updated(move |_| u.render());
        ui
    }
    pub fn focus_model(&self, model: &str) {
        if let Some(index) = self
            .groups
            .borrow()
            .iter()
            .position(|g| g.iter().any(|m| m == model))
        {
            self.channel.set_selection(index as u32);
        }
        // Focusing the configured list must not hide other candidates from
        // the selected channel, including models returned by discovery.
        self.search.change_value("");
        self.render();
    }
    pub fn on_changed(&self, changed: impl Fn(Vec<String>) + 'static) {
        *self.changed.borrow_mut() = Some(Rc::new(changed));
    }
    fn notify_changed(&self) {
        let changed = self.changed.borrow().clone();
        if let Some(changed) = changed {
            changed(self.selected_models());
        }
    }
    pub fn remove_model(&self, model: &str) {
        self.selected.borrow_mut().remove(model);
        self.render();
        self.notify_changed();
    }
    pub fn show_discovered(&self, index: usize, models: &[String]) {
        if let Some(group) = self.groups.borrow_mut().get_mut(index) {
            for model in models {
                if !group.iter().any(|m| m == model) {
                    group.push(model.clone());
                }
            }
        }
        self.channel.set_selection(index as u32);
        self.search.change_value("");
        self.render();
    }
    pub fn update(&self, groups: Vec<Vec<String>>, selected: Option<&[String]>) {
        if let Some(selected) = selected {
            *self.selected.borrow_mut() = selected.iter().cloned().collect();
        }
        let initialize = selected.is_some();
        if !initialize
            && *self.groups.borrow() == groups
            && self.rendered_group.get() == self.channel.get_selection()
        {
            return;
        }
        *self.groups.borrow_mut() = groups;
        self.render();
        if initialize {
            self.notify_changed();
        }
    }
    pub fn select_models(&self, models: &[String]) {
        self.selected.borrow_mut().extend(models.iter().cloned());
        self.render();
        self.notify_changed();
    }
    pub fn replace_selected_models(&self, models: &[String]) {
        *self.selected.borrow_mut() = models.iter().cloned().collect();
        self.render();
        self.notify_changed();
    }
    pub fn selected_models(&self) -> Vec<String> {
        self.selected.borrow().iter().cloned().collect()
    }
    pub fn enable(&self, enabled: bool) {
        self.enabled.set(enabled);
        self.channel.enable(enabled);
        self.search.enable(enabled);
        for check in self.checks.borrow().iter() {
            check.enable(enabled);
        }
    }
    fn render(&self) {
        self.rendered_group.set(self.channel.get_selection());
        let group = self.channel.get_selection().unwrap_or(0) as usize;
        let query = self.search.get_value().trim().to_lowercase();
        let names = self.groups.borrow().get(group).cloned().unwrap_or_default();
        for check in self.checks.borrow_mut().drain(..) {
            check.destroy();
        }
        for name in names.iter().filter(|n| n.to_lowercase().contains(&query)) {
            let check = CheckBox::builder(&self.list)
                .with_label(name)
                .with_value(self.selected.borrow().contains(name))
                .build();
            check.set_name(&format!("codex-visible-{name}"));
            check.set_min_size(Size::new(0, self.list.from_dip(24)));
            check.set_background_color(self.list.get_background_color());
            check.set_tooltip(name);
            check.set_foreground_color(theme::theme().ink_primary);
            check.enable(self.enabled.get());
            let (u, name) = (self.clone(), name.clone());
            check.on_toggled(move |_| {
                if check.is_checked() {
                    u.selected.borrow_mut().insert(name.clone());
                } else {
                    u.selected.borrow_mut().remove(&name);
                }
                u.update_summary();
                u.notify_changed();
            });
            self.grid.add(&check, 0, SizerFlag::Expand, 0);
            self.checks.borrow_mut().push(check);
        }
        self.update_summary();
        self.list.layout();
        self.list.fit_inside();
        self.page.layout();
    }
    fn update_summary(&self) {
        self.summary.set_label(&format!(
            "当前渠道 {} 个模型 · 共选 {} 个",
            self.checks.borrow().len(),
            self.selected.borrow().len()
        ));
    }
}
