//! Isolated product-renderer review. No Hub modules, configuration, daemon,
//! network, clients, login, or interactive window are initialized here.
#![allow(dead_code)]
use std::path::{Path, PathBuf};
use wxdragon::{
    dc::{GraphicsDC, MemoryDC},
    prelude::*,
};
#[path = "../src/desktop/clients.rs"]
pub mod client_types;
mod desktop {
    pub use super::client_types as clients;
    pub fn state_root(_: &std::path::Path) -> std::path::PathBuf {
        unreachable!("no configuration in visual review")
    }
    pub mod transaction {
        pub fn read(_: &std::path::Path) -> std::io::Result<Option<Vec<u8>>> {
            unreachable!("no configuration in visual review")
        }
        pub fn atomic_write(_: &std::path::Path, _: &[u8]) -> std::io::Result<()> {
            unreachable!("no configuration writes in visual review")
        }
    }
}
fn daemon_config_path() -> Option<PathBuf> {
    None
}
fn show_info<W: WxWidget>(_: &W, _: &str) {
    unreachable!("no interactive dialogs in visual review")
}
#[path = "../src/gui/controls.rs"]
mod controls;
#[path = "../src/gui/shell.rs"]
mod native_shell;
#[path = "../src/gui/theme.rs"]
mod theme;
#[path = "../src/gui/visual.rs"]
mod visual;
use client_types::ClientId;
use controls::{ButtonKind, draw_button};
use visual::{Canvas, ClientDiagram, Icon, RuntimeClient, RuntimeMcp, RuntimeRoute};

fn raster(width: i32, height: i32, scale: f64, render: impl FnOnce(&Canvas<'_>)) -> Bitmap {
    let mut bitmap = Bitmap::new(
        (width as f64 * scale).round() as i32,
        (height as f64 * scale).round() as i32,
    )
    .unwrap();
    {
        let mut memory = MemoryDC::new();
        memory.select_object(&mut bitmap);
        let graphics = GraphicsDC::new(&memory);
        let c = Canvas::new(&graphics, scale);
        c.clear(theme::theme().bg_app);
        render(&c);
    }
    bitmap
}
fn save(root: &Path, name: &str, bitmap: Bitmap) {
    let (w, h) = (bitmap.get_width() as u32, bitmap.get_height() as u32);
    let mut rgba = bitmap.get_rgba_data().unwrap();
    // Windows GDI may leave unused bitmap alpha at zero; the product surface is opaque.
    for px in rgba.chunks_exact_mut(4) {
        px[3] = 255;
    }
    let image = image::RgbaImage::from_raw(w, h, rgba).unwrap();
    image.save(root.join(format!("{name}.png"))).unwrap();
    println!("{name}: {w} x {h}");
}
fn button(c: &Canvas<'_>, r: Rect, label: &str, kind: ButtonKind, icon: Icon, selected: bool) {
    draw_button(
        c,
        r,
        label,
        kind as u32 | ((icon as u32) << 8) | if selected { 0x10000 } else { 0 },
        8,
        theme::theme().ink_secondary,
    );
}
fn shell(c: &Canvas<'_>, width: i32, height: i32, client: bool) {
    let t = theme::theme();
    c.rounded(Rect::new(0, 0, 222, height), 0, t.bg_card, None);
    c.line(
        Point::new(221, 0),
        Point::new(221, height),
        t.divider,
        false,
    );
    visual::draw_brand(c, 22, 23);
    button(
        c,
        Rect::new(10, 88, 202, 40),
        "概览",
        ButtonKind::Nav,
        Icon::Overview,
        !client,
    );
    button(
        c,
        Rect::new(10, 138, 202, 40),
        "天才小分队",
        ButtonKind::Nav,
        Icon::Squad,
        false,
    );
    c.text("客户端", 24, 201, 11, false, t.ink_muted);
    for (i, id) in [ClientId::Codex, ClientId::WorkBuddy, ClientId::Tiangong]
        .into_iter()
        .enumerate()
    {
        button(
            c,
            Rect::new(6, 227 + i as i32 * 46, 210, 40),
            visual::client_short(id),
            ButtonKind::Nav,
            Icon::client(id),
            client && id == ClientId::WorkBuddy,
        );
    }
    button(
        c,
        Rect::new(10, 365, 202, 30),
        "⋯",
        ButtonKind::Ghost,
        Icon::None,
        false,
    );
    c.text("共用能力", 24, 418, 11, false, t.ink_muted);
    for (i, (label, icon)) in [
        ("模型与渠道", Icon::Model),
        ("Skill 管理", Icon::Skill),
        ("MCP 管理", Icon::Mcp),
        ("聊天接入", Icon::Chat),
        ("请求日志", Icon::Logs),
    ]
    .into_iter()
    .enumerate()
    {
        button(
            c,
            Rect::new(8, 446 + i as i32 * 48, 206, 40),
            label,
            ButtonKind::Nav,
            icon,
            false,
        );
    }
    button(
        c,
        Rect::new(10, height - 140, 202, 40),
        "设置",
        ButtonKind::Nav,
        Icon::Settings,
        false,
    );
    button(
        c,
        Rect::new(10, height - 90, 202, 40),
        "帮助",
        ButtonKind::Nav,
        Icon::Help,
        false,
    );
    c.text(
        concat!("TianCaiSpaceHub  ", env!("CARGO_PKG_VERSION")),
        20,
        height - 30,
        10,
        false,
        t.ink_muted,
    );
    c.rounded(Rect::new(222, 0, width - 222, 48), 0, t.bg_card, None);
    c.text(
        if client {
            "工作台 / WorkBuddy"
        } else {
            "工作台 / 概览"
        },
        248,
        16,
        11,
        false,
        t.ink_secondary,
    );
    c.dot(width - 166, 24, 3, t.ok);
    c.text("本机服务 · 运行中", width - 155, 16, 11, false, t.ok);
    c.text(
        if client { "WorkBuddy" } else { "概览" },
        248,
        78,
        26,
        true,
        t.ink_primary,
    );
    if client {
        c.dot(433, 96, 3, t.accent);
        c.text("1 个请求处理中", 445, 88, 11, false, t.accent);
    }
    button(
        c,
        Rect::new(width - 58, 80, 30, 30),
        "?",
        ButtonKind::Ghost,
        Icon::None,
        false,
    );
    if client {
        for (i, label) in ["状态概览", "模型", "Skill", "MCP", "接入设置"]
            .into_iter()
            .enumerate()
        {
            button(
                c,
                Rect::new(248 + i as i32 * 96, 137, 90, 42),
                label,
                ButtonKind::Tab,
                Icon::None,
                i == 0,
            );
        }
    } else {
        button(
            c,
            Rect::new(248, 137, 100, 42),
            "运行状态",
            ButtonKind::Tab,
            Icon::None,
            true,
        );
        button(
            c,
            Rect::new(360, 137, 100, 42),
            "使用统计",
            ButtonKind::Tab,
            Icon::None,
            false,
        );
    }
    c.line(
        Point::new(248, 180),
        Point::new(width - 26, 180),
        t.divider,
        false,
    );
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| format!("target/v{}-visual-review", env!("CARGO_PKG_VERSION")));
    std::fs::create_dir_all(&output)?;
    wxdragon::app::render_offscreen(move || {
        native_layout_checks();
        let root = Path::new(&output);
        for (name, mode, width, height, scale) in [
            ("runtime-light", theme::ThemeMode::Light, 1366, 900, 1.),
            ("client-light", theme::ThemeMode::Light, 1366, 900, 1.),
            ("runtime-wide", theme::ThemeMode::Light, 1920, 1080, 1.),
            ("runtime-dark", theme::ThemeMode::Dark, 1366, 900, 1.),
            ("client-dark", theme::ThemeMode::Dark, 1366, 900, 1.),
            (
                "runtime-compact-150",
                theme::ThemeMode::Light,
                1024,
                768,
                1.5,
            ),
            (
                "client-compact-150",
                theme::ThemeMode::Light,
                1024,
                768,
                1.5,
            ),
            ("runtime-idle", theme::ThemeMode::Light, 1366, 900, 1.),
        ] {
            theme::init(mode);
            let client = name.starts_with("client");
            let empty = name == "runtime-idle";
            let clients = if empty {
                Vec::new()
            } else {
                vec![
                    RuntimeClient {
                        id: ClientId::Codex,
                        state: "已打开 · 空闲".into(),
                        busy: false,
                        flowing: false,
                        opacity: 1.,
                    },
                    RuntimeClient {
                        id: ClientId::WorkBuddy,
                        state: "1 个请求处理中".into(),
                        busy: true,
                        flowing: true,
                        opacity: 1.,
                    },
                    RuntimeClient {
                        id: ClientId::Tiangong,
                        state: "已打开 · 空闲".into(),
                        busy: false,
                        flowing: false,
                        opacity: 1.,
                    },
                ]
            };
            let routes = if empty {
                Vec::new()
            } else {
                vec![RuntimeRoute {
                    route: "synthetic".into(),
                    active: true,
                    opacity: 1.,
                    model: "gpt-6".into(),
                    channel: "天才小分队".into(),
                    detail: "WorkBuddy · 1 个请求".into(),
                    flowing: true,
                }]
            };
            let graph = raster(width - 274, 430, scale, |c| {
                let size = Size::new(width - 274, 430);
                if client {
                    let v = ClientDiagram {
                        id: ClientId::WorkBuddy,
                        opened: "已打开",
                        execution: "支持模型接入",
                        model: "1 个请求处理中",
                        channel: "天才小分队",
                        skill: "2 项使用中",
                        mcp: "3 项使用中",
                        chat: "外部执行待接通",
                        states: [
                            Some(true),
                            Some(true),
                            None,
                            Some(true),
                            Some(true),
                            Some(false),
                        ],
                        hub: Some(true),
                        flowing: true,
                    };
                    let hits = visual::draw_client(c, size, &v, 0.3);
                    assert_eq!(hits.len(), 6);
                    for (rect, _) in hits {
                        assert!(rect.x >= 0 && rect.y >= 0);
                        assert!(rect.x + rect.width <= c.px(size.width));
                        assert!(rect.y + rect.height <= c.px(size.height));
                    }
                } else {
                    let hits =
                        visual::draw_runtime(c, size, &clients, &routes, &[], Some(true), 0.3);
                    assert_eq!(hits.clients.len(), clients.len());
                    assert_eq!(hits.models.len(), routes.len());
                    for rect in hits
                        .clients
                        .iter()
                        .chain(&hits.models)
                        .chain(&hits.channels)
                    {
                        assert!(
                            rect.x >= 0 && rect.y >= 0 && rect.x + rect.width <= c.px(size.width),
                            "clipped runtime node"
                        );
                    }
                }
            });
            let full = raster(width, height, scale, |c| {
                shell(c, width, height, client);
                c.dc.draw_bitmap(&graph, c.px(248), c.px(203), true);
            });
            save(root, name, full);
        }
        for (name, mode, width, scale) in [
            ("runtime-seven-clients", theme::ThemeMode::Light, 1092, 1.),
            (
                "runtime-seven-compact-150",
                theme::ThemeMode::Light,
                698,
                1.5,
            ),
            ("runtime-seven-dark", theme::ThemeMode::Dark, 1092, 1.),
        ] {
            theme::init(mode);
            let clients = ClientId::ALL
                .into_iter()
                .enumerate()
                .map(|(i, id)| RuntimeClient {
                    id,
                    state: if i < 3 {
                        "请求处理中"
                    } else {
                        "已打开 · 空闲"
                    }
                    .into(),
                    busy: i < 3,
                    flowing: i < 3,
                    opacity: 1.,
                })
                .collect::<Vec<_>>();
            let routes = (0..6)
                .map(|i| RuntimeRoute {
                    route: "synthetic".into(),
                    active: true,
                    opacity: 1.,
                    model: format!("example-model-{}", i + 1),
                    channel: format!("示例渠道 {}", i + 1),
                    detail: format!("{} 个请求", i + 1),
                    flowing: i < 4,
                })
                .collect::<Vec<_>>();
            let h = visual::runtime_height(clients.len(), routes.len(), 0);
            let bitmap = raster(width, h, scale, |c| {
                let hits = visual::draw_runtime(
                    c,
                    Size::new(width, h),
                    &clients,
                    &routes,
                    &[],
                    Some(true),
                    0.3,
                );
                assert_eq!(hits.clients.len(), 7);
                assert_eq!(hits.models.len(), 6);
                assert_eq!(hits.channels.len(), 6);
                let all = hits
                    .clients
                    .iter()
                    .chain(&hits.models)
                    .chain(&hits.channels)
                    .collect::<Vec<_>>();
                for (i, rect) in all.iter().enumerate() {
                    assert!(rect.x >= 0 && rect.y >= 0);
                    assert!(rect.x + rect.width <= c.px(width));
                    assert!(rect.y + rect.height <= c.px(h));
                    for other in all.iter().skip(i + 1) {
                        assert!(
                            rect.x + rect.width <= other.x
                                || other.x + other.width <= rect.x
                                || rect.y + rect.height <= other.y
                                || other.y + other.height <= rect.y,
                            "overlapping route targets"
                        );
                    }
                }
                for (model, channel) in hits.models.iter().zip(&hits.channels) {
                    assert_eq!(model.y, channel.y, "model/channel pairing moved");
                }
            });
            save(root, name, bitmap);
        }
        theme::init(theme::ThemeMode::Light);
        let unlabelled = raster(900, 430, 1., |c| {
            let routes = [RuntimeRoute {
                route: "synthetic".into(),
                active: true,
                opacity: 1.,
                model: "example-model".into(),
                channel: "示例渠道".into(),
                detail: "1 个请求".into(),
                flowing: true,
            }];
            let hits =
                visual::draw_runtime(c, Size::new(900, 430), &[], &routes, &[], Some(true), 0.3);
            assert!(hits.clients.is_empty());
            assert_eq!(hits.models.len(), 1);
            assert_eq!(hits.channels.len(), 1);
            assert!(
                hits.models[0].x > 360,
                "model must stay clear of Hub with an unlabelled caller"
            );
        });
        save(root, "runtime-unlabelled-caller", unlabelled);

        for (name, mode, width, height, dim, scale) in [
            (
                "runtime-mcp-small",
                theme::ThemeMode::Light,
                620,
                430,
                false,
                1.,
            ),
            (
                "runtime-mcp-wide",
                theme::ThemeMode::Light,
                1600,
                980,
                false,
                1.,
            ),
            (
                "runtime-mcp-faded",
                theme::ThemeMode::Light,
                1092,
                650,
                true,
                1.,
            ),
            (
                "runtime-mcp-dark-150",
                theme::ThemeMode::Dark,
                698,
                650,
                false,
                1.5,
            ),
        ] {
            theme::init(mode);
            let opacity = if dim { 0.4 } else { 1. };
            let clients = [ClientId::Codex, ClientId::WorkBuddy, ClientId::Tiangong]
                .into_iter()
                .map(|id| RuntimeClient {
                    id,
                    state: if dim { "本页已使用" } else { "已打开" }.into(),
                    busy: !dim,
                    flowing: !dim,
                    opacity,
                })
                .collect::<Vec<_>>();
            let routes = vec![RuntimeRoute {
                route: "fixture".into(),
                model: "gpt-6".into(),
                channel: "示例渠道".into(),
                detail: if dim {
                    "请求已结束"
                } else {
                    "1 个请求"
                }
                .into(),
                active: !dim,
                flowing: !dim,
                opacity,
            }];
            let mcps = vec![
                RuntimeMcp {
                    name: "Dumpling-MCP".into(),
                    detail: if dim {
                        "调用已结束"
                    } else {
                        "1 个调用"
                    }
                    .into(),
                    active: !dim,
                    flowing: !dim,
                    opacity,
                },
                RuntimeMcp {
                    name: "filesystem".into(),
                    detail: "调用已结束".into(),
                    active: false,
                    flowing: false,
                    opacity: 1.,
                },
            ];
            assert!(height >= visual::runtime_height(3, 1, 2));
            let bitmap = raster(width, height, scale, |c| {
                let hits = visual::draw_runtime(
                    c,
                    Size::new(width, height),
                    &clients,
                    &routes,
                    &mcps,
                    Some(true),
                    0.4,
                );
                assert_eq!(hits.mcps.len(), 2);
                let all = hits
                    .clients
                    .iter()
                    .chain(&hits.models)
                    .chain(&hits.channels)
                    .chain(&hits.mcps)
                    .collect::<Vec<_>>();
                for (i, r) in all.iter().enumerate() {
                    assert!(
                        r.x >= 0
                            && r.y >= 0
                            && r.x + r.width <= c.px(width)
                            && r.y + r.height <= c.px(height)
                    );
                    for other in all.iter().skip(i + 1) {
                        assert!(
                            r.x + r.width <= other.x
                                || other.x + other.width <= r.x
                                || r.y + r.height <= other.y
                                || other.y + other.height <= r.y,
                            "MCP and model targets must never overlap"
                        );
                    }
                }
            });
            save(root, name, bitmap);
        }
        let chart = raster(1040, 300, 1., |c| {
            visual::draw_usage_chart(
                c,
                Size::new(1040, 300),
                &[
                    ("11-02".into(), Some(82000), Some(27000)),
                    ("11-03".into(), Some(114000), Some(35000)),
                    ("11-04".into(), Some(72000), Some(24000)),
                    ("11-05".into(), Some(164000), Some(58000)),
                    ("11-06".into(), Some(126000), Some(42000)),
                    ("11-07".into(), Some(202000), Some(71000)),
                    ("11-08".into(), Some(153000), Some(54000)),
                ],
            )
        });
        save(root, "usage-chart-light", chart);
        for (name, mode) in [
            ("controls-light", theme::ThemeMode::Light),
            ("controls-dark", theme::ThemeMode::Dark),
        ] {
            theme::init(mode);
            let t = theme::theme();
            let bitmap = raster(840, 360, 1., |c| {
                c.text("共用控件 · 合成数据", 24, 24, 20, true, t.ink_primary);
                for (i, (label, kind)) in [
                    ("添加 MCP", ButtonKind::Primary),
                    ("编辑配置", ButtonKind::Secondary),
                    ("重新读取", ButtonKind::Ghost),
                ]
                .into_iter()
                .enumerate()
                {
                    button(
                        c,
                        Rect::new(24 + i as i32 * 160, 78, 136, 36),
                        label,
                        kind,
                        Icon::None,
                        false,
                    );
                }
                for (i, id) in ClientId::ALL.into_iter().enumerate() {
                    button(
                        c,
                        Rect::new(24 + i as i32 % 4 * 124, 158 + i as i32 / 4 * 78, 112, 62),
                        id.name(),
                        ButtonKind::Tile,
                        Icon::client(id),
                        i < 3,
                    );
                }
            });
            save(root, name, bitmap);
        }
    })?;
    Ok(())
}

fn native_layout_checks() {
    theme::init(theme::ThemeMode::Light);
    let font = theme::font(theme::TextRole::Body);
    assert!(font.is_ok());
    #[cfg(windows)]
    assert_eq!(font.get_face_name(), "Microsoft YaHei UI");
    println!("native UI font: {}", font.get_face_name());
    // Only this hidden, synthetic container is constructed. It has no Hub app
    // initializer, API client, config location, timers, processes or event loop.
    let frame = Frame::builder().with_title("isolated layout check").build();
    frame.set_font(&font);
    for (w, h) in [(1366, 900), (1024, 768), (1920, 1080)] {
        frame.set_client_size(visual::dip_size(&frame, w, h));
        let root = Panel::builder(&frame).build();
        let shell = native_shell::Shell::new(&root, &frame);
        let home = Panel::builder(&shell.book).build();
        let wb = Panel::builder(&shell.book).build();
        shell.register(native_shell::PageId::Overview, &home);
        shell.register(native_shell::PageId::Client(ClientId::WorkBuddy), &wb);
        for id in [ClientId::VsCode, ClientId::ZCode, ClientId::Codex] {
            let page = Panel::builder(&shell.book).build();
            shell.register(native_shell::PageId::Client(id), &page);
        }
        let rs = BoxSizer::builder(Orientation::Vertical).build();
        rs.add(&shell.panel, 1, SizerFlag::Expand, 0);
        root.set_sizer(rs, true);
        let fs = BoxSizer::builder(Orientation::Vertical).build();
        fs.add(&root, 1, SizerFlag::Expand, 0);
        frame.set_sizer(fs, true);
        frame.layout();
        root.layout();
        shell.panel.layout();
        let side = shell.panel.find_window_by_name("WebStorm").unwrap();
        assert!(
            !side.is_shown(),
            "collapsed sidebar must hide additional clients"
        );
        assert!(
            !shell
                .panel
                .find_window_by_name("collapse-clients")
                .unwrap()
                .is_shown()
        );
        assert!(
            shell
                .panel
                .find_window_by_name("expand-clients")
                .unwrap()
                .is_shown()
        );
        #[cfg(windows)]
        {
            synthetic_key(
                &shell.panel.find_window_by_name("expand-clients").unwrap(),
                32,
            );
            assert!(
                side.is_shown(),
                "expand command did not reveal additional clients"
            );
            assert!(
                !shell
                    .panel
                    .find_window_by_name("expand-clients")
                    .unwrap()
                    .is_shown()
            );
            let collapse = shell.panel.find_window_by_name("collapse-clients").unwrap();
            assert!(collapse.is_shown());
            let codex = shell.panel.find_window_by_name("ChatGPT / Codex").unwrap();
            let vscode = shell.panel.find_window_by_name("VS Code").unwrap();
            let before_codex = codex.get_position();
            let before_vscode = vscode.get_position();
            synthetic_key(&vscode, 13);
            assert_eq!(
                shell.current(),
                native_shell::PageId::Client(ClientId::VsCode)
            );
            assert_eq!(
                vscode.get_position(),
                before_vscode,
                "expanded client moved on selection"
            );
            assert_eq!(codex.get_position(), before_codex);
            synthetic_key(&codex, 13);
            assert_eq!(
                shell.current(),
                native_shell::PageId::Client(ClientId::Codex),
                "frozen slot activated a different client"
            );
            assert_eq!(vscode.get_position(), before_vscode);
            synthetic_key(&collapse, 13);
            assert!(
                !side.is_shown(),
                "collapse command did not hide additional clients"
            );
        }
        let codex = shell.panel.find_window_by_name("ChatGPT / Codex").unwrap();
        assert!(codex.is_shown());
        assert!(codex.accepts_focus_from_keyboard());
        let r = codex.get_size();
        assert!(r.width >= codex.from_dip(178), "navigation was squeezed");
        shell.navigate(native_shell::PageId::Client(ClientId::WorkBuddy));
        assert_eq!(
            shell.current(),
            native_shell::PageId::Client(ClientId::WorkBuddy)
        );
        assert!(!side.is_shown(), "navigation must preserve collapse");
        let refresh = controls::Button::builder(&root)
            .with_label("重新读取")
            .with_icon(Icon::Refresh)
            .build();
        assert!(
            refresh.get_min_size().width >= refresh.from_dip(116),
            "icon stole label width"
        );
        println!(
            "native layout: {w} x {h}, nav {} x {}, extra clients hidden",
            r.width, r.height
        );
        #[cfg(windows)]
        {
            use std::{cell::Cell, rc::Rc};
            let count = Rc::new(Cell::new(0));
            let called = count.clone();
            let command = controls::Button::builder(&root)
                .with_label("synthetic command")
                .build();
            command.on_click(move |_| called.set(called.get() + 1));
            synthetic_key(&command, 32);
            assert_eq!(count.get(), 1);
            synthetic_key(&command, 13);
            assert_eq!(count.get(), 2);
            command.enable(false);
            synthetic_key(&command, 32);
            assert_eq!(count.get(), 2, "disabled command activated");
            command.enable(true);
            synthetic_key(&command, 13);
            assert_eq!(count.get(), 3);
        }
        // Replace the previous synthetic root without leaving an interactive window.
        root.destroy();
    }
    frame.destroy();
}

#[cfg(windows)]
fn synthetic_key(window: &dyn WxWidget, key: usize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_KEYDOWN, WM_KEYUP};
    // Direct messages to our own hidden synthetic control, never global input
    // injection and never an HWND belonging to Hub or a user's client.
    unsafe {
        SendMessageW(window.get_handle(), WM_KEYDOWN, key, 0);
        SendMessageW(window.get_handle(), WM_KEYDOWN, key, 1 << 30);
        SendMessageW(window.get_handle(), WM_KEYUP, key, 0);
    }
}
