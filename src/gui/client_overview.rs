use std::{cell::Cell, rc::Rc};

use wxdragon::prelude::*;

use super::{
    UiHandles,
    api::{DashboardSnapshot, ImAccountItem},
    text::{GuiLocale, GuiText},
    theme,
    widgets::{
        StateTone, StatusIconKind, StatusPanel, set_im_channel_row, set_status_panel, status_panel,
    },
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum OverviewKind {
    #[default]
    Codex,
    WorkBuddy,
    GmClaw,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct LinkState {
    kind: OverviewKind,
    connected: [bool; 3],
}

#[derive(Clone)]
pub(super) struct TopologyConnector {
    pub(super) panel: Panel,
    state: Rc<Cell<LinkState>>,
}

impl TopologyConnector {
    pub(super) fn new(parent: &Panel, outgoing: bool) -> Self {
        let panel = Panel::builder(parent).build();
        panel.set_min_size(Size::new(56, 176));
        panel.set_background_color(theme::theme().bg_card);
        let state = Rc::new(Cell::new(LinkState::default()));
        let paint_state = state.clone();
        panel.on_paint(move |_| {
            let dc = PaintDC::new(&panel);
            let theme = theme::theme();
            dc.set_background(theme.bg_card);
            dc.clear();
            let state = paint_state.get();
            // WorkBuddy has a model path, but no external-message execution path.
            if outgoing && state.kind == OverviewKind::WorkBuddy {
                return;
            }
            let size = panel.get_client_size();
            let width = size.width.max(1);
            let height = size.height.max(1);
            let middle = height / 2;
            let trunk = width * 5 / 12;
            let positions = if outgoing || state.kind == OverviewKind::Codex {
                [height / 6, middle, height * 5 / 6]
            } else {
                [height / 4, height * 3 / 4, height * 3 / 4]
            };
            // Paint active paths last so a disconnected sibling cannot gray
            // out the shared trunk of a verified connection.
            let mut indices = [0, 1, 2];
            indices.sort_by_key(|index| state.connected[*index]);
            for index in indices {
                let y = positions[index];
                let visible = match (outgoing, state.kind, index) {
                    (true, OverviewKind::GmClaw, 1) => false, // Telegram is Codex-only.
                    (false, OverviewKind::WorkBuddy, 1 | 2) => false,
                    (false, OverviewKind::GmClaw, 2) => false,
                    _ => true,
                };
                if !visible {
                    continue;
                }
                dc.set_pen(
                    if state.connected[index] {
                        theme.ok
                    } else {
                        theme.divider
                    },
                    2,
                    PenStyle::Solid,
                );
                if outgoing {
                    dc.draw_line(0, middle, trunk, middle);
                    dc.draw_line(trunk, middle, trunk, y);
                    dc.draw_line(trunk, y, width - 1, y);
                } else {
                    dc.draw_line(0, y, trunk, y);
                    dc.draw_line(trunk, y, trunk, middle);
                    dc.draw_line(trunk, middle, width - 1, middle);
                }
            }
        });
        Self { panel, state }
    }

    fn update(&self, kind: OverviewKind, connected: [bool; 3]) {
        let next = LinkState { kind, connected };
        if self.state.replace(next) != next {
            self.panel.refresh(true, None);
        }
    }
}

#[derive(Clone)]
pub(super) struct ClientOverviewUi {
    kind: Rc<Cell<OverviewKind>>,
    root: Panel,
    container: Panel,
    model: StatusPanel,
    bridge: StatusPanel,
    entry_connector: TopologyConnector,
    bridge_connector: TopologyConnector,
}

impl ClientOverviewUi {
    pub(super) fn new(
        parent: &Panel,
        root: Panel,
        entry_column: &BoxSizer,
        entry_connector: TopologyConnector,
        bridge_connector: TopologyConnector,
        text: GuiText,
    ) -> Self {
        let model = status_panel(parent, "", StatusIconKind::Service, text);
        let bridge = status_panel(parent, "", StatusIconKind::Service, text);
        entry_column.add(&model.panel, 1, SizerFlag::Expand | SizerFlag::Bottom, 4);
        entry_column.add(&bridge.panel, 1, SizerFlag::Expand, 0);
        model.panel.hide();
        bridge.panel.hide();
        Self {
            kind: Rc::new(Cell::new(OverviewKind::Codex)),
            root,
            container: *parent,
            model,
            bridge,
            entry_connector,
            bridge_connector,
        }
    }

    pub(super) fn select_page(&self, page: i32) -> bool {
        let kind = match page {
            0 => OverviewKind::Codex,
            4 => OverviewKind::WorkBuddy,
            5 => OverviewKind::GmClaw,
            _ => return false,
        };
        self.kind.replace(kind) != kind
    }
}

fn tr(text: GuiText, zh: &'static str, en: &'static str) -> &'static str {
    match text.locale {
        GuiLocale::ZhCn => zh,
        GuiLocale::EnUs => en,
    }
}

pub(super) fn refresh(handles: &UiHandles, snapshot: Option<&DashboardSnapshot>, starting: bool) {
    let ui = &handles.client_overview;
    let kind = ui.kind.get();
    let text = handles.text;
    let is_codex = kind == OverviewKind::Codex;
    for panel in [
        &handles.codex_status,
        &handles.vscode_status,
        &handles.cli_status,
    ] {
        panel.panel.show(is_codex);
    }
    ui.model.panel.show(!is_codex);
    ui.bridge.panel.show(!is_codex);
    let online = snapshot.is_some_and(|value| value.service_online);
    let accounts = snapshot.and_then(|value| value.im_accounts.as_ref());
    let transport_ready = |platform| {
        online
            && accounts.is_some_and(|value| {
                value
                    .accounts
                    .iter()
                    .any(|account| im_ready(account, platform))
            })
    };
    let mut entry_connected = [false; 3];
    let mut executor_connected = false;
    if is_codex {
        if online {
            if let Some(remote) = snapshot.and_then(|value| value.remote.as_ref()) {
                for (index, source) in ["codex_app", "vscode", "cli"].into_iter().enumerate() {
                    entry_connected[index] = remote.connections.iter().any(|connection| {
                        connection.source_kind == source
                            && connection.connected
                            && connection.initialized
                    });
                }
            }
            executor_connected = entry_connected.iter().any(|connected| *connected);
            if accounts.is_none() {
                for row in [
                    &handles.im_status.feishu,
                    &handles.im_status.telegram,
                    &handles.im_status.wechat,
                    &handles.im_status.wecom,
                ] {
                    set_im_channel_row(row, text.reading(), "", StateTone::Muted);
                }
            }
        }
    } else {
        let is_gmclaw = kind == OverviewKind::GmClaw;
        ui.model.title.set_label(if is_gmclaw {
            tr(text, "天工 Claw · 模型接入", "GMClaw · Models")
        } else {
            tr(text, "WorkBuddy · 模型接入", "WorkBuddy · Models")
        });
        ui.bridge.title.set_label(if is_gmclaw {
            tr(text, "天工 Claw · 外部消息", "GMClaw · External messages")
        } else {
            tr(
                text,
                "WorkBuddy · 外部消息",
                "WorkBuddy · External messages",
            )
        });
        let status = snapshot
            .and_then(|value| value.client_overview.as_ref())
            .map(|value| {
                if is_gmclaw {
                    &value.gmclaw
                } else {
                    &value.workbuddy
                }
            });
        if !online {
            let label = if starting {
                text.waiting_service()
            } else {
                text.unavailable()
            };
            set_status_panel(&ui.model, label, "", StateTone::Muted);
            set_status_panel(&ui.bridge, label, "", StateTone::Muted);
        } else if let Some(status) = status {
            let model = &status.model;
            let (label, detail, tone) = if model.error.is_some() {
                (
                    text.error().to_string(),
                    tr(
                        text,
                        "配置读取或校验失败，请在下方刷新。",
                        "Could not read or validate settings; refresh below.",
                    )
                    .to_string(),
                    StateTone::Error,
                )
            } else if !model.installed {
                (
                    text.not_configured().to_string(),
                    tr(
                        text,
                        "尚未检测到客户端配置。",
                        "Client settings have not been detected.",
                    )
                    .to_string(),
                    StateTone::Muted,
                )
            } else if is_gmclaw && model.configured && model.connected && model.running {
                (
                    text.connected().to_string(),
                    format!(
                        "{} · {}",
                        text.overview_model_count(model.count),
                        tr(
                            text,
                            "天工已连接 Hub 本地模型服务。",
                            "GMClaw has connected to the local Hub model service.",
                        )
                    ),
                    StateTone::Ok,
                )
            } else if is_gmclaw && model.configured {
                let detail = if !model.running {
                    tr(
                        text,
                        "配置已保存；天工尚未启动。",
                        "Settings saved; GMClaw is not running.",
                    )
                } else if !model.current_local_route {
                    tr(
                        text,
                        "接入地址与当前 Hub 不一致，请重新保存模型配置。",
                        "The configured address differs from this Hub; save model settings again.",
                    )
                } else {
                    tr(
                        text,
                        "天工已启动；等待其连接 Hub 本地模型服务。",
                        "GMClaw is running; waiting for its connection to the local Hub model service.",
                    )
                };
                (
                    if model.running {
                        tr(text, "等待连接", "Waiting for connection").to_string()
                    } else {
                        text.overview_model_count(model.count)
                    },
                    detail.to_string(),
                    StateTone::Warn,
                )
            } else if model.configured {
                (
                    text.overview_model_count(model.count),
                    tr(
                        text,
                        "配置已保存；模型调用尚未验证。",
                        "Settings saved; model calls are not verified.",
                    )
                    .to_string(),
                    StateTone::Warn,
                )
            } else {
                (
                    text.not_configured().to_string(),
                    tr(
                        text,
                        "请在下方完成模型接入。",
                        "Configure model access below.",
                    )
                    .to_string(),
                    StateTone::Warn,
                )
            };
            set_status_panel(&ui.model, &label, &detail, tone);
            let bridge = &status.bridge;
            if is_gmclaw && !bridge.enabled {
                set_status_panel(
                    &ui.bridge,
                    tr(text, "待首次使用", "Ready for first use"),
                    tr(
                        text,
                        "在 IM 中发送 /tg，Hub 会自动连接天工。",
                        "Send /tg in IM to let Hub connect GMClaw automatically.",
                    ),
                    StateTone::Muted,
                );
            } else {
                let (label, tone) = bridge_label(text, &bridge.state);
                set_status_panel(&ui.bridge, label, &bridge.detail, tone);
            }
            executor_connected = is_gmclaw && bridge.enabled && bridge.state == "connected";
            // Model HTTP access and IM Harness authorization are independent.
            // A local model connection does not require a successful upstream
            // call, and a Harness connection does not prove model HTTP access.
            entry_connected[0] = is_gmclaw && model.connected && model.running;
            entry_connected[1] = executor_connected;
        } else {
            set_status_panel(&ui.model, text.reading(), "", StateTone::Muted);
            set_status_panel(&ui.bridge, text.reading(), "", StateTone::Muted);
        }
        if !is_gmclaw {
            set_status_panel(
                &ui.bridge,
                text.overview_unsupported(),
                tr(
                    text,
                    "/wb 暂保留入口，不执行外部任务。",
                    "/wb is reserved; external tasks are unavailable.",
                ),
                StateTone::Muted,
            );
        }
        for (platform, row) in [
            ("feishu", &handles.im_status.feishu),
            ("telegram", &handles.im_status.telegram),
            ("wechat", &handles.im_status.wechat),
            ("wecom", &handles.im_status.wecom),
        ] {
            if !is_gmclaw || platform == "telegram" {
                set_im_channel_row(row, text.overview_unsupported(), "", StateTone::Muted);
            } else if !online {
                set_im_channel_row(row, text.waiting_service(), "", StateTone::Muted);
            } else if accounts.is_none() {
                set_im_channel_row(row, text.reading(), "", StateTone::Muted);
            } else if transport_ready(platform) {
                set_im_channel_row(
                    row,
                    text.im_connected(),
                    if executor_connected {
                        tr(text, "发送 /tg 切换到天工", "Send /tg to select GMClaw")
                    } else {
                        tr(
                            text,
                            "天工执行端尚未连通",
                            "GMClaw execution is not connected",
                        )
                    },
                    if executor_connected {
                        StateTone::Ok
                    } else {
                        StateTone::Warn
                    },
                );
            } else {
                set_im_channel_row(
                    row,
                    text.not_connected(),
                    tr(
                        text,
                        "请在聊天页配置并连接通道",
                        "Connect this channel on the Chat tab",
                    ),
                    StateTone::Warn,
                );
            }
        }
    }
    ui.entry_connector.update(kind, entry_connected);
    ui.bridge_connector.update(
        kind,
        [
            executor_connected && transport_ready("feishu"),
            executor_connected && transport_ready("telegram"),
            executor_connected && (transport_ready("wechat") || transport_ready("wecom")),
        ],
    );
    ui.container.layout();
    ui.root.layout();
}

fn im_ready(account: &ImAccountItem, platform: &str) -> bool {
    account.platform == platform
        && account.enabled
        && account.configured
        && account.secret_set
        && !account
            .last_error
            .as_deref()
            .is_some_and(|error| !error.trim().is_empty())
        && (account.connected || (matches!(platform, "wechat" | "telegram") && account.polling))
}

fn bridge_label(text: GuiText, state: &str) -> (&'static str, StateTone) {
    match state {
        "connected" => (
            tr(text, "授权已连通", "Authorization verified"),
            StateTone::Ok,
        ),
        "not_configured" => (text.not_configured(), StateTone::Warn),
        "not_running" => (text.not_running(), StateTone::Warn),
        "unverified" => (tr(text, "尚未验证", "Not verified"), StateTone::Warn),
        "auth_failed" => (
            tr(text, "授权不一致", "Authorization mismatch"),
            StateTone::Error,
        ),
        "unsupported" => (text.overview_unsupported(), StateTone::Muted),
        "error" => (text.error(), StateTone::Error),
        _ => (text.reading(), StateTone::Muted),
    }
}
