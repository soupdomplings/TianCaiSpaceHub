//! Lightweight, borderless model navigation. Native buttons retain keyboard
//! focus and accessibility; selection changes are separate from data refreshes.
use super::{
    controls::{Button, ButtonKind},
    theme, visual,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wxdragon::prelude::*;

type Changed = Rc<dyn Fn(())>;
#[derive(Clone)]
pub(super) struct ModelList {
    page: ScrolledWindow,
    rows: Rc<RefCell<Vec<Button>>>,
    selected: Rc<Cell<Option<u32>>>,
    changed: Rc<RefCell<Option<Changed>>>,
}
impl ModelList {
    pub fn new(parent: &dyn WxWidget) -> Self {
        let page = ScrolledWindow::builder(parent)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        page.set_background_color(theme::theme().bg_app);
        page.set_scroll_rate(0, page.from_dip(10));
        page.show_scroll_indicators(false);
        page.set_min_size(visual::dip_size(&page, 170, 120));
        page.set_max_size(visual::dip_size(&page, 206, -1));
        page.set_sizer(BoxSizer::builder(Orientation::Vertical).build(), true);
        Self {
            page,
            rows: Default::default(),
            selected: Default::default(),
            changed: Default::default(),
        }
    }
    pub fn clear(&self) {
        self.selected.set(None);
        for row in self.rows.borrow_mut().drain(..) {
            row.destroy();
        }
        self.page.layout();
        self.page.fit_inside();
    }
    pub fn append(&self, label: &str) {
        let index = self.rows.borrow().len() as u32;
        let b = Button::builder(&self.page)
            .with_label(label)
            .with_kind(ButtonKind::Nav)
            .with_size(Size::new(170, 38))
            .build();
        b.set_tooltip(label);
        b.set_name(&format!("client-model-{index}"));
        let list = self.clone();
        b.on_click(move |_| {
            list.set_selection(index, true);
            let changed = list.changed.borrow().clone();
            if let Some(changed) = changed {
                changed(());
            }
        });
        if let Some(s) = self.page.get_sizer() {
            s.add(
                &b,
                0,
                SizerFlag::Expand | SizerFlag::Bottom,
                self.page.from_dip(4),
            );
        }
        self.rows.borrow_mut().push(b);
        self.page.layout();
        self.page.fit_inside();
    }
    pub fn replace(&self, labels: &[String]) {
        if self
            .rows
            .borrow()
            .iter()
            .map(Button::get_label)
            .eq(labels.iter().cloned())
        {
            return;
        }
        // Keep existing native controls alive while selection callbacks run.
        // Recreating the clicked wxControl inside its own command event can
        // invalidate both native focus and the callback's owning event handler.
        for (index, label) in labels.iter().enumerate() {
            let row = self.rows.borrow().get(index).copied();
            if let Some(row) = row {
                row.set_label(label);
                row.set_name(&format!("client-model-{index}"));
                row.set_tooltip(label);
            } else {
                self.append(label);
            }
        }
        let removed = self.rows.borrow_mut().split_off(labels.len());
        for row in removed {
            row.destroy();
        }
        if self
            .selected
            .get()
            .is_some_and(|i| i as usize >= labels.len())
        {
            self.selected.set(None);
        }
        self.page.layout();
        self.page.fit_inside();
    }
    pub fn set_selection(&self, index: u32, select: bool) {
        self.selected
            .set((select && (index as usize) < self.rows.borrow().len()).then_some(index));
        for (i, b) in self.rows.borrow().iter().enumerate() {
            b.set_selected(self.selected.get() == Some(i as u32));
        }
    }
    pub fn get_selection(&self) -> Option<u32> {
        self.selected.get()
    }
    pub fn on_selection_changed(&self, changed: impl Fn(()) + 'static) {
        *self.changed.borrow_mut() = Some(Rc::new(changed));
    }
}
impl WxWidget for ModelList {
    fn handle_ptr(&self) -> *mut wxdragon::ffi::wxd_Window_t {
        self.page.handle_ptr()
    }
    fn is_valid(&self) -> bool {
        self.page.is_valid()
    }
}
