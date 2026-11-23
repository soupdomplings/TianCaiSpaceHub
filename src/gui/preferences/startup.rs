use crate::gui::{show_error, theme};
use crate::windows_startup::{self, Status};
use wxdragon::prelude::*;

pub(super) fn add(panel: &Panel, layout: &BoxSizer, frame: Frame) {
    let checkbox = CheckBox::builder(panel).with_label("开机启动 Hub").build();
    checkbox.set_font(&theme::font(theme::TextRole::Body));
    checkbox.set_foreground_color(theme::theme().ink_primary);
    checkbox.set_background_color(theme::theme().bg_card);
    layout.add(&checkbox, 0, SizerFlag::Expand, 0);
    let hint = StaticText::builder(panel).build();
    hint.set_font(&theme::font(theme::TextRole::Caption));
    hint.set_foreground_color(theme::theme().ink_secondary);
    layout.add(
        &hint,
        0,
        SizerFlag::Expand | SizerFlag::Top,
        panel.from_dip(4),
    );

    let refresh = move || {
        let state = windows_startup::read();
        checkbox.set_value(matches!(state, Ok(Status::On)));
        checkbox.enable(state.is_ok());
        let note = match state {
            Ok(Status::Off | Status::On) => "登录当前 Windows 账户时自动打开 Hub。".to_owned(),
            Ok(Status::OldLocation) => "启动项仍指向旧位置，勾选后更新为当前 Hub。".to_owned(),
            Ok(Status::DisabledByWindows) => {
                "已被系统停用，可在 Windows 设置 → 应用 → 启动中重新开启。".to_owned()
            }
            Err(error) => format!("读取开机启动设置失败：{error}"),
        };
        hint.set_label(&note);
    };
    refresh();
    checkbox.on_toggled(move |_| {
        let result = windows_startup::update(checkbox.is_checked());
        refresh();
        if let Err(error) = result {
            show_error(&frame, &format!("无法更新开机启动设置：{error}"));
        }
    });
    checkbox.on_set_focus(move |event| {
        refresh();
        event.skip(true);
    });
    frame.on_activate(move |event| {
        if matches!(&event, WindowEventData::Activate(data) if data.is_active()) {
            refresh();
        }
        event.skip(true);
    });
}
