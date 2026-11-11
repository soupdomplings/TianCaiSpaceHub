//! Small choice cards stay inside the Hub overlay and retain edits while polling.
use super::{
    controls::{Button, ButtonKind, TextCtrl},
    theme,
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wxdragon::prelude::*;

type Submit = Rc<RefCell<Option<Box<dyn Fn(Value)>>>>;
struct Row {
    id: String,
    labels: Vec<StaticText>,
    options: Vec<(String, Button)>,
    selected: Rc<RefCell<Option<String>>>,
    text: TextCtrl,
}
#[derive(Clone)]
pub(super) struct QuestionCard {
    pub(super) panel: Panel,
    scroll: ScrolledWindow,
    body: Rc<Cell<Option<Panel>>>,
    rows: Rc<RefCell<Vec<Row>>>,
    nonce: Rc<RefCell<Option<String>>>,
    busy: Rc<Cell<bool>>,
    submit: Submit,
    next: Button,
    cancel: Button,
}
impl QuestionCard {
    pub(super) fn new(parent: &impl WxWidget) -> Self {
        let panel = Panel::builder(parent).build();
        panel.set_background_color(theme::theme().bg_card);
        panel.set_min_size(Size::new(0, -1));
        let column = BoxSizer::builder(Orientation::Vertical).build();
        let scroll = ScrolledWindow::builder(&panel)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        scroll.set_scroll_rate(0, scroll.from_dip(8));
        scroll.set_background_color(theme::theme().bg_card);
        scroll.set_min_size(Size::new(0, scroll.from_dip(72)));
        column.add(&scroll, 1, SizerFlag::Expand, 0);
        let actions = BoxSizer::builder(Orientation::Horizontal).build();
        let cancel = Button::builder(&panel)
            .with_label("取消本次操作")
            .with_kind(ButtonKind::Ghost)
            .build();
        let next = Button::builder(&panel)
            .with_label("继续")
            .with_kind(ButtonKind::Primary)
            .build();
        actions.add(&cancel, 0, SizerFlag::default(), 0);
        actions.add_stretch_spacer(1);
        actions.add(&next, 0, SizerFlag::default(), 0);
        column.add_sizer(
            &actions,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            panel.from_dip(8),
        );
        panel.set_sizer(column, false);
        panel.hide();
        let card = Self {
            panel,
            scroll,
            body: Default::default(),
            rows: Default::default(),
            nonce: Default::default(),
            busy: Default::default(),
            submit: Default::default(),
            next,
            cancel,
        };
        let current = card.clone();
        next.on_click(move |_| current.send(false));
        let current = card.clone();
        cancel.on_click(move |_| current.send(true));
        card
    }
    pub(super) fn set_submit_handler(&self, callback: impl Fn(Value) + 'static) {
        *self.submit.borrow_mut() = Some(Box::new(callback));
    }
    fn refresh_enabled(&self) {
        let enabled = !self.busy.get();
        self.next.enable(
            enabled
                && !self.rows.borrow().is_empty()
                && self.rows.borrow().iter().all(|row| {
                    row.selected.borrow().is_some() || !row.text.get_value().trim().is_empty()
                }),
        );
        self.cancel.enable(enabled);
        for row in self.rows.borrow().iter() {
            row.text.enable(enabled);
            for (id, button) in &row.options {
                button.enable(enabled);
                button.set_selected(row.selected.borrow().as_ref() == Some(id));
            }
        }
    }
    fn send(&self, cancelled: bool) {
        if self.busy.get() || (!cancelled && !self.next.is_enabled()) {
            return;
        }
        let Some(nonce) = self.nonce.borrow().clone() else {
            return;
        };
        let answers: Vec<Value> = if cancelled {
            Vec::new()
        } else {
            self.rows
                .borrow()
                .iter()
                .map(|row| {
                    json!({"id":row.id,
                "optionId":row.selected.borrow().clone(),"text":row.text.get_value().trim()})
                })
                .collect()
        };
        if let Some(submit) = self.submit.borrow().as_ref() {
            self.busy.set(true);
            self.refresh_enabled();
            submit(json!({"nonce":nonce,"cancelled":cancelled,"answers":answers}));
        }
    }
    pub(super) fn apply(&self, pending: &Value, busy: bool) {
        let nonce = pending["nonce"].as_str().map(str::to_owned);
        self.busy.set(busy);
        if *self.nonce.borrow() != nonce {
            self.rows.borrow_mut().clear();
            if let Some(body) = self.body.take() {
                body.destroy();
            }
            *self.nonce.borrow_mut() = nonce.clone();
            if nonce.is_some() {
                let body = Panel::builder(&self.scroll).build();
                body.set_background_color(theme::theme().bg_card);
                body.set_min_size(Size::new(0, -1));
                let column = BoxSizer::builder(Orientation::Vertical).build();
                for question in pending["questions"].as_array().into_iter().flatten() {
                    let Some(id) = question["id"].as_str() else {
                        continue;
                    };
                    let title = label(
                        &body,
                        question["title"].as_str().unwrap_or("请选择"),
                        theme::TextRole::Body,
                    );
                    column.add(
                        &title,
                        0,
                        SizerFlag::Expand | SizerFlag::Bottom,
                        body.from_dip(8),
                    );
                    let selected: Rc<RefCell<Option<String>>> = Default::default();
                    let mut labels = vec![title];
                    let mut options = Vec::new();
                    for option in question["options"].as_array().into_iter().flatten() {
                        let Some(option_id) = option["id"].as_str() else {
                            continue;
                        };
                        let button = Button::builder(&body)
                            .with_label(option["label"].as_str().unwrap_or(option_id))
                            .with_kind(ButtonKind::Select)
                            .build();
                        button.set_tooltip(option["label"].as_str().unwrap_or(option_id));
                        column.add(
                            &button,
                            0,
                            SizerFlag::Expand | SizerFlag::Bottom,
                            body.from_dip(4),
                        );
                        if let Some(description) =
                            option["description"].as_str().filter(|s| !s.is_empty())
                        {
                            let text = label(&body, description, theme::TextRole::Caption);
                            text.set_foreground_color(theme::theme().ink_secondary);
                            column.add(
                                &text,
                                0,
                                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Bottom,
                                body.from_dip(8),
                            );
                            labels.push(text);
                        }
                        options.push((option_id.to_owned(), button));
                    }
                    let input = TextCtrl::builder(&body).build();
                    input.set_hint(if options.is_empty() {
                        "填写回答"
                    } else {
                        "也可以自己填写"
                    });
                    input.set_name(&format!("assistant-question-{id}"));
                    column.add(
                        &input,
                        0,
                        SizerFlag::Expand | SizerFlag::Bottom,
                        body.from_dip(12),
                    );
                    for (id, button) in &options {
                        let (selected, current, text, id) =
                            (selected.clone(), self.clone(), input, id.clone());
                        button.on_click(move |_| {
                            *selected.borrow_mut() = Some(id.clone());
                            text.change_value("");
                            current.refresh_enabled();
                        });
                    }
                    let (selection, current) = (selected.clone(), self.clone());
                    input.on_text_updated(move |_| {
                        if !input.get_value().trim().is_empty() {
                            *selection.borrow_mut() = None;
                        }
                        current.refresh_enabled();
                    });
                    self.rows.borrow_mut().push(Row {
                        id: id.into(),
                        labels,
                        options,
                        selected,
                        text: input,
                    });
                }
                body.set_sizer(column, false);
                let outer = BoxSizer::builder(Orientation::Vertical).build();
                outer.add(&body, 0, SizerFlag::Expand, 0);
                self.scroll.set_sizer(outer, true);
                self.body.set(Some(body));
                self.scroll.scroll_coords(0, 0);
            }
        }
        self.panel.show(nonce.is_some());
        self.refresh_enabled();
    }
    pub(super) fn layout(&self, width: i32, max_height: i32) {
        if !self.panel.is_shown() {
            return;
        }
        let content_width = (width - self.scroll.from_dip(28)).max(1);
        if let Some(body) = self.body.get() {
            body.set_size(Size::new(content_width, body.get_size().height.max(1)));
            for row in self.rows.borrow().iter() {
                for label in &row.labels {
                    label.wrap(content_width);
                }
            }
            body.layout();
            let height = body
                .get_best_size()
                .height
                .min(max_height)
                .max(self.panel.from_dip(48));
            self.scroll.set_min_size(Size::new(0, height));
            self.scroll.set_max_size(Size::new(-1, height));
        }
        self.panel.layout();
        self.scroll.layout();
        self.scroll.fit_inside();
    }
}
fn label(parent: &impl WxWidget, text: &str, role: theme::TextRole) -> StaticText {
    let label = StaticText::builder(parent).with_label(text).build();
    label.set_font(&theme::font(role));
    label.set_foreground_color(theme::theme().ink_primary);
    label.set_min_size(Size::new(0, label.get_best_size().height));
    label
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    #[ignore = "isolated hidden native question card layout"]
    fn question_card_keeps_draft_and_choice_on_poll_and_fits_small_overlay() {
        wxdragon::app::render_offscreen(|| {
            theme::init(theme::ThemeMode::Light);
            let frame = Frame::builder()
                .with_title("synthetic questions")
                .with_size(Size::new(360, 400))
                .build();
            let card = QuestionCard::new(&frame);
            let sizer = BoxSizer::builder(Orientation::Vertical).build();
            sizer.add(&card.panel, 0, SizerFlag::Expand | SizerFlag::All, 12);
            frame.set_sizer(sizer, false);
            let pending = json!({"nonce":"fixture-one","questions":[{"id":"source","title":"使用哪个模型渠道？","options":[{"id":"a","label":"渠道A","description":"已有可用的模型"},{"id":"b","label":"渠道B"}]}]});
            card.apply(&pending, false);
            card.rows.borrow()[0].text.change_value("保留的回答");
            card.apply(&pending, false);
            assert_eq!(card.rows.borrow()[0].text.get_value(), "保留的回答");
            frame.layout();
            card.layout(336, 120);
            frame.layout();
            assert!(card.scroll.get_min_size().height <= 120);
            assert!(card.next.is_enabled());
            let sent: Rc<RefCell<Vec<Value>>> = Default::default();
            let output = sent.clone();
            card.set_submit_handler(move |body| output.borrow_mut().push(body));
            card.send(false);
            card.send(false);
            assert_eq!(sent.borrow().len(), 1);
            assert_eq!(sent.borrow()[0]["answers"][0]["text"], "保留的回答");
            card.apply(&Value::Null, false);
            assert!(!card.panel.is_shown());
            assert!(card.rows.borrow().is_empty());
            frame.destroy();
        }).unwrap();
    }
}
