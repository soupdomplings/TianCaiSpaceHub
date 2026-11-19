//! The same moving rectangle is clipped into each native row. Labels, hit
//! targets, Tab/Space handling and accessible button objects never move with it.
use super::{
    PageId, theme,
    visual::{self, Canvas, Icon},
};
use crate::desktop::clients::ClientId;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Instant,
};
use wxdragon::{dc::GraphicsDC, prelude::*, timer::Timer, widgets::action_button::ActionButton};

const HEAD: i32 = 31;
const LEAF: i32 = 35;
const GROUPS: [&str; 4] = ["工作空间", "客户端", "共用能力", "更多"];
pub(super) fn group(id: PageId) -> usize {
    match id {
        PageId::Overview | PageId::Sources | PageId::Squad => 0,
        PageId::Client(_) => 1,
        PageId::Settings | PageId::Help => 3,
        _ => 2,
    }
}
pub(super) fn group_name(id: PageId) -> &'static str {
    GROUPS[group(id)]
}
pub(super) fn entries() -> Vec<(PageId, Icon)> {
    let mut rows = vec![
        (PageId::Overview, Icon::Overview),
        (PageId::Sources, Icon::Model),
        (PageId::Squad, Icon::Squad),
    ];
    rows.extend(
        ClientId::VISIBLE
            .into_iter()
            .map(|id| (PageId::Client(id), Icon::client(id))),
    );
    rows.extend([
        (PageId::Logs, Icon::Logs),
        (PageId::Assistant, Icon::Chat),
        (PageId::Dumpling, Icon::Mcp),
        (PageId::Chat, Icon::Chat),
        (PageId::Remote, Icon::Hub),
        (PageId::Settings, Icon::Settings),
        (PageId::Help, Icon::Help),
    ]);
    rows
}
#[derive(Clone, Copy)]
struct Row {
    button: ActionButton,
    id: Option<PageId>,
    icon: Icon,
    group: usize,
    order: usize,
}
struct State {
    host: ScrolledWindow,
    rows: Vec<Row>,
    open: [Cell<bool>; 4],
    selected: Cell<PageId>,
    dumpling: Cell<bool>,
    // Start rectangles are sampled from the on-screen track, even on a rapid reversal.
    flight: Cell<Option<(Rect, Instant)>>,
    tracks: RefCell<Vec<(usize, i32, i32, Instant, f64)>>,
    timer: Timer<Panel>,
}
fn progress(start: Instant, now: Instant, delay: f64, duration: f64) -> f64 {
    (((now - start).as_secs_f64() * 1000. - delay) / duration).clamp(0., 1.)
}
fn lerp(a: i32, b: i32, t: f64) -> i32 {
    (f64::from(a) + f64::from(b - a) * (1. - (1. - t).powi(3))).round() as i32
}
pub(super) fn glide(a: Rect, b: Rect, t: f64) -> Rect {
    Rect::new(
        lerp(a.x, b.x, t),
        lerp(a.y, b.y, t),
        lerp(a.width, b.width, t),
        lerp(a.height, b.height, t),
    )
}
impl State {
    fn target(&self) -> Option<Row> {
        let id = self.selected.get();
        let g = group(id);
        self.rows
            .iter()
            .find(|r| r.group == g && r.id == if self.open[g].get() { Some(id) } else { None })
            .copied()
    }
    fn rect(&self, row: Row) -> Rect {
        let b = row.button;
        let p = b.get_position();
        let sz = b.get_client_size();
        let scroll = self.host.view_start();
        let (sx, sy) = self.host.get_scroll_pixels_per_unit();
        Rect::new(
            p.x + scroll.x * sx,
            p.y + scroll.y * sy,
            sz.width,
            sz.height,
        )
    }
    fn marker(&self, now: Instant) -> Option<Rect> {
        let row = self.target()?;
        if !row.button.is_shown() {
            return None;
        }
        let mut end = self.rect(row);
        let x = self.host.from_dip(if row.id.is_some() { 8 } else { 5 });
        let y = self.host.from_dip(2);
        end.x += x;
        end.width = (end.width - 2 * x).max(1);
        end.y += y;
        end.height = (end.height - 2 * y).max(1);
        Some(if let Some((from, start)) = self.flight.get() {
            glide(from, end, progress(start, now, 0., 215.))
        } else {
            end
        })
    }
    fn paint(&self, index: usize) {
        let row = self.rows[index];
        let b = row.button;
        let t = theme::theme();
        let base = AutoBufferedPaintDC::new(&b);
        let dc = GraphicsDC::new(&base);
        let s = visual::scale(&b);
        let c = Canvas::new(&dc, s);
        let dip = |v: i32| (v as f64 / s).round() as i32;
        let rect = self.rect(row);
        let w = dip(rect.width);
        let nominal = if row.id.is_some() { LEAF } else { HEAD };
        c.clear(t.bg_card);
        let state = b.visual_state();
        if state & 3 != 0 {
            c.rounded(
                Rect::new(7, 2, (w - 14).max(1), nominal - 4),
                8,
                t.bg_card_alt,
                None,
            )
        }
        if let Some(m) = self.marker(Instant::now())
            && m.y < rect.y + rect.height
            && m.y + m.height > rect.y
        {
            c.rounded(
                Rect::new(
                    dip(m.x - rect.x),
                    dip(m.y - rect.y),
                    dip(m.width).max(1),
                    dip(m.height).max(1),
                ),
                8,
                t.bg_muted,
                None,
            );
        }
        if state & 4 != 0 {
            // A keyboard focus mark remains separate from the selected page.
            c.rounded(Rect::new(9, nominal / 2 - 4, 2, 8), 1, t.accent, None);
        }
        let current = self
            .target()
            .is_some_and(|r| r.button.handle_ptr() == b.handle_ptr());
        if let Some(id) = row.id {
            let colour = if current { t.accent } else { t.ink_secondary };
            c.icon(row.icon, Rect::new(19, (LEAF - 16) / 2, 16, 16), colour);
            let label = match id {
                PageId::Client(id) => visual::client_short(id),
                _ => id.title(),
            };
            c.label(
                label,
                Rect::new(47, 0, (w - 58).max(1), LEAF),
                13,
                false,
                if current { t.accent } else { t.ink_primary },
                false,
            );
        } else {
            c.label(
                GROUPS[row.group],
                Rect::new(14, 0, (w - 44).max(1), HEAD),
                12,
                false,
                t.ink_secondary,
                false,
            );
            let x = w - 22;
            let y = HEAD / 2;
            let (a, z) = if self.open[row.group].get() {
                (Point::new(x - 4, y - 2), Point::new(x + 4, y - 2))
            } else {
                (Point::new(x - 2, y - 4), Point::new(x - 2, y + 4))
            };
            let mid = if self.open[row.group].get() {
                Point::new(x, y + 2)
            } else {
                Point::new(x + 2, y)
            };
            c.line(a, mid, t.ink_muted, false);
            c.line(mid, z, t.ink_muted, false);
        }
    }
    fn redraw(&self) {
        for r in &self.rows {
            if r.button.is_shown() {
                r.button.refresh(false, None)
            }
        }
    }
    fn pulse(&self) {
        if !self.host.is_valid() {
            self.timer.stop();
            return;
        }
        let now = Instant::now();
        let reduced = !theme::motion_enabled();
        let mut changed = false;
        self.tracks
            .borrow_mut()
            .retain(|&(idx, a, b, start, delay)| {
                let t = if reduced {
                    1.
                } else {
                    progress(start, now, delay, 155.)
                };
                let h = lerp(a, b, t);
                let button = self.rows[idx].button;
                if button.get_min_size().height != h || button.is_shown() != (h > 0) {
                    button.set_min_size(Size::new(-1, h));
                    button.show(h > 0);
                    changed = true;
                }
                t < 1.
            });
        if changed {
            self.host.layout();
            self.host.fit_inside()
        }
        if self
            .flight
            .get()
            .is_some_and(|(_, s)| reduced || progress(s, now, 0., 215.) >= 1.)
        {
            self.flight.set(None)
        }
        self.redraw();
        if self.flight.get().is_none() && self.tracks.borrow().is_empty() {
            self.timer.stop();
        }
    }
    fn select(&self, id: PageId) {
        let now = Instant::now();
        let from = self.marker(now);
        let changed = self.selected.replace(id) != id;
        if !self.open[group(id)].get() {
            self.fold(group(id), true)
        }
        if changed {
            self.launch(from, now)
        }
    }
    fn launch(&self, from: Option<Rect>, now: Instant) {
        self.flight
            .set(from.filter(|_| theme::motion_enabled()).map(|r| (r, now)));
        self.pulse();
        if !self.tracks.borrow().is_empty() || self.flight.get().is_some() {
            self.timer.start(16, false);
        }
    }
    fn fold(&self, g: usize, expand: bool) {
        let now = Instant::now();
        let from = self.marker(now);
        if self.open[g].replace(expand) == expand {
            return;
        }
        let mut runs = self.tracks.borrow_mut();
        runs.retain(|(i, ..)| self.rows[*i].group != g);
        for (i, r) in self.rows.iter().enumerate().filter(|(_, r)| r.group == g) {
            if r.id.is_none() {
                r.button.set_name(&format!(
                    "{}，{}",
                    GROUPS[g],
                    if expand { "已展开" } else { "已收起" }
                ));
                continue;
            }
            if r.id == Some(PageId::Dumpling) && !self.dumpling.get() {
                continue;
            }
            let start = if r.button.is_shown() {
                r.button.get_min_size().height
            } else {
                0
            };
            r.button.enable(expand);
            let end = if expand { self.host.from_dip(LEAF) } else { 0 };
            runs.push((
                i,
                start,
                end,
                now,
                if expand { r.order as f64 * 12. } else { 0. },
            ));
        }
        drop(runs);
        self.launch(from, now);
    }
}
#[derive(Clone)]
pub(super) struct Rail(Rc<State>);
impl Rail {
    pub(super) fn new(host: ScrolledWindow) -> Self {
        let side = BoxSizer::builder(Orientation::Vertical).build();
        let mut rows = Vec::new();
        let leafs = entries();
        for (g, name) in GROUPS.iter().enumerate() {
            let head = ActionButton::new(&host, ID_ANY as i32, name);
            head.set_min_size(Size::new(-1, host.from_dip(HEAD)));
            head.set_name(&format!("{name}，已展开"));
            head.set_tooltip(&format!("展开或收起{name}"));
            side.add(&head, 0, SizerFlag::Expand, 0);
            rows.push(Row {
                button: head,
                id: None,
                icon: Icon::None,
                group: g,
                order: 0,
            });
            for (order, &(id, icon)) in leafs.iter().filter(|(id, _)| group(*id) == g).enumerate() {
                let b = ActionButton::new(&host, ID_ANY as i32, id.title());
                b.set_name(id.title());
                b.set_min_size(Size::new(-1, host.from_dip(LEAF)));
                side.add(&b, 0, SizerFlag::Expand, 0);
                if id == PageId::Dumpling {
                    b.hide()
                }
                rows.push(Row {
                    button: b,
                    id: Some(id),
                    icon,
                    group: g,
                    order,
                });
            }
        }
        host.set_sizer(side, true);
        let owner = Panel::builder(&host).build();
        owner.hide();
        let timer = Timer::new(&owner);
        let s = Rc::new(State {
            host,
            rows,
            open: std::array::from_fn(|_| Cell::new(true)),
            selected: Cell::new(PageId::Overview),
            dumpling: Cell::new(false),
            flight: Cell::new(None),
            tracks: Default::default(),
            timer,
        });
        for (i, row) in s.rows.iter().enumerate() {
            let weak = Rc::downgrade(&s);
            row.button.on_paint(move |_| {
                if let Some(s) = weak.upgrade() {
                    s.paint(i)
                }
            });
            if row.id.is_none() {
                let weak = Rc::downgrade(&s);
                let g = row.group;
                row.button.on_click(move |_| {
                    if let Some(s) = weak.upgrade() {
                        s.fold(g, !s.open[g].get())
                    }
                });
            }
        }
        let weak = Rc::downgrade(&s);
        s.timer.on_tick(move |_| {
            if let Some(s) = weak.upgrade() {
                s.pulse()
            }
        });
        let weak = Rc::downgrade(&s);
        let owner_id = owner.handle_ptr() as usize;
        owner.on_destroy(move |e|{if matches!(&e,WindowEventData::General(ev) if ev.get_event_object().is_some_and(|w|w.as_ptr() as usize==owner_id)) && let Some(s)=weak.upgrade(){s.timer.stop();}e.skip(true)});
        Self(s)
    }
    pub(super) fn bind(&self, cb: Rc<dyn Fn(PageId)>) {
        for r in &self.0.rows {
            if let Some(id) = r.id {
                let cb = cb.clone();
                r.button.on_click(move |_| cb(id));
            }
        }
    }
    pub(super) fn select(&self, id: PageId) {
        self.0.select(id)
    }
    pub(super) fn set_dumpling(&self, visible: bool) {
        if self.0.dumpling.replace(visible) == visible {
            return;
        }
        if let Some((i, r)) = self
            .0
            .rows
            .iter()
            .enumerate()
            .find(|(_, r)| r.id == Some(PageId::Dumpling))
        {
            self.0.tracks.borrow_mut().retain(|(n, ..)| *n != i);
            let active = self.0.open[r.group].get();
            r.button.enable(active);
            r.button
                .set_min_size(Size::new(-1, self.0.host.from_dip(LEAF)));
            r.button.show(visible && active);
            self.0.host.layout();
            self.0.host.fit_inside();
            self.0.redraw();
        }
    }
    pub(super) fn dumpling(&self) -> bool {
        self.0.dumpling.get()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_selection_stays_at_intermediate_rect_on_reverse() {
        let a = Rect::new(4, 2, 170, 28);
        let b = Rect::new(8, 125, 160, 31);
        let mid = glide(a, b, 0.33);
        assert!(mid.y > a.y && mid.y < b.y);
        assert_eq!(glide(mid, a, 0.), mid);
        assert_eq!(glide(mid, a, 1.), a);
    }
}
