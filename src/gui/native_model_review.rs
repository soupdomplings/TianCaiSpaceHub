//! Offline regression for the real WorkBuddy/TianGong editors in the nested model page.
use super::*;
use wxdragon::prelude::*;

#[test]
fn integrated_shell_keeps_native_hit_targets_controls_and_vetoable_close() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HTCAPTION, HTCLIENT, HTMAXBUTTON, HTTOP, HTTRANSPARENT, SendMessageW, WM_NCHITTEST,
    };
    wxdragon::app::render_offscreen(|| {
        for (mode, suffix) in [
            (theme::ThemeMode::Light, "light"),
            (theme::ThemeMode::Dark, "dark"),
        ] {
            theme::init(mode);
            for (width, height) in [(900, 560), (1200, 780)] {
                let frame = Frame::builder()
                    .with_title("hidden integrated production shell")
                    .with_position(Point::new(-1400, -800))
                    .build();
                frame.set_font(&theme::font(theme::TextRole::Body));
                frame.set_size(visual::dip_size(&frame, width, height));
                // Signed screen coordinates also cover a monitor left of the
                // primary display. No window is shown and no real input is sent.
                let root = Panel::builder(&frame).build();
                let shell = shell::Shell::new(&root, &frame);
                let page = Panel::builder(&shell.book).build();
                shell.register(shell::PageId::Overview, &page);
                let col = BoxSizer::builder(Orientation::Vertical).build();
                col.add(&shell.panel, 1, SizerFlag::Expand, 0);
                root.set_sizer(col, false);
                let outer = BoxSizer::builder(Orientation::Vertical).build();
                outer.add(&root, 1, SizerFlag::Expand, 0);
                frame.set_sizer(outer, false);
                frame.layout();
                root.layout();
                shell.panel.layout();
                let packed = |p: Point| (p.x as u16 as u32 | ((p.y as u16 as u32) << 16)) as isize;
                let point = |w: &dyn WxWidget| {
                    let size = w.get_client_size();
                    w.client_to_screen(Point::new(size.width / 2, size.height / 2))
                };
                let hit = |w: &dyn WxWidget, p: Point| unsafe {
                    SendMessageW(w.get_handle(), WM_NCHITTEST, 0, packed(p))
                };
                let max = frame.find_window_by_name("window-maximize").unwrap();
                let min = frame.find_window_by_name("window-minimize").unwrap();
                let close = frame.find_window_by_name("window-close").unwrap();
                assert_eq!(hit(&frame, point(&max)), HTMAXBUTTON as isize);
                assert_eq!(hit(&max, point(&max)), HTTRANSPARENT as isize);
                for control in [
                    &min as &dyn WxWidget,
                    &close,
                    &shell.service,
                    &shell.account,
                ] {
                    assert_eq!(hit(&frame, point(control)), HTCLIENT as isize);
                }
                let drag = shell.panel.client_to_screen(Point::new(
                    shell.panel.from_dip(85),
                    shell.panel.from_dip(24),
                ));
                assert_eq!(hit(&frame, drag), HTCAPTION as isize);
                let top = frame.client_to_screen(Point::new(frame.from_dip(80), 1));
                assert_eq!(hit(&frame, top), HTTOP as isize);
                assert_eq!(hit(&frame, point(&page)), HTCLIENT as isize);
                assert!(
                    frame.get_size().height - frame.get_client_size().height < frame.from_dip(16),
                    "the legacy caption must not reserve an extra title row"
                );
                let closes = Rc::new(Cell::new(0));
                let observed = closes.clone();
                frame.on_close(move |event| {
                    observed.set(observed.get() + 1);
                    if let WindowEventData::General(raw) = event {
                        raw.veto();
                    }
                });
                click(&close);
                assert_eq!(
                    closes.get(),
                    1,
                    "custom close must reach the existing wx close event"
                );
                assert!(!frame.is_shown() && !frame.is_maximized());
                capture(&frame, &format!("window-shell-{width}-{suffix}"));
                frame.destroy();
            }
        }
    })
    .unwrap();
}

pub(super) fn click(window: &dyn WxWidget) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_KEYDOWN, WM_KEYUP};
    unsafe {
        SendMessageW(window.get_handle(), WM_KEYDOWN, 13, 0);
        SendMessageW(window.get_handle(), WM_KEYUP, 13, 0);
    }
}
pub(super) fn capture<W: WxWidget>(window: &W, name: &str) {
    if let Some(directory) = std::env::var_os("TIANCAI_NATIVE_REVIEW_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let bitmap = window.capture_bitmap().unwrap();
        let (w, h) = (bitmap.get_width() as u32, bitmap.get_height() as u32);
        let mut pixels = bitmap.get_rgba_data().unwrap();
        for px in pixels.chunks_exact_mut(4) {
            px[3] = 255;
        }
        image::RgbaImage::from_raw(w, h, pixels)
            .unwrap()
            .save(directory.join(format!("{name}.png")))
            .unwrap();
        std::fs::write(directory.join(format!("{name}.json")),serde_json::to_vec_pretty(&serde_json::json!({"scale":visual::scale(window),"capturedPixels":[w,h],"synthetic":true,"hiddenNativeProductionComponents":true,"productStarted":false})).unwrap()).unwrap();
    }
}
#[test]
fn native_existing_model_lists_keep_complete_unsaved_drafts_and_original_revisions() {
    wxdragon::app::render_offscreen(|| {
        for (mode,suffix) in [(theme::ThemeMode::Light,"light"),(theme::ThemeMode::Dark,"dark")] {
            theme::init(mode);
            model_fields::review_supported_fields(suffix);
            squad_view::review_account_components(suffix);
            for id in [crate::desktop::clients::ClientId::WorkBuddy,crate::desktop::clients::ClientId::Tiangong] {
                let frame = Frame::builder().with_title("hidden production inline draft regression").build();
                frame.set_font(&theme::font(theme::TextRole::Body)); frame.set_size(visual::dip_size(&frame,900,560));
                let base = Panel::builder(&frame).build(); let shell = shell::Shell::new(&base, &frame);
                let page = Panel::builder(&shell.book).build(); page.set_background_color(theme::theme().bg_app);
                let col = BoxSizer::builder(Orientation::Vertical).build();
                let header = client_header::ClientHeader::new(&page,id); col.add(&header.panel,0,SizerFlag::Expand,0);
                header.tabs[0].set_selected(false); header.tabs[1].set_selected(true);
                header.launch.set_kind(controls::ButtonKind::Secondary);
                let model = model_fields::ModelPage::new(&page,id); col.add(&model.page,1,SizerFlag::Expand,0);
                let settings = Panel::builder(&page).build(); settings.set_sizer(BoxSizer::builder(Orientation::Vertical).build(),false); settings.hide();
                page.set_sizer(col,false); shell.register(shell::PageId::Client(id),&page); shell.navigate(shell::PageId::Client(id));
                let row = BoxSizer::builder(Orientation::Vertical).build(); row.add(&shell.panel,1,SizerFlag::Expand,0); base.set_sizer(row,false);
                let root = BoxSizer::builder(Orientation::Vertical).build(); root.add(&base,1,SizerFlag::Expand,0); frame.set_sizer(root,false);
                match id {
                    crate::desktop::clients::ClientId::WorkBuddy => workbuddy::review_inline_draft(&frame,&model,&settings,&header),
                    _ => gmclaw::review_inline_draft(&frame,&model,&settings,&header),
                }
                assert!(!frame.is_shown());
                assert!(model.legacy_host.is_shown() && !model.workspace.page.is_shown());
                if let Some(directory) = std::env::var_os("TIANCAI_NATIVE_REVIEW_DIR") {
                    let dir = std::path::PathBuf::from(directory); std::fs::create_dir_all(&dir).unwrap();
                    let bitmap = shell.panel.capture_bitmap().expect("native model capture");
                    let (w,h) = (bitmap.get_width() as u32,bitmap.get_height() as u32); let mut pixels = bitmap.get_rgba_data().unwrap();
                    for pixel in pixels.chunks_exact_mut(4) { pixel[3] = 255; }
                    image::RgbaImage::from_raw(w,h,pixels).unwrap().save(dir.join(format!("native-model-{}-{suffix}.png",id.key()))).unwrap();
                    std::fs::write(dir.join(format!("native-model-{}-{suffix}.json",id.key())),serde_json::to_vec_pretty(&serde_json::json!({"requestedOuterDip":[900,560],"scale":visual::scale(&shell.panel),"capturedPixels":[w,h],"client":id.key(),"synthetic":true,"actualLegacyEditor":true})).unwrap()).unwrap();
                }
                frame.destroy();
            }
            for id in crate::desktop::clients::ClientId::ALL.into_iter().filter(|id|!matches!(id,crate::desktop::clients::ClientId::WorkBuddy | crate::desktop::clients::ClientId::Tiangong)) {
                let frame = Frame::builder().with_title("hidden actual client draft controller regression").build();
                frame.set_font(&theme::font(theme::TextRole::Body)); frame.set_size(visual::dip_size(&frame,900,560));
                let base = Panel::builder(&frame).build(); let shell = shell::Shell::new(&base, &frame);
                let col = BoxSizer::builder(Orientation::Vertical).build(); col.add(&shell.panel,1,SizerFlag::Expand,0); base.set_sizer(col,false);
                let root = BoxSizer::builder(Orientation::Vertical).build(); root.add(&base,1,SizerFlag::Expand,0); frame.set_sizer(root,false);
                clients::review_inline_draft(&frame,&shell,id);
                assert!(!frame.is_shown());
                if let Some(directory) = std::env::var_os("TIANCAI_NATIVE_REVIEW_DIR") {
                    let dir = std::path::PathBuf::from(directory); std::fs::create_dir_all(&dir).unwrap();
                    let bitmap = shell.panel.capture_bitmap().expect("actual client draft capture");
                    let (w,h) = (bitmap.get_width() as u32,bitmap.get_height() as u32); let mut pixels = bitmap.get_rgba_data().unwrap();
                    for pixel in pixels.chunks_exact_mut(4) { pixel[3] = 255; }
                    image::RgbaImage::from_raw(w,h,pixels).unwrap().save(dir.join(format!("native-model-{}-{suffix}.png",id.key()))).unwrap();
                    std::fs::write(dir.join(format!("native-model-{}-{suffix}.json",id.key())),serde_json::to_vec_pretty(&serde_json::json!({"requestedOuterDip":[900,560],"scale":visual::scale(&shell.panel),"capturedPixels":[w,h],"client":id.key(),"synthetic":true,"actualClientController":true,"codexCatalogUsesSyntheticHostLabel":id==crate::desktop::clients::ClientId::Codex})).unwrap()).unwrap();
                }
                frame.destroy();
            }
        }
    }).unwrap();
}
