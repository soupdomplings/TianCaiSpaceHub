#[cfg(not(target_os = "windows"))]
use std::process::Command;

#[cfg(target_os = "windows")]
use wxdragon::prelude::{BrowserLaunchFlags, launch_default_browser};

use super::text::GuiText;

// OAuth needs browser launching even when the application updater is disabled.
pub(super) fn open_url_in_browser(text: GuiText, url: &str) -> Result<(), String> {
    let url = url.trim();
    if url.is_empty() {
        return Err(text.empty_download_url().to_string());
    }

    #[cfg(target_os = "windows")]
    {
        // OAuth URLs contain '&' and percent escapes; avoid shell interpretation.
        return if launch_default_browser(url, BrowserLaunchFlags::Default) {
            Ok(())
        } else {
            Err(text.open_browser_failed("Windows could not open the default browser", url))
        };
    }
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };

    #[cfg(not(target_os = "windows"))]
    command
        .spawn()
        .map(|_| ())
        .map_err(|err| text.open_browser_failed(&err.to_string(), url))
}
