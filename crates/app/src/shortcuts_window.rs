//! The Keyboard Shortcuts window (⌘/ or Help ▸ Keyboard Shortcuts). Lists
//! `keymap::SECTIONS`, drawing each key for the platform it runs on.

use gpui_kit::base::Root;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::keymap::{SECTIONS, Section, Shortcut};

/// The shortcuts window, if open.
pub struct ShortcutsWindow(pub Option<WindowHandle<Root>>);

impl Global for ShortcutsWindow {}

struct ShortcutsView {
    scroll: ScrollHandle,
}

fn key_caps(row: &Shortcut) -> impl IntoElement {
    div().flex().flex_shrink_0().justify_end().gap_1().children(
        row.keys
            .iter()
            .filter_map(|key| Keystroke::parse(key).ok())
            .map(Kbd::new),
    )
}

fn shortcut_row(row: &Shortcut, last: bool, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .px_4()
        .py_2()
        .when(!last, |d| d.border_b_1().border_color(theme.border))
        .child(div().flex_1().min_w_0().text_sm().child(row.label))
        .child(key_caps(row))
}

fn section_card(section: &Section, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let rows: Vec<&Shortcut> = section.rows.iter().filter(|r| r.available()).collect();
    let count = rows.len();
    div()
        .w_full()
        .min_w(px(340.))
        .max_w(px(520.))
        .flex_1()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .px_1()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted_foreground)
                .child(section.title.to_uppercase()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .rounded_lg()
                .border_1()
                .border_color(theme.border)
                .bg(theme.secondary)
                .overflow_hidden()
                .children(
                    rows.into_iter()
                        .enumerate()
                        .map(|(i, row)| shortcut_row(row, i + 1 == count, cx)),
                ),
        )
}

impl Render for ShortcutsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .id("shortcuts")
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_6()
                    .p_6()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Keyboard Shortcuts"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child("Everything in Bible Reader works from the keyboard. Press ⌘K to find anything else."),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_start()
                            .gap_6()
                            .children(SECTIONS.iter().map(|s| section_card(s, cx))),
                    ),
            )
    }
}

/// Focus the Shortcuts window or open it.
pub fn open(cx: &mut App) {
    cx.activate(true);
    if let Some(window) = cx.global::<ShortcutsWindow>().0
        && window
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(920.), px(700.)), cx)),
        titlebar: Some(TitlebarOptions {
            title: Some("Keyboard Shortcuts".into()),
            ..Default::default()
        }),
        window_min_size: Some(size(px(420.), px(360.))),
        ..Default::default()
    };
    let opened = gpui_kit::open_window(options, cx, |window, cx| {
        crate::appearance::reapply(window, cx);
        cx.new(|_| ShortcutsView {
            scroll: ScrollHandle::new(),
        })
    });
    match opened {
        Ok((handle, _)) => cx.set_global(ShortcutsWindow(handle.downcast::<Root>())),
        Err(err) => eprintln!("error: failed to open shortcuts: {err:#}"),
    }
}
