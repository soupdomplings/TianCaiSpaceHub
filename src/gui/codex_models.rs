//! Channel-scoped editing of Codex's existing, global visible-model list.
use super::{
    controls::{Choice, TextCtrl},
    theme,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wxdragon::prelude::*;

#[derive(Clone)]
pub(super) struct ChannelModels {
    pub page: Panel,
    pub channel: Choice,
    search: TextCtrl,
    summary: StaticText,
    list: CheckListBox,
    groups: Rc<RefCell<Vec<Vec<String>>>>,
    selected: Rc<RefCell<Vec<String>>>,
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
        picker.channel.set_selection(0);
        picker.update(
            vec![vec!["saved".into(), "another".into()]],
            Some(&["orphan".into(), "saved".into()]),
        );
        picker.focus_model("saved");
        assert_eq!(picker.list.get_count(), 2);
        picker.show_discovered(0, &["new-a".into(), "new-b".into()]);
        assert_eq!(picker.list.get_count(), 4);
        assert_eq!(picker.selected_models(), ["orphan", "saved"]); // Fetching never selects candidates.
        picker.search.change_value("new-b");
        picker.render();
        assert_eq!(picker.list.get_string(0).as_deref(), Some("new-b"));
        picker.list.check(0, true);
        picker.toggle_visible(0);
        assert_eq!(picker.selected_models(), ["orphan", "saved", "new-b"]);
        picker.list.check(0, false);
        picker.toggle_visible(0);
        picker.search.change_value("");
        picker.render();
        picker.remove_model("orphan"); // Missing from the source, still removable.
        picker.remove_model("saved");
        assert!(picker.selected_models().is_empty());
        assert!(changed.borrow().is_empty());
        assert_eq!(picker.list.get_count(), 4);
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
        let list = CheckListBox::builder(&page)
            .with_style(CheckListBoxStyle::HScroll)
            .build();
        list.set_name("codex-channel-models-list");
        list.set_background_color(page.get_background_color());
        list.set_foreground_color(theme::theme().ink_primary);
        list.set_min_size(Size::new(0, list.from_dip(110)));
        list.enable(false);
        col.add(&list, 1, SizerFlag::Expand, 0);
        page.set_sizer(col, true);
        let ui = Self {
            page,
            channel,
            search,
            summary,
            list,
            groups: Default::default(),
            selected: Default::default(),
            rendered_group: Default::default(),
            changed: Default::default(),
        };
        let u = ui.clone();
        channel.on_selection_changed(move |_| u.render());
        let u = ui.clone();
        search.on_text_updated(move |_| u.render());
        let u = ui.clone();
        list.on_toggled(move |event| {
            if let Some(index) = event.get_selection() {
                u.toggle_visible(index);
            }
        });
        ui
    }
    pub fn focus_model(&self, model: &str) {
        {
            let groups = self.groups.borrow();
            let contains = |group: &Vec<String>| group.iter().any(|id| id.eq_ignore_ascii_case(model));
            let current = self.channel.get_selection().and_then(|n| groups.get(n as usize));
            if !current.is_some_and(contains)
                && let Some(index) = groups.iter().position(contains)
            {
                self.channel.set_selection(index as u32);
            }
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
        self.selected
            .borrow_mut()
            .retain(|m| !m.eq_ignore_ascii_case(model));
        self.render();
        self.notify_changed();
    }
    pub fn show_discovered(&self, index: usize, models: &[String]) {
        {
            let mut groups = self.groups.borrow_mut();
            let Some(group) = groups.get_mut(index) else {
                return;
            };
            for model in models {
                if !group.iter().any(|m| m.eq_ignore_ascii_case(model)) {
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
            *self.selected.borrow_mut() = selected.to_vec();
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
        {
            let mut selected = self.selected.borrow_mut();
            for model in models {
                if !selected.iter().any(|m| m.eq_ignore_ascii_case(model)) {
                    selected.push(model.clone());
                }
            }
        }
        self.render();
        self.notify_changed();
    }
    pub fn replace_selected_models(&self, models: &[String]) {
        *self.selected.borrow_mut() = models.to_vec();
        self.render();
        self.notify_changed();
    }
    pub fn selected_models(&self) -> Vec<String> {
        self.selected.borrow().clone()
    }
    pub fn enable(&self, enabled: bool) {
        let has_channel = self.channel.get_selection().is_some();
        self.channel.enable(enabled && has_channel);
        self.search.enable(enabled && has_channel);
        self.list.enable(enabled);
    }
    fn toggle_visible(&self, index: u32) {
        let Some(name) = self.list.get_string(index as usize) else {
            return;
        };
        {
            let mut selected = self.selected.borrow_mut();
            if self.list.is_checked(index) {
                if !selected.iter().any(|m| m.eq_ignore_ascii_case(&name)) {
                    selected.push(name);
                }
            } else {
                selected.retain(|m| !m.eq_ignore_ascii_case(&name));
            }
        }
        self.update_summary();
        self.notify_changed();
    }
    #[cfg(test)]
    pub(super) fn has_visible_candidate(&self, name: &str) -> bool {
        (0..self.list.get_count())
            .any(|i| self.list.get_string(i as usize).as_deref() == Some(name))
    }
    fn render(&self) {
        self.rendered_group.set(self.channel.get_selection());
        let group = self.channel.get_selection().unwrap_or(0) as usize;
        let query = self.search.get_value().trim().to_lowercase();
        let names = self.groups.borrow().get(group).cloned().unwrap_or_default();
        self.list.freeze();
        self.list.clear();
        {
            let selected = self.selected.borrow();
            for name in names.iter().filter(|n| n.to_lowercase().contains(&query)) {
                self.list.append(name);
                if selected.iter().any(|m| m.eq_ignore_ascii_case(name)) {
                    self.list.check(self.list.get_count() - 1, true);
                }
            }
        }
        self.list.thaw();
        self.update_summary();
        self.page.layout();
    }
    fn update_summary(&self) {
        if self.groups.borrow().is_empty() {
            self.summary
                .set_label("请先在「模型与渠道」中添加并启用厂商渠道。");
            return;
        }
        self.summary.set_label(&format!(
            "当前渠道 {} 个模型 · 共选 {} 个",
            self.list.get_count(),
            self.selected.borrow().len()
        ));
    }
}

#[cfg(all(test, windows))]
#[test]
fn direct_checks_from_empty_survive_channel_switch_save_and_reopen() {
    wxdragon::app::render_offscreen(|| {
        use crate::{ai_gateway::config::ProviderConfig, config::AppConfig};
        super::theme::init(super::theme::ThemeMode::Light);
        let frame = Frame::builder().build();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hub.toml");
        let mut config = AppConfig::default();
        config.ai_gateway.providers = vec![
            ProviderConfig { name: "one".into(), models: vec!["alpha".into(), "shared".into()], ..Default::default() },
            ProviderConfig { name: "two".into(), models: vec!["beta".into(), "shared".into()], ..Default::default() },
        ];
        let open = |config: &AppConfig| {
            let picker = ChannelModels::new(&frame);
            let (names, groups) = super::codex_model_catalog::channel_groups(&config.ai_gateway);
            for name in names {
                picker.channel.append(&name);
            }
            picker.channel.set_selection(0);
            picker.update(groups, Some(&config.ai_gateway.codex_visible_models));
            picker
        };
        let picker = open(&config);
        picker.list.check(1, true); // shared in the first channel
        picker.toggle_visible(1);
        picker.channel.set_selection(1);
        picker.render();
        picker.focus_model("shared");
        assert_eq!(picker.channel.get_selection(), Some(1));
        assert!(picker.list.is_checked(1));
        picker.list.check(0, true); // beta in the second channel
        picker.toggle_visible(0);
        assert!(super::codex_tab::prepare_visible_models(
            &mut config.ai_gateway, picker.selected_models(), Some(Vec::new())
        ).unwrap());
        config.save(&path).unwrap();
        let saved = AppConfig::load_or_default(&path).unwrap();
        assert!(saved.ai_gateway.enabled);
        assert_eq!(open(&saved).selected_models(), ["shared", "beta"]);
        let catalog = crate::ai_gateway::catalog::configured_models_response(&saved.ai_gateway);
        assert_eq!(catalog["models"][0]["slug"], "shared");
        assert_eq!(catalog["models"][1]["slug"], "beta");
        frame.destroy();
    }).unwrap();
}
