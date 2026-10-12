//! The Settings window (⌘,): reading font, size, line spacing and column
//! width, plus theme and display toggles. Built on gpui-kit's `setting`
//! components. Every change writes the global `Settings` and saves, and
//! `ReaderView` repaints because it observes that global.

use gpui_kit::base::Root;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::link::Link;
use gpui_kit::component::setting::{
    NumberFieldOptions, SelectIndex, SettingField, SettingGroup, SettingItem, SettingPage,
    Settings as SettingsPanel,
};
use gpui_kit::*;

use crate::app_icon::app_icon;
use crate::library::{
    Appearance, FONT_SIZE_RANGE, FontPairing, LINE_HEIGHT_RANGE, READING_WIDTH_RANGE, Settings,
};

const SIDEBAR_WIDTH: f32 = 180.;

/// About ▸ Built with: (name, what it does here, link).
const BUILT_WITH: &[(&str, &str, &str)] = &[
    (
        "Source code",
        "Bible Reader is open source (MIT).",
        env!("CARGO_PKG_REPOSITORY"),
    ),
    (
        "GPUI",
        "GPU-accelerated UI framework from Zed.",
        "https://www.gpui.rs",
    ),
    (
        "gpui-kit",
        "UI components built on GPUI.",
        "https://gpui-kit.com",
    ),
    (
        "Rust",
        "The language the app is written in.",
        "https://www.rust-lang.org",
    ),
    (
        "SQLite",
        "Storage and full-text search.",
        "https://sqlite.org",
    ),
];

/// A URL without its scheme or `www.`, for display.
fn display_url(url: &str) -> &str {
    let url = url.split_once("://").map_or(url, |(_, rest)| rest);
    url.strip_prefix("www.").unwrap_or(url)
}

/// Byte ranges of web addresses in `text`: words such as `ccel.org` or
/// `www.openbible.info/labs/cross-references`, without surrounding punctuation.
fn find_links(text: &str) -> Vec<std::ops::Range<usize>> {
    const TLDS: &[&str] = &["org", "info", "com", "net", "io", "dev", "rs"];
    let mut links = Vec::new();
    let mut start = 0;
    for word in text.split(char::is_whitespace) {
        let trimmed = word.trim_start_matches(['(', '"', '\'']);
        let lead = word.len() - trimmed.len();
        let trimmed = trimmed.trim_end_matches(['.', ',', ';', ':', ')', '"', '\'']);
        let host = display_url(trimmed).split('/').next().unwrap_or("");
        let is_link = host.rsplit_once('.').is_some_and(|(name, tld)| {
            !name.is_empty() && TLDS.contains(&tld.to_ascii_lowercase().as_str())
        });
        if is_link {
            links.push(start + lead..start + lead + trimmed.len());
        }
        start += word.len() + 1;
    }
    links
}

/// `text` with every web address in it underlined and clickable.
fn linked_text(id: impl Into<ElementId>, text: String, color: Hsla) -> impl IntoElement {
    let links = find_links(&text);
    let urls: Vec<String> = links
        .iter()
        .map(|r| {
            let url = &text[r.clone()];
            if url.contains("://") {
                url.to_string()
            } else {
                format!("https://{url}")
            }
        })
        .collect();
    let style = HighlightStyle {
        color: Some(color),
        underline: Some(UnderlineStyle {
            thickness: px(1.),
            color: Some(color.opacity(0.5)),
            wavy: false,
        }),
        ..Default::default()
    };
    let styled = StyledText::new(text).with_highlights(links.iter().map(|r| (r.clone(), style)));
    InteractiveText::new(id, styled).on_click(links, move |ix, _, cx| cx.open_url(&urls[ix]))
}

/// Dev-only: `BIBLE_READER_SOURCES_GROUP=2` opens a sub-section of Bible
/// sources, so screenshots can reach below the fold.
#[cfg(feature = "screenshot")]
fn debug_group() -> Option<usize> {
    std::env::var("BIBLE_READER_SOURCES_GROUP")
        .ok()?
        .parse()
        .ok()
}

/// Dev-only: `BIBLE_READER_SETTINGS_HEIGHT=1400` for a tall screenshot.
#[cfg(feature = "screenshot")]
fn debug_height() -> f32 {
    std::env::var("BIBLE_READER_SETTINGS_HEIGHT")
        .ok()
        .and_then(|h| h.parse().ok())
        .unwrap_or(520.)
}

#[cfg(not(feature = "screenshot"))]
fn debug_height() -> f32 {
    520.
}

#[cfg(not(feature = "screenshot"))]
fn debug_group() -> Option<usize> {
    None
}

/// The settings window, if open.
pub struct SettingsWindow(pub Option<WindowHandle<Root>>);

impl Global for SettingsWindow {}

struct SettingsView {
    /// Open on the Bible sources page (set when a pane sends the user here).
    open_sources: bool,
    _observe: Vec<Subscription>,
}

/// Write one setting and persist it.
fn update(cx: &mut App, f: impl FnOnce(&mut Settings)) {
    cx.update_global::<Settings, _>(|s, _| {
        f(s);
        s.save();
    });
}

fn number(
    range: (f32, f32),
    step: f64,
    get: fn(&Settings) -> f32,
    set: fn(&mut Settings, f32),
    default: f32,
) -> SettingField<f64> {
    SettingField::number_input(
        NumberFieldOptions {
            min: range.0 as f64,
            max: range.1 as f64,
            step,
        },
        // f32 -> f64 leaves noise like 1.6499999; show two decimals.
        move |cx: &App| (get(cx.global::<Settings>()) as f64 * 100.).round() / 100.,
        move |value: f64, cx: &mut App| {
            let value = (value as f32).clamp(range.0, range.1);
            update(cx, |s| set(s, value));
        },
    )
    .default_value(default as f64)
}

fn switch(
    get: fn(&Settings) -> bool,
    set: fn(&mut Settings, bool),
    default: bool,
) -> SettingField<bool> {
    SettingField::switch(
        move |cx: &App| get(cx.global::<Settings>()),
        move |value, cx: &mut App| update(cx, |s| set(s, value)),
    )
    .default_value(default)
}

fn pages(cx: &App, open_sources: bool) -> Vec<SettingPage> {
    let defaults = Settings::default();
    let fonts = FontPairing::ALL
        .iter()
        .map(|p| {
            (
                SharedString::from(p.key()),
                SharedString::from(p.description()),
            )
        })
        .collect();
    let themes = Appearance::ALL
        .iter()
        .map(|a| (SharedString::from(a.key()), SharedString::from(a.label())))
        .collect();

    let reading = SettingPage::new("Reading")
        .default_open(!open_sources)
        .description("How the chapter text is set.")
        .group(
            SettingGroup::new().title("Text").items([
                SettingItem::new(
                    "Font pairing",
                    SettingField::dropdown(
                        fonts,
                        |cx: &App| cx.global::<Settings>().font_pairing.key().into(),
                        |value: SharedString, cx: &mut App| {
                            if let Some(p) = FontPairing::from_key(&value) {
                                update(cx, |s| s.font_pairing = p)
                            }
                        },
                    )
                    .default_value(SharedString::from(defaults.font_pairing.key())),
                )
                .description("Typeface for titles and verse numbers, and for the text.")
                .keywords(["font", "typeface", "serif", "playfair", "inter", "lora"]),
                SettingItem::new(
                    "Text size",
                    number(
                        FONT_SIZE_RANGE,
                        1.,
                        |s| s.font_size,
                        |s, v| s.font_size = v,
                        defaults.font_size,
                    ),
                )
                .description("Points. Also ⌘+, ⌘− and ⌘0.")
                .keywords(["font", "zoom"]),
                SettingItem::new(
                    "Line spacing",
                    number(
                        LINE_HEIGHT_RANGE,
                        0.05,
                        |s| s.line_height,
                        |s, v| s.line_height = (v * 100.).round() / 100.,
                        defaults.line_height,
                    ),
                )
                .description("Line height as a multiple of the text size.")
                .keywords(["leading", "height"]),
                SettingItem::new(
                    "Column width",
                    number(
                        READING_WIDTH_RANGE,
                        20.,
                        |s| s.reading_width,
                        |s, v| s.reading_width = v,
                        defaults.reading_width,
                    ),
                )
                .description("Width of the text column in points.")
                .keywords(["measure", "margin"]),
            ]),
        );

    let appearance = SettingPage::new("Appearance")
        .description("Theme and what the text shows.")
        .group(
            SettingGroup::new().title("Theme").item(SettingItem::new(
                "Theme",
                SettingField::dropdown(
                    themes,
                    |cx: &App| cx.global::<Settings>().appearance.key().into(),
                    |value: SharedString, cx: &mut App| {
                        if let Some(a) = Appearance::from_key(&value) {
                            crate::set_appearance(a, cx);
                        }
                    },
                )
                .default_value(SharedString::from(defaults.appearance.key())),
            )),
        )
        .group(
            SettingGroup::new().title("Text display").items([
                SettingItem::new(
                    "Red letter",
                    switch(
                        |s| s.red_letter,
                        |s, v| s.red_letter = v,
                        defaults.red_letter,
                    ),
                )
                .description("Show the words of Jesus in red."),
                SettingItem::new(
                    "Verse numbers",
                    switch(
                        |s| s.show_verse_numbers,
                        |s, v| s.show_verse_numbers = v,
                        defaults.show_verse_numbers,
                    ),
                ),
                SettingItem::new(
                    "Sidebar",
                    switch(
                        |s| s.sidebar_open,
                        |s, v| s.sidebar_open = v,
                        defaults.sidebar_open,
                    ),
                )
                .description("Book and chapter list. Also ⌘B."),
            ]),
        );

    let about = SettingPage::new("About")
        .description("Where the text and study data come from.")
        .group(SettingGroup::new().item(SettingItem::render(|_, _, cx| {
            div()
                .flex()
                .items_center()
                .gap_4()
                .child(app_icon(80.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_xl()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Bible Reader"),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("An offline Bible reader for macOS."),
                        ),
                )
        })))
        .group(
            SettingGroup::new()
                .title("Credits")
                .item(SettingItem::render(|_, _, cx| {
                    let library = cx.global::<crate::library::Library>();
                    let mut lines: Vec<String> = library
                        .translations
                        .iter()
                        .map(|t| format!("{}: {}", t.info.abbrev, t.info.copyright))
                        .collect();
                    if let Some(study) = &library.study {
                        lines.push(study.attribution.clone());
                    }
                    let link = cx.theme().link;
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .children(
                            lines
                                .into_iter()
                                .enumerate()
                                .map(|(ix, line)| linked_text(("credit", ix), line, link)),
                        )
                })),
        )
        .group(
            SettingGroup::new()
                .title("Built with")
                .items(BUILT_WITH.iter().map(|&(name, about, url)| {
                    SettingItem::new(
                        name,
                        SettingField::render(move |_, _, _| {
                            Link::new(name).href(url).text_sm().child(display_url(url))
                        }),
                    )
                    .description(about)
                })),
        );

    let sources = crate::sources_page::page(cx).default_open(open_sources);
    vec![reading, appearance, sources, about]
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pages = pages(cx, self.open_sources);
        // The panel has no sidebar footer slot, so the app badge is laid over
        // the bottom of the sidebar, under its three page rows.
        div()
            .relative()
            .size_full()
            .child(
                SettingsPanel::new("settings")
                    .sidebar_width(px(SIDEBAR_WIDTH))
                    .default_selected_index(SelectIndex {
                        // Reading, Appearance, Bible sources, About.
                        page_ix: if self.open_sources { 2 } else { 0 },
                        group_ix: debug_group(),
                    })
                    .pages(pages),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .bottom_0()
                    .w(px(SIDEBAR_WIDTH))
                    .p_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(app_icon(32.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("Bible Reader"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
                            ),
                    ),
            )
    }
}

/// Focus the Settings window or open it.
pub fn open(cx: &mut App) {
    open_page(false, cx);
}

/// Open Settings on ▸ Bible sources (a failed YouVersion chapter links here).
pub fn open_sources(cx: &mut App) {
    // The panel only reads its starting page when it is created, so an open
    // window is closed and reopened on the right one.
    if let Some(window) = cx.global::<SettingsWindow>().0 {
        let _ = window.update(cx, |_, window, _| window.remove_window());
    }
    open_page(true, cx);
}

fn open_page(sources: bool, cx: &mut App) {
    cx.activate(true);
    if let Some(window) = cx.global::<SettingsWindow>().0
        && window
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    if cx
        .try_global::<crate::remote::Remote>()
        .is_some_and(|r| r.has_key() && r.catalog == crate::remote::Catalog::Idle)
    {
        crate::remote::refresh_catalog(cx);
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(
            size(px(760.), px(debug_height())),
            cx,
        )),
        titlebar: Some(TitlebarOptions {
            title: Some("Settings".into()),
            ..Default::default()
        }),
        window_min_size: Some(size(px(560.), px(360.))),
        ..Default::default()
    };
    let opened = gpui_kit::open_window(options, cx, |window, cx| {
        crate::appearance::reapply(window, cx);
        cx.new(|cx| SettingsView {
            open_sources: sources,
            // Repaint when a setting changes elsewhere (⌘+, the menu) and
            // when the YouVersion state (sign-in, catalog, cache) moves.
            _observe: vec![
                cx.observe_global::<Settings>(|_, cx| cx.notify()),
                cx.observe_global::<crate::remote::Remote>(|_, cx| cx.notify()),
                cx.observe_global::<crate::sources_page::Filter>(|_, cx| cx.notify()),
            ],
        })
    });
    match opened {
        Ok((handle, _)) => cx.set_global(SettingsWindow(handle.downcast::<Root>())),
        Err(err) => eprintln!("error: failed to open settings: {err:#}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn links(text: &str) -> Vec<&str> {
        find_links(text).into_iter().map(|r| &text[r]).collect()
    }

    #[test]
    fn finds_web_addresses_in_credits() {
        assert_eq!(
            links(
                "licensed CC-BY 4.0 (www.openbible.info/labs/cross-references). Torrey's (public domain, ccel.org)."
            ),
            ["www.openbible.info/labs/cross-references", "ccel.org"]
        );
        assert_eq!(links("is a trademark of eBible.org."), ["eBible.org"]);
        assert!(links("King James Version (1769). Public domain (outside the UK).").is_empty());
        assert!(links("The Holy Bible, Berean Standard Bible, BSB.").is_empty());
    }

    #[test]
    fn display_url_drops_scheme_and_www() {
        assert_eq!(display_url("https://www.gpui.rs"), "gpui.rs");
        assert_eq!(
            display_url("https://github.com/bryaneaton13/bible-reader-desktop"),
            "github.com/bryaneaton13/bible-reader-desktop"
        );
    }
}
