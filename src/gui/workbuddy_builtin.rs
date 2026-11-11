//! Explicit WorkBuddy sharing form. Opening reads state; only Enable starts the host.
use super::{
    api::ApiClient,
    controls::{Button, ButtonKind, Choice},
    dialog_job::{self, InlineJob},
    feedback,
    squad_dialog::{self, SquadDialog},
    theme,
};
use crate::desktop::clients::ClientId;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    rc::Rc,
};
use wxdragon::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Read,
    Enable,
    Disable,
    Share,
}
impl Operation {
    fn title(self) -> &'static str {
        match self {
            Self::Read => "读取 WorkBuddy 分享状态",
            Self::Enable => "启用 WorkBuddy 模型分享",
            Self::Disable => "停用 WorkBuddy 模型分享",
            Self::Share => "保存模型到客户端",
        }
    }
    fn request(
        self,
        view: &Value,
        model: Option<&Value>,
        targets: &BTreeSet<String>,
    ) -> Result<(String, Value), String> {
        let mut body = json!({});
        let operation = match self {
            Self::Read => "read",
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::Share => {
                if view["ready"] != true {
                    return Err("WorkBuddy 分享尚未就绪，请按当前提示处理后刷新。".into());
                }
                let model_id = model
                    .and_then(|model| model["id"].as_str())
                    .ok_or("请选择要分享的内置模型")?;
                if targets.is_empty() {
                    return Err("请选择要保存到的客户端".into());
                }
                body["modelId"] = json!(model_id);
                body["targets"] = json!(targets);
                "share"
            }
        };
        if self != Self::Read && view["revision"].is_string() {
            body["revision"] = view["revision"].clone();
        }
        Ok((format!("/api/desktop/workbuddy-builtin/{operation}"), body))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    id: String,
    label: String,
}
fn targets(view: &Value) -> Vec<Target> {
    view["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|target| {
            let client = serde_json::from_value::<ClientId>(target["client"].clone()).ok()?;
            if client == ClientId::WorkBuddy
                || !client.is_visible()
                || target["modelsInherited"] == true
            {
                return None;
            }
            let id = target["id"].as_str()?.to_owned();
            let label = match target["label"].as_str() {
                Some(label) if !matches!(label, "" | "默认" | "Default") => {
                    format!("{} · {label}", client.name())
                }
                _ => client.name().into(),
            };
            Some(Target { id, label })
        })
        .collect()
}
fn models(view: &Value) -> &[Value] {
    view["models"].as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// An open receipt belongs to visibility, not the end of a modal event loop.
#[derive(Clone)]
struct OpenReceipt(Rc<RefCell<Option<Box<dyn FnOnce(bool)>>>>);
impl OpenReceipt {
    fn new(callback: impl FnOnce(bool) + 'static) -> Self {
        Self(Rc::new(RefCell::new(Some(Box::new(callback)))))
    }
    fn observe(&self, shown: bool) -> bool {
        if !shown {
            return false;
        }
        let callback = self.0.borrow_mut().take();
        if let Some(callback) = callback {
            callback(true);
            true
        } else {
            false
        }
    }
    fn finish(&self) {
        let callback = self.0.borrow_mut().take();
        if let Some(callback) = callback {
            callback(false);
        }
    }
}

#[derive(Clone)]
struct SharePanel {
    dialog: Dialog,
    body: ScrolledWindow,
    job_owner: Panel,
    account: StaticText,
    detail: StaticText,
    models: Choice,
    model_detail: StaticText,
    targets_host: Panel,
    targets_empty: StaticText,
    choices: Rc<RefCell<Vec<(Target, CheckBox)>>>,
    selected: Rc<RefCell<BTreeSet<String>>>,
    read: Button,
    enable: Button,
    disable: Button,
    share: Button,
    close: Button,
    outcome: StaticText,
    view: Rc<RefCell<Value>>,
    busy: Rc<Cell<bool>>,
}
impl SharePanel {
    fn selected_model(&self) -> Option<Value> {
        models(&self.view.borrow())
            .get(self.models.get_selection()? as usize)
            .cloned()
    }
    fn update_controls(&self) {
        let view = self.view.borrow();
        let idle = !self.busy.get();
        let ready = view["ready"] == true;
        let read = view["revision"].is_string();
        self.read.enable(idle);
        self.close.enable(idle);
        self.enable.enable(idle && read && view["enabled"] != true);
        self.disable.enable(idle && view["enabled"] == true);
        self.models
            .enable(idle && ready && !models(&view).is_empty());
        self.share.enable(
            idle && ready && self.selected_model().is_some() && !self.selected.borrow().is_empty(),
        );
        for (_, checkbox) in self.choices.borrow().iter() {
            checkbox.enable(idle && ready);
        }
    }
    fn model_description(&self) -> String {
        let mut details = Vec::new();
        if let Some(model) = self.selected_model() {
            if let Some(context) = model["contextWindow"].as_u64().filter(|v| *v > 0) {
                details.push(format!("上下文 {context}"));
            }
            if let Some(output) = model["maxOutputTokens"].as_u64().filter(|v| *v > 0) {
                details.push(format!("最大输出 {output}"));
            }
            if model["supportsThinking"] == true {
                let effort = model["defaultThinkingLevel"].as_str().unwrap_or("");
                details.push(if effort.is_empty() {
                    "支持思考".into()
                } else {
                    format!("默认思考强度 {effort}")
                });
            }
        }
        details.join(" · ")
    }
    fn apply(&self, view: Value) {
        let selected = self
            .selected_model()
            .and_then(|model| model["id"].as_str().map(str::to_owned));
        self.account.set_label(if view["loggedIn"] == true {
            view["accountName"].as_str().unwrap_or("WorkBuddy 已登录")
        } else if view["enabled"] == true {
            "等待 WorkBuddy 登录"
        } else {
            "WorkBuddy 分享未启用"
        });
        self.models.clear();
        let rows = models(&view);
        for model in rows {
            self.models.append(
                model["name"]
                    .as_str()
                    .or_else(|| model["id"].as_str())
                    .unwrap_or("WorkBuddy 内置模型"),
            );
        }
        if !rows.is_empty() {
            let index = selected
                .and_then(|id| rows.iter().position(|model| model["id"] == id))
                .unwrap_or(0);
            self.models.set_selection(index as u32);
        }
        let catalog = targets(&view);
        self.selected
            .borrow_mut()
            .retain(|id| catalog.iter().any(|target| &target.id == id));
        let targets_changed = self
            .choices
            .borrow()
            .iter()
            .map(|(target, _)| target)
            .ne(catalog.iter());
        if targets_changed {
            for (_, checkbox) in self.choices.borrow_mut().drain(..) {
                checkbox.destroy();
            }
            for target in catalog {
                let checkbox = CheckBox::builder(&self.targets_host)
                    .with_label(&target.label)
                    .build();
                checkbox.set_min_size(Size::new(0, checkbox.from_dip(44)));
                checkbox.set_font(&theme::font(theme::TextRole::Body));
                checkbox.set_foreground_color(theme::theme().ink_primary);
                checkbox.set_tooltip(&target.label);
                let (selected, id, panel) =
                    (self.selected.clone(), target.id.clone(), self.clone());
                checkbox.on_toggled(move |event| {
                    if event.is_checked() {
                        selected.borrow_mut().insert(id.clone());
                    } else {
                        selected.borrow_mut().remove(&id);
                    }
                    panel.update_controls();
                });
                if let Some(sizer) = self.targets_host.get_sizer() {
                    sizer.add(&checkbox, 0, SizerFlag::Expand, 0);
                }
                self.choices.borrow_mut().push((target, checkbox));
            }
        }
        for result in view["outcomes"].as_array().into_iter().flatten() {
            if result["ok"] == true
                && let Some(id) = result["targetId"].as_str()
            {
                self.selected.borrow_mut().remove(id);
            }
        }
        for (target, checkbox) in self.choices.borrow().iter() {
            checkbox.set_value(self.selected.borrow().contains(&target.id));
        }
        self.targets_empty.show(self.choices.borrow().is_empty());
        *self.view.borrow_mut() = view;
        self.update_controls();
        self.layout();
    }
    fn layout(&self) {
        let width = (self.dialog.get_client_size().width
            - self
                .dialog
                .from_dip(theme::PAGE_GUTTER * 2 + theme::PANEL_INSET * 2))
        .max(1);
        // Reapply raw labels so widening removes line breaks from an earlier wrap.
        let view = self.view.borrow();
        let detail = view["detail"]
            .as_str()
            .unwrap_or("只有点击“启用分享”才会准备并启动 WorkBuddy。");
        let description = self.model_description();
        let has_failure = view["outcomes"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["ok"] != true));
        let summary = if has_failure {
            outcome_summary(&view, &self.choices.borrow())
        } else {
            String::new()
        };
        for (label, text) in [
            (self.detail, detail),
            (self.model_detail, description.as_str()),
            (self.targets_empty, "未发现可保存的客户端。"),
            (self.outcome, summary.as_str()),
        ] {
            label.set_label(text);
            label.wrap(width);
        }
        self.model_detail.show(!description.is_empty());
        self.outcome.show(!summary.is_empty());
        drop(view);
        self.targets_host.layout();
        self.dialog.layout();
        self.body.layout();
        self.body.fit_inside();
    }
    fn run(&self, job: &Rc<InlineJob>, api: &ApiClient, operation: Operation) {
        if job.is_running() {
            return;
        }
        let request = operation.request(
            &self.view.borrow(),
            self.selected_model().as_ref(),
            &self.selected.borrow(),
        );
        let (path, body) = match request {
            Ok(request) => request,
            Err(error) => {
                feedback::error(&self.dialog, &error);
                return;
            }
        };
        self.busy.set(true);
        self.update_controls();
        let panel = self.clone();
        job.start_with_feedback(
            api.clone(),
            operation.title(),
            &path,
            body,
            operation == Operation::Disable,
            move |result| {
                panel.busy.set(false);
                if let Ok(value) = result {
                    let needs_restart = value["needsRestart"] == true;
                    let ready = value["ready"] == true;
                    let detail = value["detail"].as_str().unwrap_or("").to_owned();
                    let partial_failure = value["outcomes"]
                        .as_array()
                        .is_some_and(|rows| rows.iter().any(|row| row["ok"] != true));
                    let summary = outcome_summary(&value, &panel.choices.borrow());
                    panel.apply(value);
                    if operation == Operation::Enable {
                        if needs_restart {
                            feedback::error(&panel.dialog, &detail);
                        } else if ready {
                            feedback::success(&panel.dialog, "WorkBuddy 模型分享已就绪");
                        }
                    } else if operation == Operation::Share {
                        if partial_failure {
                            feedback::error(&panel.dialog, &summary);
                        } else if !summary.is_empty() {
                            feedback::success(&panel.dialog, &summary);
                        }
                    }
                } else {
                    panel.update_controls();
                    panel.layout();
                }
            },
        );
    }
}

fn outcome_summary(view: &Value, targets: &[(Target, CheckBox)]) -> String {
    let mut lines = Vec::new();
    let mut saved = 0;
    for outcome in view["outcomes"].as_array().into_iter().flatten() {
        if outcome["ok"] == true {
            saved += 1;
        } else {
            let label = targets
                .iter()
                .find(|(target, _)| outcome["targetId"] == target.id)
                .map(|(target, _)| target.label.as_str())
                .unwrap_or("客户端");
            lines.push(format!(
                "{label}：{}",
                outcome["message"]
                    .as_str()
                    .unwrap_or("保存失败，请刷新后重试")
            ));
        }
    }
    if saved > 0 {
        lines.insert(
            0,
            format!("已保存到 {saved} 个客户端，可在客户端中选择此模型。"),
        );
    }
    lines.join("\n")
}

fn panel(parent: &Frame) -> (SquadDialog, SharePanel) {
    let chrome = SquadDialog::new(
        parent,
        "WorkBuddy 内置模型分享",
        "启用后使用当前 WorkBuddy 账号的内置模型，可同时保存到多个客户端。",
        Size::new(680, 620),
    );
    let (state_card, state_column) = chrome.section("分享状态");
    let account = squad_dialog::field_label(&state_card, "正在读取 WorkBuddy 状态…");
    state_column.add(&account, 0, SizerFlag::Expand, 0);
    let detail = squad_dialog::note(&state_card, "只有点击“启用分享”才会准备并启动 WorkBuddy。");
    state_column.add(
        &detail,
        0,
        SizerFlag::Expand | SizerFlag::Top,
        state_card.from_dip(8),
    );
    let switches = BoxSizer::builder(Orientation::Horizontal).build();
    let enable = Button::builder(&state_card)
        .with_label("启用分享")
        .with_kind(ButtonKind::Secondary)
        .build();
    let disable = Button::builder(&state_card)
        .with_label("停用分享")
        .with_kind(ButtonKind::Ghost)
        .build();
    switches.add(&enable, 0, SizerFlag::Right, state_card.from_dip(8));
    switches.add(&disable, 0, SizerFlag::default(), 0);
    state_column.add_sizer(&switches, 0, SizerFlag::Top, state_card.from_dip(12));
    let (model_card, model_column) = chrome.section("选择内置模型");
    let models = Choice::builder(&model_card).build();
    model_column.add(&models, 0, SizerFlag::Expand, 0);
    let model_detail = squad_dialog::note(&model_card, "");
    model_column.add(
        &model_detail,
        0,
        SizerFlag::Expand | SizerFlag::Top,
        model_card.from_dip(8),
    );
    let (targets_host, targets_column) = chrome.section("保存到客户端（可多选）");
    let targets_empty = squad_dialog::note(&targets_host, "未发现可保存的客户端。");
    targets_column.add(&targets_empty, 0, SizerFlag::Expand, 0);
    // The existing section owns the destination controls and their native labels.
    let target_choices = Panel::builder(&targets_host).build();
    target_choices.set_background_color(targets_host.get_background_color());
    target_choices.set_sizer(BoxSizer::builder(Orientation::Vertical).build(), false);
    targets_column.add(&target_choices, 0, SizerFlag::Expand, 0);
    let outcome = squad_dialog::note(&targets_host, "");
    outcome.hide();
    targets_column.add(
        &outcome,
        0,
        SizerFlag::Expand | SizerFlag::Top,
        targets_host.from_dip(8),
    );
    let read = Button::builder(&chrome.dialog)
        .with_label("刷新状态")
        .with_kind(ButtonKind::Ghost)
        .build();
    let close = Button::builder(&chrome.dialog)
        .with_id(ID_CANCEL)
        .with_label("关闭")
        .build();
    let share = Button::builder(&chrome.dialog)
        .with_label("保存到所选客户端")
        .with_kind(ButtonKind::Primary)
        .build();
    for button in [read, close, share] {
        chrome
            .actions
            .add(&button, 0, SizerFlag::Left, chrome.dialog.from_dip(8));
    }
    let panel = SharePanel {
        dialog: chrome.dialog,
        body: chrome.body,
        job_owner: state_card,
        account,
        detail,
        models,
        model_detail,
        targets_host: target_choices,
        targets_empty,
        choices: Default::default(),
        selected: Default::default(),
        read,
        enable,
        disable,
        share,
        close,
        outcome,
        view: Default::default(),
        busy: Default::default(),
    };
    panel.update_controls();
    let changed = panel.clone();
    models.on_selection_changed(move |_| {
        changed.update_controls();
        changed.layout();
    });
    let resized = panel.clone();
    chrome.dialog.on_size(move |event| {
        resized.layout();
        event.skip(true);
    });
    chrome.finish();
    (chrome, panel)
}

/// Only the explicit assistant page request reveals this form. No eager host startup.
pub(super) fn show(parent: &Frame, api: &ApiClient, on_open: impl FnOnce(bool) + 'static) {
    let (chrome, panel) = panel(parent);
    let job = Rc::new(InlineJob::new(panel.job_owner, chrome.status));
    for (button, operation) in [
        (panel.read, Operation::Read),
        (panel.enable, Operation::Enable),
        (panel.disable, Operation::Disable),
        (panel.share, Operation::Share),
    ] {
        let (panel, api, job) = (panel.clone(), api.clone(), Rc::downgrade(&job));
        button.on_click(move |_| {
            if let Some(job) = job.upgrade() {
                panel.run(&job, &api, operation);
            }
        });
    }
    let closing_job = Rc::downgrade(&job);
    chrome.dialog.on_close(move |event| {
        if closing_job.upgrade().is_some_and(|job| job.is_running()) {
            if let WindowEventData::General(event) = event {
                event.veto();
            }
        } else {
            event.skip(true);
        }
    });
    let closing_job = Rc::downgrade(&job);
    let dialog = chrome.dialog;
    panel.close.on_click(move |_| {
        if closing_job.upgrade().is_none_or(|job| !job.is_running()) {
            dialog.end_modal(ID_CANCEL);
        }
    });
    let receipt = OpenReceipt::new(on_open);
    let (opened, first_read, api, pending) = (
        receipt.clone(),
        panel.clone(),
        api.clone(),
        Rc::downgrade(&job),
    );
    chrome.dialog.on_idle(move |event| {
        if opened.observe(dialog.is_shown())
            && let Some(job) = pending.upgrade()
        {
            first_read.run(&job, &api, Operation::Read);
        }
        event.skip(true);
    });
    chrome.show_modal();
    receipt.finish();
    drop(job);
    chrome.dialog.destroy();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_open_reads_only_and_visible_receipt_does_not_wait_for_close() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let recorded = calls.clone();
        let receipt = OpenReceipt::new(move |opened| recorded.borrow_mut().push(opened));
        assert!(!receipt.observe(false));
        assert!(calls.borrow().is_empty());
        assert!(receipt.observe(true));
        assert_eq!(*calls.borrow(), [true]);
        assert!(!receipt.observe(true));
        receipt.finish();
        assert_eq!(*calls.borrow(), [true]);
        let not_opened = OpenReceipt::new({
            let calls = calls.clone();
            move |opened| calls.borrow_mut().push(opened)
        });
        not_opened.finish();
        assert_eq!(*calls.borrow(), [true, false]);
        let (path, body) = Operation::Read
            .request(&json!({"revision":"existing"}), None, &BTreeSet::new())
            .unwrap();
        assert_eq!(path, "/api/desktop/workbuddy-builtin/read");
        assert_eq!(body, json!({}));
        assert!(
            Operation::Share
                .request(&json!({"ready":false}), None, &BTreeSet::new())
                .is_err()
        );
    }

    #[test]
    fn destinations_exclude_self_hidden_and_inherited_and_share_uses_catalog_ids() {
        let view = json!({"revision":"current","ready":true,"targets":[
            {"id":"wb","client":"workbuddy"},
            {"id":"ide","client":"vscode"},
            {"id":"webstorm","client":"webstorm"},
            {"id":"inherited","client":"codex","modelsInherited":true},
            {"id":"codex-main","client":"codex","label":"默认"},
            {"id":"tg-office","client":"tiangong","label":"办公"}
        ]});
        let destinations = targets(&view);
        assert_eq!(destinations.len(), 2);
        assert_eq!(destinations[0].label, "ChatGPT / Codex");
        assert_eq!(destinations[1].label, "天工 Work · 办公");
        let selected = destinations.into_iter().map(|target| target.id).collect();
        let (path, body) = Operation::Share
            .request(
                &view,
                Some(&json!({"id":"actual-runtime-model"})),
                &selected,
            )
            .unwrap();
        assert_eq!(path, "/api/desktop/workbuddy-builtin/share");
        assert_eq!(
            body,
            json!({"revision":"current","modelId":"actual-runtime-model","targets":["codex-main","tg-office"]})
        );
    }

    #[cfg(windows)]
    #[test]
    fn hidden_share_form_keeps_model_and_targets_and_shows_restart_guidance() {
        wxdragon::app::render_offscreen(|| {
            for mode in [theme::ThemeMode::Light, theme::ThemeMode::Dark] {
                theme::init(mode);
                let parent = Frame::builder()
                    .with_title("hidden WorkBuddy sharing parent")
                    .with_size(Size::new(640, 520))
                    .build();
                let (chrome, panel) = panel(&parent);
                assert!(!panel.enable.is_enabled());
                assert!(!panel.share.is_enabled());
                assert!(!panel.dialog.is_shown());
                let mut view = json!({"revision":"one","enabled":false,"ready":false,
                    "needsRestart":true,"detail":"请先退出 WorkBuddy，再点击启用分享；现有任务不会自动关闭",
                    "models":[],"targets":[]});
                panel.apply(view.clone());
                assert!(panel.enable.is_enabled());
                assert!(!panel.disable.is_enabled());
                assert!(!panel.share.is_enabled());
                assert!(panel.detail.get_label().contains("先退出 WorkBuddy"));
                view["enabled"] = json!(true);
                view["ready"] = json!(true);
                view["loggedIn"] = json!(true);
                view["accountName"] = json!("合成测试账号");
                view["models"] = json!([
                    {"id":"one","name":"模型一","contextWindow":128000},
                    {"id":"two","name":"模型二","supportsThinking":true,"defaultThinkingLevel":"high"}
                ]);
                view["targets"] = json!([
                    {"id":"codex","client":"codex"},
                    {"id":"zcode","client":"zcode"},
                    {"id":"self","client":"workbuddy"}
                ]);
                panel.apply(view.clone());
                panel.models.set_selection(1);
                panel.selected.borrow_mut().insert("zcode".into());
                panel.apply(view.clone());
                assert_eq!(panel.selected_model().unwrap()["id"], "two");
                assert_eq!(panel.choices.borrow().len(), 2);
                assert!(panel.choices.borrow()[1].1.is_checked());
                assert!(panel.share.is_enabled());
                assert!(!panel.enable.is_enabled());
                assert!(panel.disable.is_enabled());
                assert!(panel.model_detail.get_label().contains("high"));
                let original_checkbox = panel.choices.borrow()[0].1.handle_ptr();
                view["models"].as_array_mut().unwrap().reverse();
                panel.apply(view.clone());
                assert_eq!(panel.selected_model().unwrap()["id"], "two");
                assert_eq!(panel.choices.borrow()[0].1.handle_ptr(), original_checkbox);
                view["outcomes"] = json!([
                    {"targetId":"zcode","ok":true},
                    {"targetId":"codex","ok":false,"message":"合成失败"}
                ]);
                panel.apply(view);
                assert!(panel.selected.borrow().is_empty());
                assert!(!panel.share.is_enabled());
                assert!(panel.outcome.get_label().contains("已保存到 1 个客户端"));
                assert!(panel.outcome.get_label().contains("合成失败"));
                let size = panel.dialog.get_client_size();
                assert!(panel.share.get_position().y + panel.share.get_size().height <= size.height);
                assert!(panel.body.get_size().height > 0);
                assert!(!panel.dialog.is_shown() && !parent.is_shown());
                dialog_job::finish_dialog(chrome.dialog);
                chrome.dialog.destroy();
                parent.destroy();
            }
        }).unwrap();
    }
}
