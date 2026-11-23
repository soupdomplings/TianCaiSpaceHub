use super::{
    controls::{Button, ButtonKind},
    shell::{self, PageId, Shell},
    theme,
};
use crate::{
    desktop::clients::ClientId,
    xshellb::{self, launcher, policy},
};
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
};
use wxdragon::prelude::*;
use wxdragon::timer::Timer;

pub(super) fn register(shell: &Shell) -> Rc<dyn Fn()> {
    let page = ScrolledWindow::builder(&shell.book)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    page.set_background_color(theme::theme().bg_app);
    page.set_font(&theme::font(theme::TextRole::Body));
    page.set_scroll_rate(0, page.from_dip(12));
    shell.register(PageId::XshellB, &page);
    let mounted = Cell::new(false);
    let on_visit = RefCell::new(None::<Rc<dyn Fn()>>);
    Rc::new(move || {
        if mounted.replace(true) {
            if let Some(refresh) = on_visit.borrow().as_ref() {
                refresh();
            }
        } else {
            match mount(page) {
                Ok(refresh) => *on_visit.borrow_mut() = Some(refresh),
                Err(error) => {
                    let column = BoxSizer::builder(Orientation::Vertical).build();
                    let status = StaticText::builder(&page)
                        .with_label(&error.to_string())
                        .build();
                    status.set_foreground_color(theme::theme().error);
                    column.add(
                        &status,
                        0,
                        SizerFlag::Expand | SizerFlag::All,
                        page.from_dip(16),
                    );
                    shell::inset_page_content(&page, column);
                    page.fit_inside();
                }
            }
        }
    })
}

fn mount(page: ScrolledWindow) -> anyhow::Result<Rc<dyn Fn()>> {
    let config = super::daemon::daemon_config_path()
        .ok_or_else(|| anyhow::anyhow!("找不到 Hub 配置目录"))?;
    let config =
        std::path::absolute(config).map_err(|_| anyhow::anyhow!("无法确定 Hub 配置目录"))?;
    let manual_config = config.clone();
    let content = BoxSizer::builder(Orientation::Vertical).build();
    let pad = page.from_dip(16);
    content.add_spacer(page.from_dip(8));
    let add_line = |label: &str| {
        let line = StaticText::builder(&page).with_label(label).build();
        content.add(&line, 0, SizerFlag::Bottom, page.from_dip(8));
    };
    let row = || BoxSizer::builder(Orientation::Horizontal).build();
    let place = |line: &BoxSizer| {
        content.add_sizer(line, 0, SizerFlag::Expand | SizerFlag::Bottom, pad);
    };

    add_line("1. 配置一个本机客户端，配置后在该客户端重开对话。");
    let client_row = row();
    let client = Choice::builder(&page).build();
    let clients = [ClientId::Codex, ClientId::WorkBuddy, ClientId::ZCode];
    for target in clients {
        client.append(target.name());
    }
    client.set_selection(0);
    client_row.add(&client, 1, SizerFlag::Right, 8);
    let enable = Button::builder(&page).with_label("配置客户端").build();
    let disable = Button::builder(&page).with_label("移除配置").build();
    client_row.add(&enable, 0, SizerFlag::Right, 8);
    client_row.add(&disable, 0, SizerFlag::default(), 0);
    place(&client_row);

    let permissions_root = xshellb::root(&config);
    let permission_row = row();
    let settings = Button::builder(&page).with_label("设置巡检权限…").build();
    let summary = StaticText::builder(&page).build();
    let update_permissions = move |root: &Path| {
        summary.set_label(&match policy::Policy::load(root) {
            Ok(p) if p.approved.is_empty() => "巡检命令每次询问".to_owned(),
            Ok(p) => format!("已自动同意 {} 条巡检命令", p.approved.len()),
            Err(_) => "巡检设置无法读取，暂不自动同意".to_owned(),
        });
    };
    update_permissions(&permissions_root);
    permission_row.add(&settings, 0, SizerFlag::Right, page.from_dip(12));
    permission_row.add(&summary, 1, SizerFlag::AlignCenterVertical, 0);
    place(&permission_row);
    settings.on_click(move |_| {
        if edit_permissions(page, permissions_root.clone()) {
            update_permissions(&permissions_root);
            page.layout();
            super::feedback::success(&page, "巡检权限已保存。");
        }
    });

    add_line(
        "2. 选好已登录的 Xshell 标签页。启动会替换该标签页正在运行的脚本，并允许已配置的客户端读取它。",
    );
    let window_row = row();
    let window = Choice::builder(&page).build();
    window.set_min_size(Size::new(page.from_dip(200), -1));
    window_row.add(&window, 1, SizerFlag::Right, 8);
    let refresh = Button::builder(&page).with_label("刷新").build();
    let start = Button::builder(&page)
        .with_label("启动脚本")
        .with_kind(ButtonKind::Primary)
        .build();
    window_row.add(&refresh, 0, SizerFlag::Right, 8);
    window_row.add(&start, 0, SizerFlag::default(), 0);
    place(&window_row);

    let attached = StaticText::builder(&page).build();
    attached.set_foreground_color(theme::theme().ink_secondary);
    content.add(&attached, 0, SizerFlag::Expand | SizerFlag::Bottom, pad);
    let tab_config = config.clone();
    let update_tabs: Rc<dyn Fn()> = Rc::new(move || {
        let mut tabs = xshellb::attached_tabs(&tab_config);
        tabs.sort_by_key(|(_, current)| *current);
        let old = tabs.iter().any(|(_, current)| !current);
        let labels = tabs
            .iter()
            .map(|(title, current)| {
                let title: String = title.chars().filter(|c| !c.is_control()).collect();
                if *current {
                    title
                } else {
                    format!("【旧脚本】{title}")
                }
            })
            .collect::<Vec<_>>();
        let mut caption = match labels.len() {
            0 => "本机暂无已接入的标签页。".to_owned(),
            n => {
                let few = labels
                    .iter()
                    .take(3)
                    .map(|s| {
                        if s.chars().count() > 40 {
                            format!("{}…", s.chars().take(39).collect::<String>())
                        } else {
                            s.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("、");
                if n > 3 {
                    format!("本机已接入：{few} 等 {n} 个标签页")
                } else {
                    format!("本机已接入：{few}")
                }
            }
        };
        if old {
            caption.push_str("\n如需更新旧脚本，选中对应标签页，在这里点一次「启动脚本」。");
        }
        attached.set_label(&caption.replace('&', "&&"));
        attached.set_tooltip(&labels.join("\n"));
        let width = page.get_client_size().width - page.from_dip(theme::PAGE_GUTTER * 2);
        attached.wrap(width.max(page.from_dip(200)));
        page.layout();
        page.fit_inside();
    });

    let fallback = Button::builder(&page)
        .with_label("复制手动启动路径")
        .build();
    fallback.show(false);
    content.add(&fallback, 0, SizerFlag::Bottom, pad);
    fallback.on_click(move |_| match xshellb::prepare_script(&manual_config) {
        Ok(path) if wxdragon::clipboard::Clipboard::get().set_text(&path.to_string_lossy()) => {
            super::feedback::success(&page, "已复制，可在 Xshell「工具 → 脚本 → 运行」中粘贴。");
        }
        Ok(_) => super::feedback::error(&page, "复制脚本路径失败。"),
        Err(error) => super::feedback::error(&page, &error.to_string()),
    });

    let status = StaticText::builder(&page).with_label("　").build();
    status.set_foreground_color(theme::theme().ink_secondary);
    content.add(&status, 0, SizerFlag::Expand | SizerFlag::Bottom, pad);
    shell::inset_page_content(&page, content);

    let set_status = move |message: &str| {
        status.set_label(message);
        let width = page.get_client_size().width - page.from_dip(theme::PAGE_GUTTER * 2);
        status.wrap(width.max(page.from_dip(200)));
        page.layout();
        page.fit_inside();
    };
    let windows = Rc::new(RefCell::new(Vec::new()));
    let update = {
        let windows = windows.clone();
        let update_tabs = update_tabs.clone();
        move || {
            window.clear();
            let found = launcher::windows();
            for item in &found {
                let label: String = item.title.chars().take(92).collect();
                window.append(&label);
            }
            if found.len() == 1 {
                window.set_selection(0);
            }
            update_tabs();
            set_status(if found.is_empty() {
                "未找到打开的 Xshell 8 窗口。"
            } else {
                ""
            });
            start.enable(!found.is_empty());
            *windows.borrow_mut() = found;
        }
    };
    update();
    refresh.on_click(move |_| update());
    for (button, on) in [(enable, true), (disable, false)] {
        let config = config.clone();
        button.on_click(move |_| {
            let Some(id) = client.get_selection().and_then(|i| clients.get(i as usize)) else {
                return;
            };
            let outcome = xshellb::configure(&config, *id, on);
            set_status(&match outcome {
                Ok(true) if on => format!("已配置 {}。请重开客户端或对话。", id.name()),
                Ok(true) => format!("已从 {} 移除配置。", id.name()),
                Ok(false) if on => format!("{} 已配置。", id.name()),
                Ok(false) => format!("{} 没有这项配置。", id.name()),
                Err(e) => e.to_string(),
            });
        });
    }
    let result = Rc::new(RefCell::new(
        None::<std::sync::mpsc::Receiver<anyhow::Result<()>>>,
    ));
    let timer = Rc::new(Timer::new(&page));
    let observe = result.clone();
    let tick_timer = Rc::downgrade(&timer);
    let refresh_after_launch = update_tabs.clone();
    timer.on_tick(move |_| {
        let outcome = match observe.borrow().as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(value)) => value,
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                Err(anyhow::anyhow!("脚本启动操作已中断"))
            }
            _ => return,
        };
        if let Some(timer) = tick_timer.upgrade() {
            timer.stop();
        }
        observe.borrow_mut().take();
        start.enable(true);
        refresh.enable(true);
        refresh_after_launch();
        match outcome {
            Ok(()) => set_status("已接入所选标签页。"),
            Err(e) => {
                fallback.show(true);
                set_status(&e.to_string());
            }
        }
    });
    start.on_click(move |_| {
        let selected = window
            .get_selection()
            .and_then(|i| windows.borrow().get(i as usize).cloned());
        let Some(selected) = selected else {
            set_status("请先选择 Xshell 窗口。");
            return;
        };
        start.enable(false);
        refresh.enable(false);
        fallback.show(false);
        set_status("正在接入所选标签页…");
        let config = config.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        *result.borrow_mut() = Some(rx);
        timer.start(100, false);
        std::thread::spawn(move || {
            let _ = tx.send((|| {
                let launch = xshellb::prepare_launch(&config, selected.process_id())?;
                launcher::run(selected, &launch.path)?;
                launch.wait_ready()
            })());
        });
    });
    Ok(update_tabs)
}

fn edit_permissions(page: ScrolledWindow, root: PathBuf) -> bool {
    let dialog = Dialog::builder(&page, "巡检命令权限")
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    super::motion::install_dialog(dialog);
    dialog.set_background_color(theme::theme().bg_card);
    dialog.set_font(&theme::font(theme::TextRole::Body));
    let col = BoxSizer::builder(Orientation::Vertical).build();
    let gap = dialog.from_dip(10);
    let text = |value: &str| {
        let label = StaticText::builder(&dialog).with_label(value).build();
        label.set_foreground_color(theme::theme().ink_primary);
        col.add(&label, 0, SizerFlag::Bottom, gap);
    };
    let initial = policy::Policy::load(&root).unwrap_or_else(|_| {
        text("原设置无法读取。保存将用本次选择替换原设置。");
        Default::default()
    });
    let (automatic, ask) = initial.batch_display();
    text("批量自动同意");
    text("填写后，所有已接入标签页都可直接运行这些只读巡检。用分号或换行分隔命令。");
    let auto_input = TextCtrl::builder(&dialog)
        .with_value(&automatic)
        .with_style(TextCtrlStyle::MultiLine)
        .build();
    auto_input.set_name("批量自动同意");
    auto_input.set_min_size(Size::new(dialog.from_dip(490), dialog.from_dip(84)));
    auto_input.set_hint("例如：pwd; df -h; free -m; uname -a");
    col.add(&auto_input, 1, SizerFlag::Expand | SizerFlag::Bottom, gap);
    let quick = BoxSizer::builder(Orientation::Horizontal).build();
    let add_builtins = Button::builder(&dialog)
        .with_label("加上全部内置巡检")
        .build();
    let choices = Button::builder(&dialog)
        .with_label("查看支持的命令")
        .build();
    quick.add(&add_builtins, 0, SizerFlag::Right, gap);
    quick.add(&choices, 0, SizerFlag::default(), 0);
    col.add_sizer(&quick, 0, SizerFlag::Expand | SizerFlag::Bottom, gap * 2);
    add_builtins.on_click(move |_| {
        let current = auto_input.get_value();
        let existing: Vec<_> = current
            .split([';', '；', '\r', '\n'])
            .map(str::trim)
            .collect();
        let missing = policy::builtin_checks()
            .iter()
            .map(|(_, cmd)| *cmd)
            .filter(|cmd| !existing.contains(cmd))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            let separator = if current.is_empty() || current.ends_with([';', '；', '\r', '\n']) {
                ""
            } else {
                "\n"
            };
            auto_input.append_text(&format!("{separator}{}", missing.join("; ")));
        }
    });
    choices.on_click(move |_| {
        super::feedback::information(&dialog, "支持的只读巡检命令", &policy::supported_commands())
    });

    text("每次询问的扩展命令（可选）");
    text("内置巡检未设为自动同意时会询问。扩展命令不填就不可用；两栏都填时自动同意。");
    let ask_input = TextCtrl::builder(&dialog)
        .with_value(&ask)
        .with_style(TextCtrlStyle::MultiLine)
        .build();
    ask_input.set_name("每次询问的扩展命令");
    ask_input.set_min_size(Size::new(-1, dialog.from_dip(56)));
    col.add(&ask_input, 1, SizerFlag::Expand | SizerFlag::Bottom, gap);
    text("需要询问时优先在客户端确认；不支持时在 Xshell 确认。");
    let invalid = StaticText::builder(&dialog).build();
    invalid.set_foreground_color(theme::theme().error);
    invalid.hide();
    col.add(&invalid, 0, SizerFlag::Expand | SizerFlag::Bottom, gap);
    let actions = BoxSizer::builder(Orientation::Horizontal).build();
    let cancel = Button::builder(&dialog).with_label("取消").build();
    let save = Button::builder(&dialog)
        .with_label("保存权限")
        .with_kind(ButtonKind::Primary)
        .build();
    actions.add_stretch_spacer(1);
    actions.add(&cancel, 0, SizerFlag::Right, gap);
    actions.add(&save, 0, SizerFlag::default(), 0);
    col.add_sizer(&actions, 0, SizerFlag::Expand, 0);
    cancel.on_click(move |_| dialog.end_modal(ID_CANCEL));
    save.on_click(move |_| {
        match policy::Policy::from_batch(&auto_input.get_value(), &ask_input.get_value())
            .and_then(|settings| settings.save(&root))
        {
            Ok(()) => dialog.end_modal(ID_OK),
            Err(error) => {
                invalid.set_label(&format!("未保存：{error}").replace('&', "&&"));
                invalid.wrap((dialog.get_client_size().width - gap * 4).max(dialog.from_dip(200)));
                invalid.show(true);
                dialog.layout();
                dialog.fit();
            }
        }
    });
    let outer = BoxSizer::builder(Orientation::Vertical).build();
    outer.add_sizer(&col, 1, SizerFlag::Expand | SizerFlag::All, gap * 2);
    dialog.set_sizer_and_fit(outer, true);
    let saved = dialog.show_modal() == ID_OK;
    dialog.destroy();
    saved
}
