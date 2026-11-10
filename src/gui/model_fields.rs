//! Shared model workspace for VS Code, Harness, ZCode and WebStorm. This is
//! the production form, also usable in hidden native layout checks.
use super::{
    controls::{Button, ButtonKind, Choice, TextCtrl, paint_surface},
    theme,
};
use crate::desktop::clients::ClientId;
use serde_json::{Value, json};
use wxdragon::prelude::*;

/// Owns the actual nested model page. Relayout must start here when switching
/// editors: laying out the outer client page leaves an unchanged book page at
/// its old child sizes (the previously hidden editor may still be zero-sized).
#[derive(Clone)]
pub(super) struct ModelPage {
    pub page: Panel,
    pub add: Button,
    pub back: Button,
    pub remove_saved: Button,
    pub workspace: ModelWorkspace,
    pub legacy_host: Panel,
    legacy: bool,
    codex: bool,
}
impl ModelPage {
    pub fn new(parent: &dyn WxWidget, client: ClientId) -> Self {
        let page = Panel::builder(parent).build();
        page.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let actions = BoxSizer::builder(Orientation::Horizontal).build();
        let add = Button::builder(&page)
            .with_label("添加模型")
            .with_icon(super::visual::Icon::Plus)
            .with_kind(ButtonKind::Secondary)
            .build();
        add.set_tooltip("选择已有渠道或其他客户端的模型，保存后用于当前客户端");
        let back = Button::builder(&page)
            .with_label("返回已有模型")
            .with_icon(super::visual::Icon::Back)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(144, 36))
            .build();
        back.set_tooltip("返回原模型列表，新增草稿保留；放弃新增不会保存");
        actions.add(&add, 0, SizerFlag::Right, page.from_dip(8));
        actions.add(&back, 0, SizerFlag::default(), 0);
        let remove_saved = Button::builder(&page)
            .with_label("删除模型")
            .with_icon(super::visual::Icon::WindowClose)
            .with_kind(ButtonKind::Ghost)
            .build();
        remove_saved.show(client == ClientId::Codex);
        remove_saved.set_tooltip("从当前客户端模型清单移除，点击保存模型列表后生效");
        actions.add_stretch_spacer(1);
        actions.add(&remove_saved, 0, SizerFlag::default(), 0);
        col.add_sizer(
            &actions,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(12),
        );
        let workspace = ModelWorkspace::new(&page, client);
        col.add(&workspace.page, 1, SizerFlag::Expand, 0);
        let codex = client == ClientId::Codex;
        let legacy_host = Panel::builder(if codex { &workspace.page } else { &page }).build();
        legacy_host.set_min_size(Size::new(0, 0));
        legacy_host.set_sizer(BoxSizer::builder(Orientation::Vertical).build(), false);
        if codex {
            if let Some(row) = workspace.page.get_sizer() {
                row.add(&legacy_host, 1, SizerFlag::Expand, 0);
            }
        } else {
            col.add(&legacy_host, 1, SizerFlag::Expand, 0);
        }
        page.set_sizer(col, false);
        page.set_min_size(Size::new(0, 0));
        let ui = Self {
            page,
            add,
            back,
            remove_saved,
            workspace,
            legacy_host,
            legacy: matches!(
                client,
                ClientId::Codex | ClientId::WorkBuddy | ClientId::Tiangong
            ),
            codex,
        };
        ui.show_draft(false);
        ui
    }
    pub fn show_draft(&self, draft: bool) {
        self.page.freeze();
        self.workspace
            .page
            .show(self.codex || !self.legacy || draft);
        // Codex has one channel-scoped candidate editor for both additions and
        // removals. A second generic editor hid discovery results on new rows.
        self.workspace
            .fields
            .page
            .show(!self.codex && (!self.legacy || draft));
        self.legacy_host.show(self.legacy && (self.codex || !draft));
        self.back.show(self.legacy && !self.codex && draft);
        self.page.layout();
        self.workspace.page.layout();
        self.workspace.fields.page.layout();
        self.workspace.fields.page.fit_inside();
        self.legacy_host.layout();
        self.page.thaw();
    }
}

#[derive(Clone)]
pub(super) struct ModelWorkspace {
    pub page: Panel,
    pub list: super::model_list::ModelList,
    pub fields: ModelFields,
}
impl ModelWorkspace {
    pub fn new(parent: &dyn WxWidget, client: ClientId) -> Self {
        let page = Panel::builder(parent).build();
        page.set_background_color(theme::theme().bg_app);
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let list = super::model_list::ModelList::new(&page);
        let fields = ModelFields::new(&page, client);
        row.add(
            &list,
            0,
            SizerFlag::Expand | SizerFlag::Right,
            page.from_dip(12),
        );
        row.add(&fields.page, 1, SizerFlag::Expand, 0);
        page.set_sizer(row, true);
        Self { page, list, fields }
    }
}

#[derive(Clone)]
pub(super) struct ModelFields {
    pub page: ScrolledWindow,
    pub content: ScrolledWindow,
    pub save: Button,
    pub remove: Button,
    pub connection: Button,
    pub restore: Button,
    pub reload: Button,
    empty: StaticText,
    form: Panel,
    heading: Panel,
    model: StaticText,
    source: StaticText,
    name_panel: Panel,
    label: TextCtrl,
    context: TextCtrl,
    output: TextCtrl,
    image: Choice,
    efforts: TextCtrl,
    default_effort: Choice,
    protocol: Choice,
    sync: CheckBox,
    replace: CheckBox,
    error: StaticText,
    draft: DraftFields,
    parameters: Panel,
    advanced: Button,
    footer: Panel,
    parameter_labels: Vec<StaticText>,
    support: std::rc::Rc<std::cell::Cell<crate::desktop::clients::ParameterSupport>>,
    is_draft: std::rc::Rc<std::cell::Cell<bool>>,
    original_image: std::rc::Rc<std::cell::Cell<u32>>,
    client: ClientId,
}
#[derive(Clone)]
struct DraftFields {
    page: Panel,
    target: Choice,
    target_label: StaticText,
    source: Choice,
    model: Choice,
    targets: std::rc::Rc<std::cell::RefCell<Vec<Value>>>,
    sources: std::rc::Rc<std::cell::RefCell<Vec<Value>>>,
}
impl DraftFields {
    fn new(parent: &Panel) -> Self {
        let page = Panel::builder(parent).build();
        page.set_background_color(theme::theme().bg_card);
        let grid = FlexGridSizer::builder(0, 2)
            .with_hgap(page.from_dip(12))
            .with_vgap(page.from_dip(8))
            .build();
        grid.add_growable_col(1, 1);
        let target = Choice::builder(&page).build();
        let source = Choice::builder(&page).build();
        let model = Choice::builder(&page).build();
        let target_label = field(&page, &grid, "配置位置", &target);
        for (name, c) in [("来源渠道", source), ("模型", model)] {
            field(&page, &grid, name, &c);
        }
        page.set_sizer(grid, true);
        let ui = Self {
            page,
            target,
            target_label,
            source,
            model,
            targets: Default::default(),
            sources: Default::default(),
        };
        ui
    }
    fn fill_models(&self) {
        self.model.clear();
        let sources = self.sources.borrow();
        if let Some(source) = self
            .source
            .get_selection()
            .and_then(|i| sources.get(i as usize))
        {
            let mut names = std::collections::BTreeSet::new();
            names.extend(
                source["models"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string),
            );
            names.extend(
                source["aliases"]
                    .as_object()
                    .into_iter()
                    .flat_map(|a| a.keys().cloned()),
            );
            for name in names {
                self.model.append(&name);
            }
            if self.model.get_count() > 0 {
                self.model.set_selection(0);
            }
        }
    }
    fn load(&self, view: &Value, client: ClientId) {
        let targets = view["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| t["client"] == client.key() && t["modelsInherited"] != true)
            .cloned()
            .collect::<Vec<_>>();
        let sources = view["sources"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|s| s["enabled"] == true)
            .cloned()
            .collect::<Vec<_>>();
        self.target.clear();
        for t in &targets {
            self.target.append(t["label"].as_str().unwrap_or("默认"));
        }
        self.source.clear();
        for s in &sources {
            let name = s["name"].as_str().unwrap_or("来源");
            let label = ClientId::ALL
                .iter()
                .find(|c| s["client"] == c.key())
                .map(|c| format!("{} · {name}", c.name()))
                .unwrap_or_else(|| name.into());
            self.source.append(&label);
        }
        if !targets.is_empty() {
            self.target.set_selection(0);
        }
        // One actual destination needs no choice; keep its selected identity
        // for the guarded save payload. Multiple Profiles remain explicit.
        self.target.show(targets.len() != 1);
        self.target_label.show(targets.len() != 1);
        if !sources.is_empty() {
            self.source.set_selection(0);
        }
        *self.targets.borrow_mut() = targets;
        *self.sources.borrow_mut() = sources;
        self.fill_models();
    }
    fn body(&self, revision: &Value) -> Result<Value, String> {
        let targets = self.targets.borrow();
        let sources = self.sources.borrow();
        let target = self
            .target
            .get_selection()
            .and_then(|i| targets.get(i as usize))
            .ok_or("请选择客户端配置位置")?;
        let source = self
            .source
            .get_selection()
            .and_then(|i| sources.get(i as usize))
            .ok_or("请先在模型与渠道中添加可用渠道")?;
        let model = self
            .model
            .get_string_selection()
            .filter(|s| !s.trim().is_empty())
            .ok_or("请选择来源渠道提供的模型")?;
        Ok(
            json!({"revision":revision,"targetId":target["id"],"sourceRoute":source["routeId"],"model":model,"using":true}),
        )
    }
}
impl ModelFields {
    pub fn new(parent: &dyn WxWidget, client: ClientId) -> Self {
        let page = ScrolledWindow::builder(parent)
            .with_style(ScrolledWindowStyle::Default)
            .build();
        page.set_scroll_rate(0, 0);
        page.set_min_size(Size::new(0, 0));
        page.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let content = ScrolledWindow::builder(&page)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        content.set_scroll_rate(0, content.from_dip(10));
        content.set_min_size(Size::new(0, 0));
        content.set_background_color(theme::theme().bg_app);
        let content_col = BoxSizer::builder(Orientation::Vertical).build();
        let empty = StaticText::builder(&content)
            .with_label("添加模型后，在这里调整此客户端的配置。")
            .build();
        empty.set_foreground_color(theme::theme().ink_secondary);
        empty.wrap(page.from_dip(350));
        content_col.add(&empty, 0, SizerFlag::All, page.from_dip(24));
        let form = Panel::builder(&content).build();
        paint_surface(form, theme::SURFACE_RADIUS);
        let body = BoxSizer::builder(Orientation::Vertical).build();
        let draft = DraftFields::new(&form);
        draft.page.hide();
        body.add(
            &draft.page,
            0,
            SizerFlag::Expand | SizerFlag::All,
            form.from_dip(theme::PANEL_INSET),
        );
        let heading = Panel::builder(&form).build();
        heading.set_background_color(theme::theme().bg_card);
        let heading_row = BoxSizer::builder(Orientation::Horizontal).build();
        let model = StaticText::builder(&heading)
            .with_style(StaticTextStyle::EllipsizeEnd)
            .build();
        model.set_min_size(Size::new(0, -1));
        model.set_font(&theme::font(theme::TextRole::Title));
        model.set_foreground_color(theme::theme().ink_primary);
        let source = StaticText::builder(&heading)
            .with_style(StaticTextStyle::EllipsizeEnd)
            .build();
        source.set_min_size(Size::new(0, -1));
        source.set_foreground_color(theme::theme().ink_secondary);
        heading_row.add(
            &model,
            1,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            form.from_dip(12),
        );
        heading_row.add(&source, 1, SizerFlag::AlignCenterVertical, 0);
        heading.set_sizer(heading_row, false);
        body.add(
            &heading,
            0,
            SizerFlag::Expand | SizerFlag::All,
            form.from_dip(16),
        );
        let parameters = Panel::builder(&form).build();
        parameters.set_background_color(theme::theme().bg_card);
        let grid = FlexGridSizer::builder(0, 2)
            .with_hgap(form.from_dip(14))
            .with_vgap(form.from_dip(10))
            .build();
        grid.add_growable_col(1, 1);
        let name_panel = Panel::builder(&form).build();
        name_panel.set_background_color(theme::theme().bg_card);
        let label = TextCtrl::builder(&name_panel).build();
        label.set_hint("留空使用模型名称");
        let name_grid = FlexGridSizer::builder(0, 2)
            .with_hgap(form.from_dip(14))
            .build();
        name_grid.add_growable_col(1, 1);
        field(&name_panel, &name_grid, "显示名称", &label);
        name_panel.set_sizer(name_grid, true);
        body.add(
            &name_panel,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            form.from_dip(theme::PANEL_INSET),
        );
        let advanced = Button::builder(&form)
            .with_label("可选参数")
            .with_icon(super::visual::Icon::Chevron)
            .with_kind(ButtonKind::Ghost)
            .build();
        body.add(
            &advanced,
            0,
            SizerFlag::Left | SizerFlag::Bottom,
            form.from_dip(8),
        );
        let context = TextCtrl::builder(&parameters).build();
        let output = TextCtrl::builder(&parameters).build();
        context.set_hint("沿用客户端默认");
        output.set_hint("沿用客户端默认");
        let image = Choice::builder(&parameters)
            .with_choices(vec!["沿用默认".into(), "支持图片".into(), "仅文本".into()])
            .build();
        let efforts = TextCtrl::builder(&parameters).build();
        efforts.set_hint("沿用客户端可选项");
        efforts.set_tooltip("客户端可选择的强度，以逗号分隔；选项不代表渠道一定支持全部能力");
        let default_effort = Choice::builder(&parameters).build();
        default_effort.set_name("model-default-reasoning-effort");
        let protocol = Choice::builder(&parameters)
            .with_choices(vec![
                "OpenAI Chat".into(),
                "OpenAI Responses".into(),
                "Anthropic Messages".into(),
            ])
            .build();
        let parameter_labels = vec![
            field(&parameters, &grid, "上下文长度", &context),
            field(&parameters, &grid, "输出上限", &output),
            field(&parameters, &grid, "图片输入", &image),
            field(&parameters, &grid, "可选思考强度", &efforts),
            field(&parameters, &grid, "客户端协议", &protocol),
            field(&parameters, &grid, "默认思考强度", &default_effort),
        ];
        parameters.set_sizer(grid, true);
        body.add(
            &parameters,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            form.from_dip(theme::PANEL_INSET),
        );
        let sync = CheckBox::builder(&form)
            .with_label("同步来源的连接设置")
            .build();
        sync.set_foreground_color(theme::theme().ink_secondary);
        sync.set_tooltip("仅本次保存时同步来源的地址、认证和协议；其他客户端不受影响");
        body.add(
            &sync,
            0,
            SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            form.from_dip(16),
        );
        form.set_sizer(body, true);
        content_col.add(&form, 0, SizerFlag::Expand, 0);
        content.set_sizer(content_col, false);
        col.add(&content, 1, SizerFlag::Expand, 0);
        let footer = Panel::builder(&page).build();
        footer.set_background_color(theme::theme().bg_app);
        let foot_col = BoxSizer::builder(Orientation::Vertical).build();
        let error = StaticText::builder(&footer).build();
        error.set_foreground_color(theme::theme().error);
        foot_col.add(
            &error,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            form.from_dip(16),
        );
        let replace = CheckBox::builder(&footer)
            .with_label("已核对客户端改动，保存时替换此项")
            .build();
        replace.set_foreground_color(theme::theme().ink_primary);
        foot_col.add(
            &replace,
            0,
            SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            form.from_dip(16),
        );
        let actions = BoxSizer::builder(Orientation::Horizontal).build();
        let save = Button::builder(&footer)
            .with_label("保存模型")
            .with_kind(ButtonKind::Primary)
            .build();
        let connection = Button::builder(&footer)
            .with_label("连接设置")
            .with_kind(ButtonKind::Ghost)
            .build();
        let remove = Button::builder(&footer)
            .with_label("移除模型")
            .with_kind(ButtonKind::Ghost)
            .build();
        let restore = Button::builder(&footer)
            .with_label("恢复配置")
            .with_kind(ButtonKind::Ghost)
            .build();
        restore.hide();
        let reload = Button::builder(&footer)
            .with_label("载入更新")
            .with_kind(ButtonKind::Ghost)
            .build();
        reload.set_tooltip("配置已更新；放弃当前未保存改动，载入最新配置");
        reload.hide();
        for b in [connection, restore, reload] {
            actions.add(
                &b,
                0,
                SizerFlag::Right | SizerFlag::Bottom,
                form.from_dip(6),
            );
        }
        actions.add_stretch_spacer(1);
        actions.add(&remove, 0, SizerFlag::Right, footer.from_dip(8));
        actions.add(&save, 0, SizerFlag::default(), 0);
        foot_col.add_sizer(
            &actions,
            0,
            SizerFlag::Expand | SizerFlag::Top | SizerFlag::Bottom,
            form.from_dip(8),
        );
        footer.set_sizer(foot_col, false);
        col.add(&footer, 0, SizerFlag::Expand, 0);
        page.set_sizer(col, true);
        let fields = Self {
            page,
            content,
            save,
            remove,
            connection,
            restore,
            reload,
            empty,
            form,
            heading,
            model,
            source,
            name_panel,
            label,
            context,
            output,
            image,
            efforts,
            default_effort,
            protocol,
            sync,
            replace,
            error,
            draft,
            parameters,
            advanced,
            footer,
            parameter_labels,
            support: Default::default(),
            is_draft: Default::default(),
            original_image: Default::default(),
            client,
        };
        let u = fields.clone();
        advanced.on_click(move |_| {
            u.parameters.show(!u.parameters.is_shown());
            u.relayout();
        });
        let u = fields.clone();
        fields.draft.target.on_selection_changed(move |_| {
            if u.is_draft.get() {
                u.configure_parameters(&u.draft_mode());
            }
        });
        let u = fields.clone();
        fields.draft.source.on_selection_changed(move |_| {
            u.draft.fill_models();
            if u.is_draft.get() {
                u.protocol.set_selection(u.source_protocol());
                u.new_reasoning();
            }
        });
        let u = fields.clone();
        fields.draft.model.on_selection_changed(move |_| {
            if u.is_draft.get() {
                u.new_reasoning();
            }
        });
        let u = fields.clone();
        fields.protocol.on_selection_changed(move |_| {
            if u.is_draft.get() {
                u.new_reasoning();
            }
        });
        fields.fill(None, false);
        fields
    }
    pub fn fill(&self, row: Option<&Value>, native: bool) {
        self.empty.show(row.is_none());
        self.form.show(row.is_some());
        self.footer.show(row.is_some());
        if let Some(row) = row {
            let is_draft = row["draft"] == true;
            self.is_draft.set(is_draft);
            self.remove.set_label(if is_draft {
                "放弃新增"
            } else {
                "移除模型"
            });
            self.remove
                .set_min_size(super::visual::dip_size(&self.remove, 100, 36));
            self.draft.page.show(is_draft);
            self.heading.show(!is_draft);
            self.parameters.hide();
            self.configure_parameters(&if is_draft {
                self.draft_mode()
            } else if native {
                "native".into()
            } else if self.client == ClientId::WebStorm {
                "acp".into()
            } else {
                "gateway".into()
            });
            // The legacy adapters use the model ID itself as the visible
            // name. Do not offer an input their save contract cannot apply.
            self.name_panel.show(
                !is_draft
                    || !matches!(
                        self.client,
                        ClientId::Codex | ClientId::WorkBuddy | ClientId::Tiangong
                    ),
            );
            self.model.show(!is_draft);
            self.source.show(!is_draft);
            self.sync.show(!is_draft);
            self.connection.show(!is_draft);
            self.model.set_label(row["model"].as_str().unwrap_or("—"));
            self.model.set_tooltip(row["model"].as_str().unwrap_or(""));
            self.source
                .set_label(row["sourceName"].as_str().unwrap_or("独立配置"));
            self.source
                .set_tooltip(row["sourceName"].as_str().unwrap_or("独立配置"));
            self.label.set_value(if is_draft {
                row["draftLabel"].as_str().unwrap_or("")
            } else {
                row["label"].as_str().unwrap_or("")
            });
            self.label.set_tooltip(if is_draft {
                "留空使用所选模型的名称"
            } else {
                "此客户端显示的模型名称"
            });
            for (key, input) in [("contextWindow", self.context), ("maxTokens", self.output)] {
                input.set_value(
                    &row["capabilities"][key]
                        .as_u64()
                        .map(|n| n.to_string())
                        .unwrap_or_default(),
                );
                input.enable(self.support.get().limits);
                input.set_tooltip(if native {
                    "此接入方式使用客户端提供的模型能力"
                } else {
                    "留空沿用客户端默认"
                });
            }
            self.image.set_selection(
                match row["capabilities"]["supportsImage"].as_bool().or_else(|| {
                    row["capabilities"]["input"].as_array().and_then(|a| {
                        if a.iter().any(|v| v == "image") {
                            Some(true)
                        } else if a.len() == 1 && a[0] == "text" {
                            Some(false)
                        } else {
                            None
                        }
                    })
                }) {
                    Some(true) => 1,
                    Some(false) => 2,
                    _ => 0,
                },
            );
            self.original_image
                .set(self.image.get_selection().unwrap_or(0));
            self.image.enable(self.support.get().image);
            self.efforts.set_value(
                &row["capabilities"]["reasoningEfforts"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default(),
            );
            self.efforts.enable(self.support.get().efforts);
            self.protocol.set_selection(if is_draft {
                self.source_protocol()
            } else {
                match row["wireProtocol"].as_str() {
                    Some("openai-responses") => 1,
                    Some("anthropic-messages") => 2,
                    _ => 0,
                }
            });
            self.fill_default_effort(
                row["capabilities"]["defaultReasoningEffort"]
                    .as_str()
                    .unwrap_or(""),
            );
            if is_draft {
                self.new_reasoning();
                self.parameters
                    .show(self.support.get().efforts || self.support.get().protocol);
            }
            self.sync.set_value(false);
            self.replace.set_value(false);
            self.replace.show(!row["stateError"].is_null());
            self.error
                .set_label(row["stateError"].as_str().unwrap_or(""));
            self.error.wrap(self.page.from_dip(400));
            self.error.show(!row["stateError"].is_null());
        }
        self.relayout();
    }
    pub fn body(&self, row: &Value, view: &Value, client: ClientId) -> Result<Value, String> {
        let mut body = if row["draft"] == true {
            self.draft.body(&view["revision"])?
        } else {
            json!({"id":row["id"],"targetId":row["targetId"],"revision":view["revision"],"using":true,"syncSource":self.sync.get_value(),"replaceConflict":self.replace.get_value()})
        };
        let mode = view["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|t| t["id"] == body["targetId"])
            .and_then(|t| t["modelMode"].as_str())
            .unwrap_or("");
        let support = crate::desktop::clients::parameter_support(client, mode);
        let label = self.label.get_value();
        body["label"] = if row["draft"] == true && label.trim().is_empty() {
            body["model"].clone()
        } else {
            label.into()
        };
        if support.limits || support.image || support.efforts || support.protocol {
            let mut caps = row["capabilities"].as_object().cloned().unwrap_or_default();
            for (key, input) in [("contextWindow", self.context), ("maxTokens", self.output)] {
                if !support.limits {
                    continue;
                }
                let value = input.get_value();
                if value.trim().is_empty() {
                    caps.remove(key);
                } else {
                    let n = value
                        .trim()
                        .parse::<u64>()
                        .ok()
                        .filter(|n| *n > 0 && *n <= 100_000_000)
                        .ok_or("上下文和输出上限须为 1–100000000 的整数")?;
                    caps.insert(key.into(), n.into());
                }
            }
            if support.image && self.image.get_selection().unwrap_or(0) != self.original_image.get()
            {
                match self.image.get_selection() {
                    Some(1) => {
                        caps.insert("supportsImage".into(), true.into());
                        caps.insert("input".into(), json!(["text", "image"]));
                    }
                    Some(2) => {
                        caps.insert("supportsImage".into(), false.into());
                        caps.insert("input".into(), json!(["text"]));
                    }
                    _ => {
                        caps.remove("supportsImage");
                        caps.remove("input");
                    }
                }
            }
            if support.efforts {
                let names = self
                    .efforts
                    .get_value()
                    .split([',', '，'])
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if names.len() > 16 || names.iter().any(|s| s.len() > 50) {
                    return Err("思考强度内容过长".into());
                }
                if names.is_empty() {
                    caps.remove("reasoningEfforts");
                } else {
                    caps.insert("reasoningEfforts".into(), names.into());
                }
                let effort = self
                    .default_effort
                    .get_string_selection()
                    .unwrap_or_default();
                if effort.is_empty() || effort == "自动" {
                    caps.remove("defaultReasoningEffort");
                } else {
                    caps.insert("defaultReasoningEffort".into(), effort.into());
                }
            }
            if row["draft"] == true {
                caps.insert("preferStream".into(), true.into());
            }
            body["capabilities"] = caps.into();
            if client == ClientId::WebStorm && mode == "acp" {
                match (
                    body["capabilities"]["contextWindow"].as_u64(),
                    body["capabilities"]["maxTokens"].as_u64(),
                ) {
                    (None, None) => {}
                    (Some(context), Some(output)) if output <= context => {}
                    (Some(_), Some(_)) => return Err("输出上限不能超过上下文长度".into()),
                    _ => return Err("请同时填写上下文长度和输出上限，或都留空".into()),
                }
            }
            if support.protocol {
                body["wireProtocol"] = [
                    "openai-completions",
                    "openai-responses",
                    "anthropic-messages",
                ][self.protocol.get_selection().unwrap_or(0) as usize]
                    .into();
            }
        }
        Ok(body)
    }
    pub fn prepare_draft(&self, view: &Value, client: ClientId) {
        self.draft.load(view, client);
    }
    fn fill_default_effort(&self, preferred: &str) {
        self.default_effort.clear();
        self.default_effort.append("自动");
        let mut names = Vec::<String>::new();
        for name in self
            .efforts
            .get_value()
            .split([',', '，'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .chain(std::iter::once(preferred).filter(|s| !s.is_empty()))
        {
            if !names.iter().any(|n| n == name) {
                names.push(name.into());
            }
        }
        if names.is_empty() {
            names.extend(
                crate::desktop::clients::reasoning_presets("", "")
                    .iter()
                    .map(|s| s.to_string()),
            );
        }
        for name in &names {
            self.default_effort.append(name);
        }
        self.default_effort.set_selection(
            names
                .iter()
                .position(|n| n == preferred)
                .map(|i| i as u32 + 1)
                .unwrap_or(0),
        );
    }
    fn new_reasoning(&self) {
        if !self.support.get().efforts {
            return;
        }
        let builtin = self.draft.source.get_selection().is_some_and(|i| {
            self.draft
                .sources
                .borrow()
                .get(i as usize)
                .is_some_and(|s| s["tiangongBuiltin"].is_string())
        });
        if builtin {
            self.efforts.change_value("");
            self.fill_default_effort("");
            return;
        }
        let mut model = self.draft.model.get_string_selection().unwrap_or_default();
        if let Some(upstream) = self.draft.source.get_selection().and_then(|i| {
            self.draft
                .sources
                .borrow()
                .get(i as usize)
                .and_then(|s| s["aliases"][&model].as_str().map(str::to_string))
        }) {
            model = upstream;
        }
        let protocol = if self.protocol.get_selection() == Some(2) {
            "anthropic-messages"
        } else {
            "openai-chat"
        };
        let choices = crate::desktop::clients::reasoning_presets(&model, protocol);
        self.efforts.change_value(&choices.join(", "));
        self.fill_default_effort(crate::desktop::clients::new_reasoning_default(choices));
    }
    fn source_protocol(&self) -> u32 {
        let sources = self.draft.sources.borrow();
        match self
            .draft
            .source
            .get_selection()
            .and_then(|i| sources.get(i as usize))
            .and_then(|s| s["type"].as_str())
        {
            Some("openai_responses" | "chatgpt_responses") => 1,
            Some("anthropic_messages") => 2,
            _ => 0,
        }
    }
    fn draft_mode(&self) -> String {
        self.draft
            .target
            .get_selection()
            .and_then(|i| {
                self.draft
                    .targets
                    .borrow()
                    .get(i as usize)
                    .and_then(|t| t["modelMode"].as_str())
                    .map(str::to_string)
            })
            .unwrap_or_default()
    }
    fn configure_parameters(&self, mode: &str) {
        let support = crate::desktop::clients::parameter_support(self.client, mode);
        self.support.set(support);
        for (i, (control, show)) in [
            (&self.context as &dyn WxWidget, support.limits),
            (&self.output, support.limits),
            (&self.image, support.image),
            (&self.efforts, support.efforts),
            (&self.protocol, support.protocol),
            (&self.default_effort, support.efforts),
        ]
        .into_iter()
        .enumerate()
        {
            self.parameter_labels[i].show(show);
            control.show(show);
            control.enable(show);
        }
        let any = support.limits || support.image || support.efforts || support.protocol;
        self.advanced.show(any);
        if !any {
            self.parameters.hide();
        }
        self.relayout();
    }
    fn relayout(&self) {
        self.form.layout();
        self.parameters.layout();
        self.footer.layout();
        self.page.layout();
        self.content.layout();
        self.content.fit_inside();
    }
    pub fn draft_snapshot(&self) -> Value {
        json!({"target":self.draft.target.get_selection(),"source":self.draft.source.get_selection(),"model":self.draft.model.get_selection(),"label":self.label.get_value(),"context":self.context.get_value(),"output":self.output.get_value(),"image":self.image.get_selection(),"efforts":self.efforts.get_value(),"defaultEffort":self.default_effort.get_string_selection(),"protocol":self.protocol.get_selection(),"parameters":self.parameters.is_shown()})
    }
    pub fn restore_draft(&self, v: &Value) {
        if !v.is_object() {
            return;
        }
        if let Some(i) = v["target"].as_u64() {
            self.draft.target.set_selection(i as u32);
        }
        if let Some(i) = v["source"].as_u64() {
            self.draft.source.set_selection(i as u32);
            self.draft.fill_models();
        }
        if let Some(i) = v["model"].as_u64() {
            self.draft.model.set_selection(i as u32);
        }
        for (key, input) in [
            ("label", self.label),
            ("context", self.context),
            ("output", self.output),
            ("efforts", self.efforts),
        ] {
            input.set_value(v[key].as_str().unwrap_or(""));
        }
        if let Some(i) = v["image"].as_u64() {
            self.image.set_selection(i as u32);
        }
        self.fill_default_effort(v["defaultEffort"].as_str().unwrap_or(""));
        if let Some(i) = v["protocol"].as_u64() {
            self.protocol.set_selection(i as u32);
        }
        if self.is_draft.get() {
            self.configure_parameters(&self.draft_mode());
        }
        let support = self.support.get();
        self.parameters.show(
            v["parameters"] == true
                && (support.limits || support.image || support.efforts || support.protocol),
        );
        self.relayout();
    }
}
fn field<W: WxWidget>(parent: &Panel, grid: &FlexGridSizer, name: &str, control: &W) -> StaticText {
    let label = StaticText::builder(parent).with_label(name).build();
    label.set_background_color(parent.get_background_color());
    label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
    grid.add(
        control,
        1,
        SizerFlag::Expand | SizerFlag::AlignCenterVertical,
        0,
    );
    label
}

#[cfg(all(test, windows))]
pub(super) fn review_supported_fields(suffix: &str) {
    for (client, mode) in [
        (ClientId::VsCode, "chat"),
        (ClientId::DeepSeekHarness, "native"),
        (ClientId::ZCode, "native"),
        (ClientId::WebStorm, "acp"),
        (ClientId::WebStorm, "native"),
    ] {
        let frame = Frame::builder()
            .with_title("hidden client model parameter contracts")
            .build();
        frame.set_font(&theme::font(theme::TextRole::Body));
        frame.set_size(super::visual::dip_size(&frame, 700, 500));
        let workspace = ModelWorkspace::new(&frame, client);
        workspace.list.replace(&["GPT-6".into(), "新建模型".into()]);
        workspace.list.set_selection(1, true);
        let fields = &workspace.fields;
        let view = json!({"revision":"original","targets":[{"id":"synthetic-target","client":client.key(),"label":"默认","modelMode":mode}],"sources":[{"name":"合成渠道","routeId":"synthetic-source","enabled":true,"type":"anthropic_messages","models":["gpt-6"]},{"name":"另一合成渠道","routeId":"synthetic-other","enabled":true,"type":"openai_responses","models":["other-model"]}]});
        let draft = json!({"draft":true,"capabilities":{}});
        fields.prepare_draft(&view, client);
        fields.fill(Some(&draft), false);
        assert!(fields.label.get_value().is_empty());
        let protocol = fields.body(&draft, &view, client).unwrap();
        if crate::desktop::clients::parameter_support(client, mode).efforts {
            assert_eq!(protocol["capabilities"]["defaultReasoningEffort"], "xhigh");
            assert!(
                protocol["capabilities"]["reasoningEfforts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v == "max")
            );
        }
        if client == ClientId::DeepSeekHarness {
            assert_eq!(protocol["wireProtocol"], "anthropic-messages");
        }
        let mut builtin_view = view.clone();
        builtin_view["sources"][0]["tiangongBuiltin"] = "tiangong-builtin".into();
        fields.prepare_draft(&builtin_view, client);
        fields.fill(Some(&draft), false);
        if crate::desktop::clients::parameter_support(client, mode).efforts {
            let builtin_body = fields.body(&draft, &builtin_view, client).unwrap();
            assert!(
                builtin_body["capabilities"]
                    .get("defaultReasoningEffort")
                    .is_none()
            );
            assert!(
                builtin_body["capabilities"]
                    .get("reasoningEfforts")
                    .is_none()
            );
        }
        fields.prepare_draft(&view, client);
        fields.fill(Some(&draft), false);
        // One selection must update both the model list and protocol. Multiple
        // native handlers for the same event can stop one another in wxWidgets.
        use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_KEYDOWN, WM_KEYUP};
        for (key, model, source, wire) in [
            (0x28, "other-model", "synthetic-other", "openai-responses"),
            (0x26, "gpt-6", "synthetic-source", "anthropic-messages"),
        ] {
            unsafe {
                SendMessageW(fields.draft.source.get_handle(), WM_KEYDOWN, key, 0);
                SendMessageW(fields.draft.source.get_handle(), WM_KEYUP, key, 0);
            }
            let body = fields.body(&draft, &view, client).unwrap();
            assert_eq!(body["model"], model);
            assert_eq!(body["sourceRoute"], source);
            if client == ClientId::DeepSeekHarness {
                assert_eq!(body["wireProtocol"], wire);
            }
        }
        fields.restore_draft(&json!({"context":"65536","output":"8192","image":1,"efforts":"low, high, max","parameters":true}));
        let body = fields.body(&draft, &view, client).unwrap();
        let support = crate::desktop::clients::parameter_support(client, mode);
        assert_eq!(fields.efforts.is_shown(), support.efforts);
        assert_eq!(fields.context.is_shown(), support.limits);
        if mode == "native" && client == ClientId::WebStorm {
            assert!(body.get("capabilities").is_none());
            assert!(!fields.advanced.is_shown());
        } else {
            assert_eq!(body["capabilities"]["contextWindow"], 65536);
            assert_eq!(body["capabilities"]["supportsImage"], true);
            if !support.efforts {
                assert!(body["capabilities"].get("reasoningEfforts").is_none());
            }
        }
        let col = BoxSizer::builder(Orientation::Vertical).build();
        col.add(&workspace.page, 1, SizerFlag::Expand, 0);
        frame.set_sizer(col, false);
        frame.layout();
        fields.relayout();
        assert!(
            fields.save.get_position().x + fields.save.get_size().width
                <= fields.footer.get_client_size().width
        );
        assert!(
            fields.footer.get_position().y + fields.footer.get_size().height
                <= fields.page.get_client_size().height + 2
        );
        assert!(
            fields.content.get_virtual_size().width
                <= fields.content.get_client_size().width + fields.page.from_dip(2)
        );
        super::native_model_review::capture(
            &workspace.page,
            &format!("native-model-fields-{}-{mode}-{suffix}", client.key()),
        );
        let row = json!({"id":"saved","targetId":"synthetic-target","label":"已有模型","model":"gpt-6","capabilities":{"input":["text","audio"],"reasoningEfforts":["max"],"compat":{"private":true},"futureField":42}});
        fields.fill(Some(&row), mode == "native" && client == ClientId::WebStorm);
        let body = fields.body(&row, &view, client).unwrap();
        if !(mode == "native" && client == ClientId::WebStorm) {
            assert_eq!(body["capabilities"]["input"], row["capabilities"]["input"]);
            assert_eq!(body["capabilities"]["futureField"], 42);
            assert_eq!(
                body["capabilities"]["reasoningEfforts"],
                row["capabilities"]["reasoningEfforts"]
            );
        }
        // Changing the destination changes field support before saving.
        if client == ClientId::WebStorm {
            let mut switched = view.clone();
            switched["targets"][0]["modelMode"] =
                if mode == "native" { "acp" } else { "native" }.into();
            fields.prepare_draft(&switched, client);
            fields.fill(Some(&draft), false);
            assert_eq!(fields.advanced.is_shown(), mode == "native");
        }
        assert!(!frame.is_shown());
        frame.destroy();
    }
}
