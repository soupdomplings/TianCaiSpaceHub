use super::*;

const GROUPS: [(&str, &str); 5] = [
    ("account", "账户"),
    ("keys", "API 密钥"),
    ("usage", "使用记录"),
    ("organization", "组织与扣费"),
    ("billing", "充值 / 订阅"),
];
fn group(key: &str) -> &str {
    match key {
        "account" | "profile" => "account",
        "keys" => "keys",
        "usage" => "usage",
        "purchase" => "billing",
        key if key.starts_with("organization") => "organization",
        _ => "removed",
    }
}
type Select = Rc<RefCell<Option<Rc<dyn Fn(String)>>>>;
#[derive(Clone)]
pub(super) struct Navigation {
    pub page: Panel,
    sub: Panel,
    sub_sizer: WrapSizer,
    groups: Vec<(&'static str, Button)>,
    items: Rc<RefCell<Vec<(String, Button)>>>,
    menus: Rc<RefCell<Vec<Value>>>,
    select: Select,
}
impl Navigation {
    pub fn new(parent: &dyn WxWidget) -> Self {
        let page = Panel::builder(parent).build();
        page.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let top = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        let sub = Panel::builder(&page).build();
        sub.set_background_color(theme::theme().bg_app);
        let sub_sizer = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        sub.set_sizer(sub_sizer, false);
        let select: Select = Default::default();
        let menus: Rc<RefCell<Vec<Value>>> = Default::default();
        let groups = GROUPS
            .into_iter()
            .map(|(key, label)| {
                let b = Button::builder(&page)
                    .with_label(label)
                    .with_kind(ButtonKind::Tab)
                    .with_size(Size::new(
                        label
                            .chars()
                            .map(|c| if c.is_ascii() { 7 } else { 14 })
                            .sum::<i32>()
                            + 24,
                        36,
                    ))
                    .build();
                let (menus, select) = (menus.clone(), select.clone());
                b.on_click(move |_| {
                    let target = menus
                        .borrow()
                        .iter()
                        .find(|m| group(m["id"].as_str().unwrap_or("")) == key)
                        .and_then(|m| m["id"].as_str().map(str::to_string));
                    let callback = select.borrow().clone();
                    if let (Some(key), Some(callback)) = (target, callback) {
                        callback(key);
                    }
                });
                top.add(&b, 0, SizerFlag::Right, page.from_dip(2));
                (key, b)
            })
            .collect();
        col.add_sizer(&top, 0, SizerFlag::Expand, 0);
        col.add(
            &sub,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            page.from_dip(6),
        );
        page.set_sizer(col, false);
        Self {
            page,
            sub,
            sub_sizer,
            groups,
            items: Default::default(),
            menus,
            select,
        }
    }
    pub fn on_select(&self, f: impl Fn(String) + 'static) {
        self.select.replace(Some(Rc::new(f)));
    }
    pub fn render(&self, menus: &[Value], key: &str) {
        let visible = menus
            .iter()
            .filter(|item| item["id"] != "channels")
            .cloned()
            .collect::<Vec<_>>();
        let menus = visible.as_slice();
        if *self.menus.borrow() != menus {
            for (_, button) in self.items.borrow_mut().drain(..) {
                button.destroy();
            }
            self.menus.replace(menus.to_vec());
            for item in menus {
                let id = item["id"].as_str().unwrap_or("account").to_string();
                let label = match id.as_str() {
                    "account" => "概览",
                    "profile" => "个人资料",
                    "organization" => "扣费身份",
                    _ => item["label"].as_str().unwrap_or("账户"),
                };
                let b = Button::builder(&self.sub)
                    .with_label(label)
                    .with_kind(ButtonKind::Segment)
                    .with_size(Size::new(
                        (label.chars().count() as i32 * 14 + 24).clamp(74, 148),
                        30,
                    ))
                    .build();
                let (target, select) = (id.clone(), self.select.clone());
                b.on_click(move |_| {
                    let callback = select.borrow().clone();
                    if let Some(callback) = callback {
                        callback(target.clone());
                    }
                });
                self.sub_sizer.add(
                    &b,
                    0,
                    SizerFlag::Right | SizerFlag::Bottom,
                    self.page.from_dip(4),
                );
                self.items.borrow_mut().push((id, b));
            }
        }
        let current = group(key);
        for (id, button) in &self.groups {
            button.show(
                menus
                    .iter()
                    .any(|m| group(m["id"].as_str().unwrap_or("")) == *id),
            );
            button.set_selected(*id == current);
        }
        let count = menus
            .iter()
            .filter(|m| group(m["id"].as_str().unwrap_or("")) == current)
            .count();
        for (id, button) in self.items.borrow().iter() {
            button.show(group(id) == current);
            button.set_selected(id == key);
        }
        self.sub.show(count > 1);
        self.page.layout();
    }
    pub fn clear(&self) {
        self.render(&[], "account");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_existing_menu_has_one_group_without_granting_permissions() {
        let menus = [
            "account",
            "profile",
            "keys",
            "usage",
            "organization",
            "organization-admin",
            "organization-usage",
            "purchase",
        ];
        for key in menus {
            assert!(GROUPS.iter().any(|g| g.0 == group(key)));
        }
        assert_eq!(group("profile"), "account");
        assert_eq!(group("channels"), "removed");
        assert_eq!(group("models"), "removed");
        assert_eq!(group("organization-usage"), "organization");
    }
}
