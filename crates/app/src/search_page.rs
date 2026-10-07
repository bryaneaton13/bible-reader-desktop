//! The search results page (⇧⌘F, or "See all results" in the palette).
//!
//! The palette is for quick jumps; this page is for looking around: filters
//! (match mode, testament or book, translation), the best passages on a
//! matching topic, every ranked match with the words highlighted, related
//! verses for any result, and per-result open here / new tab / beside.
//! It overlays the reading area and closes whenever the reader navigates.
//! Child module of `reader` so it can use the view's private state.

use std::collections::HashSet;

use bible_core::text::highlight_ranges;
use bible_store::{BOOST_STRENGTH, MatchMode, ParsedQuery, Passage, Results, SearchRequest};

use super::*;
use crate::CloseSearch;
use crate::palette::{OpenSearchPage, Scope, split_scope};
use crate::suggestions;

/// Matches fetched for the page.
const PAGE_LIMIT: usize = 200;
/// Key passages listed for a topic.
const KEY_PASSAGES: usize = 12;
/// Related verses shown when a result is expanded.
const RELATED_SHOWN: usize = 6;
const SUGGESTIONS_SHOWN: usize = 8;

/// One result row you can open.
pub(super) struct Entry {
    pub verse: VerseId,
    /// "John 3:16" or "Exodus 20:1-17".
    pub label: String,
    pub text: String,
    /// The row is in the key-passages list of a topic.
    pub key: bool,
    /// Also listed under a topic the query matched.
    pub in_topic: bool,
}

/// One line of the virtualized results list.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    KeyHeading,
    Matches,
    Entry(usize),
}

pub(super) struct SearchPage {
    pub text: String,
    pub mode: MatchMode,
    pub scope: Option<Scope>,
    pub translation: String,
    pub results: Results,
    pub topic: Option<String>,
    pub entries: Vec<Entry>,
    pub selected: usize,
    /// Results whose related verses are showing.
    pub related: HashSet<VerseId>,
    /// Verses sent to a background tab from this page.
    pub opened: HashSet<VerseId>,
    /// Virtualized list: only rows in view are laid out.
    pub list: ListState,
    rows: Vec<Row>,
    /// "John 3" for the back button.
    pub came_from: String,
    /// Suggestions for an empty page.
    pub suggestions: Vec<&'static str>,
}

impl SearchPage {
    fn books(&self) -> std::ops::RangeInclusive<u8> {
        self.scope.as_ref().map_or(1..=u8::MAX, |s| s.books.clone())
    }

    /// Only the key passages come first; the ranked hits follow, without
    /// repeating verses already listed.
    fn run(&mut self, cx: &App) {
        let library = cx.global::<Library>();
        let translation = &library.translations[library.index_of(&self.translation).unwrap_or(0)];
        let request = SearchRequest {
            query: &self.text,
            mode: self.mode,
            books: self.books(),
            limit: PAGE_LIMIT,
        };
        self.results = library.search(translation, &request);
        self.entries.clear();
        self.topic = None;
        self.selected = 0;

        let mut listed: HashSet<VerseId> = HashSet::new();
        if let (Some(study), Some(best)) = (
            library.study.as_ref(),
            self.results
                .topics
                .iter()
                .find(|t| t.strength >= BOOST_STRENGTH),
        ) {
            self.topic = Some(best.topic.name.clone());
            let books = self.books();
            for passage in study.topic_passages(best.topic.id, KEY_PASSAGES * 2) {
                if self.entries.len() >= KEY_PASSAGES {
                    break;
                }
                if !books.contains(&passage.start.book()) {
                    continue;
                }
                if let Some(text) = translation.passage_text(&passage, 3) {
                    listed.extend(passage.verses());
                    self.entries.push(Entry {
                        verse: passage.start,
                        label: passage.label(),
                        text,
                        key: true,
                        in_topic: true,
                    });
                }
            }
        }
        // Passages that also match the words come first, like in the matches.
        let score_of = |verse: VerseId| {
            self.results
                .hits
                .iter()
                .find(|h| h.verse == verse)
                .map_or(0., |h| h.score)
        };
        let mut keyed: Vec<(f32, Entry)> = self
            .entries
            .drain(..)
            .map(|e| (score_of(e.verse), e))
            .collect();
        keyed.sort_by(|a, b| b.0.total_cmp(&a.0));
        self.entries = keyed.into_iter().map(|(_, e)| e).collect();
        for hit in &self.results.hits {
            if listed.contains(&hit.verse) {
                continue;
            }
            self.entries.push(Entry {
                verse: hit.verse,
                label: hit.verse.to_string(),
                text: hit.text.clone(),
                key: false,
                in_topic: hit.in_topic,
            });
        }
        self.rebuild_rows();
    }

    fn rebuild_rows(&mut self) {
        self.rows.clear();
        let mut heading = (false, false);
        for (ix, entry) in self.entries.iter().enumerate() {
            if entry.key && !heading.0 {
                heading.0 = true;
                self.rows.push(Row::KeyHeading);
            }
            if !entry.key && !heading.1 {
                heading.1 = true;
                self.rows.push(Row::Matches);
            }
            self.rows.push(Row::Entry(ix));
        }
        self.list.reset(self.rows.len());
        self.list.scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
    }

    fn row_of(&self, entry: usize) -> Option<usize> {
        self.rows.iter().position(|r| *r == Row::Entry(entry))
    }
}

fn scope_choices(current_book: u8) -> Vec<Scope> {
    let book = bible_core::book_by_num(current_book).expect("valid book");
    vec![
        Scope {
            label: "Whole Bible".into(),
            books: 1..=u8::MAX,
        },
        Scope {
            label: "Old Testament".into(),
            books: 1..=39,
        },
        Scope {
            label: "New Testament".into(),
            books: 40..=66,
        },
        Scope {
            label: book.name.into(),
            books: book.num..=book.num,
        },
    ]
}

impl ReaderView {
    /// Open the page on `query` (which may carry `in:` scope words).
    pub(super) fn open_search_page(
        &mut self,
        action: &OpenSearchPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Called from the palette's own confirm: let it close without
        // stealing the focus this page wants.
        self.palette_handed_off = self.palette_open;
        self.palette_open = false;
        self.renaming = None;

        let (scope, text) = split_scope(&action.query);
        let translation = self.tab().translation.clone();
        let came_from = self.tab().title();
        self.search_opens += 1;
        let mut page = SearchPage {
            text: text.clone(),
            mode: MatchMode::All,
            scope,
            translation,
            results: Results::default(),
            topic: None,
            entries: Vec::new(),
            selected: 0,
            related: HashSet::new(),
            opened: HashSet::new(),
            list: ListState::new(0, ListAlignment::Top, px(600.)),
            rows: Vec::new(),
            came_from,
            suggestions: suggestions::pick(
                self.search_opens + self.suggestion_seed,
                SUGGESTIONS_SHOWN,
            ),
        };
        page.run(cx);
        self.search = Some(page);
        self.search_input.update(cx, |input, cx| {
            input.set_value(text, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn close_search(
        &mut self,
        _: &CloseSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.search.take().is_some() {
            self.focus.focus(window, cx);
            cx.notify();
        }
    }

    pub(super) fn on_search_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.search.is_none() {
            return;
        }
        match event {
            InputEvent::Change => {
                let value = input.read(cx).value().to_string();
                if let Some(page) = self.search.as_mut()
                    && page.text != value
                {
                    page.text = value;
                    page.run(cx);
                    cx.notify();
                }
            }
            InputEvent::PressEnter { secondary, shift } => {
                let how = if *secondary {
                    OpenHow::NewTab
                } else if *shift {
                    OpenHow::Beside
                } else {
                    OpenHow::Here
                };
                self.open_search_result(None, how, window, cx);
            }
            _ => {}
        }
    }

    fn search_rerun(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut SearchPage)) {
        if let Some(mut page) = self.search.take() {
            change(&mut page);
            page.run(cx);
            self.search = Some(page);
            cx.notify();
        }
    }

    pub(super) fn search_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(page) = self.search.as_mut()
            && !page.entries.is_empty()
        {
            let last = page.entries.len() as isize - 1;
            page.selected = (page.selected as isize + delta).clamp(0, last) as usize;
            if let Some(row) = page.row_of(page.selected) {
                page.list.scroll_to_reveal_item(row);
            }
            cx.notify();
        }
    }

    /// Open result `index` (the selected one when `None`).
    pub(super) fn open_search_result(
        &mut self,
        index: Option<usize>,
        how: OpenHow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(page) = self.search.as_mut() else {
            return;
        };
        let Some(entry) = page.entries.get(index.unwrap_or(page.selected)) else {
            return;
        };
        let verse = entry.verse;
        match how {
            OpenHow::Here => {
                self.search = None;
                self.focus.focus(window, cx);
                self.go_to(verse.chapter_ref(), Some(verse), cx);
            }
            OpenHow::Beside => {
                self.search = None;
                self.focus.focus(window, cx);
                self.open_passage_in_split(verse.chapter_ref(), Some(verse), cx);
            }
            OpenHow::NewTab => {
                page.opened.insert(verse);
                self.open_in_background_tab(verse.chapter_ref(), Some(verse), cx);
            }
        }
        cx.notify();
    }

    /// Open a related passage from the expanded list of a result.
    fn open_related(
        &mut self,
        passage: Passage,
        how: OpenHow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let verse = passage.start;
        match how {
            OpenHow::Here => {
                self.search = None;
                self.focus.focus(window, cx);
                self.go_to(verse.chapter_ref(), Some(verse), cx);
            }
            OpenHow::Beside => {
                self.search = None;
                self.focus.focus(window, cx);
                self.open_passage_in_split(verse.chapter_ref(), Some(verse), cx);
            }
            OpenHow::NewTab => {
                if let Some(page) = self.search.as_mut() {
                    page.opened.insert(verse);
                }
                self.open_in_background_tab(verse.chapter_ref(), Some(verse), cx);
            }
        }
        cx.notify();
    }

    /// A new tab next to the active one, without switching to it, so a
    /// search can feed several tabs.
    pub(super) fn open_in_background_tab(
        &mut self,
        chapter: ChapterRef,
        verse: Option<VerseId>,
        cx: &mut Context<Self>,
    ) {
        let mut tab = self.tab().duplicate();
        tab.location = Location { chapter, verse };
        self.tabs.insert(self.active + 1, tab);
        self.sort_pinned();
        self.save_tabs(cx);
    }

    fn render_search_filters(&self, page: &SearchPage, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let library = cx.global::<Library>();
        let label = |text: &'static str| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .w(px(72.))
                .child(text)
        };
        let modes = MatchMode::ALL.into_iter().map(|mode| {
            Button::new(("sp-mode", mode as usize))
                .label(mode.label())
                .xsmall()
                .ghost()
                .selected(page.mode == mode)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.search_rerun(cx, |p| p.mode = mode);
                }))
        });
        let current = scope_choices(self.chapter().book);
        let scopes = current.into_iter().enumerate().map(|(ix, scope)| {
            let selected = match &page.scope {
                Some(s) => s.books == scope.books,
                None => scope.books == (1..=u8::MAX),
            };
            let choice = scope.clone();
            Button::new(("sp-scope", ix))
                .label(scope.label)
                .xsmall()
                .ghost()
                .selected(selected)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let choice = choice.clone();
                    this.search_rerun(cx, move |p| {
                        p.scope = (choice.books != (1..=u8::MAX)).then_some(choice);
                    });
                }))
        });
        let versions = library.translations.iter().enumerate().map(|(ix, t)| {
            let abbrev = t.info.abbrev.clone();
            Button::new(("sp-version", ix))
                .label(abbrev.clone())
                .xsmall()
                .ghost()
                .selected(page.translation == abbrev)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let abbrev = abbrev.clone();
                    this.search_rerun(cx, move |p| p.translation = abbrev);
                }))
        });
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(label("Match"))
                    .children(modes),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(label("Where"))
                    .children(scopes),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(label("Version"))
                    .children(versions),
            )
    }

    fn render_entry(
        &self,
        ix: usize,
        entry: &Entry,
        page: &SearchPage,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let selected = ix == page.selected;
        let accent = theme.accent;
        let highlight = HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            background_color: Some(theme.warning.opacity(0.28)),
            ..Default::default()
        };
        let query = page
            .results
            .corrected
            .clone()
            .unwrap_or_else(|| ParsedQuery::parse(&page.text).plain());
        let ranges = highlight_ranges(&entry.text, &query);
        let body = StyledText::new(entry.text.clone())
            .with_highlights(ranges.into_iter().map(|r| (r, highlight)));
        let verse = entry.verse;
        let opened = page.opened.contains(&verse);
        let related_open = page.related.contains(&verse);

        let mut row = div()
            .id(("sp-row", ix))
            .flex()
            .flex_col()
            .gap_1()
            .p_3()
            .rounded_md()
            .cursor_pointer()
            .when(selected, |d| d.bg(accent))
            .when(!selected, |d| d.hover(|d| d.bg(theme.muted)))
            .on_click(cx.listener(move |this, _, window, cx| {
                if let Some(page) = this.search.as_mut() {
                    page.selected = ix;
                }
                this.open_search_result(Some(ix), OpenHow::Here, window, cx);
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if selected {
                                theme.accent_foreground
                            } else {
                                theme.primary
                            })
                            .child(entry.label.clone()),
                    )
                    .when(entry.in_topic && !entry.key, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .px_1()
                                .rounded_sm()
                                .bg(theme.muted)
                                .text_color(theme.muted_foreground)
                                .child("topic"),
                        )
                    })
                    .when(opened, |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(Icon::new(IconName::Check).size_3())
                                .child("opened in a new tab"),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                Button::new(("sp-related", ix))
                                    .icon(IconName::Network)
                                    .ghost()
                                    .xsmall()
                                    .selected(related_open)
                                    .tooltip("Related verses")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        if let Some(page) = this.search.as_mut() {
                                            if !page.related.remove(&verse) {
                                                page.related.insert(verse);
                                            }
                                            // Height changed: re-measure this row.
                                            if let Some(row) = page.row_of(ix) {
                                                page.list.splice(row..row + 1, 1);
                                            }
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new(("sp-beside", ix))
                                    .icon(IconName::PanelRight)
                                    .ghost()
                                    .xsmall()
                                    .tooltip("Open beside (⇧Enter)")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.open_search_result(
                                            Some(ix),
                                            OpenHow::Beside,
                                            window,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                Button::new(("sp-newtab", ix))
                                    .icon(IconName::ExternalLink)
                                    .ghost()
                                    .xsmall()
                                    .tooltip("Open in new tab (⌘Enter)")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.open_search_result(
                                            Some(ix),
                                            OpenHow::NewTab,
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .when(selected, |d| d.text_color(theme.accent_foreground))
                    .child(body),
            );
        if related_open {
            row = row.child(self.render_related(ix, verse, cx));
        }
        row
    }

    fn render_related(
        &self,
        ix: usize,
        verse: VerseId,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let library = cx.global::<Library>();
        let translation = self
            .search
            .as_ref()
            .and_then(|p| library.index_of(&p.translation))
            .map_or(&library.translations[0], |i| &library.translations[i]);
        let passages = library
            .study
            .as_ref()
            .map(|s| s.related(verse, RELATED_SHOWN))
            .unwrap_or_default();
        let mut list = div()
            .flex()
            .flex_col()
            .gap_1()
            .ml_3()
            .pl_3()
            .border_l_2()
            .border_color(theme.border);
        if passages.is_empty() {
            return list.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("No related verses."),
            );
        }
        for (n, passage) in passages.into_iter().enumerate() {
            let text = translation.passage_text(&passage, 2).unwrap_or_default();
            list = list.child(
                div()
                    .id(("sp-rel", ix * 100 + n))
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_1()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|d| d.bg(theme.muted))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open_related(passage, OpenHow::Here, window, cx);
                    }))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.primary)
                            .child(passage.label()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .line_clamp(2)
                            .child(text),
                    )
                    .child(
                        Button::new(("sp-rel-tab", ix * 100 + n))
                            .icon(IconName::ExternalLink)
                            .ghost()
                            .xsmall()
                            .tooltip("Open in new tab")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.open_related(passage, OpenHow::NewTab, window, cx);
                            })),
                    ),
            );
        }
        list
    }

    pub(super) fn render_search_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let page = self.search.as_ref().expect("search page is open");
        let empty = page.text.trim().is_empty();

        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                Button::new("sp-back")
                    .icon(IconName::ArrowLeft)
                    .ghost()
                    .small()
                    .tooltip(format!("Back to {} (Esc)", page.came_from))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_search(&CloseSearch, window, cx)
                    })),
            )
            .child(
                div().flex_1().child(
                    Input::new(&self.search_input)
                        .prefix(Icon::new(IconName::Search).size_4())
                        .cleanable(true),
                ),
            );

        let body = if empty {
            let chips = page.suggestions.iter().enumerate().map(|(ix, s)| {
                let query = s.to_string();
                Button::new(("sp-suggest", ix))
                    .label(*s)
                    .small()
                    .outline()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.search_input.update(cx, |input, cx| {
                            input.set_value(query.clone(), window, cx);
                        });
                    }))
            });
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("Search words or topics. Put a phrase in quotes, and use -word to leave a word out."),
                )
                .child(div().flex().flex_wrap().gap_2().children(chips))
        } else if page.entries.is_empty() {
            div().flex_1().min_h_0().child(
                div()
                    .p_3()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(format!(
                        "No verses match \u{201c}{}\u{201d}.",
                        page.text.trim()
                    )),
            )
        } else {
            div().flex_1().min_h_0().pb_6().child(
                list(
                    page.list.clone(),
                    cx.processor(|this, ix: usize, _window, cx| this.render_row(ix, cx)),
                )
                .size_full(),
            )
        };

        div()
            .id("search-page")
            .key_context("SearchPage")
            .on_action(cx.listener(Self::close_search))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                // The text field would otherwise keep the arrows for itself.
                match event.keystroke.key.as_str() {
                    "down" => this.search_move(1, cx),
                    "up" => this.search_move(-1, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .bg(theme.background)
            .child(
                div()
                    .w_full()
                    .max_w(px(760.))
                    .h_full()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .font_family(".SystemUIFont")
                    .child(header)
                    .child(self.render_search_filters(page, cx))
                    .child(body),
            )
    }

    /// One virtualized line: a heading, the topic chips, or a result.
    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(page) = self.search.as_ref() else {
            return div().into_any_element();
        };
        let Some(row) = page.rows.get(ix).copied() else {
            return div().into_any_element();
        };
        let theme = cx.theme().clone();
        match row {
            Row::KeyHeading => {
                let mut col = div().flex().flex_col().gap_1().child(self.section_heading(
                    &format!("Key passages on {}", page.topic.clone().unwrap_or_default()),
                    cx,
                ));
                let others: Vec<String> = page
                    .results
                    .topics
                    .iter()
                    .map(|t| t.topic.name.clone())
                    .filter(|n| Some(n) != page.topic.as_ref())
                    .take(5)
                    .collect();
                if !others.is_empty() {
                    let chips = others.into_iter().enumerate().map(|(n, name)| {
                        let q = name.clone();
                        Button::new(("sp-topic", n))
                            .label(name)
                            .xsmall()
                            .outline()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.search_input.update(cx, |input, cx| {
                                    input.set_value(q.clone(), window, cx);
                                });
                            }))
                    });
                    col = col.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_1()
                            .px_3()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child("Also:"),
                            )
                            .children(chips),
                    );
                }
                col.into_any_element()
            }
            Row::Matches => {
                let total = page.results.total;
                let shown = page.results.hits.len();
                let mut note = if total > shown {
                    format!("Matches \u{b7} best {shown} of {total}")
                } else {
                    format!("Matches \u{b7} {total}")
                };
                if let Some(fixed) = &page.results.corrected {
                    note = format!("Showing results for \u{201c}{fixed}\u{201d} \u{b7} {note}");
                }
                self.section_heading(&note, cx).into_any_element()
            }
            Row::Entry(n) => div()
                .pb_1()
                .child(self.render_entry(n, &page.entries[n], page, cx))
                .into_any_element(),
        }
    }

    fn section_heading(&self, text: &str, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .pt_3()
            .pb_1()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(cx.theme().muted_foreground)
            .child(text.to_uppercase())
    }
}

/// How a search result is opened.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum OpenHow {
    Here,
    NewTab,
    Beside,
}
