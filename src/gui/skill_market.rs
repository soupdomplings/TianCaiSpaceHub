//! Shared Skill repository browser. All cards are from the fetched catalog;
//! client selection is a draft until the user applies it.
use super::{
    GuiMessage,
    api::ApiClient,
    controls::{Button, ButtonKind, TextCtrl},
    resource_detail::{TargetOption, TargetPicker},
    resource_gallery::{Gallery, ResourceCard},
    resources::ResourceEvent,
    theme,
};
use crate::desktop::{
    clients::ClientId,
    resources::LibraryView,
    skill_market::{Catalog, DEFAULT_REPOSITORY, Entry},
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Arc, Mutex},
};
use wxdragon::{prelude::*, timer::Timer, widgets::SimpleBook};
type Slot = Arc<Mutex<Option<(bool, Result<Value, String>)>>>;
#[derive(Clone)]
pub(super) struct SkillMarket {
    pub page: Panel,
    book: SimpleBook,
    gallery: Gallery,
    search: TextCtrl,
    repository: Button,
    refresh: Button,
    status: StaticText,
    picker: TargetPicker,
    catalog: Rc<RefCell<Option<Catalog>>>,
    selected: Rc<RefCell<Option<Entry>>>,
    revision: Rc<RefCell<String>>,
    api: ApiClient,
    frame: Frame,
    tx: tokio::sync::mpsc::UnboundedSender<GuiMessage>,
    pending: Slot,
    timer: Rc<RefCell<Option<Timer<Panel>>>>,
    loaded: Rc<Cell<bool>>,
    busy: Rc<Cell<bool>>,
    importing: Rc<Cell<bool>>,
}
impl SkillMarket {
    pub fn new(
        parent: &dyn WxWidget,
        frame: Frame,
        api: ApiClient,
        tx: tokio::sync::mpsc::UnboundedSender<GuiMessage>,
    ) -> Self {
        let page = Panel::builder(parent).build();
        page.set_background_color(theme::theme().bg_app);
        let root = BoxSizer::builder(Orientation::Vertical).build();
        let book = SimpleBook::builder(&page).build();
        let browse = Panel::builder(&book).build();
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let tools = BoxSizer::builder(Orientation::Horizontal).build();
        let search = TextCtrl::builder(&browse).build();
        search.set_tooltip("搜索 Skill 名称或说明");
        let repository = Button::builder(&browse)
            .with_label("公共仓库")
            .with_kind(ButtonKind::Ghost)
            .build();
        repository.set_tooltip(DEFAULT_REPOSITORY);
        let refresh = Button::builder(&browse)
            .with_label("刷新广场")
            .with_kind(ButtonKind::Ghost)
            .build();
        tools.add(&search, 1, SizerFlag::Right, page.from_dip(10));
        tools.add(&repository, 0, SizerFlag::Right, page.from_dip(8));
        tools.add(&refresh, 0, SizerFlag::default(), 0);
        col.add_sizer(
            &tools,
            0,
            SizerFlag::Expand | SizerFlag::All,
            page.from_dip(12),
        );
        let status = StaticText::builder(&browse)
            .with_label("打开后读取公共仓库")
            .build();
        status.set_foreground_color(theme::theme().ink_secondary);
        status.set_min_size(Size::new(0, -1));
        col.add(
            &status,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            page.from_dip(12),
        );
        let gallery = Gallery::new(&browse);
        col.add(
            &gallery.page,
            1,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            page.from_dip(12),
        );
        browse.set_sizer(col, true);
        book.add_page(&browse, "广场", true, None);
        let picker = TargetPicker::new(&book);
        book.add_page(&picker.page, "选择客户端", false, None);
        root.add(&book, 1, SizerFlag::Expand, 0);
        page.set_sizer(root, true);
        let ui = Self {
            page,
            book,
            gallery,
            search,
            repository,
            refresh,
            status,
            picker,
            catalog: Default::default(),
            selected: Default::default(),
            revision: Default::default(),
            api,
            frame,
            tx,
            pending: Default::default(),
            timer: Default::default(),
            loaded: Default::default(),
            busy: Default::default(),
            importing: Default::default(),
        };
        let u = ui.clone();
        search.on_text_updated(move |_| u.render());
        let u = ui.clone();
        refresh.on_click(move |_| u.fetch(true, None));
        let u = ui.clone();
        repository.on_click(move |_| {
            let current = u
                .catalog
                .borrow()
                .as_ref()
                .map(|c| c.repository.clone())
                .unwrap_or_else(|| DEFAULT_REPOSITORY.into());
            let d = TextEntryDialog::builder(
                &u.frame,
                "填写公开 GitHub Skill 仓库地址，包含 SKILL.md 即可被发现。",
                "切换公共仓库",
            )
            .with_default_value(&current)
            .build();
            let value = (d.show_modal() == ID_OK).then(|| d.get_value()).flatten();
            d.destroy();
            if let Some(value) = value {
                if let Err(e) = crate::desktop::skill_market::Repository::parse(&value) {
                    super::show_error(&u.frame, &e.to_string());
                } else {
                    u.fetch(true, Some(value));
                }
            }
        });
        let u = ui.clone();
        ui.picker.back.on_click(move |_| {
            u.book.change_selection(0);
        });
        let u = ui.clone();
        ui.picker.reload.on_click(move |_| {
            u.book.change_selection(0);
        });
        let u = ui.clone();
        ui.picker.apply.on_click(move |_| u.apply());
        let timer = Timer::new(&page);
        let u = ui.clone();
        timer.on_tick(move |_|{
            let result=u.pending.lock().ok().and_then(|mut s|s.take());
            if let Some((importing,result))=result {
                u.busy.set(false);u.importing.set(false);u.set_enabled(true);
                match result {
                    Ok(v) if importing=>{
                        let failures=v["outcomes"].as_array().into_iter().flatten().filter(|v|v["success"]==false).map(|v|v["message"].as_str().unwrap_or("使用未确认")).collect::<Vec<_>>().join("；");
                        let _=u.tx.send(GuiMessage::Resources(ResourceEvent::Result(Ok(v.clone()))));
                        u.book.change_selection(0);
                        u.status.set_label(if failures.is_empty(){"已使用，可到“我的 Skill”管理客户端。"}else{&failures});
                    }
                    Ok(v)=>{
                        if v["catalog"].is_null(){ u.fetch(true,None);return; }
                        match serde_json::from_value::<Catalog>(v["catalog"].clone()) {
                            Ok(c)=>{u.repository.set_tooltip(&c.repository);*u.catalog.borrow_mut()=Some(c);u.loaded.set(true);u.status.set_label("已读取公共仓库，选择 Skill 后配置使用客户端。");u.render();}
                            Err(_)=>u.status.set_label("仓库目录无法读取，请刷新广场。"),
                        }
                    }
                    Err(e)=>{
                        u.status.set_label(&format!("{e}。可刷新广场或切换仓库；已有 Skill 不受影响。"));
                        if importing {u.picker.invalidate("操作结果未确认，请回到“我的 Skill”刷新并核对使用状态，再继续操作。");}
                    }
                }
                u.status.wrap(u.page.get_client_size().width.max(u.page.from_dip(300))-u.page.from_dip(32));u.page.layout();
            }
        });
        ui.timer.borrow_mut().replace(timer);
        ui
    }
    pub fn enter(&self, active: bool) {
        if let Some(t) = self.timer.borrow().as_ref() {
            if active {
                t.start(120, false);
                if !self.loaded.get() && !self.busy.get() {
                    self.fetch(false, None);
                }
            } else {
                t.stop();
            }
        }
    }
    fn set_enabled(&self, enabled: bool) {
        self.search.enable(enabled);
        self.repository.enable(enabled);
        self.refresh.enable(enabled);
        self.picker.set_enabled(enabled);
        self.gallery.page.enable(enabled);
    }
    fn fetch(&self, refresh: bool, repo: Option<String>) {
        if self.busy.replace(true) {
            return;
        }
        self.set_enabled(false);
        self.status.set_label("正在读取公共仓库…");
        let repository = repo
            .or_else(|| self.catalog.borrow().as_ref().map(|c| c.repository.clone()))
            .unwrap_or_else(|| DEFAULT_REPOSITORY.into());
        let (api, slot) = (self.api.clone(), self.pending.clone());
        std::thread::spawn(move || {
            let result = if refresh {
                api.desktop_post(
                    "/api/desktop/skills/market",
                    &json!({"repository":repository}),
                )
            } else {
                api.get_with_timeout(
                    "/api/desktop/skills/market",
                    std::time::Duration::from_secs(5),
                )
            };
            if let Ok(mut s) = slot.lock() {
                *s = Some((false, result));
            }
            wxdragon::wake_up_idle();
        });
    }
    fn render(&self) {
        let entries = self
            .catalog
            .borrow()
            .as_ref()
            .map(|c| c.entries.clone())
            .unwrap_or_default();
        let query = self.search.get_value().to_lowercase();
        self.gallery.clear();
        for entry in entries.into_iter().filter(|e| {
            query.is_empty()
                || format!("{} {}", e.name, e.description)
                    .to_lowercase()
                    .contains(&query)
        }) {
            let card = ResourceCard::new(
                &self.gallery.page,
                &entry.name,
                &entry.description,
                true,
                &[],
                true,
            );
            card.state.hide();
            card.delete.hide();
            card.edit.set_label("详情");
            card.manage.set_label("使用");
            let (u, e) = (self.clone(), entry.clone());
            card.manage.on_click(move |_| u.choose(e.clone()));
            let (u, e) = (self.clone(), entry);
            card.edit.on_click(move |_|{
                let repo=u.catalog.borrow().as_ref().map(|c|c.repository.clone()).unwrap_or_default();
                super::show_info(&u.frame,&format!("{}\n\n{}\n\n来源：{}\n目录：{}\n完整包：{} 个文件，{} KiB\n\n按所附许可使用。Hub 不执行包内脚本；可用性取决于客户端及 Skill 自身依赖。",e.name,e.description,repo,e.path,e.files,e.bytes.div_ceil(1024)));
            });
            self.gallery.push(card);
        }
        self.gallery
            .finish("暂无可显示的 Skill，可刷新广场或切换公开仓库。");
    }
    fn choose(&self, entry: Entry) {
        if self.busy.get() {
            return;
        }
        let Some(value) = super::dialog_job::run(
            &self.frame,
            self.api.clone(),
            "读取客户端",
            "/api/desktop/library/discover",
            json!({}),
        ) else {
            return;
        };
        let Ok(view) = serde_json::from_value::<LibraryView>(value["view"].clone()) else {
            return;
        };
        let options = view
            .targets
            .iter()
            .filter(|t| t.present)
            .map(|t| TargetOption {
                id: t.id.clone(),
                client: t.client,
                label: t.label.clone(),
                using: Some(false),
                compatible: !(t.client == ClientId::WebStorm && t.model_mode == "native"),
                error: String::new(),
                shared_key: t.skills.to_string_lossy().into_owned(),
                shared_with: Vec::new(),
                location: t.skills.clone(),
            })
            .collect();
        *self.revision.borrow_mut() = view.revision;
        self.picker.populate(&entry.name, options);
        *self.selected.borrow_mut() = Some(entry);
        self.book.change_selection(1);
        self.page.layout();
    }
    fn apply(&self) {
        if self.busy.get() {
            return;
        }
        let (Some(entry), Some(catalog)) = (
            self.selected.borrow().clone(),
            self.catalog.borrow().clone(),
        ) else {
            return;
        };
        let targets = self
            .picker
            .changes()
            .into_iter()
            .filter(|(_, using)| *using)
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return;
        }
        let sharing = self.picker.sharing();
        if !sharing.is_empty()
            && MessageDialog::builder(
                &self.frame,
                &format!(
                    "所选客户端共用 Skill 目录，会同时使用此项：{}。继续？",
                    sharing.join("、")
                ),
                "共用 Skill 目录",
            )
            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
            .build()
            .show_modal()
                != ID_YES
        {
            return;
        }
        self.busy.set(true);
        self.importing.set(true);
        self.set_enabled(false);
        let body = json!({"commit":catalog.commit,"path":entry.path,"revision":*self.revision.borrow(),"targets":targets});
        let (api, slot) = (self.api.clone(), self.pending.clone());
        std::thread::spawn(move || {
            let result = api.desktop_post("/api/desktop/skills/market/use", &body);
            if let Ok(mut s) = slot.lock() {
                *s = Some((true, result));
            }
            wxdragon::wake_up_idle();
        });
    }
}
