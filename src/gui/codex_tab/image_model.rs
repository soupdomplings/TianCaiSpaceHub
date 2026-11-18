use super::*;
use crate::gui::text::GuiLocale;

const PRESETS: [&str; 2] = ["gpt-image-2.6-sunburst", "gpt-image-2.6-flare"];

fn tr(text: GuiText, zh: &'static str, en: &'static str) -> &'static str {
    match text.locale {
        GuiLocale::ZhCn => zh,
        GuiLocale::EnUs => en,
    }
}

#[derive(Default, PartialEq)]
struct Candidates {
    images: Vec<String>,
    other: Vec<String>,
}

impl Candidates {
    fn from_config(config: &AiGatewayConfig) -> Self {
        let mut images: Vec<String> = PRESETS.iter().map(|s| s.to_string()).collect();
        let mut other = Vec::new();
        for p in config
            .providers
            .iter()
            .filter(|p| p.enabled && p.is_client_model_source())
        {
            for (id, upstream) in p
                .models
                .iter()
                .map(|id| (id, id))
                .chain(p.model_aliases.iter())
            {
                if id.is_empty() {
                    continue;
                }
                if [id, upstream]
                    .iter()
                    .any(|s| s.to_ascii_lowercase().starts_with("gpt-image-"))
                {
                    images.push(id.clone());
                } else {
                    other.push(id.clone());
                }
            }
        }
        if let Some(saved) = &config.codex_image_model {
            images.push(saved.clone());
        }
        let mut seen = std::collections::HashSet::new();
        images.retain(|id| seen.insert(id.to_ascii_lowercase()));
        other.sort_unstable();
        other.retain(|id| seen.insert(id.to_ascii_lowercase()));
        Self { images, other }
    }
}

#[derive(Clone)]
pub(super) struct Picker {
    choice: Choice,
    text: GuiText,
    candidates: Rc<RefCell<Candidates>>,
    saved: Rc<RefCell<Option<String>>>,
    busy: Rc<Cell<bool>>,
}

impl Picker {
    pub(super) fn new(parent: &Panel, section: &BoxSizer, text: GuiText) -> Self {
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let label = StaticText::builder(parent)
            .with_label(tr(text, "默认生图模型", "Default image model"))
            .build();
        let choice = Choice::builder(parent)
            .with_size(Size::new(260, 34))
            .with_choices(vec![
                tr(text, "沿用客户端默认", "Use client default").into(),
            ])
            .with_selection(0)
            .build();
        choice.enable(false);
        row.add(
            &label,
            0,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            parent.from_dip(10),
        );
        row.add(&choice, 0, SizerFlag::AlignCenterVertical, 0);
        let margin = SizerFlag::Left | SizerFlag::Right | SizerFlag::Top;
        section.add_sizer(&row, 0, margin, parent.from_dip(10));
        let help = tr(
            text,
            "下次通过 Hub 生图或改图时生效，聊天模型不变。独立生图需启用提供该模型和 Images 接口的渠道；使用托管工具时，由当前聊天渠道出图。",
            "Applies to the next image request through Hub. Your chat model stays the same. Standalone image requests need an enabled channel offering the model and the Images API. Hosted tools use the current chat channel.",
        );
        let hint = StaticText::builder(parent).with_label(help).build();
        hint.set_foreground_color(theme::theme().ink_muted);
        hint.wrap(parent.from_dip(580));
        section.add(&hint, 0, margin | SizerFlag::Expand, parent.from_dip(8));
        Self {
            choice,
            text,
            candidates: Default::default(),
            saved: Default::default(),
            busy: Default::default(),
        }
    }

    pub(super) fn enable(&self, enabled: bool) {
        self.choice
            .enable(enabled && !self.busy.get() && !self.candidates.borrow().images.is_empty());
    }

    pub(super) fn sync(&self, config: &AiGatewayConfig, enabled: bool) {
        if self.busy.get() {
            return;
        }
        let next = Candidates::from_config(config);
        if *self.candidates.borrow() != next {
            *self.candidates.borrow_mut() = next;
            self.rebuild();
        }
        self.saved.replace(config.codex_image_model.clone());
        self.restore_selection();
        self.enable(enabled);
    }

    fn rebuild(&self) {
        self.choice.clear();
        self.choice
            .append(tr(self.text, "沿用客户端默认", "Use client default"));
        let options = self.candidates.borrow();
        for id in &options.images {
            self.choice.append(id);
        }
        if !options.other.is_empty() {
            self.choice.append(tr(
                self.text,
                "选择其他渠道模型…",
                "Choose another channel model…",
            ));
        }
    }

    fn restore_selection(&self) {
        let options = self.candidates.borrow();
        let index = self
            .saved
            .borrow()
            .as_ref()
            .and_then(|id| {
                options
                    .images
                    .iter()
                    .position(|m| m.eq_ignore_ascii_case(id))
            })
            .map_or(0, |i| i as u32 + 1);
        if self.choice.get_selection() != Some(index) {
            self.choice.set_selection(index);
        }
    }

    // None means the user dismissed the picker; Some(None) restores the client default.
    fn selected(&self, frame: &Frame) -> Option<Option<String>> {
        let index = self.choice.get_selection()? as usize;
        if index == 0 {
            return Some(None);
        }
        let options = self.candidates.borrow();
        if let Some(id) = options.images.get(index - 1) {
            return Some(Some(id.clone()));
        }
        let labels: Vec<_> = options.other.iter().map(String::as_str).collect();
        let dialog = SingleChoiceDialog::builder(
            frame,
            tr(
                self.text,
                "请选择渠道提供的生图模型或别名。",
                "Select an image model or alias offered by your channel.",
            ),
            tr(self.text, "其他渠道模型", "Other channel models"),
            &labels,
        )
        .build();
        let result = (dialog.show_modal() == ID_OK)
            .then(|| usize::try_from(dialog.get_selection()).ok())
            .flatten()
            .and_then(|i| options.other.get(i).cloned());
        dialog.destroy();
        result.map(Some)
    }

    pub(super) fn finish(
        &self,
        result: Result<Option<String>, String>,
        frame: &Frame,
        enabled: bool,
    ) {
        match result {
            Ok(model) => {
                if let Some(id) = &model
                    && !self
                        .candidates
                        .borrow()
                        .images
                        .iter()
                        .any(|m| m.eq_ignore_ascii_case(id))
                {
                    let mut options = self.candidates.borrow_mut();
                    options.images.push(id.clone());
                    options.other.retain(|v| !v.eq_ignore_ascii_case(id));
                    drop(options);
                    self.rebuild();
                }
                self.saved.replace(model);
                show_info(
                    frame,
                    tr(self.text, "已保存默认生图设置", "Image preference saved"),
                );
            }
            Err(err) => show_error(frame, &err),
        }
        self.busy.set(false);
        self.restore_selection();
        self.enable(enabled);
    }
}

pub(super) fn bind(
    api: &ApiClient,
    frame: &Frame,
    tab: &CodexTab,
    refresh: &DashboardRefresh,
    tx: &UnboundedSender<crate::gui::GuiMessage>,
    in_flight: &Arc<AtomicBool>,
) {
    let picker = tab.image_model.clone();
    let service_enabled = tab.service_enabled.clone();
    let (api, frame, refresh, tx, in_flight) = (
        api.clone(),
        *frame,
        refresh.clone(),
        tx.clone(),
        in_flight.clone(),
    );
    let choice = picker.choice.clone();
    choice.on_selection_changed(move |_| {
        if picker.busy.get() || in_flight.swap(true, Ordering::SeqCst) {
            picker.restore_selection();
            return;
        }
        picker.busy.set(true);
        picker.choice.enable(false);
        let expected = picker.saved.borrow().clone();
        let model = picker.selected(&frame);
        let Some(model) = model
            .filter(|m| m != &expected && ensure_service_ready_for_action(&api, &frame, &refresh))
        else {
            picker.busy.set(false);
            picker.restore_selection();
            picker.enable(service_enabled.get());
            in_flight.store(false, Ordering::SeqCst);
            return;
        };
        let (api, tx, in_flight, text) = (api.clone(), tx.clone(), in_flight.clone(), picker.text);
        thread::spawn(move || {
            let result = (|| {
                let mut config = api.get_app_config()?;
                let gw = &mut config.ai_gateway;
                if gw.codex_image_model != expected {
                    return Err(tr(
                        text,
                        "默认生图模型已在其他位置更改，请重新选择。",
                        "The image preference changed elsewhere. Select the model again.",
                    )
                    .into());
                }
                if let Some(id) = model.as_deref()
                    && !PRESETS.contains(&id)
                    && !gw
                        .providers
                        .iter()
                        .any(|p| p.enabled && p.is_client_model_source() && p.matches_model(id))
                {
                    return Err(tr(
                        text,
                        "已启用的渠道不再提供这个模型，请重新选择。",
                        "Enabled channels no longer list this model. Select another model.",
                    )
                    .into());
                }
                gw.codex_image_model = model.clone();
                api.save_app_config(&config)?;
                Ok(model)
            })();
            in_flight.store(false, Ordering::SeqCst);
            let _ = tx.send(crate::gui::GuiMessage::CodexAction(
                CodexActionResult::ImageModel(result),
            ));
            wxdragon::wake_up_idle();
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_gateway::config::ProviderConfig;

    #[test]
    fn image_candidates_keep_public_ids_and_an_unavailable_saved_choice() {
        let mut config = AiGatewayConfig::default();
        config.providers = vec![
            ProviderConfig {
                models: vec![PRESETS[0].into(), "illustrator".into(), "gpt-chat".into()],
                model_aliases: [("studio".into(), "gpt-image-secret".into())].into(),
                ..Default::default()
            },
            ProviderConfig {
                enabled: false,
                models: vec!["gpt-image-disabled".into()],
                ..Default::default()
            },
        ];
        config.codex_image_model = Some("removed-image".into());
        let options = Candidates::from_config(&config);
        assert_eq!(
            options.images,
            [PRESETS[0], PRESETS[1], "studio", "removed-image"]
        );
        assert_eq!(options.other, ["gpt-chat", "illustrator"]);
    }
}
