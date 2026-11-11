//! Hidden native layout checks using the production shell, statistics layout,
//! fields and rendering. No Hub startup, configuration, API or client process.
#![allow(dead_code)]
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    rc::Rc,
};
use wxdragon::{dc::GraphicsDC, prelude::*};
#[path = "../src/desktop/clients.rs"]
pub mod client_types;
#[path = "../src/desktop/request_source.rs"]
pub mod request_source_types;
#[path = "../src/desktop/squad_money.rs"]
pub mod squad_money;
#[path = "../src/desktop/squad_quote.rs"]
pub mod squad_quote;
mod desktop {
    pub use super::request_source_types as request_source;
    pub use super::{squad_money, squad_quote};
    pub mod squad {
        pub const ORIGIN: &str = "https://tiancai.yc99.space";
    }
    pub use super::client_types as clients;
    pub fn state_root(_: &std::path::Path) -> std::path::PathBuf {
        unreachable!("no configuration")
    }
    pub mod transaction {
        pub fn read(_: &std::path::Path) -> std::io::Result<Option<Vec<u8>>> {
            unreachable!("no configuration")
        }
        pub fn atomic_write(_: &std::path::Path, _: &[u8]) -> std::io::Result<()> {
            unreachable!("no writes")
        }
    }
}
fn daemon_config_path() -> Option<PathBuf> {
    None
}
fn show_info<W: WxWidget>(_: &W, _: &str) {
    unreachable!("no dialogs")
}
#[path = "../src/gui/client_header.rs"]
mod client_header;
#[path = "../src/gui/codex_models.rs"]
mod codex_models;
#[path = "../src/gui/controls.rs"]
mod controls;
#[path = "../src/gui/model_fields.rs"]
mod model_fields;
#[path = "../src/gui/model_list.rs"]
mod model_list;
#[path = "../src/gui/provider_form.rs"]
mod provider_form;
#[path = "../src/gui/request_view.rs"]
mod request_view;
#[path = "../src/gui/resource_detail.rs"]
mod resource_detail;
#[path = "../src/gui/resource_gallery.rs"]
mod resource_gallery;
#[path = "../src/gui/shell.rs"]
mod shell;
#[path = "../src/gui/squad_browser.rs"]
mod squad_browser;
#[path = "../src/gui/squad_login.rs"]
mod squad_login;
#[path = "../src/gui/squad_view.rs"]
mod squad_view;
#[path = "../src/gui/theme.rs"]
mod theme;
#[path = "../src/gui/usage_view.rs"]
mod usage_view;
#[path = "../src/gui/visual.rs"]
mod visual;

// Network entry points deliberately fail if a presentation review ever reaches them.
mod api {
    #[derive(Clone)]
    pub struct ApiClient;
}
mod dialog_job {
    pub struct Job {
        pub result: std::rc::Rc<std::cell::RefCell<Option<serde_json::Value>>>,
    }
    pub fn bind(
        _: wxdragon::Dialog,
        _: super::controls::Button,
        _: wxdragon::StaticText,
        _: super::api::ApiClient,
        _: impl Fn() -> Result<(String, serde_json::Value), String> + 'static,
    ) -> Job {
        unreachable!("review must not send account requests")
    }
}
fn ai_gw_channel_dialog_size(_: &Frame, desired: Size) -> Size {
    desired
}
#[path = "../src/gui/squad_form.rs"]
mod squad_form;
fn layout_tree(window: &dyn WxWidget) {
    window.layout();
}
fn save(path: &Path, bitmap: Bitmap) {
    let w = bitmap.get_width() as u32;
    let h = bitmap.get_height() as u32;
    let mut pixels = bitmap.get_rgba_data().expect("native image data");
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    image::RgbaImage::from_raw(w, h, pixels)
        .unwrap()
        .save(path)
        .unwrap();
    println!("native capture: {} ({w} x {h})", path.display());
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or("target/v1.0.5-native-layout".into()),
    );
    std::fs::create_dir_all(&output)?;
    wxdragon::app::render_offscreen(move || {
        let probe = Frame::builder().with_title("hidden DPI metadata").build();
        std::fs::write(output.join("capture-metadata.json"),serde_json::to_vec_pretty(&serde_json::json!({"wxScale":visual::scale(&probe),"dip100Pixels":probe.from_dip(100),"synthetic":true,"productStarted":false,"nativeRuntimeDpiAcceptance":false})).unwrap()).unwrap();
        probe.destroy();
        for (label, mode, w, h) in [
            ("statistics-light", theme::ThemeMode::Light, 1366, 900),
            ("statistics-small", theme::ThemeMode::Light, 960, 640),
            ("statistics-minimum", theme::ThemeMode::Light, 900, 560),
            (
                "statistics-minimum-filtered",
                theme::ThemeMode::Light,
                900,
                560,
            ),
            ("statistics-wide", theme::ThemeMode::Light, 1920, 1080),
            ("statistics-dark", theme::ThemeMode::Dark, 1366, 900),
        ] {
            theme::init(mode);
            let frame = Frame::builder()
                .with_title("hidden production layout")
                .build();
            frame.set_font(&theme::font(theme::TextRole::Body));
            if label.starts_with("statistics-minimum") {
                // The product minimum describes the outer window, not the
                // client area left after Windows decorations.
                frame.set_size(visual::dip_size(&frame, w, h));
            } else {
                frame.set_client_size(visual::dip_size(&frame, w, h));
            }
            let root = Panel::builder(&frame).build();
            let shell = shell::Shell::new(&root, &frame);
            let home = Panel::builder(&shell.book).build();
            home.set_background_color(theme::theme().bg_app);
            let col = BoxSizer::builder(Orientation::Vertical).build();
            let tabs = BoxSizer::builder(Orientation::Horizontal).build();
            for name in ["运行状态", "使用统计"] {
                let b = controls::Button::builder(&home)
                    .with_label(name)
                    .with_kind(controls::ButtonKind::Tab)
                    .with_size(Size::new(100, 42))
                    .build();
                b.set_selected(name == "使用统计");
                tabs.add(&b, 0, SizerFlag::Right, 12);
            }
            col.add_sizer(
                &tabs,
                0,
                SizerFlag::Expand | SizerFlag::Bottom,
                home.from_dip(12),
            );
            let usage = usage_view::UsageView::new(&home);
            assert_eq!(usage.periods.map(|b| b.selected()), [false, true, false]);
            assert_eq!(usage_view::PERIOD_DAYS[1], usage_view::DEFAULT_DAYS);
            if label.ends_with("filtered") {
                usage.client.set_selection(1);
                usage.update_filter_label("gpt-6", "示例渠道");
            }
            for (stat, value) in usage
                .stats
                .iter()
                .zip(["4587", "93.3%", "582.84M", "3.43M"])
            {
                stat.set_label(value);
            }
            for (i, name) in ["ChatGPT / Codex", "WorkBuddy", "天工 Work"]
                .into_iter()
                .enumerate()
            {
                usage.breakdown.insert_item(i as i64, name, None);
                for (j, value) in ["1529", "194.28M", "1.14M"].into_iter().enumerate() {
                    usage
                        .breakdown
                        .set_item_text_by_column(i as i64, j as i32 + 1, value);
                }
            }
            usage.chart.on_paint(move |_| {
                let base = AutoBufferedPaintDC::new(&usage.chart);
                let dc = GraphicsDC::new(&base);
                let factor = visual::scale(&usage.chart);
                let c = visual::Canvas::new(&dc, factor);
                let s = usage.chart.get_client_size();
                visual::draw_usage_chart(
                    &c,
                    Size::new(
                        (s.width as f64 / factor) as i32,
                        (s.height as f64 / factor) as i32,
                    ),
                    &[
                        ("11-02".into(), Some(82000), Some(27000)),
                        ("11-03".into(), Some(114000), Some(35000)),
                        ("11-04".into(), Some(72000), Some(24000)),
                        ("11-05".into(), Some(164000), Some(58000)),
                        ("11-06".into(), Some(126000), Some(42000)),
                        ("11-07".into(), Some(202000), Some(71000)),
                        ("11-08".into(), Some(153000), Some(54000)),
                    ],
                );
                visual::draw_usage_hover(
                    &c,
                    Size::new(
                        (s.width as f64 / factor) as i32,
                        (s.height as f64 / factor) as i32,
                    ),
                    &[
                        ("11-02".into(), Some(82000), Some(27000)),
                        ("11-03".into(), Some(114000), Some(35000)),
                        ("11-04".into(), Some(72000), Some(24000)),
                        ("11-05".into(), Some(164000), Some(58000)),
                        ("11-06".into(), Some(126000), Some(42000)),
                        ("11-07".into(), Some(202000), Some(71000)),
                        ("11-08".into(), Some(153000), Some(54000)),
                    ],
                    3,
                    "11-05",
                    42,
                );
            });
            col.add(&usage.page, 1, SizerFlag::Expand, 0);
            home.set_sizer(col, true);
            shell.register(shell::PageId::Overview, &home);
            shell.service.set_label("本机服务 · 运行中");
            let rs = BoxSizer::builder(Orientation::Vertical).build();
            rs.add(&shell.panel, 1, SizerFlag::Expand, 0);
            root.set_sizer(rs, true);
            let fs = BoxSizer::builder(Orientation::Vertical).build();
            fs.add(&root, 1, SizerFlag::Expand, 0);
            frame.set_sizer(fs, true);
            layout_tree(&frame);
            layout_tree(&frame);
            assert!(!frame.is_shown(), "must remain hidden");
            let content = usage.page.get_client_size();
            let virtual_size = usage.page.get_best_size();
            assert!(
                virtual_size.width <= content.width + usage.page.from_dip(2)
                    && virtual_size.height <= content.height + usage.page.from_dip(2),
                "statistics must fit its viewport in both directions: {virtual_size:?} vs {content:?}"
            );
            println!(
                "chart size {:?}, min {:?}, page {:?}, virtual {:?}",
                usage.chart.get_size(),
                usage.chart.get_min_size(),
                content,
                virtual_size
            );
            assert!(usage.chart.get_size().height >= usage.chart.from_dip(168));
            let metric_height = usage.stats[0].get_parent().unwrap().get_size().height;
            assert!(
                metric_height <= usage.page.from_dip(82),
                "metric cards grew too tall: {metric_height}"
            );
            let side = shell
                .panel
                .find_window_by_name("expand-clients")
                .unwrap()
                .get_parent()
                .unwrap();
            let _ = side;
            println!(
                "layout {label}: DPI {}, viewport {content:?}, virtual {virtual_size:?}, metric height {metric_height}",
                usage.page.from_dip(100)
            );
            if let Some(bitmap) = root.capture_bitmap() {
                save(&output.join(format!("{label}.png")), bitmap);
            }
            let selector = controls::Choice::builder(&home)
                .with_choices(vec!["a&b".into(), "b".into()])
                .build();
            let events = Rc::new(Cell::new(0));
            let count = events.clone();
            selector.on_selection_changed(move |_| count.set(count.get() + 1));
            selector.set_selection(1);
            assert_eq!(selector.get_value(), "b");
            assert_eq!(
                events.get(),
                0,
                "programmatic selection must not fire user events"
            );
            #[cfg(windows)]
            {
                use windows_sys::Win32::UI::WindowsAndMessaging::{
                    SendMessageW, WM_KEYDOWN, WM_KEYUP,
                };
                unsafe {
                    SendMessageW(selector.get_handle(), WM_KEYDOWN, 0x26, 0);
                    SendMessageW(selector.get_handle(), WM_KEYUP, 0x26, 0);
                }
                assert_eq!(selector.get_value(), "a&b");
                assert_eq!(events.get(), 1);
                selector.enable(false);
                unsafe {
                    SendMessageW(selector.get_handle(), WM_KEYDOWN, 0x28, 0);
                    SendMessageW(selector.get_handle(), WM_KEYUP, 0x28, 0);
                }
                assert_eq!(selector.get_value(), "a&b");
                assert_eq!(events.get(), 1);
                selector.enable(true);
            }
            selector.clear();
            assert_eq!(selector.get_count(), 0);
            selector.append("late");
            selector.set_value("late");
            assert_eq!(selector.get_value(), "late");
            let input = controls::TextCtrl::builder(&home)
                .with_value("合成输入")
                .build();
            let edits = Rc::new(Cell::new(0));
            let count = edits.clone();
            input.on_text_changed(move |_| count.set(count.get() + 1));
            input.change_value("静默更新");
            assert_eq!(edits.get(), 0);
            input.set_value("事件更新");
            assert!(edits.get() > 0);
            input.set_editable(false);
            assert!(!input.is_editable());
            selector.hide();
            input.hide();
            #[cfg(windows)]
            if content.height < usage.page.from_dip(560) {
                let toggle = usage.page.find_window_by_name("用量分布").unwrap();
                assert!(toggle.is_shown());
                use windows_sys::Win32::UI::WindowsAndMessaging::{
                    SendMessageW, WM_KEYDOWN, WM_KEYUP,
                };
                unsafe {
                    SendMessageW(toggle.get_handle(), WM_KEYDOWN, 13, 0);
                    SendMessageW(toggle.get_handle(), WM_KEYUP, 13, 0);
                }
                assert!(!usage.chart.is_shown());
                assert!(usage.breakdown.get_parent().unwrap().is_shown());
                assert!(usage.breakdown.get_size().height >= usage.page.from_dip(100));
                let content = usage.page.get_client_size();
                let best = usage.page.get_best_size();
                assert!(
                    best.width <= content.width + usage.page.from_dip(2)
                        && best.height <= content.height + usage.page.from_dip(2),
                    "distribution must fit its viewport: {best:?} vs {content:?}"
                );
                if let Some(bitmap) = root.capture_bitmap() {
                    save(&output.join(format!("{label}-distribution.png")), bitmap);
                }
            }
            root.destroy();
            frame.destroy();
            assert!(!selector.is_valid());
            assert!(!input.is_valid());
        }
        // A native control gallery checks the common field surfaces. This is
        // intentionally a gallery, not a recreated business configuration page.
        for (label, mode) in [
            ("fields-light", theme::ThemeMode::Light),
            ("fields-dark", theme::ThemeMode::Dark),
        ] {
            theme::init(mode);
            let frame = Frame::builder().with_title("hidden native fields").build();
            frame.set_font(&theme::font(theme::TextRole::Body));
            frame.set_client_size(visual::dip_size(&frame, 760, 590));
            let page = Panel::builder(&frame).build();
            page.set_background_color(theme::theme().bg_app);
            let column = BoxSizer::builder(Orientation::Vertical).build();
            let title = StaticText::builder(&page)
                .with_label("共用原生表单 · 合成数据")
                .build();
            title.set_font(&theme::font(theme::TextRole::Title));
            title.set_foreground_color(theme::theme().ink_primary);
            column.add(&title, 0, SizerFlag::All, page.from_dip(18));
            let form = Panel::builder(&page).build();
            controls::paint_surface(form, 10);
            let fields = BoxSizer::builder(Orientation::Vertical).build();
            for (name, value, style) in [
                ("名称", "示例服务", TextCtrlStyle::Default),
                (
                    "服务地址",
                    "https://mcp.example.invalid/endpoint",
                    TextCtrlStyle::Default,
                ),
                ("密钥", "synthetic-key", TextCtrlStyle::Password),
            ] {
                let caption = StaticText::builder(&form).with_label(name).build();
                caption.set_foreground_color(theme::theme().ink_secondary);
                fields.add(
                    &caption,
                    0,
                    SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
                    form.from_dip(14),
                );
                let field = controls::TextCtrl::builder(&form)
                    .with_value(value)
                    .with_style(style)
                    .build();
                fields.add(
                    &field,
                    0,
                    SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
                    form.from_dip(8),
                );
            }
            let protocol = controls::Choice::builder(&form)
                .with_choices(vec!["HTTP".into(), "SSE".into(), "本地命令".into()])
                .with_selection(0u32)
                .build();
            fields.add(
                &protocol,
                0,
                SizerFlag::Left | SizerFlag::Top,
                form.from_dip(14),
            );
            let json = controls::TextCtrl::builder(&form)
                .with_value("{\n  \"url\": \"https://mcp.example.invalid/endpoint\"\n}")
                .with_style(TextCtrlStyle::MultiLine)
                .with_size(Size::new(-1, 90))
                .build();
            fields.add(
                &json,
                1,
                SizerFlag::Expand | SizerFlag::All,
                form.from_dip(14),
            );
            form.set_sizer(fields, true);
            column.add(
                &form,
                1,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
                page.from_dip(18),
            );
            let actions = BoxSizer::builder(Orientation::Horizontal).build();
            actions.add_stretch_spacer(1);
            for (name, kind) in [
                ("取消", controls::ButtonKind::Ghost),
                ("保存", controls::ButtonKind::Primary),
            ] {
                let button = controls::Button::builder(&page)
                    .with_label(name)
                    .with_kind(kind)
                    .build();
                actions.add(&button, 0, SizerFlag::Left, page.from_dip(8));
            }
            column.add_sizer(
                &actions,
                0,
                SizerFlag::Expand | SizerFlag::All,
                page.from_dip(18),
            );
            page.set_sizer(column, true);
            let root = BoxSizer::builder(Orientation::Vertical).build();
            root.add(&page, 1, SizerFlag::Expand, 0);
            frame.set_sizer(root, true);
            frame.layout();
            assert!(!frame.is_shown());
            if let Some(bitmap) = page.capture_bitmap() {
                save(&output.join(format!("{label}.png")), bitmap);
            }
            page.destroy();
            frame.destroy();
        }
        // All client pages use this same production header and action order.
        theme::init(theme::ThemeMode::Light);
        for id in client_types::ClientId::ALL {
            let frame = Frame::builder().with_title("hidden client layout").build();
            frame.set_font(&theme::font(theme::TextRole::Body));
            frame.set_client_size(Size::new(1080, 780));
            let root = Panel::builder(&frame).build();
            let shell = shell::Shell::new(&root, &frame);
            let page = Panel::builder(&shell.book).build();
            page.set_background_color(theme::theme().bg_app);
            let col = BoxSizer::builder(Orientation::Vertical).build();
            let header = client_header::ClientHeader::new(&page, id);
            col.add(
                &header.panel,
                0,
                SizerFlag::Expand | SizerFlag::Bottom,
                page.from_dip(12),
            );
            let chart = Panel::builder(&page).build();
            chart.set_min_size(Size::new(620, 430));
            chart.set_background_style(BackgroundStyle::Paint);
            chart.on_paint(move |_| {
                let base = AutoBufferedPaintDC::new(&chart);
                let dc = GraphicsDC::new(&base);
                let factor = visual::scale(&chart);
                let c = visual::Canvas::new(&dc, factor);
                let s = chart.get_client_size();
                visual::draw_client(
                    &c,
                    Size::new(
                        (s.width as f64 / factor) as i32,
                        (s.height as f64 / factor) as i32,
                    ),
                    &visual::ClientDiagram {
                        id,
                        opened: "已打开",
                        execution: "支持模型接入",
                        model: "1 个请求处理中",
                        channel: "示例渠道",
                        skill: "2 项使用中",
                        mcp: "3 项使用中",
                        chat: "未使用",
                        states: [
                            Some(true),
                            Some(true),
                            Some(true),
                            Some(true),
                            Some(true),
                            None,
                        ],
                        hub: Some(true),
                        flowing: true,
                    },
                    0.3,
                );
            });
            col.add(&chart, 0, SizerFlag::Expand, 0);
            page.set_sizer(col, true);
            shell.register(shell::PageId::Client(id), &page);
            shell.navigate(shell::PageId::Client(id));
            let rs = BoxSizer::builder(Orientation::Vertical).build();
            rs.add(&shell.panel, 1, SizerFlag::Expand, 0);
            root.set_sizer(rs, true);
            let fs = BoxSizer::builder(Orientation::Vertical).build();
            fs.add(&root, 1, SizerFlag::Expand, 0);
            frame.set_sizer(fs, true);
            frame.layout();
            page.layout();
            header.panel.layout();
            let start = header.launch.get_position();
            assert!(
                !header.reload.is_shown(),
                "reload must not be a visible user task"
            );
            assert!(header.tabs[4].is_shown());
            assert_eq!(header.tabs[4].get_label(), "接入设置");
            for tab in &header.tabs {
                assert!(
                    tab.get_size().width <= tab.from_dip(104),
                    "tab stretched to fill the header"
                );
            }
            assert!(!frame.is_shown());
            println!(
                "client header {}: launch {start:?}, automatic refresh",
                id.key()
            );
            if matches!(
                id,
                client_types::ClientId::Codex | client_types::ClientId::WorkBuddy
            ) {
                if let Some(bitmap) = root.capture_bitmap() {
                    save(&output.join(format!("client-{}.png", id.key())), bitmap);
                }
            }
            root.destroy();
            frame.destroy();
        }
        review_provider_fields(&output);
        review_resource_cards(&output);
        review_resource_navigation(&output);
        review_request_table(&output);
        review_client_marks(&output);
        review_model_workspace(&output);
        review_channel_models(&output);
        review_runtime_surface(&output);
        review_browser_login(&output);
        review_account_workspace(&output);
        review_account_secondaries(&output);
        review_account_forms(&output);
        review_client_model_switches(&output);
    })?;
    Ok(())
}

fn review_provider_fields(output: &Path) {
    for (label, mode, points) in [
        ("provider-light-10pt", theme::ThemeMode::Light, 10),
        ("provider-light-13pt", theme::ThemeMode::Light, 13),
        ("provider-dark-15pt", theme::ThemeMode::Dark, 15),
        ("provider-light-20pt", theme::ThemeMode::Light, 20),
    ] {
        theme::init(mode);
        let frame = Frame::builder()
            .with_title("hidden production provider fields")
            .build();
        let mut font = theme::font(theme::TextRole::Body);
        font.set_point_size(points);
        frame.set_font(&font);
        frame.set_client_size(visual::dip_size(&frame, 760, 690));
        let page = Panel::builder(&frame).build();
        page.set_background_color(theme::theme().bg_app);
        let form = provider_form::ProviderFields::new(
            &page,
            [
                "协议",
                "名称",
                "API 地址",
                "模型列表地址",
                "API 密钥",
                "权重",
            ],
            "OpenAI Responses",
            "Chat 不发送思考参数",
            "不填写时使用默认的模型列表地址",
        );
        for (input, value) in form.inputs.iter().zip([
            "OpenAI Responses",
            "示例渠道",
            "https://api.example.invalid/v1",
            "https://api.example.invalid/v1/models",
            "synthetic-key",
            "100",
        ]) {
            input.set_font(&font);
            input.set_value(value);
            // Reproduce the obsolete height requested by the old dialog.
            input.set_min_size(Size::new(300, 30));
        }
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        sizer.add_sizer(
            &form.grid,
            0,
            SizerFlag::Expand | SizerFlag::All,
            page.from_dip(20),
        );
        page.set_sizer(sizer, true);
        let root = BoxSizer::builder(Orientation::Vertical).build();
        root.add(&page, 1, SizerFlag::Expand, 0);
        frame.set_sizer(root, true);
        frame.layout();
        page.layout();
        assert!(!frame.is_shown());
        for input in &form.inputs {
            let editor = input.editor_size();
            let best = input.editor_best_size();
            assert!(
                editor.height >= best.height,
                "{label}: native text clipped: {editor:?} vs {best:?}"
            );
            assert!(
                input.get_size().height >= best.height + page.from_dip(12),
                "{label}: wrapper padding clipped"
            );
            println!(
                "provider {label}: wrapper {:?}, editor {editor:?}, best {best:?}",
                input.get_size()
            );
        }
        save(
            &output.join(format!("{label}.png")),
            page.capture_bitmap().unwrap(),
        );
        page.destroy();
        frame.destroy();
    }
}

fn review_resource_cards(output: &Path) {
    use client_types::ClientId;
    for (label, mode, w, scoped, skill) in [
        (
            "skills-global-light",
            theme::ThemeMode::Light,
            1080,
            false,
            true,
        ),
        (
            "skills-client-light",
            theme::ThemeMode::Light,
            1080,
            true,
            true,
        ),
        (
            "mcp-client-narrow",
            theme::ThemeMode::Light,
            620,
            true,
            false,
        ),
        ("mcp-client-dark", theme::ThemeMode::Dark, 1080, true, false),
    ] {
        theme::init(mode);
        let frame = Frame::builder()
            .with_title("hidden production resource gallery")
            .build();
        frame.set_font(&theme::font(theme::TextRole::Body));
        frame.set_client_size(visual::dip_size(&frame, w, 740));
        let gallery = resource_gallery::Gallery::new(&frame);
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        sizer.add(
            &gallery.page,
            1,
            SizerFlag::Expand | SizerFlag::All,
            frame.from_dip(16),
        );
        frame.set_sizer(sizer, true);
        frame.layout();
        for round in 0..2 {
            gallery.clear();
            for (i, (name, description)) in [
                (
                    "代码审查与项目变更质量检查",
                    "检查变更并给出可执行的改进建议。",
                ),
                ("项目文档", "阅读和维护项目文档，整理相关知识。"),
                ("浏览器工具", "查看网页并帮助完成日常操作。"),
                ("团队资料", "查找团队内容并返回相关引用。"),
            ]
            .into_iter()
            .enumerate()
            {
                let targets = if scoped {
                    vec![(ClientId::WorkBuddy, "默认".into(), Some(true))]
                } else {
                    ClientId::ALL
                        .into_iter()
                        .map(|id| {
                            (
                                id,
                                visual::client_short(id).into(),
                                Some(id == ClientId::Codex || id == ClientId::WorkBuddy),
                            )
                        })
                        .collect()
                };
                let card = resource_gallery::ResourceCard::new(
                    &gallery.page,
                    if skill {
                        name
                    } else {
                        ["工作区工具", "知识库服务", "网页检索", "数据服务"][i]
                    },
                    description,
                    skill,
                    &targets,
                    scoped,
                );
                if scoped {
                    assert!(card.manage.is_shown());
                    assert!(!card.delete.is_shown());
                }
                gallery.push(card);
            }
            gallery.finish("");
            frame.layout();
            gallery.page.layout();
            gallery.page.fit_inside();
            let viewport = gallery.page.get_client_size();
            let size = gallery.page.get_virtual_size();
            assert!(
                size.width <= viewport.width + frame.from_dip(2),
                "{label}: horizontal overflow {size:?} vs {viewport:?}"
            );
            assert_eq!(gallery.cards.borrow().len(), 4, "rebuild duplicated cards");
            println!("resources {label}, rebuild {round}: viewport {viewport:?}, virtual {size:?}");
        }
        assert!(!frame.is_shown());
        save(
            &output.join(format!("{label}.png")),
            gallery.page.capture_bitmap().unwrap(),
        );
        frame.destroy();
    }
}

fn review_request_table(output: &Path) {
    for (label, mode, w, h) in [
        ("requests-wide", theme::ThemeMode::Light, 1500, 900),
        ("requests-compact", theme::ThemeMode::Light, 920, 640),
        ("requests-dark", theme::ThemeMode::Dark, 1200, 820),
    ] {
        theme::init(mode);
        let frame = Frame::builder()
            .with_title("hidden production request page")
            .build();
        frame.set_font(&theme::font(theme::TextRole::Body));
        frame.set_client_size(visual::dip_size(&frame, w, h));
        let view = request_view::RequestView::new(&frame, ["", "", "", ""], 0, 0);
        for i in 0..24 {
            view.table.insert_item(i, "2026-11-08 16:24:03", None);
            for (j, value) in [
                "ChatGPT / Codex",
                "gpt-example",
                "示例渠道",
                "成功",
                "1280",
                "246",
                "1.23s",
            ]
            .into_iter()
            .enumerate()
            {
                view.table.set_item_text_by_column(i, j as i32 + 1, value);
            }
        }
        view.status.set_label("24 条 · 第 1 页");
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        sizer.add(&view.page, 1, SizerFlag::Expand, 0);
        frame.set_sizer(sizer, true);
        frame.layout();
        view.page.layout();
        request_view::fit_columns(view.table, false);
        let viewport = view.page.get_client_size();
        let table = view.table.get_size();
        assert!(
            table.width >= viewport.width - view.page.from_dip(28),
            "{label}: table not filling page: {table:?} vs {viewport:?}"
        );
        assert!(
            table.height >= viewport.height - view.page.from_dip(180),
            "{label}: table vertically constrained: {table:?} vs {viewport:?}"
        );
        let columns = (0..8).map(|i| view.table.get_column_width(i)).sum::<i32>();
        assert!(
            (columns - (view.table.get_client_size().width - view.page.from_dip(4))).abs() <= 10,
            "{label}: columns not filling table: {columns}"
        );
        assert!(!frame.is_shown());
        println!(
            "requests {label}: viewport {viewport:?}, table {table:?}, column total {columns}"
        );
        save(
            &output.join(format!("{label}.png")),
            view.page.capture_bitmap().unwrap(),
        );
        frame.destroy();
    }
}

fn review_client_marks(output: &Path) {
    for (label, mode) in [
        ("client-marks-light", theme::ThemeMode::Light),
        ("client-marks-dark", theme::ThemeMode::Dark),
    ] {
        theme::init(mode);
        let frame = Frame::builder().with_title("hidden brand outlines").build();
        frame.set_client_size(visual::dip_size(&frame, 900, 180));
        let page = Panel::builder(&frame).build();
        page.set_background_style(BackgroundStyle::Paint);
        page.on_paint(move |_| {
            let base = AutoBufferedPaintDC::new(&page);
            let dc = GraphicsDC::new(&base);
            let c = visual::Canvas::new(&dc, visual::scale(&page));
            let t = theme::theme();
            c.clear(t.bg_app);
            for (i, id) in client_types::ClientId::ALL.into_iter().enumerate() {
                let x = 18 + i as i32 * 124;
                c.icon(
                    visual::Icon::client(id),
                    Rect::new(x + 34, 22, 48, 48),
                    t.ink_primary,
                );
                c.label(
                    visual::client_short(id),
                    Rect::new(x, 88, 116, 30),
                    12,
                    false,
                    t.ink_primary,
                    true,
                );
            }
        });
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        sizer.add(&page, 1, SizerFlag::Expand, 0);
        frame.set_sizer(sizer, true);
        frame.layout();
        save(
            &output.join(format!("{label}.png")),
            page.capture_bitmap().unwrap(),
        );
        frame.destroy();
    }
}

fn review_channel_models(output: &Path) {
    for (name, mode, width, height) in [
        ("codex-channels-small", theme::ThemeMode::Light, 900, 560),
        ("codex-channels-wide", theme::ThemeMode::Light, 1366, 900),
        ("codex-channels-dark", theme::ThemeMode::Dark, 1180, 760),
    ] {
        theme::init(mode);
        let frame = Frame::builder()
            .with_title("hidden channel model selector")
            .build();
        frame.set_font(&theme::font(theme::TextRole::Body));
        frame.set_size(visual::dip_size(&frame, width, height));
        let host = Panel::builder(&frame).build();
        let shell = shell::Shell::new(&host, &frame);
        let page = Panel::builder(&shell.book).build();
        page.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let header = client_header::ClientHeader::new(&page, client_types::ClientId::Codex);
        header.tabs[0].set_selected(false);
        header.tabs[1].set_selected(true);
        header.launch.set_kind(controls::ButtonKind::Secondary);
        col.add(
            &header.panel,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(12),
        );
        let picker = codex_models::ChannelModels::new(&page);
        picker.channel.append("合成渠道 A");
        picker.channel.append("合成渠道 B");
        picker.channel.set_selection(1);
        let groups = vec![
            vec!["builtin-model".into(), "custom-old".into()],
            (0..120).map(|i| format!("model-{i:03}")).collect(),
            vec!["channel-b-model".into()],
        ];
        picker.update(
            groups.clone(),
            Some(&["custom-old".into(), "model-000".into()]),
        );
        picker.enable(true);
        col.add(
            &picker.page,
            1,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            page.from_dip(12),
        );
        let save_button = controls::Button::builder(&page)
            .with_label("保存可见模型")
            .build();
        col.add(
            &save_button,
            0,
            SizerFlag::AlignRight | SizerFlag::All,
            page.from_dip(12),
        );
        page.set_sizer(col, true);
        shell.register(shell::PageId::Client(client_types::ClientId::Codex), &page);
        shell.navigate(shell::PageId::Client(client_types::ClientId::Codex));
        let s = BoxSizer::builder(Orientation::Vertical).build();
        s.add(&shell.panel, 1, SizerFlag::Expand, 0);
        host.set_sizer(s, true);
        let s = BoxSizer::builder(Orientation::Vertical).build();
        s.add(&host, 1, SizerFlag::Expand, 0);
        frame.set_sizer(s, true);
        frame.layout();
        let selected = picker
            .page
            .find_window_by_name("codex-visible-model-000")
            .unwrap();
        // A background refresh of identical data keeps focused native controls.
        picker.update(groups.clone(), None);
        assert_eq!(
            selected.handle_ptr(),
            picker
                .page
                .find_window_by_name("codex-visible-model-000")
                .unwrap()
                .handle_ptr()
        );
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{BM_CLICK, SendMessageW};
            let check = picker
                .page
                .find_window_by_name("codex-visible-model-001")
                .unwrap();
            SendMessageW(check.get_handle(), BM_CLICK, 0, 0);
            assert!(picker.selected_models().contains(&"model-001".to_string()));
        }
        let chosen = picker.selected_models();
        picker.channel.set_selection(2);
        picker.update(groups.clone(), None);
        assert!(
            picker
                .page
                .find_window_by_name("codex-visible-channel-b-model")
                .is_some()
        );
        assert!(
            picker
                .page
                .find_window_by_name("codex-visible-model-000")
                .is_none()
        );
        assert_eq!(picker.selected_models(), chosen);
        // A renamed provider can reset the Choice to group zero while the model
        // data stays identical. The displayed group must still change.
        picker.channel.set_selection(0);
        picker.update(groups.clone(), None);
        assert!(
            picker
                .page
                .find_window_by_name("codex-visible-custom-old")
                .is_some()
        );
        picker.channel.set_selection(1);
        picker.update(groups, None);
        assert_eq!(picker.selected_models(), chosen);
        frame.layout();
        let list = picker
            .page
            .find_window_by_name("codex-channel-models-list")
            .unwrap();
        assert!(list.get_size().height > picker.page.from_dip(96));
        assert!(picker.page.get_size().width <= page.get_size().width);
        assert!(!frame.is_shown());
        save(
            &output.join(format!("{name}.png")),
            host.capture_bitmap().unwrap(),
        );
        frame.destroy();
    }
}

fn review_client_model_switches(output: &Path) {
    use client_types::ClientId;
    use serde_json::json;
    for id in ClientId::ALL {
        theme::init(if id == ClientId::Tiangong {
            theme::ThemeMode::Dark
        } else {
            theme::ThemeMode::Light
        });
        let frame = Frame::builder()
            .with_title("hidden production nested model-page switching")
            .build();
        frame.set_font(&theme::font(theme::TextRole::Body));
        frame.set_size(visual::dip_size(&frame, 900, 560));
        let base = Panel::builder(&frame).build();
        let shell = shell::Shell::new(&base, &frame);
        let page = Panel::builder(&shell.book).build();
        page.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let header = client_header::ClientHeader::new(&page, id);
        col.add(
            &header.panel,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(12),
        );
        let book = wxdragon::widgets::SimpleBook::builder(&page).build();
        let status = Panel::builder(&book).build();
        book.add_page(&status, "概览", true, None);
        let model = model_fields::ModelPage::new(&book, id);
        let legacy = matches!(
            id,
            ClientId::Codex | ClientId::WorkBuddy | ClientId::Tiangong
        );
        let codex_picker = if id == ClientId::Codex {
            let picker = codex_models::ChannelModels::new(&model.legacy_host);
            picker.channel.append("合成渠道");
            picker.update(
                vec![vec![], vec!["gpt-6".into(), "gpt-5.6".into()]],
                Some(&["gpt-6".into()]),
            );
            picker.enable(true);
            model.legacy_host.get_sizer().unwrap().add(
                &picker.page,
                1,
                SizerFlag::Expand | SizerFlag::All,
                model.page.from_dip(12),
            );
            Some(picker)
        } else {
            let saved = StaticText::builder(&model.legacy_host)
                .with_label("GPT-6 · 已有模型（合成）")
                .build();
            model
                .legacy_host
                .get_sizer()
                .unwrap()
                .add(&saved, 0, SizerFlag::All, 12);
            None
        };
        book.add_page(&model.page, "模型", false, None);
        let viewport = ScrolledWindow::builder(&book)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        viewport.set_scroll_rate(0, viewport.from_dip(10));
        viewport.set_background_color(theme::theme().bg_app);
        let settings_host = Panel::builder(&viewport).build();
        settings_host.set_background_color(theme::theme().bg_app);
        let settings = client_header::ClientSettings::new(&settings_host, id);
        let host_col = BoxSizer::builder(Orientation::Vertical).build();
        host_col.add(
            &settings.panel,
            0,
            SizerFlag::Expand | SizerFlag::All,
            settings_host.from_dip(18),
        );
        settings_host.set_sizer(host_col, false);
        let viewport_col = BoxSizer::builder(Orientation::Vertical).build();
        viewport_col.add(&settings_host, 1, SizerFlag::Expand, 0);
        viewport.set_sizer(viewport_col, false);
        book.add_page(&viewport, "接入设置", false, None);
        col.add(&book, 1, SizerFlag::Expand, 0);
        page.set_sizer(col, false);
        shell.register(shell::PageId::Client(id), &page);
        shell.navigate(shell::PageId::Client(id));
        let root = BoxSizer::builder(Orientation::Vertical).build();
        root.add(&shell.panel, 1, SizerFlag::Expand, 0);
        base.set_sizer(root, false);
        let outer = BoxSizer::builder(Orientation::Vertical).build();
        outer.add(&base, 1, SizerFlag::Expand, 0);
        frame.set_sizer(outer, false);
        frame.layout();
        book.change_selection(1);
        model.show_draft(false);
        let view = json!({"revision":"synthetic-revision","targets":[{"id":"synthetic-target","client":id.key(),"label":"默认","modelMode":"gateway","root":"C:/Synthetic/client","skills":"C:/Synthetic/client/skills","mcp":"C:/Synthetic/client/mcp.json"}],"sources":[{"routeId":"source-a","name":"合成渠道","enabled":true,"models":["gpt-6"]}]});
        if let Some(picker) = codex_picker {
            for _ in 0..4 {
                model.show_draft(true);
                picker.show_discovered(1, &["discovered-model".into()]);
                picker.select_models(&["discovered-model".into()]);
                model.workspace.list.replace(&[
                    "gpt-6".into(),
                    "discovered-model".into(),
                    "新建模型".into(),
                ]);
                assert!(!model.workspace.fields.page.is_shown());
                assert!(model.legacy_host.is_shown());
                let bounds = picker.page.get_client_size();
                assert!(bounds.width > model.page.from_dip(250));
                assert!(bounds.height > model.page.from_dip(160));
                picker.remove_model("discovered-model");
                assert_eq!(picker.selected_models(), vec!["gpt-6"]);
                book.change_selection(0);
                book.change_selection(1);
                model.show_draft(false);
                assert!(model.legacy_host.is_shown());
            }
            header.tabs[0].set_selected(false);
            header.tabs[1].set_selected(true);
            header.launch.set_kind(controls::ButtonKind::Secondary);
            save(
                &output.join("model-switch-codex.png"),
                shell.panel.capture_bitmap().unwrap(),
            );
        } else if !matches!(id, ClientId::WorkBuddy | ClientId::Tiangong) {
            let draft = json!({"id":"__draft__","draft":true,"label":"新建模型","capabilities":{}});
            model.workspace.fields.prepare_draft(&view, id);
            model.workspace.list.replace(&["新建模型".into()]);
            model.workspace.list.set_selection(0, true);
            model.workspace.fields.fill(Some(&draft), false);
            let m = model.clone();
            model.add.on_click(move |_| m.show_draft(true));
            let m = model.clone();
            model.back.on_click(move |_| m.show_draft(false));
            // Simulate the formerly broken sequence at a fixed outer size. No
            // window resize is allowed to incidentally repair the nested layout.
            for _ in 0..4 {
                #[cfg(windows)]
                draft_click(&model.add);
                #[cfg(not(windows))]
                model.show_draft(true);
                let bounds = model.workspace.page.get_client_size();
                let field_bounds = model.workspace.fields.page.get_client_size();
                assert!(
                    bounds.width > model.page.from_dip(400)
                        && bounds.height > model.page.from_dip(160),
                    "new model editor is blank: {id:?} {bounds:?}"
                );
                assert!(
                    field_bounds.width > model.page.from_dip(250)
                        && field_bounds.height > model.page.from_dip(160)
                );
                let save = model.workspace.fields.save;
                let save_bottom = save.get_parent().unwrap().get_position().y
                    + save.get_position().y
                    + save.get_size().height;
                assert!(
                    save_bottom <= field_bounds.height,
                    "default draft save clipped: {id:?} {save_bottom} > {}",
                    field_bounds.height
                );
                let virtual_size = model.workspace.fields.page.get_virtual_size();
                assert!(
                    virtual_size.height <= field_bounds.height + model.page.from_dip(2),
                    "default draft must fit: {id:?} {virtual_size:?} {field_bounds:?}"
                );
                assert!(!model.back.is_shown());
                assert_eq!(model.legacy_host.is_shown(), false);
                let body = model.workspace.fields.body(&draft, &view, id).unwrap();
                assert_eq!(body["revision"], "synthetic-revision");
                assert_eq!(body["sourceRoute"], "source-a");
                assert_eq!(body["model"], "gpt-6");
                book.change_selection(0);
                book.change_selection(1);
                model.show_draft(true);
                assert!(model.workspace.fields.page.get_size().height > 100);
                model.show_draft(false);
                assert_eq!(model.legacy_host.is_shown(), legacy);
                if legacy {
                    assert!(model.legacy_host.get_size().height > 100);
                }
            }
            model.show_draft(true);
            header.tabs[0].set_selected(false);
            header.tabs[1].set_selected(true);
            header.launch.set_kind(controls::ButtonKind::Secondary);
            assert!(!frame.is_shown());
            save(
                &output.join(format!("model-switch-{}.png", id.key())),
                shell.panel.capture_bitmap().unwrap(),
            );
        }
        book.change_selection(2);
        header.tabs[1].set_selected(false);
        header.tabs[4].set_selected(true);
        header.launch.set_kind(controls::ButtonKind::Secondary);
        settings.set_targets(&view, id);
        assert_eq!(settings.program.is_shown(), !legacy);
        assert_eq!(
            settings.directory().unwrap(),
            PathBuf::from("C:/Synthetic/client")
        );
        #[cfg(windows)]
        draft_click(
            &settings
                .panel
                .find_window_by_name("client-settings-files")
                .unwrap(),
        );
        frame.layout();
        settings.panel.layout();
        let folder_bottom = settings.folder.get_position().y + settings.folder.get_size().height;
        assert!(folder_bottom <= settings.folder.get_parent().unwrap().get_size().height);
        let settings_extent = settings.panel.get_position().y + settings.panel.get_size().height;
        assert!(
            settings_extent <= settings_host.get_size().height,
            "settings clipped: {id:?}"
        );
        save(
            &output.join(format!("client-settings-{}.png", id.key())),
            shell.panel.capture_bitmap().unwrap(),
        );
        frame.destroy();
    }
}

fn review_model_workspace(output: &Path) {
    use client_types::ClientId;
    use serde_json::json;
    for (name, client, mode, w, h) in [
        (
            "model-vscode",
            ClientId::VsCode,
            theme::ThemeMode::Light,
            1180,
            760,
        ),
        (
            "model-harness",
            ClientId::DeepSeekHarness,
            theme::ThemeMode::Light,
            1180,
            760,
        ),
        (
            "model-zcode",
            ClientId::ZCode,
            theme::ThemeMode::Light,
            1180,
            760,
        ),
        (
            "model-webstorm",
            ClientId::WebStorm,
            theme::ThemeMode::Light,
            1180,
            760,
        ),
        (
            "model-small",
            ClientId::VsCode,
            theme::ThemeMode::Light,
            960,
            640,
        ),
        (
            "model-dark",
            ClientId::DeepSeekHarness,
            theme::ThemeMode::Dark,
            1180,
            760,
        ),
    ] {
        theme::init(mode);
        let frame = Frame::builder()
            .with_title("hidden production model workspace")
            .build();
        frame.set_font(&theme::font(theme::TextRole::Body));
        frame.set_client_size(visual::dip_size(&frame, w, h));
        let root = Panel::builder(&frame).build();
        let shell = shell::Shell::new(&root, &frame);
        let page = Panel::builder(&shell.book).build();
        page.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let header = client_header::ClientHeader::new(&page, client);
        header.tabs[0].set_selected(false);
        header.tabs[1].set_selected(true);
        header.launch.set_kind(controls::ButtonKind::Secondary);
        col.add(
            &header.panel,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(12),
        );
        let add = controls::Button::builder(&page)
            .with_label("添加模型")
            .with_icon(visual::Icon::Plus)
            .build();
        col.add(&add, 0, SizerFlag::All, page.from_dip(12));
        let workspace = model_fields::ModelWorkspace::new(&page, client);
        let labels = vec![
            "GPT-6".into(),
            "GPT-5.6 · 备用".into(),
            "Claude · 工作".into(),
        ];
        workspace.list.replace(&labels);
        workspace.list.set_selection(0, true);
        // Same content refresh must keep the native selection and row handle.
        let first = workspace
            .list
            .find_window_by_name("client-model-0")
            .unwrap();
        workspace.list.replace(&labels);
        assert_eq!(
            workspace
                .list
                .find_window_by_name("client-model-0")
                .unwrap()
                .handle_ptr(),
            first.handle_ptr()
        );
        assert_eq!(workspace.list.get_selection(), Some(0));
        let mut renamed = labels.clone();
        renamed[0] = "GPT-6 · 新名称".into();
        workspace.list.replace(&renamed);
        assert_eq!(
            workspace
                .list
                .find_window_by_name("client-model-0")
                .unwrap()
                .handle_ptr(),
            first.handle_ptr()
        );
        workspace.list.replace(&labels);
        let row = json!({"id":"synthetic-model", "targetId":"synthetic-target", "model":"gpt-6", "label":"GPT-6", "sourceName":"示例渠道", "wireProtocol":"openai-responses", "capabilities":{"contextWindow":200000,"maxTokens":8192,"supportsImage":true,"input":["text","image"],"reasoningEfforts":["low","medium","high","max"],"futureField":"preserved"}});
        let view = json!({"revision":"draft-baseline","targets":[{"id":"synthetic-target","modelMode":if client==ClientId::WebStorm {"acp"}else{"gateway"}}]});
        workspace.fields.fill(Some(&row), false);
        let body = workspace.fields.body(&row, &view, client).unwrap();
        assert_eq!(body["revision"], "draft-baseline");
        assert_eq!(body["capabilities"], row["capabilities"]);
        assert_eq!(body["syncSource"], false);
        if client == ClientId::DeepSeekHarness {
            assert_eq!(body["wireProtocol"], "openai-responses");
        }
        col.add(
            &workspace.page,
            1,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            page.from_dip(12),
        );
        page.set_sizer(col, true);
        shell.register(shell::PageId::Client(client), &page);
        shell.navigate(shell::PageId::Client(client));
        let rs = BoxSizer::builder(Orientation::Vertical).build();
        rs.add(&shell.panel, 1, SizerFlag::Expand, 0);
        root.set_sizer(rs, true);
        let fs = BoxSizer::builder(Orientation::Vertical).build();
        fs.add(&root, 1, SizerFlag::Expand, 0);
        frame.set_sizer(fs, true);
        frame.layout();
        workspace.fields.page.fit_inside();
        frame.layout();
        let view_size = workspace.fields.page.get_client_size();
        let virtual_size = workspace.fields.page.get_virtual_size();
        assert!(
            virtual_size.width <= view_size.width + workspace.fields.page.from_dip(2),
            "model form must fit horizontally: {virtual_size:?} vs {view_size:?}"
        );
        assert!(
            virtual_size.height <= view_size.height + workspace.fields.page.from_dip(2),
            "collapsed model form should fit: {virtual_size:?} vs {view_size:?}"
        );
        assert!(workspace.list.get_size().width <= workspace.list.from_dip(206));
        assert!(!header.reload.is_shown());
        assert!(!frame.is_shown());
        save(
            &output.join(format!("{name}.png")),
            root.capture_bitmap().unwrap(),
        );
        let draft = json!({"draft":true,"label":"新建模型","capabilities":{}});
        let mut draft_labels = labels.clone();
        draft_labels.push("新建模型".into());
        workspace.list.replace(&draft_labels);
        workspace
            .list
            .set_selection((draft_labels.len() - 1) as u32, true);
        let draft_view = json!({"revision":"draft-baseline","targets":[{"id":"synthetic-target","client":client.key(),"modelMode":if client==ClientId::WebStorm {"acp"}else{"gateway"},"label":"默认"}],"sources":[{"routeId":"source-a","name":"合成渠道","enabled":true,"models":["gpt-6"],"aliases":{"friendly":"gpt-6"}}]});
        workspace.fields.prepare_draft(&draft_view, client);
        workspace.fields.fill(Some(&draft), false);
        let draft_body = workspace.fields.body(&draft, &draft_view, client).unwrap();
        assert!(draft_body.get("id").is_none());
        assert_eq!(draft_body["revision"], "draft-baseline");
        assert_eq!(draft_body["sourceRoute"], "source-a");
        assert_eq!(draft_body["model"], "friendly");
        assert_eq!(
            draft_body["label"], "friendly",
            "unsaved list marker must not become the saved name"
        );
        frame.layout();
        workspace.fields.page.layout();
        workspace.fields.page.fit_inside();
        let viewport = workspace.fields.page.get_client_size();
        let full = workspace.fields.page.get_virtual_size();
        assert!(
            full.height <= viewport.height + workspace.fields.page.from_dip(2),
            "draft should fit: {full:?} vs {viewport:?}"
        );
        save(
            &output.join(format!("{name}-draft.png")),
            root.capture_bitmap().unwrap(),
        );
        frame.destroy();
    }
}

#[cfg(windows)]
fn draft_click(window: &dyn WxWidget) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_KEYDOWN, WM_KEYUP};
    unsafe {
        SendMessageW(window.get_handle(), WM_KEYDOWN, 13, 0);
        SendMessageW(window.get_handle(), WM_KEYUP, 13, 0);
    }
}
fn review_resource_navigation(output: &Path) {
    use client_types::ClientId;
    for (name, mode, width, height, advanced) in [
        (
            "resource-picker-small",
            theme::ThemeMode::Light,
            900,
            560,
            false,
        ),
        (
            "resource-picker-wide",
            theme::ThemeMode::Light,
            1366,
            900,
            false,
        ),
        (
            "resource-picker-config",
            theme::ThemeMode::Light,
            960,
            640,
            true,
        ),
        (
            "resource-picker-dark",
            theme::ThemeMode::Dark,
            960,
            640,
            false,
        ),
    ] {
        theme::init(mode);
        let frame = Frame::builder()
            .with_title("hidden assignment draft")
            .build();
        frame.set_font(&theme::font(theme::TextRole::Body));
        frame.set_size(visual::dip_size(&frame, width, height));
        let host = Panel::builder(&frame).build();
        let shell = shell::Shell::new(&host, &frame);
        let picker = resource_detail::TargetPicker::new(&shell.book);
        // This is a direct component review. General Skill/MCP navigation is
        // hidden in the product, so use an ordinary fixture page here.
        shell.register(shell::PageId::Overview, &picker.page);
        let root = BoxSizer::builder(Orientation::Vertical).build();
        root.add(&shell.panel, 1, SizerFlag::Expand, 0);
        host.set_sizer(root, true);
        let outer = BoxSizer::builder(Orientation::Vertical).build();
        outer.add(&host, 1, SizerFlag::Expand, 0);
        frame.set_sizer(outer, true);
        let mut options = Vec::new();
        for (i, client) in [
            ClientId::Codex,
            ClientId::WorkBuddy,
            ClientId::Tiangong,
            ClientId::DeepSeekHarness,
            ClientId::ZCode,
            ClientId::WebStorm,
            ClientId::VsCode,
        ]
        .into_iter()
        .enumerate()
        {
            options.push(resource_detail::TargetOption {
                id: format!("fixture-{i}"),
                client,
                label: "默认".into(),
                using: if i == 3 { None } else { Some(i == 0) },
                compatible: i != 4,
                error: if i == 3 {
                    "配置文件需核对，原文件保持".into()
                } else {
                    String::new()
                },
                shared_key: if i == 1 || i == 6 {
                    "fixture-shared".into()
                } else {
                    String::new()
                },
                shared_with: if i == 1 || i == 6 {
                    vec!["WorkBuddy".into(), "VS Code".into()]
                } else {
                    vec![]
                },
                location: PathBuf::from(format!("C:/Synthetic/client-{i}/config.json")),
            });
        }
        // Multiple actual profile locations remain distinct and all selectable.
        for i in 7..10 {
            let mut item = options[5].clone();
            item.id = format!("fixture-{i}");
            item.label = format!("Profile {i} · Air / ACP");
            options.push(item);
        }
        picker.populate("示例 Skill", options.clone());
        shell.navigate(shell::PageId::Overview);
        frame.layout();
        picker.page.layout();
        assert!(picker.changes().is_empty());
        assert!(!picker.apply.is_enabled());
        #[cfg(windows)]
        {
            let target = picker.page.find_window_by_name("target-fixture-1").unwrap();
            draft_click(&target);
            assert_eq!(
                picker.changes().len(),
                2,
                "shared Skill selection must disclose both changes"
            );
            assert!(picker.apply.is_enabled());
            picker.set_enabled(false);
            assert!(!target.is_enabled());
            assert!(!picker.apply.is_enabled());
            assert!(!picker.back.is_enabled());
            picker.set_enabled(true);
            assert!(target.is_enabled());
            assert!(
                !picker
                    .page
                    .find_window_by_name("target-fixture-3")
                    .unwrap()
                    .is_enabled()
            );
            assert!(
                !picker
                    .page
                    .find_window_by_name("target-fixture-4")
                    .unwrap()
                    .is_enabled()
            );
            // Invalidation after an uncertain result must prohibit replay.
            picker.invalidate("请重新读取后再选择");
            assert!(!picker.apply.is_enabled());
            assert!(picker.reload.is_enabled());
            assert!(!target.is_enabled());
            picker.populate("示例 Skill", options.clone());
            assert!(picker.changes().is_empty());
            if advanced {
                draft_click(
                    &picker
                        .page
                        .find_window_by_name("resource-config-files")
                        .unwrap(),
                );
                assert!(
                    picker
                        .page
                        .find_window_by_name("resource-config-details")
                        .unwrap()
                        .is_shown()
                );
            }
        }
        picker.page.layout();
        frame.layout();
        let content = picker.page.get_client_size();
        let best = picker.page.get_best_size();
        assert!(
            best.width <= content.width + 2 && best.height <= content.height + 2,
            "picker {name}: {best:?} exceeds {content:?}"
        );
        let apply = picker.apply.get_position();
        assert!(
            apply.y >= 0 && apply.y + picker.apply.get_size().height <= content.height,
            "apply action clipped"
        );
        assert!(!frame.is_shown());
        if let Some(bitmap) = shell.panel.capture_bitmap() {
            save(&output.join(format!("{name}.png")), bitmap);
        }
        shell.panel.destroy();
        frame.destroy();
    }
    for (name, mode) in [
        ("resource-issues-small", theme::ThemeMode::Light),
        ("resource-issues-dark", theme::ThemeMode::Dark),
    ] {
        theme::init(mode);
        let frame = Frame::builder()
            .with_title("hidden actionable issues")
            .build();
        frame.set_font(&theme::font(theme::TextRole::Body));
        frame.set_client_size(visual::dip_size(&frame, 760, 520));
        let issues = resource_detail::IssueList::new(&frame);
        let rows=vec![resource_detail::IssueItem {title:"ChatGPT / Codex · 示例 Skill".into(),reason:"Skill 文件过多或内容超过 32 MiB".into(),guidance:"原客户端仍可继续使用。请打开文件夹核对是否选中了整个仓库，将需要共享的 Skill 整理为独立小包后重新读取。Hub 不会自动删减文件。".into(),location:PathBuf::from("C:/Synthetic/.codex/skills/example")},resource_detail::IssueItem {title:"ChatGPT / Codex · config.toml".into(),reason:"配置文件超过 8 MiB，已跳过".into(),guidance:"打开文件夹核对配置文件是否正确；不要为了清除提示直接删除文件。修正后点击重新读取。".into(),location:PathBuf::from("C:/Synthetic/.codex/config.toml")}];
        issues.populate(&rows);
        issues.populate(&rows); // rebuild must destroy old controls safely
        let root = BoxSizer::builder(Orientation::Vertical).build();
        root.add(&issues.page, 1, SizerFlag::Expand, 0);
        frame.set_sizer(root, true);
        frame.layout();
        issues.page.layout();
        assert!(issues.back.is_enabled());
        assert!(issues.retry.is_enabled());
        assert!(!frame.is_shown());
        if let Some(bitmap) = issues.page.capture_bitmap() {
            save(&output.join(format!("{name}.png")), bitmap);
        }
        issues.page.destroy();
        frame.destroy();
    }
    theme::init(theme::ThemeMode::Light);
    let frame = Frame::builder()
        .with_title("hidden secondary navigation")
        .build();
    frame.set_font(&theme::font(theme::TextRole::Body));
    frame.set_client_size(visual::dip_size(&frame, 960, 640));
    let host = Panel::builder(&frame).build();
    let shell = shell::Shell::new(&host, &frame);
    for id in [
        shell::PageId::Overview,
        shell::PageId::Dumpling,
        shell::PageId::Client(ClientId::Codex),
    ] {
        let panel = Panel::builder(&shell.book).build();
        shell.register(id, &panel);
    }
    let root = BoxSizer::builder(Orientation::Vertical).build();
    root.add(&shell.panel, 1, SizerFlag::Expand, 0);
    host.set_sizer(root, true);
    let outer = BoxSizer::builder(Orientation::Vertical).build();
    outer.add(&host, 1, SizerFlag::Expand, 0);
    frame.set_sizer(outer, true);
    for origin in [
        shell::PageId::Overview,
        shell::PageId::Client(ClientId::Codex),
    ] {
        shell.navigate(origin);
        shell.navigate(shell::PageId::Dumpling);
        frame.layout();
        let back = shell.panel.find_window_by_name("back-to-parent").unwrap();
        assert!(
            !back.is_shown(),
            "Dumpling now has a standalone navigation entry"
        );
        assert_eq!(shell.current(), shell::PageId::Dumpling);
    }
    shell.navigate(shell::PageId::Overview);
    shell.navigate(shell::PageId::Dumpling);
    frame.layout();
    if let Some(bitmap) = shell.panel.capture_bitmap() {
        save(&output.join("dumpling-standalone-navigation.png"), bitmap);
    }
    shell.panel.destroy();
    frame.destroy();
    for (name, error) in [
        ("squad-inline-login-placeholder", false),
        ("squad-inline-login-retry", true),
    ] {
        let frame = Frame::builder()
            .with_title("hidden inline login placeholder; no website")
            .build();
        frame.set_font(&theme::font(theme::TextRole::Body));
        frame.set_client_size(visual::dip_size(&frame, 740, 480));
        let login = squad_login::InlineLogin::new(&frame, "https://login.example.invalid");
        // Never activate WebView: review the real container and failure recovery only.
        if error {
            login.show_error("示例：登录页未能加载，请核对网络后重新加载。若一直为空白，请修复 WebView2 Runtime。");
        }
        let root = BoxSizer::builder(Orientation::Vertical).build();
        root.add(&login.page, 1, SizerFlag::Expand, 0);
        frame.set_sizer(root, true);
        frame.layout();
        login.page.layout();
        assert!(login.retry.is_enabled());
        assert!(login.take_session().is_none());
        assert!(!frame.is_shown());
        if let Some(bitmap) = login.page.capture_bitmap() {
            save(&output.join(format!("{name}.png")), bitmap);
        }
        login.clear();
        login.page.destroy();
        frame.destroy();
    }
}

fn review_runtime_surface(output: &Path) {
    use visual::{RuntimeClient, RuntimeMcp, RuntimeRoute};
    for (name, mode, width, height, idle) in [
        (
            "runtime-adaptive-idle",
            theme::ThemeMode::Light,
            900,
            560,
            true,
        ),
        (
            "runtime-adaptive-small",
            theme::ThemeMode::Light,
            900,
            560,
            false,
        ),
        (
            "runtime-adaptive-wide",
            theme::ThemeMode::Light,
            1366,
            900,
            false,
        ),
        (
            "runtime-adaptive-dark",
            theme::ThemeMode::Dark,
            1180,
            760,
            false,
        ),
    ] {
        theme::init(mode);
        let frame = Frame::builder()
            .with_title("hidden production runtime viewport")
            .build();
        frame.set_font(&theme::font(theme::TextRole::Body));
        frame.set_size(visual::dip_size(&frame, width, height));
        let root = Panel::builder(&frame).build();
        let shell = shell::Shell::new(&root, &frame);
        let page = Panel::builder(&shell.book).build();
        page.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let tabs = BoxSizer::builder(Orientation::Horizontal).build();
        for text in ["运行状态", "使用统计"] {
            let b = controls::Button::builder(&page)
                .with_label(text)
                .with_kind(controls::ButtonKind::Tab)
                .with_size(Size::new(100, 42))
                .build();
            b.set_selected(text == "运行状态");
            tabs.add(&b, 0, SizerFlag::Right, 12);
        }
        col.add_sizer(
            &tabs,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(12),
        );
        let (scroll, graph) = visual::runtime_surface(&page);
        let clients = if idle {
            vec![]
        } else {
            [
                client_types::ClientId::Codex,
                client_types::ClientId::WorkBuddy,
                client_types::ClientId::Tiangong,
            ]
            .into_iter()
            .map(|id| RuntimeClient {
                id,
                state: "已打开".into(),
                busy: id == client_types::ClientId::WorkBuddy,
                flowing: id == client_types::ClientId::WorkBuddy,
                opacity: 1.,
            })
            .collect::<Vec<_>>()
        };
        let routes = if idle {
            vec![]
        } else {
            vec![RuntimeRoute {
                route: "synthetic".into(),
                model: "gpt-6".into(),
                channel: "合成渠道".into(),
                detail: "1 个请求".into(),
                active: true,
                flowing: true,
                opacity: 1.,
            }]
        };
        let mcps = if idle {
            vec![]
        } else {
            vec![
                RuntimeMcp {
                    models: vec![],
                    name: "Dumpling-MCP".into(),
                    detail: "1 个调用".into(),
                    active: true,
                    flowing: true,
                    opacity: 1.,
                },
                RuntimeMcp {
                    models: vec![],
                    name: "filesystem".into(),
                    detail: "调用已结束".into(),
                    active: false,
                    flowing: false,
                    opacity: 1.,
                },
            ]
        };
        let min_height = visual::runtime_height(clients.len(), routes.len(), mcps.len());
        graph.set_min_size(visual::dip_size(&graph, 620, min_height));
        graph.on_paint(move |_| {
            let base = AutoBufferedPaintDC::new(&graph);
            let dc = GraphicsDC::new(&base);
            let scale = visual::scale(&graph);
            let size = graph.get_client_size();
            let c = visual::Canvas::new(&dc, scale);
            let hits = visual::draw_runtime(
                &c,
                Size::new(
                    (size.width as f64 / scale) as i32,
                    (size.height as f64 / scale) as i32,
                ),
                &clients,
                &routes,
                &mcps,
                Some(true),
                0.3,
            );
            assert_eq!(hits.mcps.len(), mcps.len());
        });
        col.add(&scroll, 1, SizerFlag::Expand, 0);
        page.set_sizer(col, true);
        shell.register(shell::PageId::Overview, &page);
        shell.service.set_label("本机服务 · 运行中");
        let fs = BoxSizer::builder(Orientation::Vertical).build();
        let rs = BoxSizer::builder(Orientation::Vertical).build();
        rs.add(&shell.panel, 1, SizerFlag::Expand, 0);
        root.set_sizer(rs, true);
        fs.add(&root, 1, SizerFlag::Expand, 0);
        frame.set_sizer(fs, true);
        frame.layout();
        frame.layout();
        scroll.fit_inside();
        assert!(!frame.is_shown());
        let view = scroll.get_client_size();
        let graph_size = graph.get_client_size();
        assert!(
            graph_size.height >= view.height,
            "runtime must fill available height"
        );
        assert!(graph_size.width <= view.width + 2, "runtime must fit width");
        assert!(
            scroll.get_virtual_size().height <= view.height + 2,
            "three opened clients and a two-item MCP branch must fit the minimum window"
        );
        if width >= 1180 {
            assert!(
                graph_size.height > graph.from_dip(430),
                "runtime must no longer cap at 430 DIP"
            );
        }
        println!(
            "runtime {name}: view {view:?}, graph {graph_size:?}, virtual {:?}",
            scroll.get_virtual_size()
        );
        save(
            &output.join(format!("{name}.png")),
            shell.panel.capture_bitmap().unwrap(),
        );
        graph.set_min_size(visual::dip_size(
            &graph,
            620,
            visual::runtime_height(7, 6, 8),
        ));
        scroll.layout();
        scroll.fit_inside();
        assert!(
            scroll.get_virtual_size().height > view.height,
            "real overflow must remain accessible"
        );
        graph.set_min_size(visual::dip_size(
            &graph,
            620,
            visual::runtime_height(0, 0, 0),
        ));
        scroll.layout();
        scroll.fit_inside();
        assert!(
            scroll.get_virtual_size().height <= scroll.get_client_size().height + 2,
            "old routes must not leave a false scrollbar after a fresh visit"
        );
        frame.destroy();
    }
}

fn review_browser_login(output: &Path) {
    for (name, mode, waiting, failure) in [
        (
            "squad-browser-login-small",
            theme::ThemeMode::Light,
            false,
            false,
        ),
        (
            "squad-browser-login-waiting",
            theme::ThemeMode::Light,
            true,
            false,
        ),
        (
            "squad-browser-login-fallback",
            theme::ThemeMode::Light,
            false,
            true,
        ),
        (
            "squad-browser-login-dark",
            theme::ThemeMode::Dark,
            true,
            false,
        ),
    ] {
        theme::init(mode);
        let frame = Frame::builder()
            .with_title("hidden browser-login presentation; no network")
            .build();
        frame.set_font(&theme::font(theme::TextRole::Body));
        frame.set_size(visual::dip_size(&frame, 900, 560));
        let root = Panel::builder(&frame).build();
        let shell = shell::Shell::new(&root, &frame);
        let login = squad_browser::BrowserLogin::new(&shell.book);
        shell.register(shell::PageId::Squad, &login.page);
        shell.navigate(shell::PageId::Squad);
        if waiting {
            login.capability(&serde_json::json!({"state":"available"}));
            login.waiting("123456");
        }
        if failure {
            login.capability(&serde_json::json!({"state":"unsupported","message":"当前站点尚未启用浏览器登录，请使用内置登录"}));
        }
        let col = BoxSizer::builder(Orientation::Vertical).build();
        col.add(&shell.panel, 1, SizerFlag::Expand, 0);
        root.set_sizer(col, true);
        let outer = BoxSizer::builder(Orientation::Vertical).build();
        outer.add(&root, 1, SizerFlag::Expand, 0);
        frame.set_sizer(outer, true);
        frame.layout();
        frame.layout();
        login.page.layout();
        let size = login.page.get_client_size();
        for button in [login.start, login.builtin] {
            let position = button.get_position();
            let extent = button.get_size();
            assert!(position.x >= 0 && position.y >= 0);
            assert!(position.x + extent.width <= size.width + 2);
            assert!(position.y + extent.height <= size.height + 2);
        }
        assert_eq!(login.cancel.is_shown(), waiting);
        assert!(!frame.is_shown());
        save(
            &output.join(format!("{name}.png")),
            shell.panel.capture_bitmap().unwrap(),
        );
        squad_browser::render_profile(
            shell.account,
            &serde_json::json!({"signedIn":true,"user":{"username":"天才小分队示例用户资料","balance":12.34}}),
        );
        assert!(!shell.account.get_label().contains("用户资料"));
        if name.ends_with("small") {
            frame.layout();
            save(
                &output.join("squad-account-profile-small.png"),
                shell.panel.capture_bitmap().unwrap(),
            );
        }
        frame.destroy();
    }
}

fn review_account_workspace(output: &Path) {
    use serde_json::{Value, json};
    let menus = [
        ("account", "我的账户"),
        ("profile", "账户设置"),
        ("keys", "API 密钥"),
        ("usage", "使用记录"),
        ("organization", "组织与扣费"),
        ("organization-admin", "组织管理"),
        ("organization-usage", "组织用量"),
        ("channels", "可用渠道"),
        ("purchase", "充值 / 订阅"),
    ]
    .map(|(id, label)| json!({"id":id,"label":label}))
    .to_vec();
    let user = json!({"id":7,"username":"示例用户","email":"synthetic@example.invalid","balance":128.5,"status":"active","concurrency":3,"balance_notify_enabled":true,"balance_notify_threshold":10,"balance_notify_extra_emails":[],"created_at":"2026-06-09T09:24:46+08:00"});
    let mut state =
        json!({"signedIn":true,"user":user,"menus":menus,"pending":[],"money":{"usd_to_cny":7.15}});
    for (name, key, w, h, dark, variant) in [
        ("squad-account-small", "account", 900, 560, false, ""),
        ("squad-account-wide", "account", 1366, 900, false, ""),
        ("squad-account-dark", "account", 1200, 780, true, ""),
        ("squad-profile-small", "profile", 900, 560, false, ""),
        ("squad-profile-wide", "profile", 1366, 900, false, ""),
        ("squad-profile-dark", "profile", 1200, 780, true, ""),
        ("squad-keys-small", "keys", 900, 560, false, ""),
        ("squad-keys-wide", "keys", 1366, 900, false, ""),
        ("squad-keys-dark", "keys", 1200, 780, true, ""),
        ("squad-usage-small", "usage", 900, 560, false, ""),
        ("squad-usage-dark", "usage", 1200, 780, true, ""),
        (
            "squad-organization-small",
            "organization",
            900,
            560,
            false,
            "",
        ),
        ("squad-purchase-small", "purchase", 900, 560, false, ""),
        ("squad-purchase-dark", "purchase", 1200, 780, true, ""),
        (
            "squad-subscription-small",
            "purchase",
            900,
            560,
            false,
            "subscription",
        ),
        (
            "squad-subscription-dark",
            "purchase",
            1200,
            780,
            true,
            "subscription",
        ),
        ("squad-empty-keys-small", "keys", 900, 560, false, "empty"),
        ("squad-profile-error", "profile", 900, 560, false, "error"),
        ("squad-profile-simple", "profile", 900, 560, false, "simple"),
        ("squad-keys-page", "keys", 900, 560, false, "paged"),
    ] {
        theme::init(if dark {
            theme::ThemeMode::Dark
        } else {
            theme::ThemeMode::Light
        });
        let frame = Frame::builder()
            .with_title("hidden production account workspace; synthetic data only")
            .build();
        frame.set_font(&theme::font(theme::TextRole::Body));
        frame.set_size(visual::dip_size(&frame, w, h));
        let root = Panel::builder(&frame).build();
        let shell = shell::Shell::new(&root, &frame);
        let workspace = squad_view::Workspace::new(&shell.book);
        shell.register(shell::PageId::Squad, &workspace.page);
        shell.navigate(shell::PageId::Squad);
        squad_browser::render_profile(shell.account, &state);
        let mut current = state.clone();
        current["money"] = json!({"usd_to_cny":7.15});
        if variant == "simple" {
            current["menus"] = json!([{"id":"account","label":"我的账户"},{"id":"keys","label":"API 密钥"},{"id":"profile","label":"账户设置"}]);
        }
        let mut data = match key {
            "account" => {
                json!({"stats":{"today_actual_cost":3.284,"today_requests":186,"active_api_keys":3},"trend":{"trend":[{"date":"2026-11-03","actual_cost":0.52},{"date":"2026-11-04","actual_cost":1.18},{"date":"2026-11-05","actual_cost":0.88},{"date":"2026-11-06","actual_cost":2.62},{"date":"2026-11-07","actual_cost":1.62},{"date":"2026-11-08","actual_cost":4.2},{"date":"2026-11-09","actual_cost":3.284}]}})
            }
            "profile" => user.clone(),
            "keys" => {
                json!({"items":[{"id":17,"name":"日常使用","status":"active","group":{"name":"标准模型"},"quota_used":12.68,"today_actual_cost":0.2368,"total_actual_cost":12.68,"quota":100.0,"expires_at":null},{"id":18,"name":"开发测试","status":"inactive","group":{"name":"代码模型"},"quota_used":0,"today_actual_cost":0.000057,"total_actual_cost":1.32,"quota":0,"expires_at":"2026-12-31T23:59:00+08:00"}],"total":2})
            }
            "usage" => {
                json!({"items":[{"id":2,"created_at":"2026-11-09T10:28:00+08:00","model":"gpt-6","input_tokens":1200,"cache_read_tokens":600,"output_tokens":428,"actual_cost":0.018,"billing_type":"balance"},{"id":3,"created_at":"2026-11-09T10:25:00+08:00","model":"claude-opus-4-6","input_tokens":2530,"output_tokens":836,"actual_cost":0.052,"billing_type":"subscription"}],"total":2})
            }
            "organization" => {
                json!({"memberships":[{"id":11,"business_unit_name":"示例团队","department_name":"研发组","is_active_billing":true,"today_usage":3.28,"month_usage":56.2},{"id":12,"business_unit_name":"示例团队","department_name":"内容组","is_active_billing":false,"today_usage":1.28,"month_usage":19.2}]})
            }
            "purchase" => {
                json!({"global_min":10,"global_max":1000,"subscription_usd_to_cny_rate":1,"balance_recharge_multiplier":1,"balance_disabled":false,"methods":{"alipay":{"currency":"CNY","available":true,"display_name":"支付宝"}},"plans":[{"id":3,"name":"轻量订阅","price":29,"validity_days":30,"daily_limit_usd":10,"currency":"CNY","description":"适合日常使用"},{"id":4,"name":"进阶订阅","price":99,"validity_days":30,"daily_limit_usd":40,"currency":"CNY","description":"更充足的使用额度"}]})
            }

            "orders" => {
                json!({"items":[{"id":9,"out_trade_no":"DEMO-20261109-001","status":"pending","pay_amount":29,"currency":"CNY","order_type":"subscription","created_at":"2026-11-09T10:28:00+08:00"}],"total":60})
            }
            _ => Value::Null,
        };
        if variant == "empty" {
            data = json!({"items":[],"total":0});
        }
        if variant == "paged" {
            data["total"] = 60.into();
        }
        if variant == "error" {
            data = json!({"loadError":"HTTP 409 Conflict: {\"error\":\"站点暂时无法响应，请重新读取\"}"});
        }
        let callbacks = Rc::new(Cell::new(0));
        let count = callbacks.clone();
        workspace.on_select(move |_| count.set(count.get() + 1));
        workspace.render(key, &current, &data, 0);
        workspace.render(key, &current, &data, 0);
        assert_eq!(
            callbacks.get(),
            0,
            "data refresh must never trigger navigation or another request"
        );
        if variant == "subscription" {
            workspace.purchase.select_subscription(true);
        }
        let col = BoxSizer::builder(Orientation::Vertical).build();
        col.add(&shell.panel, 1, SizerFlag::Expand, 0);
        root.set_sizer(col, false);
        let outer = BoxSizer::builder(Orientation::Vertical).build();
        outer.add(&root, 1, SizerFlag::Expand, 0);
        frame.set_sizer(outer, false);
        frame.layout();
        frame.layout();
        workspace.page.layout();
        assert!(!frame.is_shown());
        assert_eq!(
            workspace.filters.is_shown(),
            matches!(key, "usage" | "organization-audit")
        );
        assert_eq!(workspace.previous.is_shown(), variant == "paged");
        assert_eq!(workspace.next.is_shown(), variant == "paged");
        assert!(!workspace.status.get_label().contains("HTTP 409"));
        for button in [workspace.website, workspace.refresh, workspace.logout]
            .into_iter()
            .filter(|b| b.is_shown())
        {
            let p = button.get_position();
            let extent = button.get_size();
            let size = workspace.page.get_client_size();
            assert!(
                p.x >= 0
                    && p.y >= 0
                    && p.x + extent.width <= size.width + 2
                    && p.y + extent.height <= size.height + 2,
                "{name}: header fits"
            );
        }
        if key == "purchase" {
            let b = workspace.purchase.submit;
            let pos = b.get_position();
            let extent = b.get_size();
            let page = workspace.purchase.panel.get_client_size();
            assert!(
                b.is_shown() && pos.x >= 0 && pos.y >= 0 && pos.y + extent.height <= page.height,
                "{name}: checkout button fits in viewport"
            );
            assert!(workspace.purchase.draft().is_ok());
            let origin = b.client_to_screen(Point::new(0, 0));
            let origin = workspace.purchase.panel.screen_to_client(origin);
            let selection = workspace
                .purchase
                .panel
                .find_window_by_name("squad-purchase-selection")
                .unwrap();
            let preview = workspace
                .purchase
                .panel
                .find_window_by_name("squad-purchase-preview")
                .unwrap();
            assert!(
                selection.get_client_size().height >= 80 && preview.get_client_size().height >= 80,
                "{name}: checkout content must have visible height"
            );
            assert!(
                selection.get_position().x < preview.get_position().x
                    && selection.get_position().y == preview.get_position().y,
                "{name}: payment choice and preview are side by side at desktop widths"
            );
            assert!(
                origin.x >= 0
                    && origin.y >= 0
                    && origin.x + extent.width <= page.width + 2
                    && origin.y + extent.height <= page.height + 2,
                "{name}: checkout action must be visible in the purchase viewport, including its ancestors"
            );
        }
        if workspace.table.is_shown() {
            if key == "usage" {
                let top = workspace.table.get_position().y;
                let body_height = workspace.table.get_client_size().height;
                let page_height = workspace
                    .table
                    .get_parent()
                    .unwrap()
                    .get_client_size()
                    .height;
                assert!(
                    top <= 2 && body_height >= page_height - 4,
                    "{name}: usage table fills the body instead of reserving hidden page space"
                );
                assert_eq!(workspace.start.get_value(), workspace.end.get_value());
                assert!(
                    !workspace.start.get_value().is_empty(),
                    "usage defaults to today"
                );
            }
            let width = workspace.table.get_client_size().width;
            let sum = (0..workspace.table.get_column_count())
                .map(|i| workspace.table.get_column_width(i as i64))
                .sum::<i32>();
            assert!(
                sum <= width && sum >= width - 6,
                "{name}: table uses available width"
            );
            workspace
                .table
                .set_item_state(0, ListItemState::Selected, ListItemState::Selected);
            assert!(workspace.selected().is_some());
            if key == "keys" {
                assert!(workspace.extra.is_enabled());
            }
        }
        save(
            &output.join(format!("{name}.png")),
            shell.panel.capture_bitmap().unwrap(),
        );
        assert_eq!(workspace.logout.is_shown(), key == "profile");
        assert!(!workspace.primary.is_shown() || key != "profile" && key != "purchase");
        assert!(!workspace.extra.is_shown() || key != "profile");
        if key == "usage" {
            assert_eq!(workspace.table.get_column_count(), 5);
        }
        workspace.loading(key, &current);
        assert!(!workspace.primary.is_enabled() && !workspace.edit.is_enabled());
        assert_eq!(callbacks.get(), 0);
        workspace.clear();
        frame.destroy();
        state["user"] = user.clone();
    }
}

fn review_account_secondaries(output: &Path) {
    use serde_json::json;
    for (name, dark, w, h, path) in [
        ("squad-membership-detail-small", false, 680, 520, "detail"),
        ("squad-membership-detail-dark", true, 760, 620, "detail"),
        ("squad-org-members-small", false, 900, 560, "memberships"),
        (
            "squad-org-allocation-small",
            false,
            900,
            560,
            "allocation-policy",
        ),
        (
            "squad-org-allocation-dark",
            true,
            1120,
            740,
            "allocation-policy",
        ),
        ("squad-org-settlement-wide", false, 1120, 740, "settlement"),
    ] {
        theme::init(if dark {
            theme::ThemeMode::Dark
        } else {
            theme::ThemeMode::Light
        });
        let frame = Frame::builder()
            .with_title("hidden production account detail")
            .build();
        frame.set_font(&theme::font(theme::TextRole::Body));
        frame.set_size(visual::dip_size(&frame, w, h));
        let col = BoxSizer::builder(Orientation::Vertical).build();
        if path == "detail" {
            let detail = squad_view::detail::Detail::new(&frame);
            detail.render("扣费身份",&json!({"business_unit_name":"示例团队","department_name":"研发组","is_active_billing":true,"today_usage":1.8,"month_usage":32.4,"month_tokens":482000}),Default::default());
            col.add(&detail.panel, 1, SizerFlag::Expand | SizerFlag::All, 20);
        } else {
            let sections = squad_view::organization_data::sections("organization-admin");
            let index = sections.iter().position(|s| s.path == path).unwrap();
            let section = sections[index];
            let view = squad_view::organization::Organization::new(
                &frame,
                "示例团队",
                sections
                    .iter()
                    .map(|s| (s.name.into(), s.path.into()))
                    .collect(),
            );
            view.select(index);
            view.summary.panel.hide();
            let data = if path == "allocation-policy" {
                allocation_fixture()
            } else if path == "settlement" {
                json!({"lines":[{"department_name":"研发组","starts_at":"2026-10-01","ends_at":"2026-11-01","due_cents":30000,"paid_cents":5000,"outstanding_cents":25000,"closed":true},{"department_name":"研发组","starts_at":"2026-11-01","ends_at":"2026-12-01","due_cents":3600,"paid_cents":0,"outstanding_cents":3600,"closed":false}]})
            } else {
                json!([{"user_email":"member@example.invalid","username":"小林","department_name":"研发组","status":"active"}])
            };
            let rows = if path == "allocation-policy" {
                squad_view::organization_data::allocation_rows(&data)
            } else if path == "settlement" {
                data["lines"].as_array().unwrap().clone()
            } else {
                data.as_array().unwrap().clone()
            };
            view.set_metrics(&squad_view::organization_data::metrics(path, &data));
            for (i, key) in section.columns.iter().enumerate() {
                view.table.insert_column(
                    i as i64,
                    squad_form::label(key),
                    ListColumnFormat::Left,
                    120,
                );
            }
            for (i, row) in rows.iter().enumerate() {
                for (j, key) in section.columns.iter().enumerate() {
                    let value = squad_view::detail::formatted(key, &row[*key], Default::default());
                    if j == 0 {
                        view.table.insert_item(i as i64, &value, None);
                    } else {
                        view.table
                            .set_item_text_by_column(i as i64, j as i32, &value);
                    }
                }
            }
            view.add.show(path == "memberships");
            view.add.set_label("添加成员");
            view.remove.show(path == "memberships");
            view.batch.show(path == "memberships");
            view.edit.show(path == "allocation-policy");
            view.edit.set_label("设置分配");
            view.details.set_label(if path == "memberships" {
                "查看成员"
            } else {
                "查看详情"
            });
            view.more.show(path == "settlement");
            view.more.set_label("账单与清账");
            view.next.hide();
            view.previous.hide();
            col.add(&view.panel, 1, SizerFlag::Expand | SizerFlag::All, 20);
            frame.set_sizer(col, false);
            frame.layout();
            view.fit_columns();
            save(
                &output.join(format!("{name}.png")),
                view.panel.capture_bitmap().unwrap(),
            );
            frame.destroy();
            continue;
        }
        frame.set_sizer(col, false);
        frame.layout();
        save(
            &output.join(format!("{name}.png")),
            frame.capture_bitmap().unwrap(),
        );
        frame.destroy();
    }
    review_member_allocation_selection(output);
}
fn allocation_fixture() -> serde_json::Value {
    use serde_json::json;
    json!({"configured":true,"budget_limit_amount":1200,"allocation_mode":"custom","current_period_allocation_mode":"custom","allocated_basis_points":7500,"reserve_basis_points":2500,"members":[
        {"user_id":1,"email":"lin@example.invalid","username":"小林","custom_share_basis_points":2500,"projected_amount":300,"current_period_allocated_amount":400,"current_period_used_amount":112.5,"current_period_remaining_amount":287.5},
        {"user_id":2,"email":"zhou@example.invalid","username":"小周","custom_share_basis_points":5000,"projected_amount":600,"current_period_allocated_amount":800,"current_period_used_amount":220,"current_period_remaining_amount":580}
    ]})
}
fn review_member_allocation_selection(output: &Path) {
    use serde_json::json;
    for dark in [false, true] {
        theme::init(if dark {
            theme::ThemeMode::Dark
        } else {
            theme::ThemeMode::Light
        });
        let mode = if dark { "dark" } else { "light" };
        for kind in ["member", "allocation", "batch-picker"] {
            let frame = Frame::builder()
                .with_title("hidden production organization editor")
                .build();
            frame.set_font(&theme::font(theme::TextRole::Body));
            frame.set_size(visual::dip_size(&frame, 820, 640));
            let col = BoxSizer::builder(Orientation::Vertical).build();
            if kind == "member" {
                let row = json!({"username":"小林","user_email":"lin@example.invalid","department_name":"研发组"});
                let view = squad_view::member::Member::new(&frame, &row, true, true);
                view.populate(
                    0,
                    "/organization-admin/business-units/1/members/1/profile",
                    &json!({"username":"小林"}),
                    &Default::default(),
                );
                let i = view.sections.iter().position(|s| s.0 == "limits").unwrap();
                view.populate(i,"/organization-admin/business-units/1/members/1/limits",&json!({"daily_enabled":true,"daily_limit":10,"weekly_enabled":false,"weekly_limit":50,"monthly_enabled":true,"monthly_limit":300,"expected_updated_at":"synthetic-revision"}),&Default::default());
                assert_eq!(view.body().unwrap()["daily_limit"], 10);
                assert_eq!(
                    view.body().unwrap()["expected_updated_at"],
                    "synthetic-revision"
                );
                col.add(&view.panel, 1, SizerFlag::Expand | SizerFlag::All, 22);
                frame.set_sizer(col, false);
                frame.layout();
                assert!(
                    view.save.get_position().y + view.save.get_size().height
                        <= view.panel.get_client_size().height
                );
                save(
                    &output.join(format!("squad-member-editor-{mode}.png")),
                    view.panel.capture_bitmap().unwrap(),
                );
            } else if kind == "allocation" {
                let view = squad_view::allocation::Allocation::new(&frame, &allocation_fixture());
                col.add(&view.panel, 1, SizerFlag::Expand | SizerFlag::All, 22);
                frame.set_sizer(col, false);
                frame.layout();
                assert_eq!(
                    view.body().unwrap()["members"][0]["share_basis_points"],
                    2500
                );
                assert!(view.save.is_enabled());
                assert!(
                    view.save.get_position().y + view.save.get_size().height
                        <= view.panel.get_client_size().height
                );
                save(
                    &output.join(format!("squad-allocation-editor-{mode}.png")),
                    view.panel.capture_bitmap().unwrap(),
                );
            } else {
                let view = squad_view::selection::Selection::new(
                    &frame,
                    "选择批量设置的成员",
                    (0..24)
                        .map(|i| format!("成员 {}  ·  member{i}@example.invalid", i + 1))
                        .collect(),
                    true,
                );
                col.add(&view.panel, 1, SizerFlag::Expand | SizerFlag::All, 22);
                frame.set_sizer(col, false);
                frame.layout();
                #[cfg(windows)]
                {
                    draft_click(
                        &view
                            .panel
                            .find_window_by_name("organization-picker-row-2")
                            .unwrap(),
                    );
                    view.search.set_value("member20@");
                    assert_eq!(view.indices(), vec![2]);
                    draft_click(
                        &view
                            .panel
                            .find_window_by_name("organization-picker-row-20")
                            .unwrap(),
                    );
                    view.search.set_value("");
                    assert_eq!(view.indices(), vec![2, 20]);
                }
                assert!(view.accept.is_enabled());
                save(
                    &output.join(format!("squad-batch-picker-{mode}.png")),
                    view.panel.capture_bitmap().unwrap(),
                );
            }
            assert!(!frame.is_shown());
            frame.destroy();
        }
    }
}

fn review_account_forms(output: &Path) {
    use serde_json::json;
    for (name, dark, fields) in [
        (
            "squad-batch-balance-light",
            false,
            json!({"operation":"add","amount":120,"user_ids":[1,2,3]}),
        ),
        (
            "squad-batch-balance-dark",
            true,
            json!({"operation":"add","amount":120,"user_ids":[1,2,3]}),
        ),
        (
            "squad-key-form-light",
            false,
            json!({"name":"日常使用","quota":100,"expires_in_days":30,"ip_whitelist":[]}),
        ),
        (
            "squad-member-limit-form",
            false,
            json!({"daily_enabled":true,"daily_limit":10,"monthly_enabled":true,"monthly_limit":100}),
        ),
    ] {
        theme::init(if dark {
            theme::ThemeMode::Dark
        } else {
            theme::ThemeMode::Light
        });
        let frame = Frame::builder().with_title("hidden form host").build();
        let options = [
            (
                "period_type".into(),
                vec![
                    ("每月".into(), "month".into()),
                    ("每周".into(), "week".into()),
                ],
            ),
            (
                "operation".into(),
                vec![
                    ("增加".into(), "add".into()),
                    ("扣减".into(), "subtract".into()),
                ],
            ),
        ]
        .into_iter()
        .collect();
        let dialog = squad_form::review_form(
            &frame,
            if name.contains("key") {
                "新建密钥"
            } else if name.contains("member") {
                "成员限额"
            } else {
                "批量调整余额 · 3 位成员"
            },
            "/organization-admin/business-units/1/members/balance/adjust-batch",
            &fields,
            &options,
            squad_money::Money::default(),
        );
        dialog.layout();
        dialog.layout();
        assert!(!dialog.is_shown());
        save(
            &output.join(format!("{name}.png")),
            dialog.capture_bitmap().unwrap(),
        );
        dialog.destroy();
        frame.destroy();
    }
}
