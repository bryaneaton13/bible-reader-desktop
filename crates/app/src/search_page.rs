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
/// Verses per YouVersion search page. Each one costs a request for its text.
const YV_PAGE: u32 = 6;

/// A verse found by YouVersion's search, with its text.
pub(super) struct YvEntry {
    pub verse: VerseId,
    pub text: String,
}

#[derive(Default, Clone, PartialEq)]
pub(super) enum YvState {
    #[default]
    Idle,
    Loading,
    Done,
    Failed(String),
}

/// The optional "Search with YouVersion" part of the page. Local search is
/// always what runs; this only runs when the user asks.
#[derive(Default)]
pub(super) struct YvSearch {
    /// Bible id to search; the first saved Bible when `None`.
    pub bible: Option<u32>,
    pub state: YvState,
    pub entries: Vec<YvEntry>,
    pub next_token: Option<String>,
    pub did_you_mean: Vec<String>,
    /// API requests the last search actually used (cached ones are free).
    pub requests_used: u64,
}

impl YvSearch {
    /// A new query makes the old YouVersion results stale; keep the Bible.
    fn reset(&mut self) {
        *self = Self {
            bible: self.bible,
            ..Self::default()
        };
    }
}

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
    pub yv: YvSearch,
}

impl SearchPage {
    fn books(&self) -> std::ops::RangeInclusive<u8> {
        self.scope.as_ref().map_or(1..=u8::MAX, |s| s.books.clone())
    }

    /// Only the key passages come first; the ranked hits follow, without
    /// repeating verses already listed.
    fn run(&mut self, cx: &App) {
        self.yv.reset();
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
        let mut translation = self.tab().translation.clone();
        // Local search only knows the bundled translations; a YouVersion tab
        // searches the first of those and offers YouVersion's own search.
        let mut yv_bible = None;
        if let Some(id) = remote_id(&translation) {
            yv_bible = Some(id);
            translation = cx.global::<Library>().translations[0].info.abbrev.clone();
        }
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
            yv: YvSearch {
                bible: yv_bible,
                ..YvSearch::default()
            },
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

    /// The Bible YouVersion search would use: the chosen one, else the first
    /// the user added.
    fn yv_bible(&self, page: &SearchPage, cx: &App) -> Option<(u32, String)> {
        let saved = &cx.global::<Settings>().youversion.bibles;
        let pick = page
            .yv
            .bible
            .and_then(|id| saved.iter().find(|b| b.id == id))
            .or_else(|| saved.first())?;
        Some((pick.id, pick.abbrev.clone()))
    }

    /// Run YouVersion's search for the page's text (the next page when
    /// `more`). Costs one request for the search plus one per verse shown.
    pub(super) fn search_yv(&mut self, more: bool, cx: &mut Context<Self>) {
        let Some(page) = self.search.as_ref() else {
            return;
        };
        let query = page.text.trim().to_string();
        let Some((bible, _)) = self.yv_bible(page, cx) else {
            return;
        };
        if query.is_empty() || page.yv.state == YvState::Loading {
            return;
        }
        let token = if more {
            page.yv.next_token.clone()
        } else {
            None
        };
        let search_query = query.clone();
        let Some(task) = remote::request(cx, move |source| {
            let before = source.client().requests_made();
            let found = source.search(bible, &search_query, YV_PAGE, token.as_deref())?;
            let mut entries = Vec::new();
            // Verse text comes from whole chapters: results in the same chapter
            // share one request, cached chapters cost nothing, and opening a
            // result afterwards is instant.
            for verse in found.value.verses.iter().copied().take(YV_PAGE as usize) {
                let text = match source.chapter(bible, verse.chapter_ref()) {
                    Ok(fetched) => bible_youversion::verse_texts(&fetched.value.blocks)
                        .into_iter()
                        .find(|(v, _)| *v == verse.verse())
                        .map(|(_, text)| text)
                        .unwrap_or_default(),
                    // Stop early on errors that will repeat for every verse.
                    Err(
                        err @ (bible_youversion::Error::RateLimited { .. }
                        | bible_youversion::Error::NotPermitted
                        | bible_youversion::Error::Offline(_)),
                    ) => return Err(err),
                    Err(_) => String::new(),
                };
                entries.push(YvEntry { verse, text });
            }
            let used = source.client().requests_made().saturating_sub(before);
            Ok((found.value, entries, used))
        }) else {
            return;
        };
        if let Some(page) = self.search.as_mut() {
            if !more {
                page.yv.entries.clear();
                page.yv.did_you_mean.clear();
                page.yv.requests_used = 0;
            }
            page.yv.state = YvState::Loading;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |this, cx| {
                remote::apply_renewed(outcome.session, cx);
                let Some(page) = this.search.as_mut() else {
                    return;
                };
                // The text changed while we waited: these results are stale.
                if page.text.trim() != query {
                    return;
                }
                match outcome.result {
                    Ok((found, entries, used)) => {
                        page.yv.entries.extend(entries);
                        page.yv.next_token = found.next_page_token;
                        page.yv.did_you_mean = found.did_you_mean;
                        page.yv.requests_used += used;
                        page.yv.state = YvState::Done;
                    }
                    Err(err) => page.yv.state = YvState::Failed(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Open a YouVersion result in the Bible it was found in.
    fn open_yv_result(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = self.search.as_ref() else {
            return;
        };
        let Some(entry) = page.yv.entries.get(ix) else {
            return;
        };
        let Some((bible, _)) = self.yv_bible(page, cx) else {
            return;
        };
        let verse = entry.verse;
        self.search = None;
        self.focus.focus(window, cx);
        self.tab_mut().translation = crate::library::remote_key(bible);
        self.go_to(verse.chapter_ref(), Some(verse), cx);
    }

    /// The optional YouVersion search card. Hidden until an app key is set.
    fn render_yv_search(&self, page: &SearchPage, cx: &mut Context<Self>) -> Option<AnyElement> {
        if page.text.trim().is_empty() || remote::source(cx).is_none() {
            return None;
        }
        let theme = cx.theme().clone();
        let saved = cx.global::<Settings>().youversion.bibles.clone();
        let chosen = self.yv_bible(page, cx);
        let muted = theme.muted_foreground;
        let mut card = div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Icon::new(IconName::Globe).size_4().text_color(muted))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Search with YouVersion"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("optional · uses API requests"),
                    ),
            );

        let Some((_, label)) = chosen else {
            card = card
                .child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("Add a YouVersion Bible to search it here."),
                )
                .child(
                    div().flex().child(
                        Button::new("sp-yv-setup")
                            .label("Choose Bibles")
                            .small()
                            .on_click(|_, _, cx| crate::settings_window::open_sources(cx)),
                    ),
                );
            return Some(card.into_any_element());
        };

        if saved.len() > 1 {
            let chips = saved.iter().map(|b| {
                let id = b.id;
                let selected = Some(id) == page.yv.bible
                    || (page.yv.bible.is_none() && saved.first().map(|f| f.id) == Some(id));
                Button::new(("sp-yv-bible", id as usize))
                    .label(b.abbrev.clone())
                    .xsmall()
                    .ghost()
                    .selected(selected)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(page) = this.search.as_mut() {
                            page.yv.bible = Some(id);
                            page.yv.reset();
                        }
                        cx.notify();
                    }))
            });
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().text_xs().text_color(muted).w(px(72.)).child("Bible"))
                    .children(chips),
            );
        }

        let query = page.text.trim().to_string();
        let first_page = page.yv.entries.is_empty();
        match &page.yv.state {
            YvState::Idle => {
                card = card
                    .child(div().text_sm().text_color(muted).child(format!(
                        "Searches {label} on YouVersion. Uses 1 request to search, plus one for each chapter it needs to show the verses (none if you have read them already). Results are cached for 30 days."
                    )))
                    .child(
                        div().flex().child(
                            Button::new("sp-yv-search")
                                .icon(IconName::Search)
                                .label(format!("Search YouVersion for “{query}”"))
                                .small()
                                .primary()
                                .on_click(cx.listener(|this, _, _, cx| this.search_yv(false, cx))),
                        ),
                    );
            }
            YvState::Loading => {
                card = card
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .text_color(muted)
                            .child(Spinner::new().small())
                            .child(format!("Searching {label} on YouVersion…")),
                    )
                    .when(first_page, |c| {
                        c.children(
                            [0.9, 0.75, 0.85]
                                .map(|w| Skeleton::new().h_5().w(relative(w)).rounded_md()),
                        )
                    });
            }
            YvState::Failed(message) => {
                card = card
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap_2()
                            .text_sm()
                            .text_color(theme.danger)
                            .child(Icon::new(IconName::CircleAlert))
                            .child(message.clone()),
                    )
                    .child(
                        div().flex().child(
                            Button::new("sp-yv-retry")
                                .icon(IconName::RefreshCw)
                                .label("Try again")
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let more = this
                                        .search
                                        .as_ref()
                                        .is_some_and(|p| !p.yv.entries.is_empty());
                                    this.search_yv(more, cx)
                                })),
                        ),
                    );
            }
            YvState::Done => {
                let count = page.yv.entries.len();
                card = card.child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(muted)
                        .child(format!("{} · {label} · {count}", "YOUVERSION")),
                );
                if count == 0 {
                    card = card.child(div().text_sm().text_color(muted).child(format!(
                        "No results for “{query}” in {label}.{}",
                        page.yv
                            .did_you_mean
                            .first()
                            .map(|d| format!(" Did you mean “{d}”?"))
                            .unwrap_or_default()
                    )));
                }
                for (ix, entry) in page.yv.entries.iter().enumerate() {
                    let accent = theme.accent;
                    card = card.child(
                        div()
                            .id(("sp-yv-row", ix))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .p_2()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|d| d.bg(accent))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_yv_result(ix, window, cx)
                            }))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.primary)
                                    .child(entry.verse.to_string()),
                            )
                            .child(div().text_sm().child(if entry.text.is_empty() {
                                "(text unavailable)".to_string()
                            } else {
                                entry.text.clone()
                            })),
                    );
                }
                let used = page.yv.requests_used;
                card = card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .text_xs()
                                .text_color(muted)
                                .child(if used == 0 {
                                    "Served from cache: no requests used.".to_string()
                                } else {
                                    format!(
                                        "Used {used} API request{}.",
                                        if used == 1 { "" } else { "s" }
                                    )
                                }),
                        )
                        .when(page.yv.next_token.is_some(), |d| {
                            d.child(
                                Button::new("sp-yv-more")
                                    .label("More results")
                                    .xsmall()
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.search_yv(true, cx)),
                                    ),
                            )
                        }),
                );
            }
        }
        Some(card.into_any_element())
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
                    .children(self.render_yv_search(page, cx))
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
