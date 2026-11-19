//! Sibling native buttons share one moving surface; the timer only runs
//! while that surface is travelling. No duplicate buttons or input hooks.
use super::{
    rail, theme,
    visual::{self, Canvas},
};
use std::{cell::Cell, rc::Rc, time::Instant};
use wxdragon::{dc::GraphicsDC, prelude::*, timer::Timer, widgets::action_button::ActionButton};

struct State<const N: usize> {
    parent: Panel,
    keys: [ActionButton; N],
    active: Cell<usize>,
    from: Cell<Option<(Rect, Instant)>>,
    chevron: bool,
    timer: Timer<Panel>,
}
impl<const N: usize> State<N> {
    fn rect(&self) -> Rect {
        let b = self.keys[self.active.get()];
        let p = b.get_position();
        let size = b.get_client_size();
        let inset = b.from_dip(2);
        Rect::new(
            p.x + inset,
            p.y + inset,
            (size.width - 2 * inset).max(1),
            (size.height - 2 * inset).max(1),
        )
    }
    fn marker(&self, now: Instant) -> Rect {
        let end = self.rect();
        if let Some((from, start)) = self.from.get() {
            rail::glide(from, end, ((now - start).as_secs_f64() / 0.215).min(1.))
        } else {
            end
        }
    }
    fn redraw(&self) {
        for b in self.keys {
            if b.is_shown() {
                b.refresh(false, None)
            }
        }
    }
    fn paint(&self, index: usize) {
        let b = self.keys[index];
        let t = theme::theme();
        let s = visual::scale(&b);
        let base = AutoBufferedPaintDC::new(&b);
        let dc = GraphicsDC::new(&base);
        let c = Canvas::new(&dc, s);
        c.clear(self.parent.get_background_color());
        let size = b.get_client_size();
        let p = b.get_position();
        let size_dip = |v: i32| (v as f64 / s).round() as i32;
        let w = size_dip(size.width);
        let h = size_dip(size.height);
        let state = b.visual_state();
        if state & 3 != 0 {
            c.rounded(
                Rect::new(2, 2, (w - 4).max(1), (h - 4).max(1)),
                8,
                t.bg_card_alt,
                None,
            )
        }
        let m = self.marker(Instant::now());
        if m.x < p.x + size.width && m.x + m.width > p.x {
            c.rounded(
                Rect::new(
                    size_dip(m.x - p.x),
                    size_dip(m.y - p.y),
                    size_dip(m.width).max(1),
                    size_dip(m.height).max(1),
                ),
                8,
                t.bg_muted,
                None,
            );
        }
        let colour = if index == self.active.get() {
            t.accent
        } else {
            t.ink_secondary
        };
        c.label(
            &b.get_label().unwrap_or_default(),
            Rect::new(3, 0, (w - if self.chevron { 19 } else { 6 }).max(1), h),
            13,
            false,
            colour,
            true,
        );
        if self.chevron {
            let x = w - 14;
            let y = h / 2;
            c.line(
                Point::new(x - 3, y - 1),
                Point::new(x, y + 2),
                colour,
                false,
            );
            c.line(
                Point::new(x, y + 2),
                Point::new(x + 3, y - 1),
                colour,
                false,
            )
        }
        if state & 4 != 0 {
            c.rounded(Rect::new(w / 2 - 5, h - 6, 10, 2), 1, t.accent, None);
        }
    }
}
#[derive(Clone)]
pub(in crate::gui) struct SlidingStrip<const N: usize>(Rc<State<N>>);
impl<const N: usize> SlidingStrip<N> {
    pub(in crate::gui) fn new(
        parent: Panel,
        items: [(&str, i32); N],
        height: i32,
        chevron: bool,
    ) -> Self {
        let keys = items.map(|(name, width)| {
            let b = ActionButton::new(&parent, ID_ANY as i32, name);
            b.set_name(name);
            b.set_min_size(visual::dip_size(&b, width, height));
            b
        });
        let owner = Panel::builder(&parent).build();
        owner.hide();
        let timer = Timer::new(&owner);
        let state = Rc::new(State {
            parent,
            keys,
            active: Cell::new(0),
            from: Cell::new(None),
            chevron,
            timer,
        });
        let weak = Rc::downgrade(&state);
        state.timer.on_tick(move |_| {
            if let Some(s) = weak.upgrade() {
                if !theme::motion_enabled()
                    || s.from
                        .get()
                        .is_none_or(|(_, start)| start.elapsed().as_millis() >= 215)
                {
                    s.from.set(None);
                    s.timer.stop();
                }
                s.redraw();
            }
        });
        for (i, b) in keys.iter().enumerate() {
            let weak = Rc::downgrade(&state);
            b.on_paint(move |_| {
                if let Some(s) = weak.upgrade() {
                    s.paint(i)
                }
            });
        }
        let id = owner.handle_ptr() as usize;
        let weak = Rc::downgrade(&state);
        owner.on_destroy(move|e|{if matches!(&e,WindowEventData::General(ev) if ev.get_event_object().is_some_and(|w|w.as_ptr() as usize==id))&&let Some(s)=weak.upgrade(){s.timer.stop();}e.skip(true)});
        Self(state)
    }
    pub(in crate::gui) fn keys(&self) -> [ActionButton; N] {
        self.0.keys
    }
    pub(in crate::gui) fn select(&self, index: usize) {
        let s = &self.0;
        if s.active.get() == index {
            return;
        }
        let now = Instant::now();
        let start = s.marker(now);
        s.active.set(index);
        s.from.set(theme::motion_enabled().then_some((start, now)));
        if s.from.get().is_some() {
            s.timer.start(16, false);
        }
        s.redraw();
    }
}
