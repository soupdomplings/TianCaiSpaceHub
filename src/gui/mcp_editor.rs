use super::{api::ApiClient, dialog_job, theme};
use super::{
    controls::{Button, ButtonKind},
    visual::Icon,
};
use crate::desktop::{
    mcp::{self, McpServer},
    resources::{LibraryView, Resource},
    targets::ClientTarget,
};
use crate::gui::controls::{Choice, TextCtrl};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    rc::Rc,
};
use wxdragon::prelude::*;

#[derive(Clone)]
struct Form {
    name: TextCtrl,
    description: TextCtrl,
    transport: Choice,
    url: TextCtrl,
    bearer: TextCtrl,
    headers: TextCtrl,
    command: TextCtrl,
    args: TextCtrl,
    env: TextCtrl,
    pass_env: TextCtrl,
    cwd: TextCtrl,
    base: Rc<RefCell<Value>>,
}
impl Form {
    fn read(&self) -> Result<McpServer, String> {
        let mut config = self.base.borrow().clone();
        let obj = config.as_object_mut().ok_or("MCP JSON 应为对象")?;
        for key in [
            "command",
            "args",
            "url",
            "headers",
            "http_headers",
            "env",
            "env_vars",
            "cwd",
            "type",
            "transport",
            "disabled",
            "enabled",
        ] {
            obj.remove(key);
        }
        let transport = self.transport.get_selection().unwrap_or(0);
        if transport == 2 {
            obj.insert("command".into(), self.command.get_value().trim().into());
            obj.insert("args".into(), lines(&self.args.get_value()).into());
            let env = parse_pairs(&self.env.get_value(), '=')?;
            if !env.is_empty() {
                obj.insert("env".into(), env.into());
            }
            let pass = lines(&self.pass_env.get_value());
            if !pass.is_empty() {
                obj.insert("env_vars".into(), pass.into());
            }
            if !self.cwd.get_value().trim().is_empty() {
                obj.insert("cwd".into(), self.cwd.get_value().trim().into());
            }
            obj.insert("type".into(), "stdio".into());
        } else {
            obj.insert("url".into(), self.url.get_value().trim().into());
            obj.insert(
                "type".into(),
                if transport == 1 { "sse" } else { "http" }.into(),
            );
            let mut headers = parse_pairs(&self.headers.get_value(), ':')?;
            if !self.bearer.get_value().trim().is_empty() {
                headers.insert(
                    "Authorization".into(),
                    format!("Bearer {}", self.bearer.get_value().trim()).into(),
                );
            }
            if !headers.is_empty() {
                obj.insert("headers".into(), headers.into());
            }
        }
        let server = McpServer {
            name: self.name.get_value().trim().into(),
            config,
        };
        server.validate().map_err(|e| e.to_string())?;
        Ok(server)
    }
    fn populate(&self, server: &McpServer) {
        self.name.set_value(&server.name);
        *self.base.borrow_mut() = server.config.clone();
        let config = &server.config;
        self.transport.set_selection(match server.transport() {
            "stdio" => 2,
            "sse" => 1,
            _ => 0,
        });
        for (field, key) in [
            (self.url, "url"),
            (self.command, "command"),
            (self.cwd, "cwd"),
        ] {
            field.set_value(config.get(key).and_then(Value::as_str).unwrap_or(""));
        }
        self.args.set_value(&format_lines(config.get("args")));
        self.pass_env
            .set_value(&format_lines(config.get("env_vars")));
        self.env.set_value(&format_pairs(config.get("env"), '='));
        let mut headers = config
            .get("headers")
            .or(config.get("http_headers"))
            .cloned()
            .unwrap_or(json!({}));
        self.bearer.set_value("");
        if let Some(headers) = headers.as_object_mut() {
            let name = headers
                .keys()
                .find(|k| k.eq_ignore_ascii_case("authorization"))
                .cloned();
            if let Some(name) = name {
                if let Some(token) = headers
                    .get(&name)
                    .and_then(Value::as_str)
                    .and_then(|s| s.strip_prefix("Bearer "))
                {
                    self.bearer.set_value(token);
                    headers.remove(&name);
                }
            }
        }
        self.headers.set_value(&format_pairs(Some(&headers), ':'));
    }
}

pub(super) fn show(
    parent: &Frame,
    api: ApiClient,
    view: &LibraryView,
    existing: Option<(&Resource, McpServer)>,
) -> Option<Value> {
    let size = super::ai_gw_channel_dialog_size(parent, Size::new(800, 760));
    let dialog = Dialog::builder(
        parent,
        if existing.is_some() {
            "编辑 MCP"
        } else {
            "添加 MCP"
        },
    )
    .with_size(size.width, size.height)
    .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
    .build();
    dialog.set_background_color(theme::theme().bg_card);
    let column = BoxSizer::builder(Orientation::Vertical).build();
    let fields = FlexGridSizer::builder(0, 2)
        .with_vgap(8)
        .with_hgap(12)
        .build();
    fields.add_growable_col(1, 1);
    let name = field(&dialog, &fields, "名称", "", false, 0);
    let description = field(&dialog, &fields, "简介", "", false, 0);
    column.add_sizer(&fields, 0, SizerFlag::Expand | SizerFlag::All, 16);
    let notebook = Notebook::builder(&dialog).build();
    let form_page = ScrolledWindow::builder(&notebook)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    let form_column = BoxSizer::builder(Orientation::Vertical).build();
    let transport = Choice::builder(&form_page)
        .with_choices(vec![
            "远程服务 · HTTP".into(),
            "远程服务 · SSE".into(),
            "本地服务 · 命令".into(),
        ])
        .build();
    transport.set_selection(0);
    transport.set_background_color(theme::theme().bg_card);
    transport.set_foreground_color(theme::theme().ink_primary);
    form_column.add(&transport, 0, SizerFlag::Expand | SizerFlag::All, 12);
    let remote = Panel::builder(&form_page).build();
    let remote_grid = FlexGridSizer::builder(0, 2)
        .with_vgap(10)
        .with_hgap(12)
        .build();
    remote_grid.add_growable_col(1, 1);
    let url = field(&remote, &remote_grid, "服务地址", "", false, 0);
    let bearer = field(&remote, &remote_grid, "Bearer 令牌", "", true, 0);
    let headers = field(&remote, &remote_grid, "Headers\n名称: 值", "", false, 80);
    remote.set_sizer(remote_grid, true);
    form_column.add(&remote, 0, SizerFlag::Expand | SizerFlag::All, 12);
    let local = Panel::builder(&form_page).build();
    let local_grid = FlexGridSizer::builder(0, 2)
        .with_vgap(10)
        .with_hgap(12)
        .build();
    local_grid.add_growable_col(1, 1);
    let command = field(&local, &local_grid, "命令", "", false, 0);
    let args = field(&local, &local_grid, "参数\n每行一个", "", false, 84);
    let env = field(&local, &local_grid, "环境变量\n名称=值", "", false, 75);
    let pass_env = field(
        &local,
        &local_grid,
        "传递环境变量\n每行一个名称",
        "",
        false,
        55,
    );
    let cwd = field(&local, &local_grid, "工作目录", "", false, 0);
    local.set_sizer(local_grid, true);
    local.hide();
    form_column.add(&local, 0, SizerFlag::Expand | SizerFlag::All, 12);
    form_page.set_sizer(form_column, true);
    let json_page = Panel::builder(&notebook).build();
    let json_sizer = BoxSizer::builder(Orientation::Vertical).build();
    let json_input = TextCtrl::builder(&json_page)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::DontWrap)
        .with_value("{}")
        .build();
    json_input.set_background_color(theme::theme().bg_card_alt);
    json_input.set_foreground_color(theme::theme().ink_primary);
    json_sizer.add(&json_input, 1, SizerFlag::Expand | SizerFlag::All, 10);
    json_page.set_sizer(json_sizer, true);
    notebook.add_page(&form_page, "表单配置", true, None);
    notebook.add_page(&json_page, "JSON 配置", false, None);
    column.add(
        &notebook,
        1,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        16,
    );
    let form = Form {
        name,
        description,
        transport,
        url,
        bearer,
        headers,
        command,
        args,
        env,
        pass_env,
        cwd,
        base: Rc::new(RefCell::new(json!({}))),
    };
    let id = existing.as_ref().map(|(r, _)| r.id.clone());
    if let Some((resource, server)) = &existing {
        form.populate(server);
        description.set_value(&resource.description);
        json_input.set_value(&serde_json::to_string_pretty(&server.config).unwrap_or_default());
    }
    let show_transport = move || {
        let local_mode = transport.get_selection() == Some(2);
        remote.show(!local_mode);
        local.show(local_mode);
        form_page.layout();
        form_page.fit_inside();
    };
    show_transport();
    transport.on_selection_changed(move |_| show_transport());
    let syncing = Rc::new(Cell::new(false));
    {
        let (form, syncing) = (form.clone(), syncing.clone());
        notebook.on_page_changed(move |event| {
            if syncing.replace(true) {
                return;
            }
            let result = if event.get_selection() == Some(1) {
                form.read().map(|s| {
                    json_input
                        .set_value(&serde_json::to_string_pretty(&s.config).unwrap_or_default())
                })
            } else {
                selected_json(&json_input.get_value(), &name.get_value()).map(|server| {
                    form.populate(&server);
                    show_transport();
                })
            };
            if let Err(error) = result {
                super::show_error(&dialog, &error);
                notebook.change_selection(if event.get_selection() == Some(1) {
                    0
                } else {
                    1
                });
            }
            syncing.set(false);
        });
    }
    let selected = Rc::new(RefCell::new(
        existing
            .as_ref()
            .and_then(|(r, _)| view.resources.iter().find(|v| v.resource.id == r.id))
            .map(|r| {
                r.targets
                    .iter()
                    .filter(|t| t.using == Some(true))
                    .map(|t| t.target_id.clone())
                    .collect()
            })
            .unwrap_or_else(BTreeSet::<String>::new),
    ));
    let targets = target_selector(&dialog, &view.targets, selected.clone());
    column.add(&targets, 0, SizerFlag::Expand | SizerFlag::All, 12);
    let status = StaticText::builder(&dialog).with_label("").build();
    status.set_foreground_color(theme::theme().error);
    column.add(
        &status,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        16,
    );
    let buttons = BoxSizer::builder(Orientation::Horizontal).build();
    buttons.add_stretch_spacer(1);
    let cancel = Button::builder(&dialog).with_label("取消").build();
    let save = Button::builder(&dialog)
        .with_label(if existing.is_some() {
            "保存并使用"
        } else {
            "添加并使用"
        })
        .build();
    save.set_kind(ButtonKind::Primary);
    save.set_foreground_color(theme::theme().on_accent);
    buttons.add(&cancel, 0, SizerFlag::Right, 10);
    buttons.add(&save, 0, SizerFlag::default(), 0);
    column.add_sizer(&buttons, 0, SizerFlag::Expand | SizerFlag::All, 16);
    dialog.set_sizer(column, true);
    dialog.centre();
    cancel.on_click(move |_| dialog.end_modal(ID_CANCEL));
    let revision = view.revision.clone();
    let job = dialog_job::bind(dialog, save, status, api, move || {
        let server = if notebook.selection() == 1 {
            selected_json(&json_input.get_value(), &name.get_value())?
        } else {
            form.read()?
        };
        let body = json!({"id":id,"name":server.name,"description":form.description.get_value(),"json":serde_json::to_string(&server.config).map_err(|e|e.to_string())?,"revision":revision,"targets":selected.borrow().iter().cloned().collect::<Vec<_>>()});
        Ok(("/api/desktop/mcp/save".into(), body))
    });
    dialog.show_modal();
    let result = job.result.borrow_mut().take();
    drop(job);
    dialog.destroy();
    result
}

fn selected_json(text: &str, name: &str) -> Result<McpServer, String> {
    let mut servers = mcp::parse_import(text, name).map_err(|e| e.to_string())?;
    if servers.len() == 1 {
        return Ok(servers.remove(0));
    }
    let index = servers
        .iter()
        .position(|s| s.name == name.trim())
        .ok_or_else(|| {
            format!(
                "包含多个服务，请在名称中选择：{}",
                servers
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join("、")
            )
        })?;
    Ok(servers.remove(index))
}
pub(super) fn field<W: WxWidget>(
    parent: &W,
    sizer: &FlexGridSizer,
    label: &str,
    value: &str,
    password: bool,
    height: i32,
) -> TextCtrl {
    let label = StaticText::builder(parent).with_label(label).build();
    label.set_foreground_color(theme::theme().ink_secondary);
    sizer.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
    let style = if height > 0 {
        TextCtrlStyle::MultiLine
    } else if password {
        TextCtrlStyle::Password
    } else {
        TextCtrlStyle::Default
    };
    let input = TextCtrl::builder(parent)
        .with_value(value)
        .with_style(style)
        .with_size(Size::new(-1, if height > 0 { height } else { 30 }))
        .build();
    input.set_background_color(theme::theme().bg_card_alt);
    input.set_foreground_color(theme::theme().ink_primary);
    sizer.add(&input, 1, SizerFlag::Expand, 0);
    input
}
pub(super) fn target_selector(
    parent: &dyn WxWidget,
    targets: &[ClientTarget],
    selected: Rc<RefCell<BTreeSet<String>>>,
) -> Panel {
    let panel = Panel::builder(parent).build();
    panel.set_background_color(theme::theme().bg_card);
    panel.set_max_size(Size::new(500, -1));
    let grid = WrapSizer::builder(Orientation::Horizontal)
        .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
        .build();
    // One tile per concrete default target. Profiles remain in target settings.
    let mut seen = BTreeSet::new();
    for target in targets
        .iter()
        .filter(|t| seen.insert((t.client, t.model_mode.clone())))
    {
        let label = if target.client == crate::desktop::clients::ClientId::WebStorm {
            format!("WS\n{}", target.label)
        } else {
            format!("{}\n{}", target.client.glyph(), target.client.name())
        };
        let b = Button::builder(&panel)
            .with_label(&label)
            .with_size(Size::new(112, 58))
            .with_style(ButtonStyle::BorderNone)
            .build();
        b.set_kind(ButtonKind::Tile);
        b.set_icon(Icon::client(target.client));
        let on = selected.borrow().contains(&target.id);
        b.set_selected(on);
        b.set_background_color(if on {
            theme::theme().accent
        } else {
            theme::theme().bg_muted
        });
        b.set_foreground_color(if on {
            theme::theme().on_accent
        } else {
            theme::theme().ink_muted
        });
        b.set_tooltip(&format!("{} · {}", target.client.name(), target.label));
        let (id, selected) = (target.id.clone(), selected.clone());
        b.on_click(move |_| {
            let mut values = selected.borrow_mut();
            if !values.remove(&id) {
                values.insert(id.clone());
            }
            let on = values.contains(&id);
            b.set_selected(on);
            b.set_background_color(if on {
                theme::theme().accent
            } else {
                theme::theme().bg_muted
            });
            b.set_foreground_color(if on {
                theme::theme().on_accent
            } else {
                theme::theme().ink_muted
            });
        });
        grid.add(&b, 0, SizerFlag::Right | SizerFlag::Bottom, 8);
    }
    panel.set_sizer(grid, true);
    panel
}
fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
fn parse_pairs(text: &str, separator: char) -> Result<serde_json::Map<String, Value>, String> {
    let mut map = serde_json::Map::new();
    for (line_no, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once(separator)
            .ok_or_else(|| format!("第 {} 行需要使用 {} 分隔名称和值", line_no + 1, separator))?;
        if key.trim().is_empty() || map.insert(key.trim().into(), value.trim().into()).is_some() {
            return Err(format!("第 {} 行名称为空或重复", line_no + 1));
        }
    }
    Ok(map)
}
fn format_pairs(value: Option<&Value>, separator: char) -> String {
    value
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .map(|(k, v)| format!("{k}{separator}{}", v.as_str().unwrap_or("")))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}
fn format_lines(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}
