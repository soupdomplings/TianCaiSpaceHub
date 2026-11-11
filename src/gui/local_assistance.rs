//! Compact setup in TianGong's settings; work and approvals stay in TianGong.
use super::{
    api::ApiClient,
    controls::{Button, ButtonKind, Choice},
    dialog_job, theme, visual,
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wxdragon::prelude::*;

pub(super) struct Fields {
    pub dialog: Dialog,
    pub enabled: CheckBox,
    pub model: Choice,
    pub effort: Choice,
    pub effort_values: Rc<RefCell<Vec<Option<String>>>>,
    pub write: CheckBox,
    pub full_access: CheckBox,
    pub clear: CheckBox,
    pub status: StaticText,
    pub save: Button,
    pub cleanup: Button,
    pub prepare: Button,
    pub program: Button,
    pub binary: Rc<RefCell<Option<String>>>,
}
pub(super) fn fields(parent: &Frame, view: &Value) -> Fields {
    let dialog = Dialog::builder(parent, "本机协助")
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .with_size(parent.from_dip(620), parent.from_dip(650))
        .build();
    super::motion::install_dialog(dialog);
    dialog.set_background_color(theme::theme().bg_card);
    dialog.set_font(&theme::font(theme::TextRole::Body));
    let col = BoxSizer::builder(Orientation::Vertical).build();
    let title = StaticText::builder(&dialog)
        .with_label("困难时自动协助")
        .build();
    title.set_font(&theme::font(theme::TextRole::Title));
    title.set_foreground_color(theme::theme().ink_primary);
    col.add(
        &title,
        0,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        dialog.from_dip(24),
    );
    col.add_spacer(dialog.from_dip(16));
    let enabled = CheckBox::builder(&dialog)
        .with_label("启用天工本机协助")
        .build();
    enabled.set_value(view["enabled"].as_bool().unwrap_or(false));
    enabled.set_foreground_color(theme::theme().ink_primary);
    enabled.set_background_color(theme::theme().bg_card);
    col.add(
        &enabled,
        0,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        dialog.from_dip(24),
    );
    let grid = FlexGridSizer::builder(0, 2)
        .with_hgap(dialog.from_dip(16))
        .with_vgap(dialog.from_dip(14))
        .build();
    grid.add_growable_col(1, 1);
    let label = StaticText::builder(&dialog).with_label("协助模型").build();
    label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
    let model = Choice::builder(&dialog).build();
    model.set_min_size(visual::dip_size(&dialog, 280, 36));
    if let Some(models) = view["models"].as_array() {
        for option in models {
            model.append(option["label"].as_str().unwrap_or(""));
        }
        if let Some(index) = models
            .iter()
            .position(|m| m["model"] == view["model"] && m["route"] == view["route"])
        {
            model.set_selection(index as u32);
        } else if !models.is_empty() {
            model.set_selection(0);
        }
    }
    grid.add(&model, 1, SizerFlag::Expand, 0);
    let label = StaticText::builder(&dialog).with_label("思考强度").build();
    label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
    let effort = Choice::builder(&dialog).build();
    effort.set_min_size(visual::dip_size(&dialog, 180, 36));
    effort.set_tooltip("随协助模型提供选项，支持时默认 xhigh；实际能力由模型来源决定");
    let effort_values = Rc::new(RefCell::new(Vec::new()));
    let option = model
        .get_selection()
        .and_then(|i| view["models"].as_array()?.get(i as usize));
    let saved = option
        .filter(|m| m["model"] == view["model"] && m["route"] == view["route"])
        .and_then(|_| view["reasoningEffort"].as_str());
    fill_efforts(effort, &effort_values, option, saved);
    let values = effort_values.clone();
    let options = view["models"].clone();
    model.on_selection_changed(move |_| {
        let option = model
            .get_selection()
            .and_then(|i| options.as_array()?.get(i as usize));
        fill_efforts(effort, &values, option, None);
        dialog.layout();
    });
    grid.add(&effort, 0, SizerFlag::Expand, 0);
    let label = StaticText::builder(&dialog)
        .with_label("共享运行端")
        .build();
    label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
    // Detection is a display result, not an explicit program override. Keeping
    // auto mode lets a later app upgrade move its CLI without pinning old paths.
    let binary = Rc::new(RefCell::new(
        view["selectedBinary"].as_str().map(str::to_string),
    ));
    let program = Button::builder(&dialog)
        .with_label(if binary.borrow().is_some() {
            "已指定准备来源 · 更换"
        } else if view["runtime"]["ready"] == true {
            "天才搭档 Server · 已就绪"
        } else {
            "选择已有准备来源"
        })
        .with_kind(ButtonKind::Secondary)
        .build();
    program.set_tooltip(&format!(
        "与天才搭档共用一份天才搭档 Server。\n{}",
        view["runtime"]["detail"]
            .as_str()
            .unwrap_or("缺少运行端时提示安装，由你确认后才下载；已有程序可用作准备来源")
    ));
    let automatic = Button::builder(&dialog)
        .with_label("自动查找")
        .with_kind(ButtonKind::Ghost)
        .build();
    automatic.show(binary.borrow().is_some());
    let b = binary.clone();
    program.on_click(move |_| {
        let picker = FileDialog::builder(&dialog)
            .with_message("选择天才搭档 Server 的已有准备来源")
            .with_wildcard(if cfg!(windows) {
                "兼容运行程序 (*.exe)|*.exe"
            } else {
                "兼容运行程序|*"
            })
            .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
            .build();
        if picker.show_modal() == ID_OK {
            if let Some(path) = picker.get_path() {
                *b.borrow_mut() = Some(path);
                program.set_label("已选择准备来源 · 更换");
                automatic.show(true);
                dialog.layout();
            }
        }
        picker.destroy();
    });
    let selected = binary.clone();
    automatic.on_click(move |_| {
        *selected.borrow_mut() = None;
        program.set_label("天才搭档 Server · 自动查找");
        program.set_tooltip("保存时只检查现有运行端；需要准备时仍须由你确认");
        automatic.hide();
        dialog.layout();
    });
    let program_row = BoxSizer::builder(Orientation::Horizontal).build();
    program_row.add(&program, 0, SizerFlag::AlignCenterVertical, 0);
    program_row.add(
        &automatic,
        0,
        SizerFlag::Left | SizerFlag::AlignCenterVertical,
        dialog.from_dip(6),
    );
    let program_col = BoxSizer::builder(Orientation::Vertical).build();
    program_col.add_sizer(&program_row, 0, SizerFlag::Expand, 0);
    let prepare = Button::builder(&dialog)
        .with_label("准备天才搭档 Server")
        .with_kind(ButtonKind::Ghost)
        .build();
    prepare.enable(view["enabled"] == true);
    prepare.set_tooltip(
        "先启用并保存；由你确认后准备共享运行端，优先复用已有兼容程序，没有时从官方下载安装",
    );
    program_col.add(&prepare, 0, SizerFlag::Top, dialog.from_dip(4));
    let runtime_note = StaticText::builder(&dialog)
        .with_label("启用后检查，确认后准备；与天才搭档共用")
        .build();
    runtime_note.set_foreground_color(theme::theme().ink_secondary);
    program_col.add(&runtime_note, 0, SizerFlag::Top, dialog.from_dip(4));
    grid.add_sizer(&program_col, 0, SizerFlag::Expand, 0);
    col.add_sizer(
        &grid,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        dialog.from_dip(24),
    );
    let write = CheckBox::builder(&dialog)
        .with_label("天工使用完整权限时，允许协助修改当前项目")
        .build();
    write.set_value(view["allowWorkspaceWrite"].as_bool().unwrap_or(false));
    write.set_foreground_color(theme::theme().ink_primary);
    write.set_background_color(theme::theme().bg_card);
    col.add(
        &write,
        0,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        dialog.from_dip(24),
    );
    let full_access = CheckBox::builder(&dialog)
        .with_label("完全访问（协助执行时不再逐次申请授权）")
        .build();
    full_access.set_value(view["fullAccess"].as_bool().unwrap_or(false));
    full_access.set_foreground_color(theme::theme().ink_primary);
    full_access.set_background_color(theme::theme().bg_card);
    full_access.set_tooltip("允许协助访问本机文件、执行命令和联网；仍遵守当前用户任务的限制，只读协助仍只读。需要你回答的问题会继续显示。");
    write.enable(!full_access.is_checked());
    full_access.on_toggled(move |_| write.enable(!full_access.is_checked()));
    col.add_spacer(dialog.from_dip(12));
    col.add(
        &full_access,
        0,
        SizerFlag::Left | SizerFlag::Right,
        dialog.from_dip(24),
    );
    let clear = CheckBox::builder(&dialog)
        .with_label("结束后自动清理协助对话，保留天工中的结果")
        .build();
    clear.set_value(false);
    clear.set_foreground_color(theme::theme().ink_primary);
    clear.set_background_color(theme::theme().bg_card);
    // Historical setting stays readable, but temporary conversations now live
    // until an explicit clear or an actual Hub exit.
    clear.hide();
    col.add_spacer(dialog.from_dip(16));
    let note = StaticText::builder(&dialog)
        .with_label("完全访问可读写本机文件、运行命令和联网；本次只读要求仍有效。\n进度和询问在天工处理；临时会话在主动清理或真正退出 Hub 后删除。")
        .build();
    note.set_foreground_color(theme::theme().ink_secondary);
    note.wrap(dialog.from_dip(540));
    note.set_min_size(visual::dip_size(&dialog, -1, 44));
    col.add(
        &note,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        dialog.from_dip(24),
    );
    col.add_spacer(dialog.from_dip(12));
    let status = StaticText::builder(&dialog)
        .with_label(
            view["hostDetail"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("首次使用，请从 Hub 启动天工 Work，以接入窗口内的协助提示。"),
        )
        .build();
    status.set_foreground_color(theme::theme().ink_secondary);
    status.wrap(dialog.from_dip(520));
    col.add(
        &status,
        1,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        dialog.from_dip(24),
    );
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    let cleanup = Button::builder(&dialog)
        .with_label("清理协助临时内容")
        .with_kind(ButtonKind::Ghost)
        .build();
    cleanup.set_tooltip(
        "先停止协助任务，再清理对应临时内容；保留天才搭档 Server、天工主对话、用户项目和已保存成果",
    );
    row.add(&cleanup, 0, SizerFlag::Right, dialog.from_dip(8));
    row.add_stretch_spacer(1);
    let close = Button::builder(&dialog)
        .with_id(ID_CANCEL)
        .with_label("关闭")
        .with_kind(ButtonKind::Ghost)
        .build();
    let save = Button::builder(&dialog).with_label("保存设置").build();
    row.add(&close, 0, SizerFlag::Right, dialog.from_dip(8));
    row.add(&save, 0, SizerFlag::default(), 0);
    col.add_sizer(
        &row,
        0,
        SizerFlag::Expand | SizerFlag::All,
        dialog.from_dip(24),
    );
    dialog.set_sizer(col, false);
    dialog.set_min_size(visual::dip_size(&dialog, 600, 630));
    Fields {
        dialog,
        enabled,
        model,
        effort,
        effort_values,
        write,
        full_access,
        clear,
        status,
        save,
        cleanup,
        prepare,
        program,
        binary,
    }
}
fn fill_efforts(
    choice: Choice,
    values: &Rc<RefCell<Vec<Option<String>>>>,
    option: Option<&Value>,
    saved: Option<&str>,
) {
    choice.clear();
    let mut values = values.borrow_mut();
    values.clear();
    choice.append(
        if option
            .and_then(|o| o["efforts"].as_array())
            .is_some_and(|a| !a.is_empty())
        {
            "自动（优先 xhigh）"
        } else {
            "模型默认"
        },
    );
    values.push(None);
    if let Some(efforts) = option.and_then(|o| o["efforts"].as_array()) {
        for effort in efforts.iter().filter_map(Value::as_str) {
            choice.append(effort);
            values.push(Some(effort.into()));
        }
    }
    let desired = saved.or_else(|| option.and_then(|o| o["defaultEffort"].as_str()));
    let selected = desired
        .map(|s| {
            values
                .iter()
                .position(|v| v.as_deref() == Some(s))
                .unwrap_or_else(|| {
                    choice.append(&format!("{s}（当前不可用）"));
                    values.push(Some(s.into()));
                    values.len() - 1
                })
        })
        .unwrap_or(0);
    choice.set_selection(selected as u32);
    choice.enable(values.len() > 1);
}
pub(super) fn show(parent: &Frame, api: ApiClient) {
    let Some(view) = dialog_job::run(
        parent,
        api.clone(),
        "读取本机协助",
        "/api/desktop/assistance/read",
        json!({}),
    ) else {
        return;
    };
    let f = fields(parent, &view);
    let dialog = f.dialog;
    let status = f.status;
    let selected_binary = f.binary.clone();
    let source_binary = f.binary.clone();
    let program = f.program;
    let enabled_saved = Rc::new(Cell::new(view["enabled"] == true));
    let can_prepare = enabled_saved.clone();
    let prepare = dialog_job::bind_open(
        dialog,
        f.prepare,
        status,
        api.clone(),
        move || {
            if !can_prepare.get() {
                return Err("请先启用本机协助并保存设置".into());
            }
            let accepted = super::feedback::confirm(
                &dialog,
                "安装天才搭档 Server",
                "将从已有兼容程序准备共享的天才搭档 Server；本机没有可用来源时，从官方下载安装。\n\n天才搭档和天工协助共用这一份运行端，安装后长期保留。现在准备？",
                "安装",
            );
            if !accepted {
                return Err("已取消准备。功能保持启用，运行端尚未就绪；需要时可再次准备。".into());
            }
            Ok((
                "/api/desktop/assistance/prepare".into(),
                json!({"confirmed":true,"binary":source_binary.borrow().clone()}),
            ))
        },
        move |value| {
            // Keep auto mode; the prepared runtime is detected after save and on
            // later tasks without pinning a version-specific program override.
            *selected_binary.borrow_mut() = None;
            program.set_label("天才搭档 Server · 已就绪");
            program.set_tooltip(value["binary"].as_str().unwrap_or("共享运行端"));
            status.set_label(
                value["detail"]
                    .as_str()
                    .unwrap_or("天才搭档 Server 已准备就绪"),
            );
            status.wrap(dialog.from_dip(540));
            dialog.layout();
        },
    );
    let cleanup = dialog_job::bind_open(
        dialog,
        f.cleanup,
        status,
        api.clone(),
        || Ok(("/api/desktop/assistance/clear".into(), json!({}))),
        move |value| {
            status.set_label(
                value["detail"]
                    .as_str()
                    .unwrap_or("已停止协助任务并清理临时内容"),
            );
            status.wrap(dialog.from_dip(540));
            dialog.layout();
        },
    );
    let start_prepare = prepare.start.clone();
    let prepare_button = f.prepare;
    let revision = Rc::new(RefCell::new(view["revision"].clone()));
    let next_revision = revision.clone();
    let job = dialog_job::bind_open(
        dialog,
        f.save,
        f.status,
        api,
        move || {
            let selection = f.model.get_selection().map(|n| n as usize);
            let option = selection.and_then(|i| view["models"].as_array().and_then(|a| a.get(i)));
            if f.enabled.is_checked() && option.is_none() {
                return Err("请先在 ChatGPT / Codex 中接入一个可用模型".into());
            }
            Ok((
                "/api/desktop/assistance".into(),
                json!({"revision":revision.borrow().clone(),"enabled":f.enabled.is_checked(),"model":option.map(|v|v["model"].clone()).unwrap_or(json!("")),
            "route":option.map(|v|v["route"].clone()).unwrap_or(json!("")),
            "reasoningEffort":f.effort.get_selection().and_then(|i|f.effort_values.borrow().get(i as usize).cloned().flatten()),
            "binary":f.binary.borrow().clone(),"allowWorkspaceWrite":f.write.is_checked(),"fullAccess":f.full_access.is_checked(),"autoClear":false}),
            ))
        },
        move |saved| {
            if saved.get("revision").is_some() {
                *next_revision.borrow_mut() = saved["revision"].clone();
            }
            let enabled = saved["enabled"] == true;
            enabled_saved.set(enabled);
            prepare_button.enable(enabled);
            if enabled && saved["runtime"]["needsPreparation"] == true {
                status.set_label("设置已保存；天才搭档 Server 尚未就绪");
                start_prepare();
            } else {
                status.set_label(if enabled {
                    "设置已保存，天才搭档 Server 已就绪"
                } else {
                    "已停用天工本机协助"
                });
            }
            status.wrap(dialog.from_dip(540));
            dialog.layout();
        },
    );
    dialog.centre();
    dialog.show_modal();
    drop(job);
    drop(cleanup);
    drop(prepare);
    dialog.destroy();
}

#[cfg(all(test, windows))]
#[test]
fn local_assistance_setup_keeps_save_cleanup_and_fields_inside_the_hidden_native_dialog() {
    wxdragon::app::render_offscreen(||{
        for(mode,suffix)in[(theme::ThemeMode::Light,"light"),(theme::ThemeMode::Dark,"dark")]{
            theme::init(mode);let frame=Frame::builder().with_title("hidden synthetic assistance setup").build();
            frame.set_size(visual::dip_size(&frame,900,560));
            let f=fields(&frame,&json!({"enabled":true,"model":"fixture-model","route":"fixture-route","binary":"C:/synthetic/codex.exe",
                "models":[{"model":"fixture-model","route":"fixture-route","label":"fixture-model · 合成普通来源","efforts":["low","medium","high","xhigh","max"],"defaultEffort":"xhigh"}],
                "hostDetail":"最近任务已附带工具参数和自动协助规则（合成状态）","autoClear":true}));
            assert_eq!(f.effort.get_string_selection().as_deref(),Some("xhigh"));
            assert!(f.binary.borrow().is_none(),"auto-detected CLI must not become a manual override when saving");
            f.dialog.layout();let bounds=f.dialog.get_client_size();
            assert!(!f.clear.is_shown(),"legacy completion cleanup must remain hidden");
            for w in [&f.enabled as &dyn WxWidget,&f.model,&f.effort,&f.program,&f.prepare,&f.write,&f.full_access,&f.status,&f.save,&f.cleanup]{
                let p=w.get_position();let s=w.get_size();
                assert!(p.x>=0 && p.y>=0 && p.x+s.width<=bounds.width+1 && p.y+s.height<=bounds.height+1,"assistance control must fit: {p:?} {s:?} in {bounds:?}");
            }
            assert!(!f.dialog.is_shown() && !frame.is_shown());
            super::native_model_review::capture(&f.dialog,&format!("tiangong-local-assistance-{suffix}"));f.dialog.destroy();frame.destroy();
        }
    }).unwrap();
}
