//! Typefaces bundled into the binary for the font pairings (`FontPairing`).
//!
//! Static Regular / Bold / Italic instances, subset to Latin, cut from the
//! Google Fonts variable files (all SIL OFL; licenses in `assets/fonts/licenses`).
//! GPUI's macOS font loader doesn't pick weights out of variable fonts, hence
//! the static cuts. Headings and display text have no Italic, so only the body
//! faces (Lora, Source Serif 4, Literata, EB Garamond) ship one.

use std::borrow::Cow;

use gpui_kit::App;

macro_rules! font {
    ($file:literal) => {
        Cow::Borrowed(&include_bytes!(concat!("../../../assets/fonts/", $file))[..])
    };
}

/// Make the bundled families available to the text system.
pub fn register(cx: &App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        font!("Lora-Regular.ttf"),
        font!("Lora-Bold.ttf"),
        font!("Lora-Italic.ttf"),
        font!("PlayfairDisplay-Regular.ttf"),
        font!("PlayfairDisplay-Bold.ttf"),
        font!("Outfit-Regular.ttf"),
        font!("Outfit-Bold.ttf"),
        font!("SourceSerif4-Regular.ttf"),
        font!("SourceSerif4-Bold.ttf"),
        font!("SourceSerif4-Italic.ttf"),
        font!("Inter-Regular.ttf"),
        font!("Inter-Bold.ttf"),
        font!("Inter-Italic.ttf"),
        font!("Literata-Regular.ttf"),
        font!("Literata-Bold.ttf"),
        font!("Literata-Italic.ttf"),
        font!("EBGaramond-Regular.ttf"),
        font!("EBGaramond-Bold.ttf"),
        font!("EBGaramond-Italic.ttf"),
    ];
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        eprintln!("warning: failed to load bundled fonts: {err:#}");
    }
}
