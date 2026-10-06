//! Menu bar item: verse of the day, continue reading, open, quit.

use gpui_kit::{App, ClipboardItem, Global, MenuItem, NoAction};
use gpui_tray::{Icon, Tray};

use crate::library::{Settings, current_translation, verse_of_the_day};
use crate::{CopyVerseOfTheDay, OpenReader, OpenVerseOfTheDay, Quit};

struct TrayHandle(Tray);

impl Global for TrayHandle {}

pub fn install(cx: &mut App) -> gpui_tray::Result<()> {
    let tray = Tray::builder()
        .icon(book_icon()?)
        .tooltip("Bible Reader")
        .menu(build_menu)
        .build(cx)?;
    cx.set_global(TrayHandle(tray));
    Ok(())
}

/// Rebuild the menu after the reading position or translation changes.
pub fn refresh(cx: &mut App) {
    if let Some(tray) = cx.try_global::<TrayHandle>().map(|t| t.0.clone()) {
        let _ = tray.refresh_menu(cx);
    }
}

pub fn copy_verse_of_the_day(_: &CopyVerseOfTheDay, cx: &mut App) {
    let verse = verse_of_the_day();
    let translation = current_translation(cx);
    if let Ok(Some(text)) = translation.verse_text(verse) {
        let citation = format!("“{text}” — {verse} ({})", translation.info.abbrev);
        cx.write_to_clipboard(ClipboardItem::new_string(citation));
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{cut}…")
}

fn build_menu(cx: &mut App) -> Vec<MenuItem> {
    let verse = verse_of_the_day();
    let translation = current_translation(cx);
    let text = translation
        .verse_text(verse)
        .ok()
        .flatten()
        .unwrap_or_default();
    let abbrev = translation.info.abbrev.clone();
    let continue_at = cx.global::<Settings>().chapter_ref();

    let mut items = vec![MenuItem::action("Verse of the Day", NoAction).disabled(true)];
    // Native menu rows don't wrap, so split the verse over a few short lines.
    let mut line = String::new();
    let mut lines = Vec::new();
    for word in text.split_whitespace() {
        if line.len() + word.len() > 48 && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.len() > 4 {
        lines.truncate(4);
        if let Some(last) = lines.last_mut() {
            *last = truncate(&format!("{last} …"), 50);
        }
    }
    for l in lines {
        items.push(MenuItem::action(format!("  {l}"), OpenVerseOfTheDay));
    }
    items.extend([
        MenuItem::action(format!("  — {verse} ({abbrev})"), OpenVerseOfTheDay),
        MenuItem::action("Copy Verse of the Day", CopyVerseOfTheDay),
        MenuItem::separator(),
        MenuItem::action(format!("Continue Reading: {continue_at}"), OpenReader),
        MenuItem::separator(),
        MenuItem::action("Open Bible Reader", OpenReader),
        MenuItem::action("Quit Bible Reader", Quit),
    ]);
    items
}

/// 22pt open-book glyph drawn in code (2x for Retina), anti-aliased by 4x4
/// supersampling. White, since the menu bar is dark in practice; gpui-tray has
/// no template-image option to adapt it to a light bar.
fn book_icon() -> gpui_tray::Result<Icon> {
    const SIZE: u32 = 44;
    const SS: u32 = 4;
    // Coordinates are 0..1 across the icon. Each page's top edge dips toward the
    // spine and the bottom edge follows it; text lines are cut out of the page.
    fn inside(x: f32, y: f32) -> bool {
        let x = if x > 0.5 { 1.0 - x } else { x };
        if !(0.07..=0.475).contains(&x) {
            return false;
        }
        let t = (x - 0.07) / 0.405;
        let top = 0.20 + 0.10 * t * t;
        let bottom = top + 0.54;
        if !(top..=bottom).contains(&y) {
            return false;
        }
        let in_lines = (0.17..=0.40).contains(&x);
        let cut = (1..=3).any(|k| (y - (top + 0.12 * k as f32 + 0.02)).abs() < 0.018);
        !(in_lines && cut)
    }
    let mut rgba = vec![0_u8; (SIZE * SIZE * 4) as usize];
    for py in 0..SIZE {
        for px in 0..SIZE {
            let mut hits = 0;
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (px * SS + sx) as f32 + 0.5;
                    let y = (py * SS + sy) as f32 + 0.5;
                    let scale = (SIZE * SS) as f32;
                    hits += inside(x / scale, y / scale) as u32;
                }
            }
            let o = ((py * SIZE + px) * 4) as usize;
            let alpha = (hits * 255 / (SS * SS)) as u8;
            rgba[o..o + 4].copy_from_slice(&[255, 255, 255, alpha]);
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE)
}
