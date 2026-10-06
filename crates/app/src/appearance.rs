//! Applies the reading theme (light / sepia / dark / system) to gpui-kit's
//! global `Theme`. Sepia is the light theme with warm paper colors on top.

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, Window, rgb};

use crate::library::{Appearance, Settings};

pub fn apply(appearance: Appearance, window: Option<&mut Window>, cx: &mut App) {
    match appearance {
        Appearance::System => Theme::sync_system_appearance(window, cx),
        Appearance::Light => Theme::change(ThemeMode::Light, window, cx),
        Appearance::Dark => Theme::change(ThemeMode::Dark, window, cx),
        Appearance::Sepia => {
            Theme::change(ThemeMode::Light, window, cx);
            Theme::update(cx, apply_sepia);
        }
    }
}

/// Re-apply the saved appearance, e.g. after macOS switches light / dark.
pub fn reapply(window: &mut Window, cx: &mut App) {
    let appearance = cx.global::<Settings>().appearance;
    apply(appearance, Some(window), cx);
}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn apply_sepia(theme: &mut Theme) {
    let paper = c(0xF6EEDC);
    let ink = c(0x4A3424);
    let shelf = c(0xEFE4CB);
    let rule = c(0xE0D0AE);
    let wash = c(0xE8DABB);
    let faded = c(0x8C7457);
    let leather = c(0x7B5534);

    theme.background = paper;
    theme.foreground = ink;
    theme.border = rule;
    theme.input = rule;
    theme.ring = leather;
    theme.muted = wash;
    theme.muted_foreground = faded;
    theme.accent = wash;
    theme.accent_foreground = ink;
    theme.popover = c(0xFAF4E6);
    theme.popover_foreground = ink;
    theme.primary = leather;
    theme.primary_hover = c(0x8A6141);
    theme.primary_active = c(0x6A4829);
    theme.primary_foreground = c(0xFBF6EC);
    theme.secondary = shelf;
    theme.secondary_hover = wash;
    theme.secondary_active = rule;
    theme.secondary_foreground = ink;
    theme.sidebar = shelf;
    theme.sidebar_foreground = ink;
    theme.sidebar_border = rule;
    theme.sidebar_accent = wash;
    theme.sidebar_accent_foreground = ink;
    theme.colors.list = paper;
    theme.list_hover = wash;
    theme.selection = c(0xE6CF9F);
    theme.title_bar = paper;
    theme.title_bar_border = rule;
    theme.scrollbar = Hsla::transparent_black();
    theme.scrollbar_thumb = c(0xCDB993);
}
