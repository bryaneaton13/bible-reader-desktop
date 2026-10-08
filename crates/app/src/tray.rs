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

/// Closed Bible with a cross, outlined: 44px drawn as a 22pt @2x template
/// image (the vendored gpui-tray patch), so macOS tints it for the menu bar.
/// The book is 11x16pt with 1.5pt strokes on whole pixels so it stays crisp;
/// 4x4 supersampling only softens the rounded corners.
fn book_icon() -> gpui_tray::Result<Icon> {
    const SIZE: u32 = 44;
    const SS: u32 = 4;
    // Pixel coordinates: a rounded cover outline, a spine on the left, the page
    // edge along the bottom and a solid cross on the cover.
    fn inside(x: f32, y: f32) -> bool {
        const STROKE: f32 = 3.0;
        let (l, r, t, b, rad) = (11.0, 33.0, 6.0, 38.0, 4.5);
        let qx = (x - (l + r) / 2.0).abs() - ((r - l) / 2.0 - rad);
        let qy = (y - (t + b) / 2.0).abs() - ((b - t) / 2.0 - rad);
        let cover = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - rad;
        if (-STROKE..=0.0).contains(&cover) {
            return true;
        }
        let spine = (16.0..19.0).contains(&x) && y < 32.0;
        let pages = (29.0..32.0).contains(&y) && x >= 16.0;
        let cross = ((23.0..26.0).contains(&x) && (12.0..26.0).contains(&y))
            || ((20.0..29.0).contains(&x) && (15.0..18.0).contains(&y));
        cover < 0.0 && (spine || pages || cross)
    }
    let mut rgba = vec![0_u8; (SIZE * SIZE * 4) as usize];
    for py in 0..SIZE {
        for px in 0..SIZE {
            let mut hits = 0;
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                    hits += inside(x, y) as u32;
                }
            }
            let o = ((py * SIZE + px) * 4) as usize;
            let alpha = (hits * 255 / (SS * SS)) as u8;
            rgba[o..o + 4].copy_from_slice(&[255, 255, 255, alpha]);
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE)
}
