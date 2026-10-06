//! The main reading window: toolbar, book sidebar, chapter text, notes
//! inspector and the go-to / search palette.

use bible_core::{
    BOOKS, Block, BlockKind, ChapterRef, Note, NoteKind, Testament, VerseId, parse_reference,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, Theme, ThemeMode};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::chapter_view::{self, Options, Palette, note_marker, render_block};
use crate::library::{Library, Settings, current_translation};
use crate::{
    ClosePalette, DecreaseFont, IncreaseFont, NextChapter, OpenPalette, PrevChapter, ResetFont,
    SelectTranslation, ToggleDark, ToggleInspector, ToggleRedLetter, ToggleSidebar,
    ToggleVerseNumbers,
};

const READING_WIDTH: f32 = 680.;
const READING_FONT: &str = "Iowan Old Style";
const SEARCH_LIMIT: usize = 100;

enum PaletteRow {
    Goto(bible_core::Reference),
    Hit { verse: VerseId, text: String },
}

pub struct ReaderView {
    focus: FocusHandle,
    chapter: ChapterRef,
    blocks: Vec<Block>,
    notes: Vec<Note>,
    target: Option<VerseId>,
    selected_note: Option<u32>,
    inspector_open: bool,
    expanded_book: Option<u8>,
    scroll: ScrollHandle,
    palette_open: bool,
    palette_input: Entity<InputState>,
    palette_rows: Vec<PaletteRow>,
    _subscriptions: Vec<Subscription>,
}

impl ReaderView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let palette_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Go to a passage (jn 3:16) or search words…")
        });
        let subscriptions = vec![cx.subscribe_in(&palette_input, window, Self::on_palette_event)];
        let chapter = cx.global::<Settings>().chapter_ref();
        let focus = cx.focus_handle();
        focus.focus(window, cx);

        let mut this = Self {
            focus,
            chapter,
            blocks: Vec::new(),
            notes: Vec::new(),
            target: None,
            selected_note: None,
            inspector_open: false,
            expanded_book: Some(chapter.book),
            scroll: ScrollHandle::new(),
            palette_open: false,
            palette_input,
            palette_rows: Vec::new(),
            _subscriptions: subscriptions,
        };
        this.load_chapter(cx);
        this
    }

    fn load_chapter(&mut self, cx: &mut Context<Self>) {
        let translation = current_translation(cx);
        self.blocks = translation
            .chapter(self.chapter)
            .ok()
            .flatten()
            .unwrap_or_default();
        self.notes = translation
            .notes_for_chapter(self.chapter)
            .unwrap_or_default();
        self.selected_note = None;
        cx.notify();
    }

    pub fn go_to(&mut self, chapter: ChapterRef, verse: Option<VerseId>, cx: &mut Context<Self>) {
        self.chapter = chapter;
        self.target = verse;
        self.expanded_book = Some(chapter.book);
        self.load_chapter(cx);

        let ix = verse
            .and_then(|v| {
                self.blocks.iter().position(|b| {
                    b.spans.iter().any(|s| matches!(s, bible_core::Span::Verse { num, end }
                        if *num == v.verse() || end.is_some_and(|e| *num <= v.verse() && v.verse() <= e)))
                })
            })
            .unwrap_or(0);
        // Child 0 of the scroll container is the chapter title.
        self.scroll
            .scroll_to_top_of_item(if verse.is_some() { ix + 1 } else { 0 });

        cx.update_global::<Settings, _>(|s, _| {
            s.book = chapter.book;
            s.chapter = chapter.chapter;
            s.save();
        });
        crate::tray::refresh(cx);
    }

    fn next_chapter(&mut self, _: &NextChapter, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(next) = self.chapter.next() {
            self.go_to(next, None, cx);
        }
    }

    fn prev_chapter(&mut self, _: &PrevChapter, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(prev) = self.chapter.prev() {
            self.go_to(prev, None, cx);
        }
    }

    fn select_translation(
        &mut self,
        action: &SelectTranslation,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(abbrev) = cx
            .global::<Library>()
            .translations
            .get(action.index)
            .map(|t| t.info.abbrev.clone())
        else {
            return;
        };
        self.set_translation(abbrev, cx);
    }

    fn set_translation(&mut self, abbrev: String, cx: &mut Context<Self>) {
        cx.update_global::<Settings, _>(|s, _| {
            s.translation = abbrev;
            s.save();
        });
        self.load_chapter(cx);
        self.refresh_palette(cx);
        crate::tray::refresh(cx);
    }

    fn update_settings(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Settings)) {
        cx.update_global::<Settings, _>(|s, _| {
            f(s);
            s.save();
        });
        cx.notify();
    }

    fn open_palette(&mut self, _: &OpenPalette, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = true;
        self.palette_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.palette_rows.clear();
        cx.notify();
    }

    fn close_palette(&mut self, _: &ClosePalette, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = false;
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn on_palette_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => self.refresh_palette(cx),
            InputEvent::PressEnter { .. } if !self.palette_rows.is_empty() => {
                self.choose_row(0, window, cx);
            }
            _ => {}
        }
    }

    fn refresh_palette(&mut self, cx: &mut Context<Self>) {
        let query = self.palette_input.read(cx).value().to_string();
        let mut rows = Vec::new();
        if let Some(reference) = parse_reference(&query) {
            rows.push(PaletteRow::Goto(reference));
        }
        if query.trim().len() >= 2 {
            let hits = current_translation(cx)
                .search(&query, SEARCH_LIMIT)
                .unwrap_or_default();
            rows.extend(hits.into_iter().map(|h| PaletteRow::Hit {
                verse: h.verse,
                text: h.text,
            }));
        }
        self.palette_rows = rows;
        cx.notify();
    }

    fn choose_row(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let target = match self.palette_rows.get(ix) {
            Some(PaletteRow::Goto(r)) => Some((r.chapter_ref(), r.first_verse())),
            Some(PaletteRow::Hit { verse, .. }) => Some((verse.chapter_ref(), Some(*verse))),
            None => None,
        };
        if let Some((chapter, verse)) = target {
            self.go_to(chapter, verse, cx);
        }
        self.close_palette(&ClosePalette, window, cx);
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let library = cx.global::<Library>();
        let settings = cx.global::<Settings>();
        let current = settings.translation.clone();
        let dark = theme.mode.is_dark();

        let versions = library.translations.iter().enumerate().map(|(ix, t)| {
            let abbrev = t.info.abbrev.clone();
            let selected = abbrev == current;
            let button = Button::new(("version", ix))
                .label(abbrev.clone())
                .small()
                .tooltip(t.info.name.clone());
            let button = if selected {
                button.primary()
            } else {
                button.ghost()
            };
            button.on_click(
                cx.listener(move |this, _, _, cx| this.set_translation(abbrev.clone(), cx)),
            )
        });

        div()
            .flex()
            .items_center()
            .gap_1()
            .h(px(44.))
            .px_3()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.background)
            .child(
                Button::new("sidebar")
                    .label("☰")
                    .ghost()
                    .small()
                    .tooltip("Toggle sidebar (⌃⌘S)")
                    .on_click(cx.listener(|_, _, window, cx| {
                        window.dispatch_action(Box::new(ToggleSidebar), cx)
                    })),
            )
            .child(
                Button::new("prev")
                    .label("‹")
                    .ghost()
                    .small()
                    .tooltip("Previous chapter (⌘[)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.prev_chapter(&PrevChapter, window, cx)
                    })),
            )
            .child(
                Button::new("passage")
                    .label(self.chapter.to_string())
                    .ghost()
                    .tooltip("Go to passage or search (⌘K)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_palette(&OpenPalette, window, cx)
                    })),
            )
            .child(
                Button::new("next")
                    .label("›")
                    .ghost()
                    .small()
                    .tooltip("Next chapter (⌘])")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.next_chapter(&NextChapter, window, cx)
                    })),
            )
            .child(div().flex_1())
            .children(versions)
            .child(div().w(px(12.)))
            .child(
                Button::new("smaller")
                    .label("A−")
                    .ghost()
                    .small()
                    .tooltip("Smaller text (⌘−)")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.update_settings(cx, |s| s.font_size = (s.font_size - 1.).max(12.))
                    })),
            )
            .child(
                Button::new("bigger")
                    .label("A+")
                    .ghost()
                    .small()
                    .tooltip("Larger text (⌘+)")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.update_settings(cx, |s| s.font_size = (s.font_size + 1.).min(36.))
                    })),
            )
            .child(
                Button::new("notes")
                    .label("Notes")
                    .ghost()
                    .small()
                    .tooltip("Footnotes (⌥⌘N)")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.inspector_open = !this.inspector_open;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("theme")
                    .label(if dark { "☀" } else { "☾" })
                    .ghost()
                    .small()
                    .tooltip("Toggle dark mode (⇧⌘L)")
                    .on_click(cx.listener(|_, _, window, cx| {
                        window.dispatch_action(Box::new(ToggleDark), cx)
                    })),
            )
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let mut list = div()
            .id("sidebar-books")
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .size_full()
            .py_2();
        for testament in [Testament::Old, Testament::New] {
            list = list.child(
                div()
                    .px_3()
                    .pt_3()
                    .pb_1()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(if testament == Testament::Old {
                        "OLD TESTAMENT"
                    } else {
                        "NEW TESTAMENT"
                    }),
            );
            for book in BOOKS.iter().filter(|b| b.testament() == testament) {
                let num = book.num;
                let is_current = self.chapter.book == num;
                let expanded = self.expanded_book == Some(num);
                list = list.child(
                    div()
                        .id(("book", num as usize))
                        .px_3()
                        .py(px(3.))
                        .mx_1()
                        .rounded_md()
                        .text_sm()
                        .cursor_pointer()
                        .when(is_current, |d| d.font_weight(FontWeight::SEMIBOLD))
                        .hover(|d| d.bg(theme.sidebar_accent))
                        .child(book.name)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.expanded_book = if this.expanded_book == Some(num) {
                                None
                            } else {
                                Some(num)
                            };
                            cx.notify();
                        })),
                );
                if expanded {
                    let mut grid = div().flex().flex_wrap().gap_1().px_3().py_1();
                    for chapter in 1..=book.chapters {
                        let selected = is_current && self.chapter.chapter == chapter;
                        grid = grid.child(
                            div()
                                .id(("chapter", num as usize * 1000 + chapter as usize))
                                .w(px(30.))
                                .h(px(26.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_sm()
                                .text_xs()
                                .cursor_pointer()
                                .when(selected, |d| {
                                    d.bg(theme.primary).text_color(theme.primary_foreground)
                                })
                                .when(!selected, |d| d.hover(|d| d.bg(theme.sidebar_accent)))
                                .child(chapter.to_string())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.go_to(ChapterRef::new(num, chapter), None, cx)
                                })),
                        );
                    }
                    list = list.child(grid);
                }
            }
        }
        div()
            .w(px(230.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(theme.sidebar_border)
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            .child(list)
    }

    fn render_chapter(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let settings = cx.global::<Settings>();
        let dark = theme.mode.is_dark();
        let font_size = settings.font_size;
        let palette = Palette {
            verse_number: theme.muted_foreground,
            jesus: if dark {
                hsla(0., 0.75, 0.68, 1.)
            } else {
                hsla(0., 0.7, 0.4, 1.)
            },
            note_marker: theme.primary,
            target_background: if dark {
                hsla(48. / 360., 0.6, 0.3, 0.5)
            } else {
                hsla(48. / 360., 1., 0.75, 0.6)
            },
        };
        let options = Options {
            red_letter: settings.red_letter,
            show_verse_numbers: settings.show_verse_numbers,
            italic_added: settings.translation == "KJV",
            target: self.target,
        };
        let book = self.chapter.book;
        let chapter = self.chapter.chapter;
        let note_ids: Vec<u32> = self.notes.iter().map(|n| n.id).collect();
        // Body text uses explicit runs, so it can't inherit from the parent div.
        let body_style = TextStyle {
            font_family: READING_FONT.into(),
            font_size: px(font_size).into(),
            line_height: relative(1.65),
            color: theme.foreground,
            ..Default::default()
        };
        let note_index = |id: u32| note_ids.iter().position(|n| *n == id).unwrap_or(0);

        let mut column = div()
            .id("chapter-scroll")
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .size_full()
            .flex()
            .flex_col()
            .items_center();

        // Child 0: chapter title (go_to relies on this offset).
        column = column.child(
            div()
                .w_full()
                .max_w(px(READING_WIDTH))
                .px_8()
                .pt_10()
                .pb_4()
                .text_center()
                .font_family(".SystemUIFont")
                .text_color(theme.muted_foreground)
                .child(
                    div()
                        .text_sm()
                        .child(self.chapter.book_info().name.to_uppercase()),
                )
                .child(
                    div()
                        .text_size(px(font_size * 2.4))
                        .text_color(theme.foreground)
                        .child(chapter.to_string()),
                ),
        );

        for (ix, block) in self.blocks.iter().enumerate() {
            let rendered = render_block(
                block,
                |v| VerseId::new(book, chapter, v),
                &note_index,
                &palette,
                &options,
            );
            let note_ids_for_click: Vec<u32> = rendered.notes.iter().map(|(_, id)| *id).collect();
            let ranges: Vec<_> = rendered.notes.iter().map(|(r, _)| r.clone()).collect();
            let styled = if block.kind.is_heading() {
                StyledText::new(rendered.text.clone()).with_highlights(rendered.highlights.clone())
            } else {
                let mut block_style = body_style.clone();
                if block.kind == BlockKind::PsalmTitle {
                    block_style.font_size = px(font_size * 0.85).into();
                    block_style.font_style = FontStyle::Italic;
                    block_style.color = theme.muted_foreground;
                }
                let runs = chapter_view::build_runs(&rendered, &block_style);
                StyledText::new(rendered.text.clone()).with_runs(runs)
            };
            let entity = cx.entity().downgrade();
            let text = InteractiveText::new(("block", ix), styled).on_click(
                ranges,
                move |range_ix, _, cx| {
                    if let Some(id) = note_ids_for_click.get(range_ix).copied() {
                        entity
                            .update(cx, |this, cx| {
                                this.selected_note = Some(id);
                                this.inspector_open = true;
                                cx.notify();
                            })
                            .ok();
                    }
                },
            );

            let base = div().w_full().max_w(px(READING_WIDTH)).px_8();
            let el = match &block.kind {
                BlockKind::MajorHeading => base
                    .pt_6()
                    .pb_2()
                    .text_center()
                    .font_family(".SystemUIFont")
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(text),
                BlockKind::Heading(level) => base
                    .pt(px(if *level == 1 {
                        font_size * 1.2
                    } else {
                        font_size * 0.8
                    }))
                    .pb_1()
                    .font_family(".SystemUIFont")
                    .text_size(px(font_size * if *level == 1 { 0.85 } else { 0.78 }))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(text),
                BlockKind::ParallelRefs => base
                    .pb_2()
                    .text_size(px(font_size * 0.72))
                    .italic()
                    .text_color(theme.muted_foreground)
                    .child(text),
                BlockKind::PsalmTitle => base
                    .pb_2()
                    .text_size(px(font_size * 0.85))
                    .italic()
                    .text_color(theme.muted_foreground)
                    .child(text),
                BlockKind::Label => base
                    .pt_3()
                    .font_family(".SystemUIFont")
                    .text_size(px(font_size * 0.7))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(text),
                BlockKind::Blank => base.h(px(font_size * 0.7)),
                BlockKind::Poetry(level) => {
                    base.child(div().pl(px(*level as f32 * font_size * 1.4)).child(text))
                }
                BlockKind::PoetryRight => base.flex().justify_end().child(text),
                BlockKind::ListItem(level) => {
                    base.child(div().pl(px(*level as f32 * font_size * 1.2)).child(text))
                }
                BlockKind::Centered => base.text_center().pb(px(font_size * 0.6)).child(text),
                BlockKind::Paragraph | BlockKind::Continuation => {
                    let next_is_poetry = self
                        .blocks
                        .get(ix + 1)
                        .is_some_and(|b| chapter_view::block_is_poetry(&b.kind));
                    base.when(!next_is_poetry, |d| d.pb(px(font_size * 0.6)))
                        .child(text)
                }
            };
            column = column.child(el);
        }

        let next = self.chapter.next();
        column = column.child(
            div()
                .w_full()
                .max_w(px(READING_WIDTH))
                .px_8()
                .pt_10()
                .pb_16()
                .flex()
                .justify_between()
                .font_family(".SystemUIFont")
                .child(
                    Button::new("bottom-prev")
                        .label(
                            self.chapter
                                .prev()
                                .map(|c| format!("‹ {c}"))
                                .unwrap_or_default(),
                        )
                        .ghost()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.prev_chapter(&PrevChapter, window, cx)
                        })),
                )
                .child(
                    Button::new("bottom-next")
                        .label(next.map(|c| format!("{c} ›")).unwrap_or_default())
                        .ghost()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.next_chapter(&NextChapter, window, cx)
                        })),
                ),
        );

        div()
            .flex_1()
            .h_full()
            .min_w_0()
            .font_family(READING_FONT)
            .text_size(px(font_size))
            .line_height(relative(1.65))
            .text_color(theme.foreground)
            .child(column)
    }

    fn render_inspector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let translation = current_translation(cx);
        let mut list = div()
            .id("notes")
            .flex()
            .flex_col()
            .gap_2()
            .overflow_y_scroll()
            .size_full()
            .p_3();
        if self.notes.is_empty() {
            list = list.child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("No notes in this chapter."),
            );
        }
        for (ix, note) in self.notes.iter().enumerate() {
            let selected = self.selected_note == Some(note.id);
            let label = match note.kind {
                NoteKind::Footnote => "",
                NoteKind::CrossReference => "Cross-ref · ",
            };
            let verse = note.verse;
            list = list.child(
                div()
                    .id(("note", ix))
                    .p_2()
                    .rounded_md()
                    .text_sm()
                    .cursor_pointer()
                    .when(selected, |d| {
                        d.bg(theme.accent).text_color(theme.accent_foreground)
                    })
                    .when(!selected, |d| d.hover(|d| d.bg(theme.muted)))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground)
                            .child(format!(
                                "{} {}{}:{}",
                                note_marker(ix),
                                label,
                                verse.chapter(),
                                verse.verse()
                            )),
                    )
                    .child(note.text.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_note = Some(this.notes[ix].id);
                        this.target = Some(verse);
                        cx.notify();
                    })),
            );
        }
        div()
            .w(px(280.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(
                div()
                    .px_3()
                    .py_2()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .border_b_1()
                    .border_color(theme.border)
                    .child(format!("NOTES · {}", translation.info.abbrev)),
            )
            .child(list)
    }

    fn render_palette(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let mut rows = div()
            .id("palette-rows")
            .flex()
            .flex_col()
            .max_h(px(420.))
            .overflow_y_scroll();
        for (ix, row) in self.palette_rows.iter().enumerate() {
            let content = match row {
                PaletteRow::Goto(r) => div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("Go to {r}")),
                PaletteRow::Hit { verse, text } => div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground)
                            .child(verse.to_string()),
                    )
                    .child(div().text_sm().line_clamp(2).child(text.clone())),
            };
            rows = rows.child(
                div()
                    .id(("palette-row", ix))
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(ix == 0, |d| d.bg(theme.muted))
                    .hover(|d| d.bg(theme.accent))
                    .child(content)
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.choose_row(ix, window, cx)),
                    ),
            );
        }
        let hit_count = self
            .palette_rows
            .iter()
            .filter(|r| matches!(r, PaletteRow::Hit { .. }))
            .count();

        div()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(80.))
            .bg(hsla(0., 0., 0., 0.25))
            .id("palette-backdrop")
            .on_click(
                cx.listener(|this, _, window, cx| this.close_palette(&ClosePalette, window, cx)),
            )
            .child(
                div()
                    .id("palette")
                    .key_context("Palette")
                    .on_action(cx.listener(Self::close_palette))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .w(px(560.))
                    .h_auto()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background)
                    .shadow_lg()
                    .font_family(".SystemUIFont")
                    .child(Input::new(&self.palette_input))
                    .when(hit_count >= SEARCH_LIMIT, |d| {
                        d.child(
                            div()
                                .px_3()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!("Showing first {SEARCH_LIMIT} matches")),
                        )
                    })
                    .child(rows),
            )
    }
}

#[cfg(feature = "screenshot")]
impl ReaderView {
    /// Apply a scripted UI step for offscreen screenshots.
    pub fn debug_action(&mut self, action: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (name, arg) = action.split_once(':').unwrap_or((action, ""));
        match name {
            "palette" => {
                self.open_palette(&OpenPalette, window, cx);
                self.palette_input
                    .update(cx, |input, cx| input.set_value(arg.to_string(), window, cx));
                self.refresh_palette(cx);
            }
            "notes" => {
                self.inspector_open = true;
                self.selected_note = self.notes.first().map(|n| n.id);
            }
            "nosidebar" => self.update_settings(cx, |s| s.sidebar_open = false),
            "dark" => Theme::change(ThemeMode::Dark, Some(window), cx),
            "light" => Theme::change(ThemeMode::Light, Some(window), cx),
            "version" => self.set_translation(arg.to_uppercase(), cx),
            _ => eprintln!("unknown debug action {name}"),
        }
        cx.notify();
    }
}

impl Focusable for ReaderView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.palette_open {
            self.palette_input
                .update(cx, |input, cx| input.focus(window, cx));
        }
        let settings = cx.global::<Settings>();
        let sidebar_open = settings.sidebar_open;
        let theme = cx.theme();

        div()
            .id("reader")
            .key_context("Reader")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::next_chapter))
            .on_action(cx.listener(Self::prev_chapter))
            .on_action(cx.listener(Self::open_palette))
            .on_action(cx.listener(Self::select_translation))
            .on_action(cx.listener(|this, _: &IncreaseFont, _, cx| {
                this.update_settings(cx, |s| s.font_size = (s.font_size + 1.).min(36.))
            }))
            .on_action(cx.listener(|this, _: &DecreaseFont, _, cx| {
                this.update_settings(cx, |s| s.font_size = (s.font_size - 1.).max(12.))
            }))
            .on_action(cx.listener(|this, _: &ResetFont, _, cx| {
                this.update_settings(cx, |s| s.font_size = Settings::default().font_size)
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.update_settings(cx, |s| s.sidebar_open = !s.sidebar_open)
            }))
            .on_action(cx.listener(|this, _: &ToggleRedLetter, _, cx| {
                this.update_settings(cx, |s| s.red_letter = !s.red_letter)
            }))
            .on_action(cx.listener(|this, _: &ToggleVerseNumbers, _, cx| {
                this.update_settings(cx, |s| s.show_verse_numbers = !s.show_verse_numbers)
            }))
            .on_action(cx.listener(|this, _: &ToggleInspector, _, cx| {
                this.inspector_open = !this.inspector_open;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleDark, window, cx| {
                let dark = !cx.theme().mode.is_dark();
                Theme::change(
                    if dark {
                        ThemeMode::Dark
                    } else {
                        ThemeMode::Light
                    },
                    Some(window),
                    cx,
                );
                this.update_settings(cx, |s| s.dark = Some(dark));
            }))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(self.render_toolbar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(sidebar_open, |d| d.child(self.render_sidebar(cx)))
                    .child(self.render_chapter(cx))
                    .when(self.inspector_open, |d| d.child(self.render_inspector(cx))),
            )
            .when(self.palette_open, |d| d.child(self.render_palette(cx)))
    }
}
