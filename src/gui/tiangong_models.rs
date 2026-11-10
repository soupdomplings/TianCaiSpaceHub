//! TianGong account and built-in model actions within its model workspace.
use super::{
    api::ApiClient,
    controls::{Button, ButtonKind, Choice, TextCtrl},
    dialog_job, theme, visual,
};
use crate::desktop::clients::ClientId;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};
use wxdragon::prelude::*;

// Public download links from the official page, checked 2026-11-10.
// The website entry remains available when a future version replaces them.
const DOWNLOADS: [(&str, &str); 4] = [
    (
        "Windows · x64",
        "https://tiangong.jiuqi.com.cn/updates/tiangong-claw/女娲天工Work-1.0.4-win-x64-setup.exe",
    ),
    (
        "Mac · Apple 芯片",
        "https://tiangong.jiuqi.com.cn/updates/tiangong-claw/女娲天工Work-1.0.4-mac-arm64.dmg",
    ),
    (
        "Mac · Intel 芯片",
        "https://tiangong.jiuqi.com.cn/updates/tiangong-claw/女娲天工Work-1.0.4-mac-x64.dmg",
    ),
    (
        "打开下载官网",
        "https://tiangong.jiuqi.com.cn/tiangongwork/#download",
    ),
];

pub(super) fn bind_download(button: Button) {
    let parent = button.get_parent().unwrap();
    button.on_click(move |_| {
        const FIRST: i32 = 15_220;
        let mut menu = Menu::builder().build();
        for (i, (label, _)) in DOWNLOADS.iter().enumerate() {
            if i == 3 {
                menu.append_separator();
            }
            menu.append(
                FIRST + i as i32,
                label,
                "在默认浏览器中打开官方下载地址",
                ItemKind::Normal,
            );
        }
        menu.on_selected(move |event| {
            let Some((_, url)) = event
                .get_id()
                .checked_sub(FIRST)
                .and_then(|i| DOWNLOADS.get(i as usize))
            else {
                return;
            };
            let text = super::text::GuiText::new(super::text::GuiLocale::ZhCn);
            if let Err(error) = super::browser::open_url_in_browser(text, url) {
                MessageDialog::builder(
                    &parent,
                    &format!(
                        "{error}\n可在浏览器访问：https://tiangong.jiuqi.com.cn/tiangongwork/"
                    ),
                    "无法打开天工下载",
                )
                .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconError)
                .build()
                .show_modal();
            }
        });
        let position = button.client_to_screen(Point::new(0, button.get_size().height));
        button.popup_menu(&mut menu, Some(position));
    });
}

#[derive(Clone)]
pub(super) struct AccountBar {
    pub panel: Panel,
    pub login: Button,
    pub test: Button,
    pub view: Rc<RefCell<Value>>,
    label: StaticText,
}
impl AccountBar {
    pub fn new(parent: &dyn WxWidget) -> Self {
        let panel = Panel::builder(parent).build();
        panel.set_background_color(theme::theme().bg_app);
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let label = StaticText::builder(&panel)
            .with_label("天工账号 · 未登录")
            .build();
        label.set_foreground_color(theme::theme().ink_secondary);
        row.add(&label, 1, SizerFlag::AlignCenterVertical, 0);
        let login = Button::builder(&panel)
            .with_label("天工登录")
            .with_icon(visual::Icon::Tiangong)
            .with_kind(ButtonKind::Secondary)
            .build();
        let test = Button::builder(&panel)
            .with_label("接入测试")
            .with_kind(ButtonKind::Ghost)
            .build();
        row.add(&test, 0, SizerFlag::Left, panel.from_dip(8));
        row.add(&login, 0, SizerFlag::Left, panel.from_dip(8));
        panel.set_sizer(row, false);
        Self {
            panel,
            login,
            test,
            label,
            view: Default::default(),
        }
    }
    pub fn apply(&self, view: Value) {
        let logged = view["loggedIn"] == true;
        let error = view["accountError"]
            .as_str()
            .or_else(|| view["error"].as_str())
            .or_else(|| {
                view["parameterSyncErrors"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(Value::as_str)
            })
            .unwrap_or("");
        self.label.set_label(&if !error.is_empty() {
            "天工模型需要处理 · 重新读取后核对".into()
        } else if logged {
            format!(
                "天工账号 · {}",
                view["accountName"].as_str().unwrap_or("已登录")
            )
        } else if view["localSessionAvailable"] == true {
            "已发现天工登录 · 等待读取".into()
        } else {
            "内置模型 · 沿用天工配置".into()
        });
        self.login.set_label(if logged {
            "切换账号"
        } else {
            "天工登录"
        });
        self.label.set_foreground_color(if !error.is_empty() {
            theme::theme().error
        } else {
            theme::theme().ink_secondary
        });
        self.label.set_tooltip(error);
        *self.view.borrow_mut() = view;
        self.panel.layout();
    }
    pub fn models(&self) -> Vec<Value> {
        let view = self.view.borrow();
        view["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["configured"] == true)
            .cloned()
            .collect()
    }
}

struct LoginFields {
    dialog: Dialog,
    username: TextCtrl,
    password: TextCtrl,
    status: StaticText,
    read: Button,
    login: Button,
}
struct IdentityFields {
    dialog: Dialog,
    model: Choice,
    mode: Choice,
    status: StaticText,
    send: Button,
}
fn identity_fields(parent: &Frame, view: &Value) -> IdentityFields {
    let dialog = Dialog::builder(parent, "天工模型接入测试")
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .with_size(parent.from_dip(550), parent.from_dip(420))
        .build();
    dialog.set_background_color(theme::theme().bg_card);
    dialog.set_font(&theme::font(theme::TextRole::Body));
    let col = BoxSizer::builder(Orientation::Vertical).build();
    let title = StaticText::builder(&dialog)
        .with_label("选择一组身份测试")
        .build();
    title.set_font(&theme::font(theme::TextRole::Title));
    title.set_foreground_color(theme::theme().ink_primary);
    col.add(&title, 0, SizerFlag::All, dialog.from_dip(24));
    let grid = FlexGridSizer::builder(0, 2)
        .with_hgap(dialog.from_dip(14))
        .with_vgap(dialog.from_dip(14))
        .build();
    grid.add_growable_col(1, 1);
    let model = Choice::builder(&dialog).build();
    for item in view["models"].as_array().into_iter().flatten() {
        model.append(item["name"].as_str().unwrap_or("内置模型"));
    }
    if model.get_count() > 0 {
        model.set_selection(0);
    }
    let mode = Choice::builder(&dialog).build();
    for label in [
        "原生方式 · 不附个人 ID",
        "当前登录账号",
        "随机 ID · 仅本次测试",
    ] {
        mode.append(label);
    }
    mode.set_selection(0);
    for (name, control) in [("模型", model), ("身份", mode)] {
        let label = StaticText::builder(&dialog).with_label(name).build();
        label.set_foreground_color(theme::theme().ink_secondary);
        grid.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
        grid.add(&control, 1, SizerFlag::Expand, 0);
    }
    col.add_sizer(
        &grid,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        dialog.from_dip(24),
    );
    let hint=StaticText::builder(&dialog).with_label("点击后向天工服务发送一次简短请求。随机 ID 每次重新生成，测试不会更改当前账号或模型配置。").build();
    hint.set_foreground_color(theme::theme().ink_secondary);
    hint.wrap(dialog.from_dip(490));
    col.add(
        &hint,
        0,
        SizerFlag::Expand | SizerFlag::All,
        dialog.from_dip(24),
    );
    let status = StaticText::builder(&dialog).build();
    status.wrap(dialog.from_dip(490));
    status.set_foreground_color(theme::theme().ink_primary);
    col.add(
        &status,
        1,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        dialog.from_dip(24),
    );
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    row.add_stretch_spacer(1);
    let close = Button::builder(&dialog)
        .with_id(ID_CANCEL)
        .with_label("关闭")
        .with_kind(ButtonKind::Ghost)
        .build();
    let send = Button::builder(&dialog).with_label("发送测试请求").build();
    row.add(&close, 0, SizerFlag::Right, dialog.from_dip(8));
    row.add(&send, 0, SizerFlag::default(), 0);
    col.add_sizer(
        &row,
        0,
        SizerFlag::Expand | SizerFlag::All,
        dialog.from_dip(24),
    );
    dialog.set_sizer(col, false);
    dialog.set_min_size(visual::dip_size(&dialog, 510, 390));
    IdentityFields {
        dialog,
        model,
        mode,
        status,
        send,
    }
}
pub(super) fn show_identity_test(parent: &Frame, api: &ApiClient) {
    let Some(view) = dialog_job::run(
        parent,
        api.clone(),
        "读取内置模型",
        "/api/desktop/tiangong-models/read",
        json!({}),
    ) else {
        return;
    };
    let IdentityFields {
        dialog,
        model,
        mode,
        status,
        send,
    } = identity_fields(parent, &view);
    let job = dialog_job::bind_open(
        dialog,
        send,
        status,
        api.clone(),
        move || {
            let item = model
                .get_selection()
                .and_then(|i| view["models"].as_array().and_then(|m| m.get(i as usize)))
                .ok_or("请先重新读取天工内置模型")?;
            Ok((
                "/api/desktop/tiangong-models/test-identity".into(),
                json!({"revision":view["revision"],"modelId":item["id"],"mode":(["native","account","random"][mode.get_selection().unwrap_or(0) as usize])}),
            ))
        },
        move |result| {
            status.set_label(&format!(
                "HTTP {} · {}{}",
                result["httpStatus"],
                result["message"].as_str().unwrap_or("测试未完成"),
                result["testUserId"]
                    .as_str()
                    .map(|id| format!("\n测试 ID：{id}"))
                    .unwrap_or_default()
            ));
            status.set_foreground_color(if result["ok"] == true {
                theme::theme().ok
            } else {
                theme::theme().error
            });
            status.wrap(dialog.from_dip(490));
            dialog.layout();
        },
    );
    dialog.centre();
    dialog.show_modal();
    drop(job);
    dialog.destroy();
}
fn login_fields(parent: &Frame, view: &Value) -> LoginFields {
    let dialog = Dialog::builder(parent, "天工登录")
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .with_size(parent.from_dip(510), parent.from_dip(390))
        .build();
    dialog.set_background_color(theme::theme().bg_card);
    dialog.set_font(&theme::font(theme::TextRole::Body));
    let col = BoxSizer::builder(Orientation::Vertical).build();
    let title = StaticText::builder(&dialog)
        .with_label("登录天工 Work")
        .build();
    title.set_foreground_color(theme::theme().ink_primary);
    title.set_font(&theme::font(theme::TextRole::Title));
    col.add(&title, 0, SizerFlag::All, dialog.from_dip(24));
    let grid = FlexGridSizer::builder(0, 2)
        .with_hgap(dialog.from_dip(14))
        .with_vgap(dialog.from_dip(14))
        .build();
    grid.add_growable_col(1, 1);
    let username = TextCtrl::builder(&dialog).build();
    username.set_hint("天工账号");
    let password = TextCtrl::builder(&dialog)
        .with_style(TextCtrlStyle::Password)
        .build();
    for (label, input) in [("账号", username), ("密码", password)] {
        let text = StaticText::builder(&dialog).with_label(label).build();
        text.set_foreground_color(theme::theme().ink_secondary);
        grid.add(&text, 0, SizerFlag::AlignCenterVertical, 0);
        input.set_min_size(visual::dip_size(&dialog, 250, 36));
        grid.add(&input, 1, SizerFlag::Expand, 0);
    }
    col.add_sizer(
        &grid,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        dialog.from_dip(24),
    );
    let hint = StaticText::builder(&dialog)
        .with_label(if view["loggedIn"] == true {
            "切换后，已分享的内置模型使用新账号；模型参数继续沿用天工配置。密码不会保存。"
        } else {
            "个人登录用于关联天工账号；内置模型可先接入，实际权限由服务端决定。密码不会保存。"
        })
        .build();
    hint.set_foreground_color(theme::theme().ink_secondary);
    hint.wrap(dialog.from_dip(450));
    col.add(
        &hint,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        dialog.from_dip(24),
    );
    let status = StaticText::builder(&dialog)
        .with_label(
            view["accountError"]
                .as_str()
                .or_else(|| view["error"].as_str())
                .unwrap_or(""),
        )
        .build();
    status.set_foreground_color(theme::theme().error);
    status.wrap(dialog.from_dip(450));
    col.add(
        &status,
        1,
        SizerFlag::Expand | SizerFlag::All,
        dialog.from_dip(24),
    );
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    let read = Button::builder(&dialog)
        .with_label("读取天工登录")
        .with_kind(ButtonKind::Secondary)
        .build();
    row.add(&read, 0, SizerFlag::Right, dialog.from_dip(8));
    row.add_stretch_spacer(1);
    let cancel = Button::builder(&dialog)
        .with_id(ID_CANCEL)
        .with_label("取消")
        .build();
    let login = Button::builder(&dialog).with_label("登录").build();
    login.set_default();
    row.add(&cancel, 0, SizerFlag::Right, dialog.from_dip(8));
    row.add(&login, 0, SizerFlag::default(), 0);
    col.add_sizer(
        &row,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        dialog.from_dip(24),
    );
    dialog.set_sizer(col, false);
    dialog.set_min_size(visual::dip_size(&dialog, 470, 370));
    LoginFields {
        dialog,
        username,
        password,
        status,
        read,
        login,
    }
}
pub(super) fn show(parent: &Frame, api: &ApiClient) -> bool {
    let Some(view) = dialog_job::run(
        parent,
        api.clone(),
        "读取天工登录",
        "/api/desktop/tiangong-models/read",
        json!({}),
    ) else {
        return false;
    };
    let LoginFields {
        dialog,
        username,
        password,
        status,
        read,
        login,
    } = login_fields(parent, &view);
    let read_saved = Rc::new(std::cell::Cell::new(false));
    let flag = read_saved.clone();
    let job = dialog_job::bind(dialog, login, status, api.clone(), move || {
        let saved = flag.replace(false);
        let (name, pass) = if saved {
            (String::new(), String::new())
        } else {
            (username.get_value(), password.get_value())
        };
        if !saved && (name.trim().is_empty() || pass.is_empty()) {
            return Err("请填写天工账号和密码，或读取已登录的天工账号".into());
        }
        Ok((
            "/api/desktop/tiangong-models/connect".into(),
            json!({"revision":view["revision"],"username":name,"password":pass}),
        ))
    });
    let start = job.start.clone();
    read.on_click(move |_| {
        read_saved.set(true);
        start();
    });
    dialog.centre();
    let accepted = dialog.show_modal() == ID_OK;
    drop(job);
    dialog.destroy();
    accepted
}

struct ShareFields {
    dialog: Dialog,
    status: StaticText,
    save: Button,
    selected: Rc<RefCell<BTreeSet<String>>>,
    tiles: Vec<(String, Button)>,
}
fn share_fields(
    parent: &Frame,
    model: &Value,
    view: &Value,
    mut targets: Vec<Value>,
) -> ShareFields {
    let dialog = Dialog::builder(parent, "分享天工内置模型")
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .with_size(parent.from_dip(620), parent.from_dip(460))
        .build();
    dialog.set_background_color(theme::theme().bg_card);
    dialog.set_font(&theme::font(theme::TextRole::Body));
    let col = BoxSizer::builder(Orientation::Vertical).build();
    let title = StaticText::builder(&dialog)
        .with_label(model["name"].as_str().unwrap_or("天工内置模型"))
        .build();
    title.set_font(&theme::font(theme::TextRole::Title));
    title.set_foreground_color(theme::theme().ink_primary);
    col.add(&title, 0, SizerFlag::All, dialog.from_dip(24));
    let scroll = ScrolledWindow::builder(&dialog).build();
    scroll.set_background_color(theme::theme().bg_card);
    scroll.set_scroll_rate(0, dialog.from_dip(12));
    let grid = GridSizer::builder(0, 3)
        .with_hgap(dialog.from_dip(10))
        .with_vgap(dialog.from_dip(10))
        .build();
    let selected: Rc<RefCell<BTreeSet<String>>> = Default::default();
    let mut tiles = Vec::new();
    let used = |target: &Value| {
        view["shares"].as_array().is_some_and(|shares| {
            shares.iter().any(|s| {
                s["modelId"] == model["id"]
                    && (s["targetId"] == target["id"]
                        || (s["targetId"].is_null() && s["client"] == target["client"]))
            })
        })
    };
    targets.sort_by_key(|target| !used(target));
    for target in &targets {
        let Ok(id) = serde_json::from_value::<ClientId>(target["client"].clone()) else {
            continue;
        };
        if id == ClientId::Tiangong || target["modelsInherited"] == true {
            continue;
        }
        let Some(key) = target["id"].as_str() else {
            continue;
        };
        let key = key.to_string();
        let label = if target["label"]
            .as_str()
            .is_some_and(|s| !matches!(s, "默认" | "Default"))
        {
            format!("{} · {}", id.name(), target["label"].as_str().unwrap())
        } else {
            id.name().into()
        };
        let button = Button::builder(&scroll)
            .with_label(&label)
            .with_icon(visual::Icon::client(id))
            .with_kind(ButtonKind::Tile)
            .with_size(Size::new(160, 74))
            .build();
        button.set_tooltip("选择要使用此模型的客户端，确认后写入配置；模型参数沿用天工");
        if used(target) {
            button.set_selected(true);
            button.enable(false);
            button.set_tooltip("已使用此模型，参数沿用天工来源");
        }
        let choices = selected.clone();
        let target_id = key.clone();
        button.on_click(move |_| {
            let mut choices = choices.borrow_mut();
            if !choices.remove(&target_id) {
                choices.insert(target_id.clone());
            }
            button.set_selected(choices.contains(&target_id));
        });
        grid.add(&button, 0, SizerFlag::Expand, 0);
        tiles.push((key, button));
    }
    scroll.set_sizer(grid, false);
    col.add(
        &scroll,
        1,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        dialog.from_dip(24),
    );
    let status = StaticText::builder(&dialog)
        .with_label("选择客户端后确认分享。各端模型参数可独立调整。")
        .build();
    status.set_foreground_color(theme::theme().ink_secondary);
    status.wrap(dialog.from_dip(560));
    status.set_min_size(visual::dip_size(&dialog, 0, 64));
    col.add(
        &status,
        0,
        SizerFlag::Expand | SizerFlag::All,
        dialog.from_dip(24),
    );
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    row.add_stretch_spacer(1);
    let close = Button::builder(&dialog)
        .with_id(ID_CANCEL)
        .with_label("关闭")
        .build();
    let save = Button::builder(&dialog).with_label("确认分享").build();
    save.enable(!tiles.is_empty());
    row.add(&close, 0, SizerFlag::Right, dialog.from_dip(10));
    row.add(&save, 0, SizerFlag::default(), 0);
    col.add_sizer(
        &row,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        dialog.from_dip(24),
    );
    dialog.set_sizer(col, false);
    dialog.set_min_size(visual::dip_size(&dialog, 580, 430));
    ShareFields {
        dialog,
        status,
        save,
        selected,
        tiles,
    }
}
pub(super) fn share(parent: &Frame, api: &ApiClient, model: &Value) -> bool {
    let Some(view) = dialog_job::run(
        parent,
        api.clone(),
        "读取可分享的客户端",
        "/api/desktop/tiangong-models/read",
        json!({}),
    ) else {
        return false;
    };
    let library = match api.get_quick_json("/api/desktop/models") {
        Ok(v) => v,
        Err(e) => {
            super::show_error(parent, &e);
            return false;
        }
    };
    let targets = library["targets"].as_array().cloned().unwrap_or_default();
    let ShareFields {
        dialog,
        status,
        save,
        selected,
        tiles,
    } = share_fields(parent, model, &view, targets);
    let revision = Rc::new(RefCell::new(view["revision"].clone()));
    let expected = revision.clone();
    let model_id = model["id"].clone();
    let choices = selected.clone();
    let changed = Rc::new(std::cell::Cell::new(false));
    let committed = changed.clone();
    let job = dialog_job::bind_open(
        dialog,
        save,
        status,
        api.clone(),
        move || {
            let targets = choices.borrow().iter().cloned().collect::<Vec<_>>();
            if targets.is_empty() {
                return Err("请选择要使用此模型的客户端".into());
            }
            Ok((
                "/api/desktop/tiangong-models/share".into(),
                json!({"revision":expected.borrow().clone(),"modelId":model_id,"targets":targets}),
            ))
        },
        move |result| {
            *revision.borrow_mut() = result["revision"].clone();
            let mut messages = Vec::new();
            let mut success = 0;
            for outcome in result["outcomes"].as_array().into_iter().flatten() {
                if let Some((_, tile)) = tiles
                    .iter()
                    .find(|(id, _)| outcome["targetId"] == id.as_str())
                {
                    tile.set_selected(outcome["ok"] == true);
                    if outcome["ok"] == true {
                        tile.enable(false);
                        selected
                            .borrow_mut()
                            .remove(outcome["targetId"].as_str().unwrap_or(""));
                        success += 1;
                        committed.set(true);
                    } else {
                        messages.push(format!(
                            "{}：{}",
                            tile.get_label(),
                            outcome["message"].as_str().unwrap_or("请重新读取")
                        ));
                    }
                }
            }
            if success > 0 {
                messages.insert(
                    0,
                    format!("已分享至 {success} 个客户端。新建会话或重开客户端后选用。"),
                );
            }
            if messages.len() == 1 && success > 0 {
                messages.push("Codex 首次使用仍须完成接入设置。".into());
            }
            status.set_label(&messages.join("\n"));
            status.set_foreground_color(
                if result["outcomes"]
                    .as_array()
                    .is_some_and(|rows| rows.iter().any(|r| r["ok"] != true))
                {
                    theme::theme().error
                } else {
                    theme::theme().ok
                },
            );
            status.wrap(dialog.from_dip(560));
            dialog.layout();
        },
    );
    dialog.centre();
    dialog.show_modal();
    drop(job);
    dialog.destroy();
    changed.get()
}

#[cfg(all(test, windows))]
#[test]
fn native_tiangong_login_form_fits_and_keeps_secrets_out_of_captures() {
    wxdragon::app::render_offscreen(|| {
        for mode in [theme::ThemeMode::Light,theme::ThemeMode::Dark] {
            theme::init(mode);
            let frame=Frame::builder().with_title("hidden synthetic login parent").build();
            let fields=login_fields(&frame,&json!({"localSessionAvailable":true}));
            fields.dialog.layout();
            assert!(fields.username.get_value().is_empty());assert!(fields.password.get_value().is_empty());
            let size=fields.dialog.get_client_size();
            assert!(fields.login.get_position().y+fields.login.get_size().height<=size.height);
            assert!(!fields.dialog.is_shown());
            super::native_model_review::capture(&fields.dialog,if mode==theme::ThemeMode::Light{"tiangong-login-light"}else{"tiangong-login-dark"});
            fields.dialog.destroy();
            let probe=identity_fields(&frame,&json!({"models":[{"id":"tiangong-builtin","name":"天工内置-外网"},{"id":"tiangong-builtin-intranet","name":"天工内置-内网"}]}));
            probe.dialog.layout();assert_eq!(probe.model.get_count(),2);assert_eq!(probe.mode.get_count(),3);assert_eq!(probe.mode.get_selection(),Some(0));
            let size=probe.dialog.get_client_size();assert!(probe.send.get_position().y+probe.send.get_size().height<=size.height);
            super::native_model_review::capture(&probe.dialog,if mode==theme::ThemeMode::Light{"tiangong-identity-test-light"}else{"tiangong-identity-test-dark"});probe.dialog.destroy();
            let model=json!({"id":"tiangong-builtin","name":"天工内置-外网"});
            let view=json!({"shares":[{"modelId":"tiangong-builtin","client":"workbuddy","targetId":"workbuddy-default"}]});
            let targets=ClientId::ALL.iter().map(|id|json!({"id":format!("{}-default",id.key()),"client":id,"label":"默认"})).collect();
            let share=share_fields(&frame,&model,&view,targets);share.dialog.layout();
            assert_eq!(share.tiles.len(),6);assert_eq!(share.tiles[0].0,"workbuddy-default");
            let target=&share.tiles[1];super::native_model_review::click(&target.1);
            assert!(share.selected.borrow().contains(&target.0));
            super::native_model_review::click(&target.1);assert!(share.selected.borrow().is_empty());
            let size=share.dialog.get_client_size();
            assert!(share.save.get_position().y+share.save.get_size().height<=size.height);
            super::native_model_review::capture(&share.dialog,if mode==theme::ThemeMode::Light{"tiangong-share-light"}else{"tiangong-share-dark"});
            share.dialog.destroy();frame.destroy();
        }
    }).unwrap();
}
