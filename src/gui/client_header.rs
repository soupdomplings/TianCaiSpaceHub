//! Shared client task bar. The caller binds the real client-specific actions.
use super::{
    controls::{Button, ButtonKind, Choice},
    theme,
};
use crate::desktop::clients::ClientId;
use serde_json::Value;
use std::{cell::RefCell, path::PathBuf, rc::Rc};
use wxdragon::prelude::*;

pub(super) struct ClientHeader {
    pub panel: Panel,
    pub tabs: Vec<Button>,
    pub launch: Button,
    pub reload: Button,
}

/// Shared settings structure; client controllers bind their supported actions.
#[derive(Clone)]
pub(super) struct ClientSettings {
    pub panel: Panel,
    pub program: Button,
    pub folder: Button,
    pub download: Option<Button>,
    target: Choice,
    paths: StaticText,
    targets: Rc<RefCell<Vec<Value>>>,
}
impl ClientSettings {
    pub fn new(parent: &dyn WxWidget, client: ClientId) -> Self {
        let panel = Panel::builder(parent).build();
        panel.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let title = StaticText::builder(&panel).with_label("程序与配置").build();
        title.set_font(&theme::font(theme::TextRole::Title));
        title.set_foreground_color(theme::theme().ink_primary);
        col.add(&title, 0, SizerFlag::Bottom, panel.from_dip(12));
        let note = StaticText::builder(&panel).with_label(match client {
            ClientId::Codex => "初始化、执行连接与历史维护在这里调整。模型在对应页管理。",
            ClientId::WorkBuddy => "保存模型后在 WorkBuddy 中选择使用。配置恢复在下方有备份时显示；不需要额外开启接入。",
            ClientId::Tiangong => "模型保存后在天工 Work 中选择使用。桌面对话同步和配置恢复在下方管理。",
            ClientId::VsCode => "模型用于 VS Code Chat；未更新时新建聊天。各 Profile 独立，保留客户端信任确认。",
            ClientId::DeepSeekHarness => "模型用于 Harness 的对应 Profile；重新打开会话后选择，已有手工配置保留。",
            ClientId::ZCode => "重新打开 ZCode 后在新任务中选择模型，已有手工模型规则保留。",
            ClientId::WebStorm => "AI Assistant 使用辅助插件，Air / ACP 使用代理组件；两种接入可以同时保留。",
        }).build();
        note.set_foreground_color(theme::theme().ink_secondary);
        note.wrap(panel.from_dip(600));
        col.add(
            &note,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(12),
        );
        let actions = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        let program = Button::builder(&panel)
            .with_label("选择客户端程序")
            .with_kind(ButtonKind::Secondary)
            .with_size(Size::new(160, 36))
            .build();
        program.show(!matches!(
            client,
            ClientId::Codex | ClientId::WorkBuddy | ClientId::Tiangong
        ));
        let download = (client == ClientId::Tiangong).then(|| {
            let button = Button::builder(&panel)
                .with_label("下载天工 Work")
                .with_icon(super::visual::Icon::Tiangong)
                .with_kind(ButtonKind::Secondary)
                .build();
            button.set_name("tiangong-download");
            button.set_tooltip("选择 Windows 或 Mac 安装包，在默认浏览器中下载");
            button
        });
        if let Some(download) = download {
            actions.add(
                &download,
                0,
                SizerFlag::Right | SizerFlag::Bottom,
                panel.from_dip(8),
            );
        }
        let files = Button::builder(&panel)
            .with_label("查看配置文件")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(144, 36))
            .build();
        files.set_name("client-settings-files");
        for b in [program, files] {
            actions.add(
                &b,
                0,
                SizerFlag::Right | SizerFlag::Bottom,
                panel.from_dip(8),
            );
        }
        col.add_sizer(&actions, 0, SizerFlag::Expand, 0);
        let details = Panel::builder(&panel).build();
        details.set_background_color(theme::theme().bg_app);
        let dc = BoxSizer::builder(Orientation::Vertical).build();
        let target = Choice::builder(&details)
            .with_size(Size::new(260, 36))
            .build();
        dc.add(&target, 0, SizerFlag::Bottom, details.from_dip(10));
        let paths = StaticText::builder(&details)
            .with_label("正在读取配置位置…")
            .build();
        paths.set_foreground_color(theme::theme().ink_secondary);
        dc.add(
            &paths,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            details.from_dip(12),
        );
        let folder = Button::builder(&details)
            .with_label("打开配置文件夹")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(160, 34))
            .build();
        dc.add(&folder, 0, SizerFlag::default(), 0);
        details.set_sizer(dc, false);
        details.hide();
        col.add(
            &details,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            panel.from_dip(8),
        );
        panel.set_sizer(col, false);
        files.on_click(move |_| {
            details.show(!details.is_shown());
            files.set_label(if details.is_shown() {
                "隐藏配置文件"
            } else {
                "查看配置文件"
            });
            panel.layout();
            if let Some(parent) = panel.get_parent() {
                parent.layout();
                if let Some(viewport) = parent.get_parent() {
                    viewport.layout();
                }
            }
        });
        let ui = Self {
            panel,
            program,
            folder,
            download,
            target,
            paths,
            targets: Default::default(),
        };
        let u = ui.clone();
        target.on_selection_changed(move |_| u.update());
        ui
    }
    pub fn set_targets(&self, view: &Value, client: ClientId) {
        let rows = view["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|v| v["client"] == client.key())
            .cloned()
            .collect::<Vec<_>>();
        if *self.targets.borrow() != rows {
            let selected = self.target.get_selection().and_then(|i| {
                self.targets
                    .borrow()
                    .get(i as usize)
                    .map(|v| v["id"].clone())
            });
            self.target.clear();
            for row in &rows {
                self.target.append(row["label"].as_str().unwrap_or("默认"));
            }
            if !rows.is_empty() {
                self.target.set_selection(
                    rows.iter()
                        .position(|v| Some(&v["id"]) == selected.as_ref())
                        .unwrap_or(0) as u32,
                );
            }
            *self.targets.borrow_mut() = rows;
        }
        self.target.show(self.targets.borrow().len() > 1);
        self.update();
    }
    fn update(&self) {
        let rows = self.targets.borrow();
        self.paths.set_label(&if let Some(row) =
            rows.get(self.target.get_selection().unwrap_or(0) as usize)
        {
            let model_directory = row["modelRoot"]
                .as_str()
                .filter(|path| Some(*path) != row["root"].as_str())
                .map(|path| format!("模型配置目录\n{path}\n\n"))
                .unwrap_or_default();
            format!(
                "配置目录\n{}\n\n{}",
                row["root"].as_str().unwrap_or("未提供"),
                model_directory,
            )
        } else {
            "尚未读取到配置位置。请重新进入此客户端；读取失败时按页面提示处理。".into()
        });
        self.paths.wrap(
            (self.panel.get_client_size().width - self.panel.from_dip(12))
                .max(self.panel.from_dip(280)),
        );
        self.folder.enable(!rows.is_empty());
        self.panel.layout();
        if let Some(host) = self.panel.get_parent() {
            host.layout();
            if let Some(viewport) = host.get_parent() {
                viewport.layout();
            }
        }
    }
    pub fn directory(&self) -> Option<PathBuf> {
        self.targets
            .borrow()
            .get(self.target.get_selection().unwrap_or(0) as usize)?
            .get("root")?
            .as_str()
            .map(PathBuf::from)
    }
}
impl ClientHeader {
    pub fn new(parent: &dyn WxWidget, client: ClientId) -> Self {
        let panel = Panel::builder(parent).build();
        panel.set_background_color(theme::theme().bg_app);
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let left = Panel::builder(&panel).build();
        left.set_background_color(theme::theme().bg_app);
        let wrap = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        let tabs = ["状态概览", "模型", "Skill", "MCP", "接入设置"]
            .into_iter()
            .map(|label| {
                let b = Button::builder(&left)
                    .with_label(label)
                    .with_kind(ButtonKind::Tab)
                    .with_size(Size::new(
                        if matches!(label, "状态概览" | "接入设置") {
                            88
                        } else {
                            64
                        },
                        38,
                    ))
                    .build();
                wrap.add(
                    &b,
                    0,
                    SizerFlag::Right | SizerFlag::Bottom,
                    left.from_dip(4),
                );
                if matches!(label, "Skill" | "MCP") {
                    b.hide();
                }
                b
            })
            .collect::<Vec<_>>();
        tabs[0].set_selected(true);
        left.set_sizer(wrap, true);
        row.add(
            &left,
            1,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            panel.from_dip(12),
        );
        let launch = Button::builder(&panel)
            .with_label(if client == ClientId::Codex {
                "增强启动"
            } else {
                "启动客户端"
            })
            .with_kind(ButtonKind::Primary)
            .with_size(Size::new(120, theme::CONTROL_HEIGHT))
            .build();
        let reload = Button::builder(&panel).with_label("").build();
        // A hidden legacy command handle; all production refreshes are automatic.
        // Keeping it avoids changing the protected legacy editor service wiring.
        reload.hide();
        row.add(&launch, 0, SizerFlag::AlignCenterVertical, 0);
        panel.set_sizer(row, true);
        Self {
            panel,
            tabs,
            launch,
            reload,
        }
    }
}
