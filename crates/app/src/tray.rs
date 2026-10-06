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

/// 18pt open-book glyph drawn in code (2x for Retina). Mid-gray so it reads
/// on both light and dark menu bars; gpui-tray has no template-image option.
fn book_icon() -> gpui_tray::Result<Icon> {
    const SIZE: u32 = 36;
    let mut rgba = vec![0_u8; (SIZE * SIZE * 4) as usize];
    let mut set = |x: u32, y: u32| {
        if x < SIZE && y < SIZE {
            let o = ((y * SIZE + x) * 4) as usize;
            rgba[o..o + 4].copy_from_slice(&[128, 128, 128, 255]);
        }
    };
    // Two pages: outlines that dip toward the spine.
    for x in 3..=32 {
        let dist = (x as i32 - 17).unsigned_abs();
        let top = 8 + (5 - dist.min(14) / 3);
        let bottom = top + 20;
        for t in 0..2 {
            set(x, top + t);
            set(x, bottom + t);
        }
    }
    for y in 8..=31 {
        for t in 0..2 {
            set(3 + t, y);
            set(31 + t, y);
            set(17 + t / 2, y + 3);
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE)
}
