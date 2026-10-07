//! The Settings window (⌘,): reading font, size, line spacing and column
//! width, plus theme and display toggles. Built on gpui-kit's `setting`
//! components. Every change writes the global `Settings` and saves, and
//! `ReaderView` repaints because it observes that global.

use gpui_kit::base::Root;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::setting::{
    NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage,
    Settings as SettingsPanel,
};
use gpui_kit::*;

use crate::library::{
    Appearance, FONT_SIZE_RANGE, FontPairing, LINE_HEIGHT_RANGE, READING_WIDTH_RANGE, Settings,
};

/// The settings window, if open.
pub struct SettingsWindow(pub Option<WindowHandle<Root>>);

impl Global for SettingsWindow {}

struct SettingsView {
    _observe: Subscription,
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

fn pages() -> Vec<SettingPage> {
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
        .default_open(true)
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
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .children(lines)
                })),
        );

    vec![reading, appearance, about]
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        SettingsPanel::new("settings")
            .sidebar_width(px(180.))
            .pages(pages())
    }
}

/// Focus the Settings window or open it.
pub fn open(cx: &mut App) {
    cx.activate(true);
    if let Some(window) = cx.global::<SettingsWindow>().0
        && window
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(760.), px(520.)), cx)),
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
            // Repaint when a setting changes elsewhere (⌘+, the menu).
            _observe: cx.observe_global::<Settings>(|_, cx| cx.notify()),
        })
    });
    match opened {
        Ok((handle, _)) => cx.set_global(SettingsWindow(handle.downcast::<Root>())),
        Err(err) => eprintln!("error: failed to open settings: {err:#}"),
    }
}
