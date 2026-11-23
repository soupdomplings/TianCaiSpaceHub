use super::*;
use crate::gui::{motion, onboarding};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub(super) struct MobilePanel {
    hint: StaticText,
    invite: Button,
    review: Button,
    revoke: Button,
}

impl MobilePanel {
    pub(super) fn new(parent: &Panel, list: &BoxSizer) -> Self {
        list.add(
            &label(parent, "微信小程序", theme::TextRole::Body),
            0,
            SizerFlag::Left | SizerFlag::Top,
            parent.from_dip(10),
        );
        let hint = label(parent, "正在读取手机配对状态…", theme::TextRole::Caption);
        list.add(
            &hint,
            0,
            SizerFlag::Expand | SizerFlag::All,
            parent.from_dip(10),
        );
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let invite = button(parent, "配对手机", ButtonKind::Secondary);
        let review = button(parent, "查看手机申请", ButtonKind::Secondary);
        let revoke = button(parent, "撤销手机授权…", ButtonKind::Ghost);
        for action in [invite, review, revoke] {
            row.add(&action, 0, SizerFlag::Right, parent.from_dip(8));
            action.enable(false);
        }
        list.add_sizer(
            &row,
            0,
            SizerFlag::Left | SizerFlag::Bottom,
            parent.from_dip(10),
        );
        Self {
            hint,
            invite,
            review,
            revoke,
        }
    }

    pub(super) fn update(&self, status: &Value, available: bool) {
        let mobile = &status["mobile"];
        let pending = entries(mobile, "pending");
        let bindings = entries(mobile, "bindings");
        let configured = mobile["configured"] == true;
        let enabled = status["enabled"] == true;
        let error = mobile["error"].as_str().filter(|text| !text.is_empty());
        let message = if let Some(error) = error {
            error.to_owned()
        } else if !configured {
            "手机连接服务尚未配置。请联系管理员配置小程序连接服务域名。".to_owned()
        } else if !enabled {
            "在「设置」开启远程连接后，即可用微信小程序配对。".to_owned()
        } else if !pending.is_empty() {
            format!("有 {} 台手机等待本机确认。", pending.len())
        } else if bindings.is_empty() {
            "打开微信小程序，扫描本机配对码。".to_owned()
        } else {
            format!(
                "已授权 {} 台手机，可从微信小程序打开本机会话。",
                bindings.len()
            )
        };
        self.hint.set_label(&message);
        self.invite.enable(available && configured && enabled);
        self.review
            .enable(available && enabled && !pending.is_empty());
        self.revoke.enable(available && !bindings.is_empty());
    }

    pub(super) fn bind(&self, ui: &RemotePage) {
        let page = ui.clone();
        self.invite.on_click(move |_| {
            let owner = page.clone();
            let known = entries(&page.state.borrow()["mobile"], "pending");
            page.post(
                "mobile/invite",
                json!({}),
                "生成手机配对码",
                move |result| {
                    if show_qr(&owner, &result, known) {
                        review(&owner);
                    }
                },
            );
        });
        let page = ui.clone();
        self.review.on_click(move |_| review(&page));
        let page = ui.clone();
        self.revoke.on_click(move |_| {
            let rows = entries(&page.state.borrow()["mobile"], "bindings");
            if let Some(device) = choose(
                &page.frame,
                &rows,
                "撤销手机授权",
                "选择要撤销的手机，确认后将断开它的远程访问。",
            ) {
                page.post(
                    "mobile/revoke",
                    json!({"bindingId":device.id}),
                    "撤销手机授权",
                    |_| {},
                );
            }
        });
    }
}

#[derive(Clone)]
struct Device {
    id: String,
    name: String,
}

fn entries(mobile: &Value, field: &str) -> Vec<Device> {
    mobile[field]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let id = row["id"].as_str().filter(|v| !v.is_empty())?.to_owned();
            let name = row["name"]
                .as_str()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or("手机")
                .to_owned();
            Some(Device { id, name })
        })
        .collect()
}

fn choose(parent: &Frame, rows: &[Device], title: &str, prompt: &str) -> Option<Device> {
    if rows.is_empty() {
        return None;
    }
    let names = rows
        .iter()
        .map(|r| {
            if rows.iter().filter(|other| other.name == r.name).count() > 1 {
                format!("{} · {}", r.name, r.id)
            } else {
                r.name.clone()
            }
        })
        .collect::<Vec<_>>();
    let dialog = SingleChoiceDialog::builder(
        parent,
        prompt,
        title,
        &names.iter().map(String::as_str).collect::<Vec<_>>(),
    )
    .build();
    let selected = (dialog.show_modal() == ID_OK).then(|| dialog.get_selection() as usize);
    dialog.destroy();
    selected.and_then(|index| rows.get(index).cloned())
}

fn review(page: &RemotePage) {
    let rows = entries(&page.state.borrow()["mobile"], "pending");
    let candidate = match rows.as_slice() {
        [] => return,
        [only] => only.clone(),
        _ => match choose(
            &page.frame,
            &rows,
            "查看手机申请",
            "请选择要处理的手机配对申请。",
        ) {
            Some(selected) => selected,
            None => return,
        },
    };
    let dialog = Dialog::builder(&page.frame, "手机请求配对")
        .with_size(460, 205)
        .build();
    motion::install_dialog(dialog);
    let content = Panel::builder(&dialog).build();
    content.set_background_color(theme::theme().bg_app);
    let stack = BoxSizer::builder(Orientation::Vertical).build();
    let prompt = label(
        &content,
        &format!("允许「{}」通过微信访问本机的远程会话？", candidate.name),
        theme::TextRole::Body,
    );
    prompt.wrap(content.from_dip(405));
    stack.add(
        &prompt,
        0,
        SizerFlag::Expand | SizerFlag::All,
        content.from_dip(16),
    );
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    row.add_stretch_spacer(1);
    for (caption, kind, response) in [
        ("稍后处理", ButtonKind::Ghost, ID_CANCEL),
        ("拒绝", ButtonKind::Secondary, ID_NO),
        ("允许配对", ButtonKind::Primary, ID_YES),
    ] {
        let action = button(&content, caption, kind);
        row.add(&action, 0, SizerFlag::Left, content.from_dip(8));
        action.on_click(move |_| dialog.end_modal(response));
    }
    stack.add_stretch_spacer(1);
    stack.add_sizer(
        &row,
        0,
        SizerFlag::Expand | SizerFlag::All,
        content.from_dip(12),
    );
    content.set_sizer(stack, false);
    let shell = BoxSizer::builder(Orientation::Vertical).build();
    shell.add(&content, 1, SizerFlag::Expand, 0);
    dialog.set_sizer(shell, false);
    dialog.center();
    let response = dialog.show_modal();
    dialog.destroy();
    if matches!(response, ID_YES | ID_NO) {
        let approved = response == ID_YES;
        page.post(
            "mobile/decide",
            json!({"bindingId":candidate.id,"allow":approved}),
            if approved {
                "允许手机配对"
            } else {
                "拒绝手机配对"
            },
            |_| {},
        );
    }
}

fn show_qr(page: &RemotePage, invitation: &Value, known: Vec<Device>) -> bool {
    let Some((bitmap, _)) = invitation["qr"]
        .as_str()
        .filter(|v| !v.is_empty())
        .and_then(onboarding::qr_bitmap)
    else {
        feedback::error(&page.page, "配对码生成失败，请重新点「配对手机」。");
        return false;
    };
    let dialog = Dialog::builder(&page.frame, "配对手机")
        .with_size(440, 480)
        .build();
    motion::install_dialog(dialog);
    let content = Panel::builder(&dialog).build();
    content.set_background_color(theme::theme().bg_app);
    let stack = BoxSizer::builder(Orientation::Vertical).build();
    let help = label(
        &content,
        "打开微信小程序，在「远程连接」中扫码。",
        theme::TextRole::Body,
    );
    stack.add(&help, 0, SizerFlag::All, content.from_dip(16));
    let qr = StaticBitmap::builder(&content)
        .with_bitmap(Some(bitmap))
        .with_scale_mode(Some(ScaleMode::AspectFit))
        .with_size(Size::new(300, 300))
        .build();
    stack.add(
        &qr,
        1,
        SizerFlag::Expand | SizerFlag::All,
        content.from_dip(10),
    );
    let close = button(&content, "关闭", ButtonKind::Secondary);
    stack.add(
        &close,
        0,
        SizerFlag::AlignRight | SizerFlag::All,
        content.from_dip(12),
    );
    close.on_click(move |_| dialog.end_modal(ID_CANCEL));
    content.set_sizer(stack, false);
    let shell = BoxSizer::builder(Orientation::Vertical).build();
    shell.add(&content, 1, SizerFlag::Expand, 0);
    dialog.set_sizer(shell, false);
    dialog.center();
    let timer = Timer::new(&dialog);
    let state = page.state.clone();
    let expires = invitation["expiresAt"].as_u64().map(|time| {
        if time < 100_000_000_000 {
            time
        } else {
            time / 1000
        }
    });
    timer.on_tick(move |_| {
        if entries(&state.borrow()["mobile"], "pending")
            .iter()
            .any(|p| !known.iter().any(|k| k.id == p.id))
        {
            dialog.end_modal(ID_OK);
        } else if expires.is_some_and(|end| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .is_ok_and(|now| now.as_secs() >= end)
        }) {
            qr.hide();
            help.set_label("配对码已过期，请关闭并重新生成。");
            content.layout();
        }
    });
    timer.start(750, false);
    let scanned = dialog.show_modal() == ID_OK;
    timer.stop();
    dialog.destroy();
    scanned
}

#[cfg(all(test, windows))]
#[test]
fn offline_status_controls_pairing_without_consuming_a_request() {
    wxdragon::app::render_offscreen(|| {
        theme::init(theme::ThemeMode::Light);
        let frame = Frame::builder().build();
        let host = Panel::builder(&frame).build();
        let layout = BoxSizer::builder(Orientation::Vertical).build();
        let mobile = MobilePanel::new(&host, &layout);
        host.set_sizer(layout, false);
        mobile.update(&json!({"enabled":true,"mobile":{"configured":false}}), true);
        assert!(!mobile.invite.is_enabled());
        assert!(mobile.hint.get_label().contains("域名"));
        let state = json!({"enabled":true,"mobile":{"configured":true,
            "pending":[{"id":"p","name":"我的手机"}],"bindings":[{"id":"b","name":"我的手机"}]}});
        mobile.update(&state, true);
        assert!(
            mobile.invite.is_enabled() && mobile.review.is_enabled() && mobile.revoke.is_enabled()
        );
        assert_eq!(entries(&state["mobile"], "pending")[0].id, "p");
        assert_eq!(entries(&state["mobile"], "bindings")[0].id, "b");
        mobile.update(&state, false);
        assert!(
            !mobile.invite.is_enabled()
                && !mobile.review.is_enabled()
                && !mobile.revoke.is_enabled()
        );
        frame.destroy();
    })
    .unwrap();
}
