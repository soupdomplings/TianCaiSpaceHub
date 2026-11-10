//! Hidden WebView2 round-trip with a synthetic loopback login page only.
#![allow(dead_code)]
#[path = "../src/desktop/clients.rs"]
pub mod client_types;
mod desktop {
    pub use super::client_types as clients;
    pub fn state_root(_: &std::path::Path) -> std::path::PathBuf {
        unreachable!()
    }
    pub mod transaction {
        pub fn read(_: &std::path::Path) -> std::io::Result<Option<Vec<u8>>> {
            unreachable!()
        }
    }
}
#[path = "../src/gui/controls.rs"]
mod controls;
#[path = "../src/gui/squad_login.rs"]
mod squad_login;
#[path = "../src/gui/theme.rs"]
mod theme;
#[path = "../src/gui/visual.rs"]
mod visual;
fn daemon_config_path() -> Option<std::path::PathBuf> {
    None
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };
    use wxdragon::{prelude::*, widgets::SimpleBook};
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let origin: &'static str =
        Box::leak(format!("http://{}", listener.local_addr()?).into_boxed_str());
    listener.set_nonblocking(true)?;
    let finished = Arc::new(AtomicBool::new(false));
    let stop = finished.clone();
    let server = std::thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request);
                let html = r#"<!doctype html><meta charset="utf-8"><title>Synthetic login</title><p>Fixture only</p><script>setTimeout(()=>{localStorage.setItem('auth_token','synthetic-access');localStorage.setItem('refresh_token','synthetic-refresh');localStorage.setItem('token_expires_at','1800000000000');history.replaceState(null,'','/dashboard');},600);</script>"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    html.len(),
                    html
                );
                let _ = stream.write_all(response.as_bytes());
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    });
    let result = wxdragon::app::render_offscreen(move || {
        theme::init(theme::ThemeMode::Light);
        let frame = Frame::builder()
            .with_title("hidden synthetic login bridge")
            .build();
        frame.set_client_size(Size::new(700, 500));
        let book = SimpleBook::builder(&frame).build();
        let login = squad_login::InlineLogin::new(&book, origin);
        book.add_page(&login.page, "fixture login", true, None);
        let account = Panel::builder(&book).build();
        book.add_page(&account, "native account", false, None);
        let s = BoxSizer::builder(Orientation::Vertical).build();
        s.add(&book, 1, SizerFlag::Expand, 0);
        frame.set_sizer(s, true);
        frame.layout();
        login.activate();
        let started = Instant::now();
        let mut captured = None;
        while started.elapsed() < Duration::from_secs(40) {
            #[cfg(windows)]
            unsafe {
                use windows_sys::Win32::UI::WindowsAndMessaging::{
                    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
                };
                let mut message: MSG = std::mem::zeroed();
                while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            wxdragon::app::process_callbacks();
            if let Some(session) = login.take_session() {
                captured = Some(session);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let session = captured.expect("synthetic login session did not return through WebView2");
        assert_eq!(session["accessToken"], "synthetic-access");
        assert_eq!(session["refreshToken"], "synthetic-refresh");
        assert_eq!(session["expiresAt"], 1800000000000_i64);
        assert!(session.get("nonce").is_none());
        book.change_selection(1);
        login.clear();
        assert!(!login.page.is_shown());
        assert!(account.is_shown());
        assert!(!frame.is_shown());
        println!(
            "Synthetic WebView2 capture and native page transition passed in {} ms; no real account.",
            started.elapsed().as_millis()
        );
        frame.destroy();
    });
    finished.store(true, Ordering::SeqCst);
    let _ = server.join();
    result
}
