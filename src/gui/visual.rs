//! Shared product drawing. Window painting and offline PNG reviews use this
//! module, so review images cannot silently drift to a separate HTML layout.
use super::theme;
use crate::desktop::clients::ClientId;
use std::{cell::RefCell, collections::HashMap};
use wxdragon::{
    dc::DeviceContext,
    font::{Font, FontWeight},
    prelude::*,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(super) enum Icon {
    None,
    Overview,
    Squad,
    Codex,
    WorkBuddy,
    Tiangong,
    VsCode,
    DeepSeek,
    ZCode,
    WebStorm,
    Model,
    Skill,
    Mcp,
    Chat,
    Logs,
    Settings,
    Help,
    More,
    Chevron,
    Refresh,
    Plus,
    Hub,
    Channel,
    Collapse,
    Back,
    WindowMinimize,
    WindowMaximize,
    WindowRestore,
    WindowClose,
    Calendar,
}
impl Icon {
    pub fn client(id: ClientId) -> Self {
        match id {
            ClientId::Codex => Self::Codex,
            ClientId::WorkBuddy => Self::WorkBuddy,
            ClientId::Tiangong => Self::Tiangong,
            ClientId::VsCode => Self::VsCode,
            ClientId::DeepSeekHarness => Self::DeepSeek,
            ClientId::ZCode => Self::ZCode,
            ClientId::WebStorm => Self::WebStorm,
        }
    }
    pub fn from_code(code: u32) -> Self {
        const ALL: &[Icon] = &[
            Icon::None,
            Icon::Overview,
            Icon::Squad,
            Icon::Codex,
            Icon::WorkBuddy,
            Icon::Tiangong,
            Icon::VsCode,
            Icon::DeepSeek,
            Icon::ZCode,
            Icon::WebStorm,
            Icon::Model,
            Icon::Skill,
            Icon::Mcp,
            Icon::Chat,
            Icon::Logs,
            Icon::Settings,
            Icon::Help,
            Icon::More,
            Icon::Chevron,
            Icon::Refresh,
            Icon::Plus,
            Icon::Hub,
            Icon::Channel,
            Icon::Collapse,
            Icon::Back,
            Icon::WindowMinimize,
            Icon::WindowMaximize,
            Icon::WindowRestore,
            Icon::WindowClose,
            Icon::Calendar,
        ];
        ALL.get(code as usize).copied().unwrap_or(Self::None)
    }
}
thread_local! {
    static FONTS: RefCell<HashMap<(i32,bool),Font>> = RefCell::new(HashMap::new());
    static CLIENT_MARKS: RefCell<HashMap<(u32,i32,u8,u8,u8),Bitmap>> = RefCell::new(HashMap::new());
    static HUB_MARKS: RefCell<HashMap<i32, Bitmap>> = RefCell::new(HashMap::new());
}

pub(super) fn scale<W: WxWidget>(window: &W) -> f64 {
    window.from_dip(100) as f64 / 100.
}
pub(super) fn dip_size<W: WxWidget>(window: &W, w: i32, h: i32) -> Size {
    Size::new(
        if w < 0 { w } else { window.from_dip(w) },
        if h < 0 { h } else { window.from_dip(h) },
    )
}

/// Invalidate the whole painted surface, including the old right/bottom edge.
/// wxMSW otherwise may only repaint newly exposed strips during a resize.
pub(super) fn redraw_on_resize(panel: Panel) {
    panel.on_size(move |event| {
        panel.refresh(false, None);
        event.skip(true);
    });
}

pub(super) struct Canvas<'a> {
    pub dc: &'a dyn DeviceContext,
    pub scale: f64,
}
impl<'a> Canvas<'a> {
    pub fn new(dc: &'a dyn DeviceContext, scale: f64) -> Self {
        Self { dc, scale }
    }
    pub fn px(&self, v: i32) -> i32 {
        (v as f64 * self.scale).round() as i32
    }
    pub fn physical(&self, r: Rect) -> Rect {
        Rect::new(
            self.px(r.x),
            self.px(r.y),
            self.px(r.width),
            self.px(r.height),
        )
    }
    pub fn clear(&self, color: Colour) {
        self.dc.set_background(color);
        self.dc.clear();
    }
    pub fn font(&self, height: i32, bold: bool) {
        let height = self.px(height);
        FONTS.with(|fonts| {
            let mut fonts = fonts.borrow_mut();
            let font = fonts.entry((height, bold)).or_insert_with(|| {
                let mut f = theme::font(theme::TextRole::Body);
                f.set_pixel_height(height);
                if bold {
                    f.set_weight(FontWeight::Bold);
                }
                f
            });
            self.dc.set_font(font);
        });
    }
    pub fn text(&self, text: &str, x: i32, y: i32, height: i32, bold: bool, color: Colour) {
        self.font(height, bold);
        self.dc.set_text_foreground(color);
        self.dc.draw_text(text, self.px(x), self.px(y));
    }
    pub fn fit_text(&self, text: &str, width: i32) -> String {
        if self.dc.get_text_extent(text).0 <= self.px(width) {
            return text.into();
        }
        let mut s = text.to_string();
        while !s.is_empty() && self.dc.get_text_extent(&format!("{s}…")).0 > self.px(width) {
            s.pop();
        }
        if s.is_empty() {
            String::new()
        } else {
            format!("{s}…")
        }
    }
    pub fn label(&self, text: &str, r: Rect, height: i32, bold: bool, color: Colour, center: bool) {
        self.font(height, bold);
        let text = self.fit_text(text, r.width);
        let (w, h) = self.dc.get_text_extent(&text);
        self.dc.set_text_foreground(color);
        self.dc.draw_text(
            &text,
            self.px(r.x)
                + if center {
                    (self.px(r.width) - w) / 2
                } else {
                    0
                },
            self.px(r.y) + (self.px(r.height) - h) / 2,
        );
    }
    pub fn rounded(&self, r: Rect, radius: i32, fill: Colour, stroke: Option<Colour>) {
        let r = self.physical(r);
        self.dc.set_pen(
            stroke.unwrap_or(fill),
            self.px(1).max(1),
            if stroke.is_some() {
                PenStyle::Solid
            } else {
                PenStyle::Transparent
            },
        );
        self.dc.set_brush(fill, BrushStyle::Solid);
        self.dc
            .draw_rounded_rectangle(r.x, r.y, r.width, r.height, radius as f64 * self.scale);
    }
    pub fn line(&self, a: Point, b: Point, color: Colour, dashed: bool) {
        self.dc.set_pen(
            color,
            self.px(1).max(1),
            if dashed {
                PenStyle::ShortDash
            } else {
                PenStyle::Solid
            },
        );
        self.dc
            .draw_line(self.px(a.x), self.px(a.y), self.px(b.x), self.px(b.y));
    }
    pub fn dot(&self, x: i32, y: i32, r: i32, color: Colour) {
        self.dc.set_pen(color, 1, PenStyle::Transparent);
        self.dc.set_brush(color, BrushStyle::Solid);
        self.dc.draw_circle(self.px(x), self.px(y), self.px(r));
    }
    pub fn curve(&self, a: Point, b: Point, color: Colour, phase: f64, flow: bool, dashed: bool) {
        let dx = (b.x - a.x) as f64 * 0.48;
        let at = |t: f64| -> Point {
            let u = 1. - t;
            Point::new(
                (u * u * u * a.x as f64
                    + 3. * u * u * t * (a.x as f64 + dx)
                    + 3. * u * t * t * (b.x as f64 - dx)
                    + t * t * t * b.x as f64)
                    .round() as i32,
                (u * u * u * a.y as f64
                    + 3. * u * u * t * a.y as f64
                    + 3. * u * t * t * b.y as f64
                    + t * t * t * b.y as f64)
                    .round() as i32,
            )
        };
        let points = (0..=64)
            .map(|i| {
                let p = at(i as f64 / 64.);
                wxdragon::dc::Point::new(self.px(p.x), self.px(p.y))
            })
            .collect::<Vec<_>>();
        self.dc.set_pen(
            color,
            self.px(1).max(1),
            if dashed {
                PenStyle::ShortDash
            } else {
                PenStyle::Solid
            },
        );
        self.dc.draw_lines(&points, 0, 0);
        if flow {
            for i in 0..4 {
                let p = at((phase + i as f64 / 4.) % 1.);
                self.dot(p.x, p.y, 2, color);
            }
        }
    }
    pub fn icon(&self, icon: Icon, r: Rect, color: Colour) {
        let source = match icon {
            Icon::Codex => Some(include_str!("../../packaging/brand/chatgpt-codex-line.svg")),
            Icon::WorkBuddy => Some(include_str!("../../packaging/brand/workbuddy-line.svg")),
            Icon::Tiangong => Some(include_str!("../../packaging/brand/tiangong-claw-line.svg")),
            Icon::DeepSeek => Some(include_str!("../../packaging/brand/providers/deepseek.svg")),
            Icon::ZCode => Some(include_str!("../../packaging/brand/zcode-line.svg")),
            Icon::WebStorm => Some(include_str!("../../packaging/brand/webstorm-line.svg")),
            Icon::VsCode => Some(include_str!("../../packaging/brand/vscode-line.svg")),
            _ => None,
        };
        if let Some(source) = source {
            let size = self.px(r.width.min(r.height)).max(1);
            CLIENT_MARKS.with(|cache| {
                let mut cache = cache.borrow_mut();
                let key = (icon as u32, size, color.r, color.g, color.b);
                if !cache.contains_key(&key) {
                    let tint = format!("#{:02x}{:02x}{:02x}",color.r,color.g,color.b);
                    let svg = source.replace("currentColor", &tint);
                    let svg = if icon == Icon::DeepSeek {
                        svg.replace("fill=\"#4D6BFE\"", &format!("fill=\"none\" stroke=\"{tint}\" stroke-width=\"1.1\" stroke-linejoin=\"round\""))
                    } else { svg };
                    if let Some(bitmap) = BitmapBundle::from_svg_data(svg.as_bytes(), Size::new(size,size))
                        .and_then(|b| b.get_bitmap(Size::new(size,size))) {
                        cache.insert(key, bitmap);
                    }
                }
                if let Some(bitmap) = cache.get(&key) {
                    self.dc.draw_bitmap(bitmap, self.px(r.x), self.px(r.y), true);
                }
            });
            return;
        }
        let p = |x: i32, y: i32| Point::new(r.x + x * r.width / 24, r.y + y * r.height / 24);
        let path = |coords: &[(i32, i32)]| {
            let points = coords
                .iter()
                .map(|(x, y)| {
                    let q = p(*x, *y);
                    wxdragon::dc::Point::new(self.px(q.x), self.px(q.y))
                })
                .collect::<Vec<_>>();
            self.dc.set_pen(color, self.px(1).max(1), PenStyle::Solid);
            self.dc.draw_lines(&points, 0, 0);
        };
        match icon {
            Icon::None => {}
            Icon::Hub
            | Icon::Tiangong
            | Icon::WorkBuddy
            | Icon::WebStorm
            | Icon::DeepSeek
            | Icon::ZCode => {
                self.label(
                    match icon {
                        Icon::WorkBuddy => "W",
                        Icon::WebStorm => "WS",
                        Icon::DeepSeek => "D",
                        Icon::ZCode => "Z",
                        _ => "T",
                    },
                    r,
                    if icon == Icon::WebStorm {
                        (r.height / 2).max(10)
                    } else {
                        (r.height * 3 / 4).max(12)
                    },
                    true,
                    color,
                    true,
                );
            }
            Icon::Codex => {
                for i in 0..6 {
                    let angle = i as f64 * std::f64::consts::TAU / 6.;
                    let x = r.x + r.width / 2 + (angle.cos() * r.width as f64 * 0.21) as i32;
                    let y = r.y + r.height / 2 + (angle.sin() * r.height as f64 * 0.21) as i32;
                    self.dc.set_pen(color, self.px(1).max(1), PenStyle::Solid);
                    self.dc.set_brush(color, BrushStyle::Transparent);
                    self.dc
                        .draw_circle(self.px(x), self.px(y), self.px(r.width / 5));
                }
            }
            Icon::VsCode => path(&[
                (3, 6),
                (17, 2),
                (22, 4),
                (22, 20),
                (17, 22),
                (3, 8),
                (3, 16),
                (17, 2),
                (17, 22),
                (3, 16),
            ]),
            Icon::Overview => {
                path(&[(2, 14), (7, 14), (10, 6), (14, 18), (17, 10), (22, 10)]);
            }
            Icon::Squad => {
                self.dot(r.x + r.width / 2, r.y + r.height / 3, 3, color);
                path(&[(4, 21), (4, 18), (7, 15), (17, 15), (20, 18), (20, 21)]);
            }
            Icon::Model => path(&[(12, 2), (22, 12), (12, 22), (2, 12), (12, 2), (12, 22)]),
            Icon::Channel => {
                path(&[
                    (3, 7),
                    (12, 2),
                    (21, 7),
                    (12, 12),
                    (3, 7),
                    (3, 18),
                    (12, 23),
                    (21, 18),
                    (21, 7),
                ]);
            }
            Icon::Skill => {
                path(&[(4, 2), (20, 2), (20, 22), (4, 22), (4, 2)]);
                for y in [7, 12, 17] {
                    path(&[(7, y), (17, y)]);
                }
            }
            Icon::Mcp => {
                path(&[(3, 13), (12, 4), (17, 4), (21, 8), (21, 13), (10, 23)]);
                path(&[(3, 18), (2, 14), (12, 4), (17, 9), (7, 19), (3, 18)]);
            }
            Icon::Chat => path(&[
                (3, 3),
                (21, 3),
                (21, 17),
                (12, 17),
                (7, 22),
                (7, 17),
                (3, 17),
                (3, 3),
            ]),
            Icon::Logs => {
                path(&[(5, 2), (19, 2), (19, 22), (5, 22), (5, 2)]);
                path(&[(9, 7), (15, 7)]);
                path(&[(9, 12), (15, 12)]);
                path(&[(9, 17), (13, 17)]);
            }
            Icon::Settings => {
                for i in 0..8 {
                    let a = i as f64 * std::f64::consts::TAU / 8.;
                    path(&[
                        (12 + (a.cos() * 8.) as i32, 12 + (a.sin() * 8.) as i32),
                        (12 + (a.cos() * 11.) as i32, 12 + (a.sin() * 11.) as i32),
                    ]);
                }
                self.dc.set_pen(color, self.px(1).max(1), PenStyle::Solid);
                self.dc.set_brush(color, BrushStyle::Transparent);
                self.dc.draw_circle(
                    self.px(r.x + r.width / 2),
                    self.px(r.y + r.height / 2),
                    self.px(r.width / 3),
                );
            }
            Icon::Help => {
                self.label("?", r, r.height * 4 / 5, true, color, true);
            }
            Icon::More => {
                for x in [5, 12, 19] {
                    let q = p(x, 12);
                    self.dot(q.x, q.y, 2, color);
                }
            }
            Icon::Collapse => {
                path(&[(5, 9), (12, 3), (19, 9)]);
                path(&[(5, 20), (12, 14), (19, 20)]);
            }
            Icon::Chevron => path(&[(9, 5), (16, 12), (9, 19)]),
            Icon::WindowMinimize => path(&[(3, 13), (21, 13)]),
            Icon::WindowMaximize => path(&[(4, 4), (20, 4), (20, 20), (4, 20), (4, 4)]),
            Icon::WindowRestore => {
                path(&[(8, 8), (8, 3), (21, 3), (21, 16), (16, 16)]);
                path(&[(3, 8), (16, 8), (16, 21), (3, 21), (3, 8)]);
            }
            Icon::WindowClose => {
                path(&[(4, 4), (20, 20)]);
                path(&[(20, 4), (4, 20)]);
            }
            Icon::Back => {
                path(&[(12, 5), (5, 12), (12, 19)]);
                path(&[(5, 12), (21, 12)]);
            }
            Icon::Calendar => {
                path(&[(4, 5), (20, 5), (20, 21), (4, 21), (4, 5)]);
                path(&[(4, 10), (20, 10)]);
                path(&[(8, 2), (8, 7)]);
                path(&[(16, 2), (16, 7)]);
                for (x, y) in [(8, 14), (15, 14), (8, 18), (15, 18)] {
                    let q = p(x, y);
                    self.dot(q.x, q.y, 1, color);
                }
            }
            Icon::Refresh => {
                path(&[
                    (21, 10),
                    (19, 5),
                    (14, 2),
                    (7, 3),
                    (3, 8),
                    (3, 15),
                    (8, 20),
                    (15, 21),
                    (20, 17),
                ]);
                path(&[(21, 3), (21, 10), (14, 10)]);
            }
            Icon::Plus => {
                path(&[(12, 4), (12, 20)]);
                path(&[(4, 12), (20, 12)]);
            }
        }
    }
}

pub(super) fn client_short(id: ClientId) -> &'static str {
    match id {
        ClientId::Codex => "ChatGPT / Codex",
        ClientId::DeepSeekHarness => "DeepSeek",
        _ => id.name(),
    }
}

#[derive(Clone)]
pub(super) struct RuntimeClient {
    pub id: ClientId,
    pub state: String,
    pub busy: bool,
    pub flowing: bool,
    pub opacity: f64,
}
#[derive(Clone)]
pub(super) struct RuntimeRoute {
    pub route: String,
    pub model: String,
    pub channel: String,
    pub detail: String,
    pub flowing: bool,
    pub active: bool,
    pub opacity: f64,
}
#[derive(Clone)]
pub(super) struct RuntimeMcp {
    pub name: String,
    pub detail: String,
    pub models: Vec<RuntimeMcpModel>,
    pub active: bool,
    pub flowing: bool,
    pub opacity: f64,
}
#[derive(Clone)]
pub(super) struct RuntimeMcpModel {
    pub route: String,
    pub model: String,
    pub flowing: bool,
}
pub(super) struct RuntimeHits {
    pub clients: Vec<Rect>,
    pub models: Vec<Rect>,
    pub channels: Vec<Rect>,
    pub mcps: Vec<Rect>,
}

/// Only the minimum is content-bound; the actual canvas fills its viewport.
pub(super) fn runtime_surface(parent: &dyn WxWidget) -> (ScrolledWindow, Panel) {
    let scroll = ScrolledWindow::builder(parent)
        .with_style(ScrolledWindowStyle::VScroll | ScrolledWindowStyle::HScroll)
        .build();
    scroll.set_scroll_rate(8, 12);
    scroll.set_background_color(theme::theme().bg_app);
    let graph = Panel::builder(&scroll).build();
    graph.set_background_color(theme::theme().bg_app);
    graph.set_background_style(BackgroundStyle::Paint);
    redraw_on_resize(graph);
    graph.set_min_size(dip_size(&graph, 620, 300));
    let col = BoxSizer::builder(Orientation::Vertical).build();
    col.add(&graph, 1, SizerFlag::Expand, 0);
    scroll.set_sizer(col, true);
    (scroll, graph)
}
pub(super) fn runtime_height(clients: usize, routes: usize, mcps: usize) -> i32 {
    (clients.max(routes).saturating_sub(1) as i32 * 64 + 114).max(if mcps > 0 { 242 } else { 300 })
        + mcp_height(mcps)
}
fn mcp_height(mcps: usize) -> i32 {
    if mcps == 0 {
        0
    } else {
        mcps.div_ceil(2) as i32 * 66 + 30
    }
}
fn faded(color: Colour, opacity: f64) -> Colour {
    let bg = theme::theme().bg_card;
    let mix = |a: u8, b: u8| (b as f64 + (a as f64 - b as f64) * opacity.clamp(0., 1.)) as u8;
    Colour::rgb(mix(color.r, bg.r), mix(color.g, bg.g), mix(color.b, bg.b))
}
pub(super) fn draw_runtime(
    c: &Canvas<'_>,
    size: Size,
    clients: &[RuntimeClient],
    routes: &[RuntimeRoute],
    mcps: &[RuntimeMcp],
    hub: Option<bool>,
    phase: f64,
) -> RuntimeHits {
    let t = theme::theme();
    c.clear(t.bg_app);
    let w = size.width;
    let ox = 0;
    let h = size.height;
    let content_h = runtime_height(clients.len(), routes.len(), mcps.len()).min(h);
    let oy = if mcps.is_empty() {
        0
    } else {
        (h - content_h) / 2
    };
    let main_h = if mcps.is_empty() {
        h
    } else {
        content_h - mcp_height(mcps.len())
    };
    c.rounded(Rect::new(ox, 0, w, h), 18, t.bg_card, None);
    c.text("路由运行", ox + 22, 20, 16, true, t.ink_primary);
    let active = clients.iter().filter(|v| v.busy).count();
    if active > 0 {
        c.dot(ox + 116, 31, 3, t.accent);
        c.text(
            &format!("{active} 个客户端请求中"),
            ox + 128,
            24,
            11,
            false,
            t.accent,
        );
    }
    let cy = oy + (main_h + 36) / 2;
    let hx = if clients.is_empty() && routes.is_empty() && mcps.is_empty() {
        w / 2
    } else if routes.is_empty() && mcps.is_empty() {
        w / 2 + 105
    } else {
        w * 38 / 100
    };
    let hs = if w < 900 { 104 } else { 128 };
    let hub_rect = Rect::new(ox + hx - hs / 2, cy - hs / 2, hs, hs);
    let cw = (w * 24 / 100).clamp(144, 218);
    let client_left = if routes.is_empty() && mcps.is_empty() {
        (w / 2 - 270).max(22)
    } else {
        22
    };
    let mw = (w * 18 / 100).clamp(108, 190);
    let rw = mw;
    let mx = w * 66 / 100;
    let rx = w - 22 - rw / 2;
    let ys = |count: usize, i: usize| cy - (count as i32 - 1) * 32 + i as i32 * 64;
    let mut hits = RuntimeHits {
        clients: Vec::new(),
        models: Vec::new(),
        channels: Vec::new(),
        mcps: Vec::new(),
    };
    for (i, client) in clients.iter().enumerate() {
        let y = ys(clients.len(), i);
        c.curve(
            Point::new(ox + client_left + cw, y),
            Point::new(hub_rect.x, cy),
            faded(
                if client.busy { t.accent } else { t.border },
                client.opacity,
            ),
            phase,
            client.flowing,
            !client.busy,
        );
    }
    for (i, route) in routes.iter().enumerate() {
        let y = ys(routes.len(), i);
        let color = faded(t.accent, route.opacity);
        c.curve(
            Point::new(hub_rect.x + hs, cy),
            Point::new(ox + mx - mw / 2, y),
            color,
            phase,
            route.flowing,
            false,
        );
        c.curve(
            Point::new(ox + mx + mw / 2, y),
            Point::new(ox + rx - rw / 2, y),
            color,
            phase,
            route.flowing,
            false,
        );
    }
    for (i, client) in clients.iter().enumerate() {
        let y = ys(clients.len(), i);
        let r = Rect::new(ox + client_left, y - 28, cw, 56);
        c.rounded(r, 10, t.bg_card, Some(faded(t.border, client.opacity)));
        let color = match client.id {
            ClientId::WorkBuddy => Colour::rgb(119, 96, 206),
            ClientId::Tiangong => t.warn,
            _ => t.ink_secondary,
        };
        c.icon(
            Icon::client(client.id),
            Rect::new(r.x + 12, r.y + 17, 21, 21),
            faded(color, client.opacity),
        );
        c.label(
            client_short(client.id),
            Rect::new(r.x + 43, r.y + 7, cw - 57, 20),
            12,
            true,
            faded(t.ink_primary, client.opacity),
            false,
        );
        c.label(
            &client.state,
            Rect::new(r.x + 43, r.y + 29, cw - 57, 18),
            10,
            false,
            faded(t.ink_secondary, client.opacity),
            false,
        );
        c.dot(
            r.x + cw - 11,
            r.y + 12,
            2,
            faded(
                if client.busy { t.accent } else { t.ink_muted },
                client.opacity,
            ),
        );
        hits.clients.push(c.physical(r));
    }
    for (i, route) in routes.iter().enumerate() {
        let y = ys(routes.len(), i);
        let color = faded(t.accent, route.opacity);
        let r = Rect::new(ox + mx - mw / 2, y - 30, mw, 60);
        c.rounded(r, 10, t.bg_card, Some(color));
        c.label(
            &route.model,
            Rect::new(r.x + 12, r.y + 9, mw - 24, 20),
            13,
            true,
            faded(t.ink_primary, route.opacity),
            false,
        );
        c.label(
            &route.detail,
            Rect::new(r.x + 12, r.y + 34, mw - 24, 16),
            10,
            false,
            faded(t.ink_secondary, route.opacity),
            false,
        );
        hits.models.push(c.physical(r));
        let r = Rect::new(ox + rx - rw / 2, y - 30, rw, 60);
        c.rounded(r, 10, t.bg_card, Some(color));
        c.icon(
            Icon::Channel,
            Rect::new(r.x + 12, r.y + 20, 18, 18),
            faded(t.accent, route.opacity),
        );
        c.label(
            &route.channel,
            Rect::new(r.x + 40, r.y + 9, rw - 52, 20),
            12,
            true,
            faded(t.ink_primary, route.opacity),
            false,
        );
        c.label(
            if route.detail == "等待状态更新" {
                "等待更新"
            } else if route.detail == "等待响应" {
                "等待响应"
            } else if route.active {
                "处理中"
            } else {
                "已结束"
            },
            Rect::new(r.x + 40, r.y + 34, rw - 52, 16),
            10,
            false,
            faded(
                if route.active {
                    t.accent
                } else {
                    t.ink_secondary
                },
                route.opacity,
            ),
            false,
        );
        hits.channels.push(c.physical(r));
    }
    if !mcps.is_empty() {
        let left = ox + hx + 20;
        let card_width = ((w - hx - 42 - 16) / 2).min(232);
        c.text("MCP", left, oy + main_h, 12, true, t.ink_secondary);
        for (i, mcp) in mcps.iter().enumerate() {
            let r = Rect::new(
                left + i as i32 % 2 * (card_width + 16),
                oy + main_h + 26 + i as i32 / 2 * 66,
                card_width,
                54,
            );
            let color = faded(if mcp.flowing { t.accent } else { t.border }, mcp.opacity);
            // The vertical trunk runs in the gap beside the model column;
            // each branch then enters the MCP row below the model routes.
            let junction = Point::new(ox + hx, r.y - 12);
            c.curve(
                Point::new(ox + hx, hub_rect.y + hs),
                junction,
                color,
                phase,
                mcp.flowing,
                false,
            );
            c.curve(
                junction,
                Point::new(r.x + r.width / 2, r.y),
                color,
                phase,
                mcp.flowing,
                false,
            );
            for model in &mcp.models {
                if let Some((index, route)) = routes
                    .iter()
                    .enumerate()
                    .find(|(_, route)| route.route == model.route && route.model == model.model)
                {
                    let trunk_x = ox + mx - mw / 2 - 18;
                    let junction = Point::new(trunk_x, r.y - 12);
                    let target = Point::new(trunk_x, ys(routes.len(), index) + 18);
                    let color = faded(t.accent, mcp.opacity.min(route.opacity));
                    let flowing = model.flowing && mcp.flowing;
                    c.curve(
                        Point::new(r.x + r.width / 2, r.y),
                        junction,
                        color,
                        phase,
                        flowing,
                        false,
                    );
                    c.curve(junction, target, color, phase, flowing, false);
                    c.curve(
                        target,
                        Point::new(ox + mx - mw / 2, target.y),
                        color,
                        phase,
                        flowing,
                        false,
                    );
                }
            }
            c.rounded(r, 10, t.bg_card, Some(color));
            c.icon(
                Icon::Mcp,
                Rect::new(r.x + 12, r.y + 19, 20, 20),
                faded(t.accent, mcp.opacity),
            );
            c.label(
                &mcp.name,
                Rect::new(r.x + 42, r.y + 8, r.width - 52, 20),
                12,
                true,
                faded(t.ink_primary, mcp.opacity),
                false,
            );
            c.label(
                &mcp.detail,
                Rect::new(r.x + 42, r.y + 32, r.width - 52, 17),
                10,
                false,
                faded(t.ink_secondary, mcp.opacity),
                false,
            );
            hits.mcps.push(c.physical(r));
        }
    }
    c.rounded(
        Rect::new(hub_rect.x - 6, hub_rect.y - 6, hs + 12, hs + 12),
        26,
        t.bg_card,
        Some(t.bg_muted),
    );
    c.rounded(hub_rect, 22, t.bg_card, Some(t.border));
    let compact = hs < 128;
    let icon = Rect::new(
        ox + hx - 19,
        hub_rect.y + if compact { 10 } else { 17 },
        38,
        38,
    );
    c.rounded(icon, 12, t.accent, None);
    c.icon(
        Icon::Hub,
        Rect::new(icon.x + 4, icon.y + 2, 30, 32),
        t.on_accent,
    );
    c.label(
        "Hub",
        Rect::new(
            hub_rect.x,
            hub_rect.y + if compact { 53 } else { 64 },
            hs,
            24,
        ),
        17,
        true,
        t.ink_primary,
        true,
    );
    c.label(
        match hub {
            Some(true) => "本机服务运行中",
            Some(false) => "本机服务已停止",
            None => "正在读取状态",
        },
        Rect::new(
            hub_rect.x + 5,
            hub_rect.y + if compact { 82 } else { 94 },
            hs - 10,
            16,
        ),
        10,
        false,
        if hub == Some(true) { t.ok } else { t.ink_muted },
        true,
    );
    hits
}

pub(super) struct ClientDiagram<'a> {
    pub id: ClientId,
    pub opened: &'a str,
    pub execution: &'a str,
    pub model: &'a str,
    pub channel: &'a str,
    pub skill: &'a str,
    pub mcp: &'a str,
    pub chat: &'a str,
    pub states: [Option<bool>; 6],
    pub hub: Option<bool>,
    pub flowing: bool,
}
pub(super) fn draw_client(
    c: &Canvas<'_>,
    size: Size,
    v: &ClientDiagram<'_>,
    phase: f64,
) -> Vec<(Rect, usize)> {
    let t = theme::theme();
    c.clear(t.bg_app);
    let w = size.width.min(1200);
    let ox = (size.width - w) / 2;
    let h = size.height.min(510);
    c.text("状态概览", ox + 6, 20, 16, true, t.ink_primary);
    let xs = [ox + w * 17 / 100, ox + w / 2, ox + w * 83 / 100];
    let label_width = (w / 3 - 8).min(220);
    let half_label = label_width / 2;
    let top = 140;
    let bottom = (h - 105).max(320);
    let sz = 60;
    let active = if v.flowing { t.accent } else { t.border };
    c.curve(
        Point::new(xs[0] + 30, top),
        Point::new(xs[1] - 30, top),
        active,
        phase,
        v.flowing,
        true,
    );
    c.curve(
        Point::new(xs[1] + 30, top),
        Point::new(xs[2] - 30, top),
        active,
        phase,
        v.flowing,
        true,
    );
    c.curve(
        Point::new(xs[1], top + 30),
        Point::new(xs[1], bottom - 30),
        t.border,
        0.,
        false,
        true,
    );
    let mut hits = Vec::new();
    let mut draw = |x: i32,
                    y: i32,
                    icon: Icon,
                    title: &str,
                    status: &str,
                    state: Option<bool>,
                    idx: usize,
                    brand: bool| {
        let r = Rect::new(x - sz / 2, y - sz / 2, sz, sz);
        c.rounded(
            r,
            19,
            if brand { t.accent } else { t.bg_card },
            if brand { None } else { Some(t.border) },
        );
        c.icon(
            icon,
            Rect::new(x - 15, y - 17, 30, 34),
            if brand { t.on_accent } else { t.accent },
        );
        let dot = match state {
            Some(true) => t.ok,
            Some(false) => t.ink_muted,
            None => t.warn,
        };
        c.dot(x + 27, y + 27, 5, t.bg_app);
        c.dot(x + 27, y + 27, 3, dot);
        c.label(
            title,
            Rect::new(x - half_label, y + 41, label_width, 24),
            13,
            true,
            t.ink_primary,
            true,
        );
        c.label(
            status,
            Rect::new(x - half_label, y + 70, label_width, 20),
            11,
            false,
            t.ink_secondary,
            true,
        );
        if idx != usize::MAX {
            hits.push((
                c.physical(Rect::new(x - half_label, y - 35, label_width, 130)),
                idx,
            ));
        }
    };
    draw(
        xs[0],
        top,
        Icon::client(v.id),
        client_short(v.id),
        v.opened,
        v.states[0],
        0,
        false,
    );
    draw(
        xs[1],
        top,
        Icon::Hub,
        "天才 Hub",
        match v.hub {
            Some(true) => "本机服务运行中",
            Some(false) => "本机服务已停止",
            None => "正在读取状态",
        },
        v.hub,
        usize::MAX,
        true,
    );
    draw(
        xs[2],
        top,
        Icon::Model,
        "模型",
        v.model,
        v.states[1],
        1,
        false,
    );
    draw(
        xs[1],
        bottom,
        Icon::Chat,
        "聊天与执行",
        v.chat,
        v.states[5],
        5,
        false,
    );
    let channel = Rect::new(xs[2] - half_label, top + 101, label_width, 26);
    c.label(v.channel, channel, 10, false, t.accent, true);
    hits.push((c.physical(channel), 2));
    c.label(
        v.execution,
        Rect::new(xs[0] - half_label, top + 101, label_width, 26),
        10,
        false,
        t.ink_secondary,
        true,
    );
    c.line(
        Point::new(ox + 6, h - 3),
        Point::new(ox + w - 6, h - 3),
        t.divider,
        false,
    );
    hits
}

pub(super) fn draw_brand(c: &Canvas<'_>, x: i32, y: i32) {
    let t = theme::theme();
    // Keep the existing product artwork, matching the executable, installer
    // and tray. This is a brand image, so never tint it to the theme accent.
    let size = c.px(26).max(1);
    HUB_MARKS.with(|marks| {
        let mut marks = marks.borrow_mut();
        if !marks.contains_key(&size) {
            if let Ok(image) = image::load_from_memory(include_bytes!(
                "../../packaging/icons/dolphin-rounded-256.png"
            )) {
                let rgba = image
                    .resize_exact(
                        size as u32,
                        size as u32,
                        image::imageops::FilterType::Lanczos3,
                    )
                    .to_rgba8();
                if let Some(bitmap) = Bitmap::from_rgba(rgba.as_raw(), size as u32, size as u32) {
                    marks.insert(size, bitmap);
                }
            }
        }
        if let Some(bitmap) = marks.get(&size) {
            c.dc.draw_bitmap(bitmap, c.px(x), c.px(y), true);
        }
    });
    c.label(
        "TianCai Hub",
        Rect::new(x + 37, y, 126, 26),
        13,
        true,
        t.ink_secondary,
        false,
    );
}

/// Nearest recorded bucket, shared by hover and click; coordinates follow
/// exactly the same endpoints as the rendered chart at every DPI.
pub(super) fn usage_chart_index(
    size: Size,
    point: Point,
    scale: f64,
    count: usize,
) -> Option<usize> {
    if !scale.is_finite() || scale <= 0. {
        return None;
    }
    // Paint receives an integer logical size and rounds back to physical
    // pixels. At 125%, for example, x=54 lands between two physical pixels;
    // accepting that one-pixel boundary prevents losing the endpoint hover.
    let logical = Size::new(
        (size.width as f64 / scale) as i32,
        (size.height as f64 / scale) as i32,
    );
    let w = (logical.width - 84) as f64;
    let px = |n: f64| (n * scale).round() as i32;
    if count == 0
        || w <= 0.
        || point.x < px(54.) - 1
        || point.x > px(54. + w) + 1
        || point.y < px(60.) - 1
        || point.y > px((logical.height - 48) as f64) + 1
    {
        return None;
    }
    let x = (point.x as f64 / scale).clamp(54., 54. + w);
    Some(
        if count == 1 {
            0
        } else {
            ((x - 54.) / w * (count - 1) as f64).round() as usize
        }
        .min(count - 1),
    )
}

#[cfg(test)]
mod hover_tests {
    use super::*;
    #[test]
    fn bucket_hit_test_matches_rendered_points_and_dpi() {
        for factor in [1.0_f64, 1.25, 1.5, 2.] {
            let size = Size::new((884. * factor) as i32, (308. * factor) as i32);
            for (x, index) in [(54., 0), (454., 3), (854., 6)] {
                // Both adjacent physical pixels must hit at fractional DPI.
                for physical_x in [(x * factor) as i32, (x * factor).round() as i32] {
                    assert_eq!(
                        usage_chart_index(
                            size,
                            Point::new(physical_x, (120. * factor) as i32),
                            factor,
                            7
                        ),
                        Some(index)
                    );
                }
            }
            assert_eq!(
                usage_chart_index(
                    size,
                    Point::new((50. * factor) as i32, (120. * factor) as i32),
                    factor,
                    7
                ),
                None
            );
            assert_eq!(
                usage_chart_index(
                    size,
                    Point::new((454. * factor) as i32, (28. * factor) as i32),
                    factor,
                    7
                ),
                None
            );
            assert_eq!(
                usage_chart_index(
                    size,
                    Point::new((454. * factor) as i32, (120. * factor) as i32),
                    factor,
                    0
                ),
                None
            );
        }
    }
}

pub(super) fn draw_usage_hover(
    c: &Canvas<'_>,
    size: Size,
    rows: &[(String, Option<i64>, Option<i64>)],
    index: usize,
    label: &str,
    requests: i64,
) {
    let Some(row) = rows.get(index) else {
        return;
    };
    let t = theme::theme();
    let w = (size.width - 84).max(1);
    let x = 54
        + if rows.len() > 1 {
            index as i32 * w / (rows.len() - 1) as i32
        } else {
            w / 2
        };
    c.line(
        Point::new(x, 60),
        Point::new(x, size.height - 48),
        t.ink_muted,
        true,
    );
    let max = rows
        .iter()
        .flat_map(|r| [r.1, r.2])
        .flatten()
        .max()
        .unwrap_or(1)
        .max(1) as f64;
    for (value, color) in [
        (row.1, t.accent),
        (
            row.2,
            if t.is_dark {
                Colour::rgb(175, 158, 242)
            } else {
                Colour::rgb(124, 102, 202)
            },
        ),
    ] {
        if let Some(value) = value {
            let y = 60 + (size.height - 108).max(1)
                - (value as f64 / max * (size.height - 108).max(1) as f64) as i32;
            c.dot(x, y, 5, color);
            c.dot(x, y, 2, t.bg_card);
        }
    }
    let width = 200.min(size.width - 20);
    let bx = if x + width + 18 < size.width {
        x + 14
    } else {
        (x - width - 14).max(10)
    };
    let r = Rect::new(bx, 52, width, 112);
    c.rounded(
        Rect::new(r.x + 2, r.y + 3, r.width, r.height),
        10,
        t.bg_muted,
        None,
    );
    c.rounded(r, 10, t.bg_card, Some(t.divider));
    c.text(label, r.x + 12, r.y + 10, 12, true, t.ink_primary);
    let value = |v: Option<i64>| v.map(|n| n.to_string()).unwrap_or("未报告".into());
    for (i, text) in [
        format!("请求    {requests}"),
        format!("输入    {}", value(row.1)),
        format!("输出    {}", value(row.2)),
    ]
    .iter()
    .enumerate()
    {
        c.text(
            text,
            r.x + 12,
            r.y + 34 + i as i32 * 22,
            12,
            false,
            t.ink_secondary,
        );
    }
}

pub(super) fn draw_usage_chart(
    c: &Canvas<'_>,
    size: Size,
    rows: &[(String, Option<i64>, Option<i64>)],
) {
    let t = theme::theme();
    c.clear(t.bg_app);
    c.rounded(
        Rect::new(0, 0, size.width, size.height),
        14,
        t.bg_card,
        None,
    );
    c.text("用量趋势", 20, 18, 15, true, t.ink_primary);
    let output = if t.is_dark {
        Colour::rgb(175, 158, 242)
    } else {
        Colour::rgb(124, 102, 202)
    };
    c.dot(size.width - 132, 28, 3, t.accent);
    c.text("输入", size.width - 122, 21, 11, false, t.ink_secondary);
    c.dot(size.width - 68, 28, 3, output);
    c.text("输出", size.width - 58, 21, 11, false, t.ink_secondary);
    if rows.is_empty() {
        c.label(
            "暂无已记录的请求",
            Rect::new(0, size.height / 2 - 10, size.width, 30),
            13,
            false,
            t.ink_muted,
            true,
        );
        return;
    }
    let compact = |n: i64| {
        if n >= 1_000_000 {
            format!("{:.1}M", n as f64 / 1_000_000.)
        } else if n >= 1000 {
            format!("{:.0}K", n as f64 / 1000.)
        } else {
            n.to_string()
        }
    };
    let max = rows
        .iter()
        .flat_map(|r| [r.1, r.2])
        .flatten()
        .max()
        .unwrap_or(1)
        .max(1) as f64;
    let (left, top, w, h) = (54, 60, (size.width - 84).max(1), (size.height - 108).max(1));
    for i in 0..=4 {
        let y = top + h * i / 4;
        c.line(
            Point::new(left, y),
            Point::new(left + w, y),
            t.divider,
            false,
        );
        c.label(
            &compact((max * (4 - i) as f64 / 4.) as i64),
            Rect::new(4, y - 8, 42, 16),
            10,
            false,
            t.ink_secondary,
            false,
        );
    }
    for (is_output, color) in [(false, t.accent), (true, output)] {
        let mut last = None;
        for (i, row) in rows.iter().enumerate() {
            let x = left
                + if rows.len() > 1 {
                    i as i32 * w / (rows.len() - 1) as i32
                } else {
                    w / 2
                };
            if let Some(value) = if is_output { row.2 } else { row.1 } {
                let y = top + h - (value as f64 / max * h as f64) as i32;
                if let Some((px, py)) = last {
                    // Horizontal Bezier tangents produce a smooth curve that
                    // stays between recorded endpoints. Missing usage breaks
                    // the path; it is never interpolated as a fabricated zero.
                    c.curve(
                        Point::new(px, py),
                        Point::new(x, y),
                        color,
                        0.,
                        false,
                        false,
                    );
                }
                c.dot(x, y, 3, color);
                last = Some((x, y));
            } else {
                last = None;
            }
        }
    }
    let ticks = rows.len().min(5);
    for tick in 0..ticks {
        let i = if ticks > 1 {
            tick * (rows.len() - 1) / (ticks - 1)
        } else {
            0
        };
        let x = left
            + if rows.len() > 1 {
                i as i32 * w / (rows.len() - 1) as i32
            } else {
                w / 2
            };
        c.label(
            &rows[i].0,
            Rect::new(
                (x - 45).clamp(10, (size.width - 100).max(10)),
                top + h + 12,
                90,
                20,
            ),
            10,
            false,
            t.ink_secondary,
            true,
        );
    }
}
