//! Centralized GUI design tokens (palette, spacing, type scale) and light/dark
//! theming.
//!
//! Historically the GUI hard-coded ~30 near-identical greys via `Colour::rgb(...)`
//! scattered across `gui.rs` and `gui/*.rs`. This module replaces that with one
//! coherent token set per appearance, so every page shares the same palette and a
//! dark variant becomes possible.
//!
//! Usage: call [`init`] once at startup (before building the frame) with the
//! resolved [`ThemeMode`], then read tokens anywhere via [`theme()`].
//!
//! Some tokens (accent, border, spacing scale, type scale) are defined ahead of
//! the call sites that will consume them in the custom-control work, so the
//! module allows currently-unused items.
#![allow(dead_code)]

use std::cell::RefCell;

use wxdragon::appearance::{Appearance, is_system_dark_mode};
use wxdragon::font::{Font, FontFamily, FontWeight};
use wxdragon::prelude::Colour;
thread_local! {static MOTION:std::cell::Cell<bool>=const{std::cell::Cell::new(true)};}
pub(super) fn motion_enabled() -> bool {
    MOTION.with(|v| v.get())
}
pub(super) fn set_motion_enabled(enabled: bool) {
    MOTION.with(|v| v.set(enabled));
}
pub(super) fn load_motion_preference() {
    let reduce = super::daemon_config_path()
        .and_then(|p| {
            crate::desktop::transaction::read(
                &crate::desktop::state_root(&p).join("appearance.json"),
            )
            .ok()
            .flatten()
        })
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["reduceMotion"].as_bool());
    #[cfg(windows)]
    let system = {
        let mut enabled = 1i32;
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
                0x1042,
                0,
                (&mut enabled as *mut i32).cast(),
                0,
            );
        }
        enabled != 0
    };
    #[cfg(not(windows))]
    let system = true;
    set_motion_enabled(reduce.map(|v| !v).unwrap_or(system));
}

/// User-facing theme preference. Persisted in config as a code string, mirroring
/// the `GuiLocale` pattern used for language.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum ThemeMode {
    /// Follow the OS appearance (recommended).
    #[default]
    System,
    /// Force light.
    Light,
    /// Force dark.
    Dark,
}

impl ThemeMode {
    pub(super) fn from_code(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "system" | "auto" => Some(Self::System),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    pub(super) fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// The wxWidgets appearance to apply. Must be set before any window is built.
    pub(super) fn appearance(self) -> Appearance {
        match self {
            Self::System => Appearance::System,
            Self::Light => Appearance::Light,
            Self::Dark => Appearance::Dark,
        }
    }

    /// Resolve to a concrete dark/light decision, consulting the OS for `System`.
    pub(super) fn is_dark(self) -> bool {
        match self {
            Self::Light => false,
            Self::Dark => true,
            Self::System => is_system_dark_mode(),
        }
    }
}

// ---------------------------------------------------------------------------
// Spacing & radius scale (appearance-independent).
// ---------------------------------------------------------------------------

/// 4px base spacing grid. Use these instead of ad-hoc sizer margins.
pub(super) const SPACE_XS: i32 = 4;
pub(super) const SPACE_SM: i32 = 8;
pub(super) const SPACE_MD: i32 = 12;
pub(super) const SPACE_LG: i32 = 16;
pub(super) const SPACE_XL: i32 = 24;

/// Shared desktop geometry, in DIP (convert once at the window boundary).
pub(super) const PAGE_GUTTER: i32 = 20;
pub(super) const PANEL_INSET: i32 = 16;
pub(super) const CONTROL_HEIGHT: i32 = 34;
pub(super) const BUTTON_RADIUS: i32 = 7;
pub(super) const SURFACE_RADIUS: i32 = 12;

/// Corner radius for cards and buttons.
pub(super) const RADIUS: f64 = 8.0;

// ---------------------------------------------------------------------------
// Type scale.
// ---------------------------------------------------------------------------

/// Logical font roles, mapped to absolute point sizes. Point sizes are
/// DPI-independent in wxWidgets, so fixed values still scale with the display.
#[derive(Clone, Copy)]
pub(super) enum TextRole {
    PageTitle,
    Metric,
    /// Section / card titles.
    Title,
    /// Primary body text.
    Body,
    /// Secondary / caption text.
    Caption,
}

impl TextRole {
    fn point_size(self) -> i32 {
        match self {
            TextRole::PageTitle => 16,
            TextRole::Metric => 20,
            TextRole::Title => 12,
            TextRole::Body => 10,
            TextRole::Caption => 9,
        }
    }

    fn bold(self) -> bool {
        matches!(
            self,
            TextRole::PageTitle | TextRole::Metric | TextRole::Title
        )
    }
}

/// Build a valid `Font` for the given role.
///
/// We must never hand a widget an *invalid* font: `Font::new()` is
/// default-constructed (not ok), and reading its point size during layout trips
/// a wxWidgets assert (`GetFractionalPointSize: invalid font`). The builder goes
/// through `wxFont`'s real constructor, so the result is always valid.
pub(super) fn font(role: TextRole) -> Font {
    let mut builder = Font::builder()
        .with_family(FontFamily::Swiss)
        .with_point_size(role.point_size());
    #[cfg(windows)]
    {
        builder = builder.with_face_name("Microsoft YaHei UI");
    }
    #[cfg(target_os = "macos")]
    {
        builder = builder.with_face_name("PingFang SC");
    }
    if role.bold() {
        builder = builder.with_weight(FontWeight::Bold);
    }
    builder
        .build()
        .or_else(|| {
            Font::builder()
                .with_family(FontFamily::Swiss)
                .with_point_size(role.point_size())
                .build()
        })
        .expect("a valid system UI font is required")
}

// ---------------------------------------------------------------------------
// Palette.
// ---------------------------------------------------------------------------

/// A full set of color tokens for one appearance. `Copy` so call sites can read a
/// snapshot cheaply via [`theme()`].
#[derive(Clone, Copy)]
pub(super) struct Theme {
    pub(super) is_dark: bool,

    // Surfaces.
    /// Window / tab page background.
    pub(super) bg_app: Colour,
    /// Card / panel surface.
    pub(super) bg_card: Colour,
    /// Slightly tinted card surface (option rows, sub-panels).
    pub(super) bg_card_alt: Colour,
    /// Muted / disabled surface.
    pub(super) bg_muted: Colour,
    /// Table/list normal row background.
    pub(super) table_row: Colour,
    /// Table/list alternate row background.
    pub(super) table_row_alt: Colour,

    // Lines.
    /// Card border / outline.
    pub(super) border: Colour,
    /// Hairline divider, connectors.
    pub(super) divider: Colour,

    // Text.
    /// Primary text.
    pub(super) ink_primary: Colour,
    /// Secondary text (labels, titles on cards).
    pub(super) ink_secondary: Colour,
    /// Muted text (captions, disabled).
    pub(super) ink_muted: Colour,

    // Accent.
    /// Brand accent (primary buttons, links).
    pub(super) accent: Colour,
    /// Accent hover/pressed.
    pub(super) accent_hover: Colour,
    /// Text/icon drawn on top of `accent`.
    pub(super) on_accent: Colour,

    // Semantic states.
    pub(super) ok: Colour,
    pub(super) warn: Colour,
    pub(super) error: Colour,
    pub(super) info: Colour,

    // Soft semantic fills (for banners / pills).
    pub(super) warn_soft: Colour,
    pub(super) error_soft: Colour,

    // Code / JSON viewer palette (Scintilla request-log editor).
    pub(super) code_bg: Colour,
    pub(super) code_fg: Colour,
    pub(super) code_gutter_bg: Colour,
    pub(super) code_line_number: Colour,
    pub(super) code_caret_line: Colour,
    pub(super) code_indent_guide: Colour,
    pub(super) code_find_match: Colour,
    pub(super) code_key: Colour,
    pub(super) code_string: Colour,
    pub(super) code_number: Colour,
    pub(super) code_keyword: Colour,
    pub(super) code_punct: Colour,
}

impl Theme {
    pub(super) const fn light() -> Self {
        Self {
            is_dark: false,

            bg_app: Colour::rgb(246, 248, 250),
            bg_card: Colour::rgb(255, 255, 255),
            bg_card_alt: Colour::rgb(241, 245, 248),
            bg_muted: Colour::rgb(231, 244, 243),
            table_row: Colour::rgb(255, 255, 255),
            table_row_alt: Colour::rgb(248, 250, 252),

            border: Colour::rgb(204, 215, 224),
            divider: Colour::rgb(229, 235, 240),

            ink_primary: Colour::rgb(30, 41, 54),
            ink_secondary: Colour::rgb(78, 96, 113),
            ink_muted: Colour::rgb(96, 112, 126),

            accent: Colour::rgb(7, 118, 124),
            accent_hover: Colour::rgb(6, 100, 108),
            on_accent: Colour::rgb(255, 255, 255),

            ok: Colour::rgb(26, 119, 83),
            warn: Colour::rgb(150, 90, 18),
            error: Colour::rgb(192, 58, 58),
            info: Colour::rgb(7, 118, 124),

            warn_soft: Colour::rgb(252, 245, 230),
            error_soft: Colour::rgb(252, 238, 238),

            code_bg: Colour::rgb(255, 255, 255),
            code_fg: Colour::rgb(38, 45, 56),
            code_gutter_bg: Colour::rgb(243, 246, 250),
            code_line_number: Colour::rgb(94, 103, 117),
            code_caret_line: Colour::rgb(244, 247, 251),
            code_indent_guide: Colour::rgb(174, 184, 199),
            code_find_match: Colour::rgb(245, 188, 45),
            code_key: Colour::rgb(26, 84, 160),
            code_string: Colour::rgb(20, 124, 74),
            code_number: Colour::rgb(151, 71, 0),
            code_keyword: Colour::rgb(128, 61, 150),
            code_punct: Colour::rgb(92, 99, 112),
        }
    }

    pub(super) const fn dark() -> Self {
        Self {
            is_dark: true,

            bg_app: Colour::rgb(19, 25, 32),
            bg_card: Colour::rgb(25, 33, 42),
            bg_card_alt: Colour::rgb(32, 42, 53),
            bg_muted: Colour::rgb(28, 58, 62),
            table_row: Colour::rgb(25, 33, 42),
            table_row_alt: Colour::rgb(29, 38, 48),

            border: Colour::rgb(57, 72, 86),
            divider: Colour::rgb(40, 52, 65),

            ink_primary: Colour::rgb(231, 237, 243),
            ink_secondary: Colour::rgb(175, 190, 203),
            ink_muted: Colour::rgb(148, 165, 181),

            accent: Colour::rgb(76, 199, 197),
            accent_hover: Colour::rgb(110, 220, 217),
            on_accent: Colour::rgb(16, 20, 28),

            ok: Colour::rgb(72, 196, 148),
            warn: Colour::rgb(225, 176, 92),
            error: Colour::rgb(238, 112, 112),
            info: Colour::rgb(110, 168, 250),

            warn_soft: Colour::rgb(54, 47, 30),
            error_soft: Colour::rgb(58, 38, 38),

            code_bg: Colour::rgb(28, 31, 37),
            code_fg: Colour::rgb(210, 216, 224),
            code_gutter_bg: Colour::rgb(36, 40, 48),
            code_line_number: Colour::rgb(120, 128, 140),
            code_caret_line: Colour::rgb(38, 43, 52),
            code_indent_guide: Colour::rgb(64, 70, 82),
            code_find_match: Colour::rgb(220, 180, 70),
            code_key: Colour::rgb(110, 168, 250),
            code_string: Colour::rgb(126, 200, 140),
            code_number: Colour::rgb(224, 170, 110),
            code_keyword: Colour::rgb(198, 148, 232),
            code_punct: Colour::rgb(150, 158, 170),
        }
    }

    fn for_mode(mode: ThemeMode) -> Self {
        if mode.is_dark() {
            Self::dark()
        } else {
            Self::light()
        }
    }
}

// ---------------------------------------------------------------------------
// Global active theme.
//
// The GUI runs single-threaded on the wx main loop and the theme is fixed for the
// lifetime of the process (changing it persists to config and prompts a restart,
// matching the language flow). A thread-local snapshot is therefore sufficient and
// avoids any locking on hot paint paths.
// ---------------------------------------------------------------------------

thread_local! {
    static ACTIVE: RefCell<Theme> = const { RefCell::new(Theme::light()) };
}

/// Install the active theme for the given mode. Call once at startup, after
/// `set_appearance` and before building widgets.
pub(super) fn init(mode: ThemeMode) {
    let resolved = Theme::for_mode(mode);
    ACTIVE.with(|cell| *cell.borrow_mut() = resolved);
}

/// Snapshot of the currently active theme.
pub(super) fn theme() -> Theme {
    ACTIVE.with(|cell| *cell.borrow())
}

#[cfg(test)]
mod contrast_tests {
    use super::*;

    fn contrast(a: Colour, b: Colour) -> f64 {
        let luminance = |c: Colour| {
            let linear = |v: u8| {
                let v = f64::from(v) / 255.;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
        };
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn normal_and_secondary_labels_stay_readable_on_all_product_surfaces() {
        for t in [Theme::light(), Theme::dark()] {
            for background in [
                t.bg_app,
                t.bg_card,
                t.bg_card_alt,
                t.bg_muted,
                t.table_row,
                t.table_row_alt,
            ] {
                for foreground in [t.ink_primary, t.ink_secondary, t.ink_muted, t.accent] {
                    assert!(
                        contrast(foreground, background) >= 4.5,
                        "dark={} text={foreground:?} background={background:?}",
                        t.is_dark
                    );
                }
            }
        }
    }

    #[test]
    fn primary_actions_and_error_warning_text_keep_readable_contrast() {
        for t in [Theme::light(), Theme::dark()] {
            for (fg, bg) in [
                (t.on_accent, t.accent),
                (t.on_accent, t.accent_hover),
                (t.error, t.error_soft),
                (t.warn, t.warn_soft),
                (t.ok, t.bg_card),
            ] {
                assert!(
                    contrast(fg, bg) >= 4.5,
                    "dark={} text={fg:?} background={bg:?}",
                    t.is_dark
                );
            }
        }
    }
}
