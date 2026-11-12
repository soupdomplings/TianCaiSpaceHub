//! TianGong history ownership. Browsing never starts a client or changes data.
use super::{
    api::ApiClient,
    controls::{Button, ButtonKind, Choice},
    dialog_job::InlineJob,
    feedback,
    squad_dialog::{self, SquadDialog},
    widgets::{apply_listctrl_row_theme, apply_listctrl_theme, listctrl_report_style},
};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};
use wxdragon::prelude::*;

const GATEWAY: &str = crate::tiangong_session_history::GATEWAY_USER_ID;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Read,
    Move,
    Restore,
}
impl Operation {
    fn request(self, selected: &[Value]) -> Result<(&'static str, Value), String> {
        if self == Self::Read {
            return Ok(("/api/gmclaw/history/read", json!({})));
        }
        if selected.is_empty() {
            return Err("请先选择要管理的会话，可按 Ctrl 或 Shift 多选".into());
        }
        let capability = if self == Self::Move {
            "canMove"
        } else {
            "canRestore"
        };
        for row in selected {
            if row[capability] != true {
                return Err(row["unavailableReason"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(if self == Self::Move {
                        "所选会话已属于 AI Gateway，或暂时不能调整归属"
                    } else {
                        "只能恢复由 Hub 迁入且保留原归属记录的会话"
                    })
                    .into());
            }
        }
        let sessions: Vec<_> = selected
            .iter()
            .map(|row| {
                json!({
                    "sessionId":row["sessionId"], "userId":row["userId"]
                })
            })
            .collect();
        Ok((
            if self == Self::Move {
                "/api/gmclaw/history/move"
            } else {
                "/api/gmclaw/history/restore"
            },
            json!({"sessions":sessions}),
        ))
    }
}

fn filtered(entries: &[Value], selection: u32) -> Vec<Value> {
    entries
        .iter()
        .filter(|row| match selection {
            1 => row["userId"] == GATEWAY,
            2 => row["userId"] != GATEWAY,
            _ => true,
        })
        .cloned()
        .collect()
}

#[derive(Clone)]
struct HistoryUi {
    dialog: Dialog,
    list: ListCtrl,
    filter: Choice,
    count: StaticText,
    read: Button,
    move_to: Button,
    restore: Button,
    entries: Rc<RefCell<Vec<Value>>>,
    rows: Rc<RefCell<Vec<Value>>>,
    job: Rc<InlineJob>,
    api: ApiClient,
}
impl HistoryUi {
    fn selected(&self) -> Vec<Value> {
        let rows = self.rows.borrow();
        let mut result = Vec::new();
        let mut index = -1_i32;
        loop {
            index = self.list.get_next_item(
                index.into(),
                ListNextItemFlag::All,
                ListItemState::Selected,
            );
            if index < 0 {
                break;
            }
            if let Some(row) = rows.get(index as usize) {
                result.push(row.clone());
            }
        }
        result
    }
    fn render(&self) {
        let rows = filtered(
            &self.entries.borrow(),
            self.filter.get_selection().unwrap_or(0),
        );
        self.list.delete_all_items();
        for (index, row) in rows.iter().enumerate() {
            let index = index as i64;
            let title = row["title"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("未命名会话");
            self.list.insert_item(index, title, None);
            let owner = row["userId"].as_str().unwrap_or("");
            self.list.set_item_text_by_column(
                index,
                1,
                if owner == GATEWAY {
                    "AI Gateway"
                } else if owner.is_empty() {
                    "未登记"
                } else {
                    owner
                },
            );
            self.list
                .set_item_text_by_column(index, 2, row["workDir"].as_str().unwrap_or(""));
            self.list
                .set_item_text_by_column(index, 3, row["updatedAt"].as_str().unwrap_or(""));
            self.list.set_item_text_by_column(
                index,
                4,
                row["unavailableReason"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(if row["canRestore"] == true {
                        "可恢复原归属"
                    } else if row["canMove"] == true {
                        "可迁入"
                    } else {
                        ""
                    }),
            );
            apply_listctrl_row_theme(&self.list, index);
        }
        self.count.set_label(&format!("{} 条会话", rows.len()));
        *self.rows.borrow_mut() = rows;
        self.controls();
        self.dialog.layout();
    }
    fn controls(&self) {
        let idle = !self.job.is_running();
        self.read.enable(idle);
        self.filter.enable(idle);
        self.list.enable(idle);
        self.move_to.enable(idle);
        self.restore.enable(idle);
    }
    fn run(&self, operation: Operation) {
        if self.job.is_running() {
            return;
        }
        let (path, body) = match operation.request(&self.selected()) {
            Ok(request) => request,
            Err(error) => {
                feedback::error(&self.dialog, &error);
                return;
            }
        };
        let ui = self.clone();
        self.job.start_with_feedback(
            self.api.clone(),
            match operation {
                Operation::Read => "读取天工会话",
                Operation::Move => "归入 AI Gateway",
                Operation::Restore => "恢复原归属",
            },
            path,
            body,
            false,
            move |result| {
                if let Ok(view) = result {
                    if operation == Operation::Read {
                        *ui.entries.borrow_mut() =
                            view["entries"].as_array().cloned().unwrap_or_default();
                        ui.render();
                    } else {
                        let count = view["changed"].as_u64().unwrap_or(0);
                        let mut message = format!("已调整 {count} 条会话，重新增强启动后生效");
                        if let Some(warnings) = view["warnings"].as_array() {
                            let warnings = warnings
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join("；");
                            if !warnings.is_empty() {
                                message.push_str(&format!("；{warnings}"));
                            }
                        }
                        feedback::success(&ui.dialog, &message);
                        ui.run(Operation::Read);
                    }
                }
                ui.controls();
            },
        );
        self.controls();
    }
}

pub(super) fn show(parent: &Frame, api: &ApiClient) {
    let view = SquadDialog::new(
        parent,
        "天工会话管理",
        "增强启动使用 AI Gateway 本地账号。选中历史可迁入或恢复原归属，正文和模型保留。调整归属前请完全退出天工，并先在 IM 中退出正在使用的会话。",
        Size::new(1120, 680),
    );
    let (panel, layout) = view.section("");
    let toolbar = BoxSizer::builder(Orientation::Horizontal).build();
    let filter = Choice::builder(&panel)
        .with_choices(vec![
            "全部归属".into(),
            "AI Gateway".into(),
            "其他归属".into(),
        ])
        .build();
    filter.set_selection(0);
    let count = squad_dialog::note(&panel, "尚未读取");
    toolbar.add(&filter, 0, SizerFlag::Right, panel.from_dip(12));
    toolbar.add(&count, 0, SizerFlag::AlignCenterVertical, 0);
    toolbar.add_stretch_spacer(1);
    let read = Button::builder(&panel)
        .with_label("刷新")
        .with_kind(ButtonKind::Ghost)
        .build();
    toolbar.add(&read, 0, SizerFlag::Left, panel.from_dip(8));
    layout.add_sizer(
        &toolbar,
        0,
        SizerFlag::Expand | SizerFlag::Bottom,
        panel.from_dip(12),
    );
    let list = ListCtrl::builder(&panel)
        .with_style(listctrl_report_style())
        .with_size(Size::new(-1, panel.from_dip(360)))
        .build();
    for (index, (label, width)) in [
        ("会话", 210),
        ("归属", 110),
        ("工作目录", 230),
        ("更新时间", 160),
        ("状态", 150),
    ]
    .into_iter()
    .enumerate()
    {
        list.insert_column(
            index as _,
            label,
            ListColumnFormat::Left,
            panel.from_dip(width),
        );
    }
    apply_listctrl_theme(&list);
    layout.add(&list, 1, SizerFlag::Expand, 0);
    let move_to = Button::builder(&view.dialog)
        .with_label("归入 AI Gateway")
        .with_kind(ButtonKind::Primary)
        .build();
    let restore = Button::builder(&view.dialog)
        .with_label("恢复原归属")
        .with_kind(ButtonKind::Secondary)
        .build();
    let close = Button::builder(&view.dialog)
        .with_label("关闭")
        .with_kind(ButtonKind::Ghost)
        .build();
    for button in [move_to, restore, close] {
        view.actions
            .add(&button, 0, SizerFlag::Left, panel.from_dip(8));
    }
    view.status.hide();
    let ui = HistoryUi {
        dialog: view.dialog,
        list,
        filter,
        count,
        read,
        move_to,
        restore,
        entries: Default::default(),
        rows: Default::default(),
        job: Rc::new(InlineJob::new(panel, view.status)),
        api: api.clone(),
    };
    let u = ui.clone();
    filter.on_selection_changed(move |_| u.render());
    for (button, op) in [
        (read, Operation::Read),
        (move_to, Operation::Move),
        (restore, Operation::Restore),
    ] {
        let u = ui.clone();
        button.on_click(move |_| u.run(op));
    }
    let dialog = view.dialog;
    close.on_click(move |_| dialog.end_modal(ID_CANCEL));
    view.finish();
    ui.run(Operation::Read);
    view.show_modal();
    view.dialog.destroy();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_selection_preserves_real_owner_and_only_allows_supported_actions() {
        let old = json!({"sessionId":"s1","userId":"local","canMove":true,"canRestore":false});
        let gateway =
            json!({"sessionId":"s2","userId":"ai-gateway","canMove":false,"canRestore":true});
        let entries = vec![old.clone(), gateway.clone()];
        assert_eq!(filtered(&entries, 1), vec![gateway.clone()]);
        assert_eq!(filtered(&entries, 2), vec![old.clone()]);
        assert_eq!(
            Operation::Move.request(&[old]).unwrap().1,
            json!({"sessions":[{"sessionId":"s1","userId":"local"}]})
        );
        assert!(Operation::Move.request(&[gateway.clone()]).is_err());
        assert_eq!(
            Operation::Restore.request(&[gateway]).unwrap().0,
            "/api/gmclaw/history/restore"
        );
        assert!(Operation::Move.request(&[]).is_err());
        assert_eq!(Operation::Read.request(&[]).unwrap().1, json!({}));
    }
}
