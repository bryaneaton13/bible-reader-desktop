//! The main reading window: title-bar toolbar, tab bar, book sidebar,
//! chapter text, notes inspector and the ⌘K palette.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use bible_core::{BOOKS, Block, BlockKind, ChapterRef, Note, NoteKind, Testament, VerseId};
use bible_store::Passage;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::resizable::{ResizableState, h_resizable, resizable_panel};
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, IndexPath, Selectable as _, Sizable as _,
    TitleBar,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::chapter_view::{self, Options, Palette, note_marker, render_block};
use crate::library::{
    Appearance, FONT_SIZE_RANGE, FontPairing, LINE_HEIGHT_RANGE, Library, Settings,
    current_translation, is_available, remote_id, saved_bible, source_label, sources,
};
use crate::palette::{
    self, ActivateTab, COMMAND_PREFIX, CloseTabAt, OpenPassage, PinTab, RenameTab, Row,
    SEARCH_LIMIT, SUGGESTIONS_SHOWN, SearchIn, SetPaletteQuery,
};
use crate::remote;
use crate::suggestions;
use crate::tabs::{Location, Tab};
use crate::translation_picker::TranslationItem;
use crate::{
    ClosePalette, ClosePane, CloseTab, CopyChapter, CopyVerse, DecreaseFont, DecreaseLineSpacing,
    FocusLeftPane, FocusReader, FocusRightPane, FocusSidebar, GoBack, GoForward, IncreaseFont,
    IncreaseLineSpacing, NewTab, NextChapter, NextTab, OpenCommands, OpenInNewTab, OpenInSplit,
    OpenPalette, PrevChapter, PrevTab, ReopenClosedTab, ResetFont, ResetLineSpacing,
    SelectTranslation, SetAppearance, SetFontPairing, SidebarDown, SidebarLeft, SidebarNextBook,
    SidebarOpen, SidebarPrevBook, SidebarRight, SidebarUp, ToggleInspector, ToggleRedLetter,
    ToggleSidebar, ToggleSplit, ToggleVerseNumbers,
};

#[path = "search_page.rs"]
mod search_page;
use search_page::SearchPage;

/// Verse (or whole chapter) text with a citation and, for translations that
/// aren't public domain, the copyright notice. `key` is the focused pane's
/// translation; `content` its loaded chapter (YouVersion text lives there,
/// not in a database).
fn copy_text(
    cx: &App,
    content: Option<&Content>,
    key: &str,
    chapter: ChapterRef,
    verse: Option<VerseId>,
) -> Option<String> {
    let settings = cx.global::<Settings>();
    let (verses, abbrev, notice): (Vec<(u16, String)>, String, String) =
        if let Some(bible) = saved_bible(settings, key) {
            let texts = bible_youversion::verse_texts(&content?.blocks);
            let notice = if bible.copyright.trim().is_empty() {
                "Provided by YouVersion.".to_string()
            } else {
                format!("{} Provided by YouVersion.", bible.copyright.trim())
            };
            (texts, bible.abbrev.clone(), notice)
        } else {
            let library = cx.global::<Library>();
            let translation = &library.translations[library.index_of(key).unwrap_or(0)];
            let mut texts = Vec::new();
            for v in 1u16.. {
                match translation.verse_text(VerseId::new(chapter.book, chapter.chapter, v)) {
                    Ok(Some(t)) => texts.push((v, t)),
                    _ => break,
                }
            }
            (
                texts,
                translation.info.abbrev.clone(),
                translation.info.copyright.trim().to_string(),
            )
        };
    let (body, reference) = match verse {
        Some(id) => {
            let (_, text) = verses.iter().find(|(v, _)| *v == id.verse())?;
            (text.clone(), id.to_string())
        }
        None => {
            if verses.is_empty() {
                return None;
            }
            let lines: Vec<String> = verses.iter().map(|(v, t)| format!("{v} {t}")).collect();
            (lines.join("\n"), chapter.to_string())
        }
    };
    let mut out = if verse.is_some() {
        format!("“{body}” — {reference} ({abbrev})")
    } else {
        format!("{body}\n— {reference} ({abbrev})")
    };
    let notice = notice.trim();
    if !notice.is_empty() && !notice.to_lowercase().contains("public domain") {
        out.push('\n');
        out.push_str(notice);
    }
    Some(out)
}

const TOOLBAR_HEIGHT: f32 = 44.;
const CLOSED_TABS_KEPT: usize = 20;
/// Chapter tiles per row in the sidebar grid (keyboard Up / Down step).
const GRID_COLUMNS: u16 = 6;

/// Payload while a tab is being dragged to a new position.
#[derive(Clone)]
struct DraggedTab {
    from: usize,
}

/// Payload while a pane header is being dragged out to the tab strip.
#[derive(Clone)]
struct DraggedPane {
    right: bool,
}

/// Narrowest content area (sidebar and notes excluded) that still shows two
/// panes side by side; below it only the focused pane is shown.
const SPLIT_MIN_WIDTH: f32 = 760.;

/// What a pane is showing right now.
#[derive(Default, Clone, PartialEq)]
enum Load {
    #[default]
    Ready,
    /// A YouVersion chapter is being fetched.
    Loading,
    /// The fetch failed. `setup` means Settings can fix it (no key, not licensed).
    Failed { message: String, setup: bool },
}

/// Chapter text and footnotes for one pane.
#[derive(Default)]
struct Content {
    blocks: Vec<Block>,
    notes: Vec<Note>,
    load: Load,
}

impl Content {
    fn failed(err: &bible_youversion::Error) -> Self {
        Self {
            load: Load::Failed {
                message: err.to_string(),
                setup: matches!(
                    err,
                    bible_youversion::Error::MissingKey | bible_youversion::Error::NotPermitted
                ),
            },
            ..Self::default()
        }
    }

    fn from_chapter(chapter: bible_youversion::Chapter) -> Self {
        Self {
            blocks: chapter.blocks,
            notes: chapter.notes,
            load: Load::Ready,
        }
    }
}

/// How long a YouVersion fetch waits before it is sent, so chapters the
/// reader skims past are never requested.
const FETCH_DELAY: std::time::Duration =
    std::time::Duration::from_millis(if cfg!(test) { 0 } else { 200 });

/// A pane's chapter. YouVersion chapters come from the disk cache when they
/// are there (no network, so this is safe on the UI thread); otherwise the
/// pane is `Loading` and the caller starts a fetch.
fn load_content(cx: &App, abbrev: &str, chapter: ChapterRef) -> Content {
    if let Some(bible) = saved_bible(cx.global::<Settings>(), abbrev) {
        let Some(source) = remote::source(cx) else {
            return Content::failed(&bible_youversion::Error::MissingKey);
        };
        return match source.chapter_cached(bible.id, chapter) {
            Some(cached) => Content::from_chapter(cached),
            None => Content {
                load: Load::Loading,
                ..Content::default()
            },
        };
    }
    let library = cx.global::<Library>();
    let translation = &library.translations[library.index_of(abbrev).unwrap_or(0)];
    Content {
        blocks: translation
            .chapter(chapter)
            .ok()
            .flatten()
            .unwrap_or_default(),
        notes: translation.notes_for_chapter(chapter).unwrap_or_default(),
        load: Load::Ready,
    }
}

/// Index of the block holding `verse`, for scrolling to it.
fn block_of_verse(blocks: &[Block], verse: VerseId) -> usize {
    blocks
        .iter()
        .position(|b| {
            b.spans.iter().any(|s| matches!(s, bible_core::Span::Verse { num, end }
                if *num == verse.verse() || end.is_some_and(|e| *num <= verse.verse() && verse.verse() <= e)))
        })
        .unwrap_or(0)
}

/// Suffix that keeps element ids of the right pane apart from the left's.
fn pane_id(base: &'static str, right: bool) -> SharedString {
    if right {
        format!("{base}-right").into()
    } else {
        base.into()
    }
}

/// The floating label that follows the pointer during a tab drag.
struct TabGhost {
    label: String,
}

impl Render for TabGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .px_3()
            .py_1()
            .rounded_md()
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .shadow_md()
            .text_sm()
            .child(self.label.clone())
    }
}

/// Where the sidebar should scroll on the next render: a book row, or the
/// grid row holding a chapter.
type SidebarTarget = (u8, Option<u16>);

type TranslationSelect = SelectState<SearchableVec<TranslationItem>>;

pub struct ReaderView {
    focus: FocusHandle,
    tabs: Vec<Tab>,
    active: usize,
    /// Recently closed tabs for ⇧⌘T, newest last.
    closed: Vec<Tab>,
    /// Chapter text per pane, by pane (tab) id.
    content: HashMap<u64, Content>,
    selected_note: Option<u32>,
    inspector_open: bool,
    expanded_book: Option<u8>,
    sidebar_scroll: ScrollHandle,
    sidebar_focus: FocusHandle,
    /// Keyboard cursor in the sidebar chapter grid (Enter opens it).
    sidebar_cursor: ChapterRef,
    /// Book the sidebar last followed, so it only auto-scrolls on a change.
    sidebar_book: Option<u8>,
    scroll_sidebar: Cell<Option<SidebarTarget>>,
    palette_open: bool,
    /// Times the palette opened, to rotate its suggested searches.
    palette_opens: usize,
    /// An action run from the palette has taken over, so its on-confirm
    /// callback must not close or refocus anything.
    palette_handed_off: bool,
    /// Tab being renamed, and the field it is typed into.
    renaming: Option<usize>,
    rename_input: Entity<InputState>,
    palette: Entity<CommandState>,
    palette_groups: Vec<palette::Group>,
    translation_select: Entity<TranslationSelect>,
    /// Keys of the rows in the translation dropdowns, to notice when the user
    /// adds or removes a YouVersion Bible.
    source_keys: Vec<String>,
    /// Translation dropdown in the right pane's header.
    side_select: Entity<TranslationSelect>,
    split_state: Entity<ResizableState>,
    /// While a tab is dragged over the panes: which half it would land in
    /// (`true` = right). `None` when it can't be dropped there.
    drop_zone: Option<bool>,
    /// Show one pane at a time even when the window is wide.
    force_collapsed: bool,
    /// The search results page, when open (it covers the panes).
    search: Option<SearchPage>,
    search_input: Entity<InputState>,
    /// Pages opened so far, so suggested searches keep changing.
    search_opens: usize,
    /// Per-launch offset into the suggestion list.
    suggestion_seed: usize,
    _subscriptions: Vec<Subscription>,
}

impl ReaderView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = cx.global::<Settings>().clone();
        let mut tabs: Vec<Tab> = settings.tabs.iter().map(Tab::from_saved).collect();
        if tabs.is_empty() {
            tabs.push(Tab::new(
                Location::chapter(settings.chapter_ref()),
                settings.translation.clone(),
            ));
        }
        let fallback = cx.global::<Library>().translations[0].info.abbrev.clone();
        for tab in &mut tabs {
            if !is_available(cx, &tab.translation) {
                tab.translation = fallback.clone();
            }
            if let Some(side) = tab.side.as_mut()
                && !is_available(cx, &side.translation)
            {
                side.translation = fallback.clone();
            }
        }
        let active = settings.active_tab.min(tabs.len() - 1);

        let entries = sources(cx);
        let source_keys: Vec<String> = entries.iter().map(|e| e.key.clone()).collect();
        let make_items = || -> Vec<TranslationItem> {
            entries.iter().map(TranslationItem::from_entry).collect()
        };
        let selected_in = |key: &str| {
            source_keys
                .iter()
                .position(|k| k == key)
                .map(|ix| IndexPath::default().row(ix))
        };
        let selected = selected_in(&tabs[active].translation);
        let side_selected = tabs[active]
            .side
            .as_ref()
            .and_then(|s| selected_in(&s.translation));
        let (items, side_items) = (make_items(), make_items());
        let translation_select = cx.new(|cx| {
            SelectState::new(SearchableVec::new(items), selected, window, cx).searchable(true)
        });
        let items = side_items;
        let side_select = cx.new(|cx| {
            SelectState::new(SearchableVec::new(items), side_selected, window, cx).searchable(true)
        });
        let split_state = cx.new(|_| ResizableState::default());
        let palette = cx.new(|cx| CommandState::new(window, cx));
        let rename_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Tab name (blank shows the passage)")
        });
        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search words, a topic like “pride”, or \"a phrase\"")
        });

        let subscriptions = vec![
            cx.subscribe_in(&translation_select, window, Self::on_translation_selected),
            cx.subscribe_in(&side_select, window, Self::on_side_translation_selected),
            cx.subscribe_in(&rename_input, window, Self::on_rename_input),
            cx.subscribe_in(&search_input, window, Self::on_search_input),
            // Settings window changes and menu actions repaint the reader.
            cx.observe_global::<Settings>(|this, cx| this.on_settings_changed(cx)),
            // Sign-in and catalog changes repaint; a changed key or session can
            // turn a failed chapter into a loadable one on retry.
            cx.observe_global::<remote::Remote>(|_, cx| cx.notify()),
            cx.observe_window_appearance(window, |_, window, cx| {
                if cx.global::<Settings>().appearance == Appearance::System {
                    crate::appearance::reapply(window, cx);
                }
            }),
        ];
        let focus = cx.focus_handle();
        focus.focus(window, cx);

        let mut this = Self {
            focus,
            expanded_book: Some(tabs[active].focused().location.chapter.book),
            sidebar_scroll: ScrollHandle::new(),
            sidebar_focus: cx.focus_handle(),
            sidebar_cursor: tabs[active].focused().location.chapter,
            sidebar_book: None,
            scroll_sidebar: Cell::new(None),
            palette_handed_off: false,
            renaming: None,
            rename_input,
            tabs,
            active,
            closed: Vec::new(),
            content: HashMap::new(),
            selected_note: None,
            inspector_open: false,
            palette_open: false,
            palette_opens: 0,
            palette,
            palette_groups: Vec::new(),
            translation_select,
            source_keys: Vec::new(),
            side_select,
            split_state,
            drop_zone: None,
            force_collapsed: false,
            search: None,
            search_input,
            search_opens: 0,
            suggestion_seed: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as usize / 3600),
            _subscriptions: subscriptions,
        };
        this.show_active(cx);
        this
    }

    /// The pane navigation applies to (the only pane unless the tab is split).
    pub fn tab(&self) -> &Tab {
        self.tabs[self.active].focused()
    }

    fn tab_mut(&mut self) -> &mut Tab {
        self.tabs[self.active].focused_mut()
    }

    pub fn chapter(&self) -> ChapterRef {
        self.tab().location.chapter
    }

    fn target(&self) -> Option<VerseId> {
        self.tab().location.verse
    }

    /// One pane of the active tab: the left (`false`) or right (`true`).
    fn pane(&self, right: bool) -> Option<&Tab> {
        let tab = &self.tabs[self.active];
        if right {
            tab.side.as_deref()
        } else {
            Some(tab)
        }
    }

    fn focused_content(&self) -> Option<&Content> {
        self.content.get(&self.tab().id)
    }

    /// Load the focused pane's chapter, scroll to its verse, make sure the
    /// other pane has text too, and persist.
    fn show_active(&mut self, cx: &mut Context<Self>) {
        self.search = None;
        let focused = self.tab();
        let (id, location, abbrev) = (focused.id, focused.location, focused.translation.clone());
        let scroll = focused.scroll.clone();
        self.load_pane(id, &abbrev, location.chapter, cx);
        if let Some(v) = location.verse {
            // Child 0 of the scroll container is the chapter title.
            let blocks = self.content.get(&id).map_or(&[][..], |c| &c.blocks[..]);
            scroll.scroll_to_top_of_item(block_of_verse(blocks, v) + 1);
        }

        let tab = &self.tabs[self.active];
        let others: Vec<(u64, String, ChapterRef)> = [Some(tab), tab.side.as_deref()]
            .into_iter()
            .flatten()
            .filter(|p| p.id != id && !self.content.contains_key(&p.id))
            .map(|p| (p.id, p.translation.clone(), p.location.chapter))
            .collect();
        for (pane_id, abbrev, chapter) in others {
            self.load_pane(pane_id, &abbrev, chapter, cx);
        }
        let live: HashSet<u64> = self
            .tabs
            .iter()
            .flat_map(|t| [Some(t.id), t.side.as_ref().map(|s| s.id)])
            .flatten()
            .collect();
        self.content.retain(|id, _| live.contains(id));

        self.selected_note = None;
        self.sync_focus(cx);
    }

    /// Load one pane's chapter, fetching it from YouVersion when it isn't cached.
    fn load_pane(&mut self, id: u64, key: &str, chapter: ChapterRef, cx: &mut Context<Self>) {
        let content = load_content(cx, key, chapter);
        let fetch = content.load == Load::Loading;
        self.content.insert(id, content);
        if fetch {
            self.fetch_remote(id, key.to_string(), chapter, cx);
        }
    }

    /// The pane (left or right, any tab) with this id.
    fn find_pane(&self, id: u64) -> Option<&Tab> {
        self.tabs
            .iter()
            .flat_map(|t| std::iter::once(t).chain(t.side.as_deref()))
            .find(|p| p.id == id)
    }

    /// Fetch a YouVersion chapter on the background executor. The pane shows a
    /// loading state meanwhile; the result is dropped if the pane has moved on.
    ///
    /// To spend fewer API requests the fetch waits a moment and is skipped if
    /// the reader has already left the chapter, so skimming with ⌘] costs
    /// nothing. (The API serves one chapter per request; ranges answer 404.)
    fn fetch_remote(&mut self, id: u64, key: String, chapter: ChapterRef, cx: &mut Context<Self>) {
        let Some(bible) = remote_id(&key) else { return };
        if remote::source(cx).is_none() {
            self.content
                .insert(id, Content::failed(&bible_youversion::Error::MissingKey));
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FETCH_DELAY).await;
            let task = this
                .update(cx, |this, cx| {
                    let pane = this.find_pane(id)?;
                    if pane.translation != key || pane.location.chapter != chapter {
                        return None;
                    }
                    remote::request(cx, move |source| {
                        source.chapter(bible, chapter).map(|fetched| fetched.value)
                    })
                })
                .ok()
                .flatten();
            let Some(task) = task else { return };
            let outcome = task.await;
            this.update(cx, |this, cx| {
                remote::apply_renewed(outcome.session, cx);
                this.finish_remote(id, &key, chapter, outcome.result, cx);
            })
            .ok();
        })
        .detach();
    }

    fn finish_remote(
        &mut self,
        id: u64,
        key: &str,
        chapter: ChapterRef,
        result: Result<bible_youversion::Chapter, bible_youversion::Error>,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.find_pane(id) else {
            return;
        };
        if pane.translation != key || pane.location.chapter != chapter {
            return;
        }
        let verse = pane.location.verse;
        let scroll = pane.scroll.clone();
        let content = match result {
            Ok(fetched) => Content::from_chapter(fetched),
            Err(err) => Content::failed(&err),
        };
        if let Some(v) = verse {
            scroll.scroll_to_top_of_item(block_of_verse(&content.blocks, v) + 1);
        }
        self.content.insert(id, content);
        cx.notify();
    }

    /// "Try again" on a pane whose YouVersion chapter failed to load.
    fn retry_pane(&mut self, right: bool, cx: &mut Context<Self>) {
        let Some(pane) = self.pane(right) else { return };
        let (id, key, chapter) = (pane.id, pane.translation.clone(), pane.location.chapter);
        self.content.insert(
            id,
            Content {
                load: Load::Loading,
                ..Content::default()
            },
        );
        self.fetch_remote(id, key, chapter, cx);
        cx.notify();
    }

    /// A Bible was added to or removed from the user's YouVersion list: move
    /// any pane showing a removed one back to a bundled translation.
    fn on_settings_changed(&mut self, cx: &mut Context<Self>) {
        let fallback = cx.global::<Library>().translations[0].info.abbrev.clone();
        let mut changed = false;
        for tab in &mut self.tabs {
            if !is_available(cx, &tab.translation) {
                tab.translation = fallback.clone();
                changed = true;
            }
            if let Some(side) = tab.side.as_mut()
                && !is_available(cx, &side.translation)
            {
                side.translation = fallback.clone();
                changed = true;
            }
        }
        if changed {
            self.show_active(cx);
        }
        cx.notify();
    }

    /// Keep the translation dropdowns in step with the user's Bible list.
    fn sync_translation_items(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entries = sources(cx);
        let keys: Vec<String> = entries.iter().map(|e| e.key.clone()).collect();
        if keys == self.source_keys {
            return;
        }
        self.source_keys = keys;
        for select in [self.translation_select.clone(), self.side_select.clone()] {
            let items: Vec<TranslationItem> =
                entries.iter().map(TranslationItem::from_entry).collect();
            select.update(cx, |select, cx| {
                select.set_items(SearchableVec::new(items), window, cx)
            });
        }
    }

    /// Point everything that follows "the current pane" (sidebar, menu bar,
    /// saved state) at the focused pane.
    fn sync_focus(&mut self, cx: &mut Context<Self>) {
        let focused = self.tab();
        let (chapter, abbrev) = (focused.location.chapter, focused.translation.clone());
        self.expanded_book = Some(chapter.book);
        self.sidebar_cursor = chapter;
        if self.sidebar_book != Some(chapter.book) {
            self.sidebar_book = Some(chapter.book);
            self.scroll_sidebar.set(Some((chapter.book, None)));
        }
        cx.update_global::<Settings, _>(|s, _| s.translation = abbrev);
        self.save_tabs(cx);
        cx.notify();
    }

    fn save_tabs(&mut self, cx: &mut Context<Self>) {
        let saved = self.tabs.iter().map(Tab::to_saved).collect();
        let active = self.active;
        let chapter = self.chapter();
        cx.update_global::<Settings, _>(|s, _| {
            s.tabs = saved;
            s.active_tab = active;
            s.book = chapter.book;
            s.chapter = chapter.chapter;
            s.push_recent(chapter);
            s.save();
        });
        crate::tray::refresh(cx);
    }

    /// Navigate the active tab, recording the current place for Back.
    pub fn go_to(&mut self, chapter: ChapterRef, verse: Option<VerseId>, cx: &mut Context<Self>) {
        self.tab_mut().navigate(Location { chapter, verse });
        self.show_active(cx);
        self.scroll_to_top_if_no_verse(verse);
    }

    /// A fresh chapter opens at its title; show_active already scrolls to a verse.
    fn scroll_to_top_if_no_verse(&self, verse: Option<VerseId>) {
        if verse.is_none() {
            self.tab().scroll.scroll_to_top_of_item(0);
        }
    }

    /// Point the translation dropdown at the active tab's translation. Runs
    /// at render time because changing a `Select` needs the `Window`.
    fn sync_translation_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &self.tabs[self.active];
        let left = tab.translation.clone();
        let right = tab.side.as_ref().map(|s| s.translation.clone());
        for (select, abbrev) in [
            (self.translation_select.clone(), Some(left)),
            (self.side_select.clone(), right),
        ] {
            let Some(abbrev) = abbrev else { continue };
            if select.read(cx).selected_value() == Some(&abbrev) {
                continue;
            }
            if let Some(ix) = self.source_keys.iter().position(|k| *k == abbrev) {
                select.update(cx, |select, cx| {
                    select.set_selected_index(Some(IndexPath::default().row(ix)), window, cx)
                });
            }
        }
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab_mut().go_back().is_some() {
            self.show_active(cx);
        }
    }

    fn go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab_mut().go_forward().is_some() {
            self.show_active(cx);
        }
    }

    fn open_passage(&mut self, action: &OpenPassage, _: &mut Window, cx: &mut Context<Self>) {
        self.tab_mut().navigate(Location {
            chapter: action.chapter,
            verse: action.verse,
        });
        self.show_active(cx);
        self.scroll_to_top_if_no_verse(action.verse);
    }

    fn next_chapter(&mut self, _: &NextChapter, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(next) = self.chapter().next() {
            self.open_passage(
                &OpenPassage {
                    chapter: next,
                    verse: None,
                },
                window,
                cx,
            );
        }
    }

    fn prev_chapter(&mut self, _: &PrevChapter, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(prev) = self.chapter().prev() {
            self.open_passage(
                &OpenPassage {
                    chapter: prev,
                    verse: None,
                },
                window,
                cx,
            );
        }
    }

    // MARK: Tabs

    fn new_tab(&mut self, _: &NewTab, _: &mut Window, cx: &mut Context<Self>) {
        let tab = self.tab().duplicate();
        self.insert_tab_after_active(tab);
        self.show_active(cx);
    }

    /// Open a passage in a new tab beside the active one (⌘Enter in the palette).
    fn open_passage_in_new_tab(
        &mut self,
        chapter: ChapterRef,
        verse: Option<VerseId>,
        cx: &mut Context<Self>,
    ) {
        let mut tab = self.tab().duplicate();
        tab.location = Location { chapter, verse };
        self.insert_tab_after_active(tab);
        self.show_active(cx);
    }

    /// Insert beside the active tab and make it active. Pinned tabs always
    /// stay first, so an unpinned tab never lands among them.
    fn insert_tab_after_active(&mut self, tab: Tab) {
        self.active += 1;
        self.tabs.insert(self.active, tab);
        self.sort_pinned();
    }

    /// Keep pinned tabs first (stable), following the active tab by identity.
    fn sort_pinned(&mut self) {
        let id = self.tabs[self.active].id;
        self.tabs.sort_by_key(|t| !t.pinned);
        self.active = self.tabs.iter().position(|t| t.id == id).unwrap_or(0);
    }

    fn pin_tab(&mut self, action: &PinTab, _: &mut Window, cx: &mut Context<Self>) {
        self.pin_tab_at(action.index, cx);
    }

    fn pin_tab_at(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        tab.pinned = !tab.pinned;
        self.sort_pinned();
        self.save_tabs(cx);
    }

    /// Drag-and-drop: the dragged tab takes the drop target's position.
    fn move_tab(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let len = self.tabs.len();
        if from == to || from >= len || to >= len {
            return;
        }
        let id = self.tabs[self.active].id;
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = self.tabs.iter().position(|t| t.id == id).unwrap_or(0);
        // Dropping across the pin boundary snaps back to the right side of it.
        self.sort_pinned();
        self.save_tabs(cx);
    }

    fn rename_tab(&mut self, action: &RenameTab, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(action.index) else {
            return;
        };
        let current = tab.name.clone().unwrap_or_default();
        self.palette_handed_off = self.palette_open;
        self.palette_open = false;
        self.renaming = Some(action.index);
        self.rename_input.update(cx, |input, cx| {
            input.set_value(current, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn on_rename_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let InputEvent::PressEnter { .. } = event
            && let Some(ix) = self.renaming.take()
        {
            let name = input.read(cx).value();
            if let Some(tab) = self.tabs.get_mut(ix) {
                tab.rename(&name);
            }
            self.focus.focus(window, cx);
            self.save_tabs(cx);
            cx.notify();
        }
    }

    fn close_tab_action(
        &mut self,
        action: &CloseTabAt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action.index < self.tabs.len() {
            self.close_tab_at(action.index, window, cx);
        }
    }

    fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tab_at(self.active, window, cx);
    }

    fn close_tab_at(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() == 1 {
            // Last tab: close the window; the app stays in the menu bar.
            window.remove_window();
            return;
        }
        let tab = self.tabs.remove(ix);
        self.closed.push(tab);
        if self.closed.len() > CLOSED_TABS_KEPT {
            self.closed.remove(0);
        }
        if self.active > ix || self.active == self.tabs.len() {
            self.active -= 1;
        }
        self.show_active(cx);
    }

    fn reopen_closed_tab(&mut self, _: &ReopenClosedTab, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.closed.pop() {
            self.insert_tab_after_active(tab);
            self.show_active(cx);
        }
    }

    fn activate_tab(&mut self, action: &ActivateTab, _: &mut Window, cx: &mut Context<Self>) {
        if action.index < self.tabs.len() && action.index != self.active {
            self.active = action.index;
            self.show_active(cx);
        }
    }

    fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        let index = (self.active + 1) % self.tabs.len();
        self.activate_tab(&ActivateTab { index }, window, cx);
    }

    fn prev_tab(&mut self, _: &PrevTab, window: &mut Window, cx: &mut Context<Self>) {
        let index = (self.active + self.tabs.len() - 1) % self.tabs.len();
        self.activate_tab(&ActivateTab { index }, window, cx);
    }

    fn tab_title(&self, tab: &Tab) -> String {
        tab.label()
    }

    // MARK: Split panes

    /// Focus the left or right pane of a split tab (a click, ⌘⌥← / ⌘⌥→).
    fn focus_pane(&mut self, right: bool, cx: &mut Context<Self>) {
        let tab = &mut self.tabs[self.active];
        if !tab.is_split() || tab.focus_side == right {
            return;
        }
        tab.focus_side = right;
        self.selected_note = None;
        self.sync_focus(cx);
    }

    fn focus_left_pane(&mut self, _: &FocusLeftPane, _: &mut Window, cx: &mut Context<Self>) {
        self.focus_pane(false, cx);
    }

    fn focus_right_pane(&mut self, _: &FocusRightPane, _: &mut Window, cx: &mut Context<Self>) {
        self.focus_pane(true, cx);
    }

    /// ⌘\: show the focused passage a second time beside itself, or, when
    /// already split, turn the right pane into its own tab.
    fn toggle_split(&mut self, _: &ToggleSplit, _: &mut Window, cx: &mut Context<Self>) {
        if self.tabs[self.active].is_split() {
            self.pane_to_tab(true, false, cx);
        } else {
            let copy = self.tab().duplicate();
            self.tabs[self.active].split_with(copy, true);
            self.show_active(cx);
        }
    }

    /// Move a pane out into a tab of its own, right after the active one.
    fn pane_to_tab(&mut self, right: bool, activate: bool, cx: &mut Context<Self>) {
        let Some(pane) = self.tabs[self.active].take_pane(right) else {
            return;
        };
        let keep = self.tabs[self.active].id;
        self.insert_tab_after_active(pane);
        if !activate {
            self.active = self.tabs.iter().position(|t| t.id == keep).unwrap_or(0);
        }
        self.show_active(cx);
    }

    fn close_pane(&mut self, _: &ClosePane, _: &mut Window, cx: &mut Context<Self>) {
        let right = self.tabs[self.active].focus_side;
        self.close_pane_at(right, cx);
    }

    /// Close one pane of a split tab; it can be reopened with ⇧⌘T.
    fn close_pane_at(&mut self, right: bool, cx: &mut Context<Self>) {
        if let Some(pane) = self.tabs[self.active].take_pane(right) {
            self.closed.push(pane);
            if self.closed.len() > CLOSED_TABS_KEPT {
                self.closed.remove(0);
            }
            self.show_active(cx);
        }
    }

    /// Whether the dragged tab can be dropped beside the active one.
    fn can_split_with(&self, from: usize) -> bool {
        from != self.active
            && from < self.tabs.len()
            && !self.tabs[self.active].is_split()
            && !self.tabs[from].is_split()
    }

    /// Dragging a tab onto the left or right half of the text.
    fn drop_tab_on_panes(&mut self, from: usize, right: bool, cx: &mut Context<Self>) {
        self.drop_zone = None;
        if !self.can_split_with(from) {
            cx.notify();
            return;
        }
        let keep = self.tabs[self.active].id;
        let tab = self.tabs.remove(from);
        self.active = self.tabs.iter().position(|t| t.id == keep).unwrap_or(0);
        self.tabs[self.active].split_with(tab, right);
        self.show_active(cx);
    }

    /// ⌥Enter in the palette: open the passage in the other pane, splitting
    /// first when the tab is single.
    fn open_passage_in_split(
        &mut self,
        chapter: ChapterRef,
        verse: Option<VerseId>,
        cx: &mut Context<Self>,
    ) {
        let tab = &mut self.tabs[self.active];
        if tab.is_split() {
            tab.focus_side = !tab.focus_side;
        } else {
            let copy = tab.duplicate();
            tab.split_with(copy, true);
        }
        self.tab_mut().navigate(Location { chapter, verse });
        self.show_active(cx);
    }

    fn open_in_split(&mut self, _: &OpenInSplit, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.palette.read(cx).selected_index();
        let row = selected.and_then(|ix| self.palette_groups.get(ix.section)?.rows.get(ix.row));
        if let Some(row) = row {
            match row.passage() {
                Some((chapter, verse)) => self.open_passage_in_split(chapter, verse, cx),
                None => window.dispatch_action(row.action(), cx),
            }
        }
        self.close_palette(&ClosePalette, window, cx);
    }

    /// Fewer than two panes' worth of room: show just the focused one.
    fn collapsed(&self, available: f32) -> bool {
        self.force_collapsed || available < SPLIT_MIN_WIDTH
    }

    // MARK: Translation

    fn select_translation(
        &mut self,
        action: &SelectTranslation,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = sources(cx).into_iter().nth(action.index) else {
            return;
        };
        self.set_translation(entry.key, cx);
    }

    fn on_translation_selected(
        &mut self,
        _: &Entity<TranslationSelect>,
        event: &SelectEvent<SearchableVec<TranslationItem>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let SelectEvent::Confirm(Some(abbrev)) = event {
            self.focus_pane(false, cx);
            self.set_translation(abbrev.clone(), cx);
        }
        self.focus.focus(window, cx);
    }

    fn on_side_translation_selected(
        &mut self,
        _: &Entity<TranslationSelect>,
        event: &SelectEvent<SearchableVec<TranslationItem>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let SelectEvent::Confirm(Some(abbrev)) = event {
            self.focus_pane(true, cx);
            self.set_translation(abbrev.clone(), cx);
        }
        self.focus.focus(window, cx);
    }

    /// Switch the focused pane's translation.
    fn set_translation(&mut self, abbrev: String, cx: &mut Context<Self>) {
        self.tab_mut().translation = abbrev;
        self.show_active(cx);
        self.refresh_palette(cx);
    }

    fn update_settings(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Settings)) {
        cx.update_global::<Settings, _>(|s, _| {
            f(s);
            s.save();
        });
        cx.notify();
    }

    // MARK: Palette

    fn open_palette(&mut self, _: &OpenPalette, window: &mut Window, cx: &mut Context<Self>) {
        self.show_palette("", window, cx);
    }

    fn open_commands(&mut self, _: &OpenCommands, window: &mut Window, cx: &mut Context<Self>) {
        self.show_palette(&COMMAND_PREFIX.to_string(), window, cx);
    }

    fn show_palette(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = true;
        self.palette_opens += 1;
        self.palette.update(cx, |state, cx| {
            state.set_query(query.to_string(), window, cx);
            state.focus(window, cx);
        });
        self.refresh_palette(cx);
    }

    fn close_palette(&mut self, _: &ClosePalette, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = false;
        self.renaming = None;
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// ⇧⌘C: copy the jumped-to verse (or verse 1) with its citation.
    fn copy_verse(&mut self, _: &CopyVerse, _: &mut Window, cx: &mut Context<Self>) {
        let chapter = self.chapter();
        let verse = self
            .target()
            .unwrap_or(VerseId::new(chapter.book, chapter.chapter, 1));
        if let Some(text) = copy_text(
            cx,
            self.focused_content(),
            &self.tab().translation,
            chapter,
            Some(verse),
        ) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// ⌥⌘C: copy the whole chapter with its citation.
    fn copy_chapter(&mut self, _: &CopyChapter, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = copy_text(
            cx,
            self.focused_content(),
            &self.tab().translation,
            self.chapter(),
            None,
        ) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// ⌘Enter: open the highlighted passage in a new tab. Other rows run as usual.
    fn open_in_new_tab(&mut self, _: &OpenInNewTab, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.palette.read(cx).selected_index();
        let row = selected.and_then(|ix| self.palette_groups.get(ix.section)?.rows.get(ix.row));
        if let Some(row) = row {
            match row.passage() {
                Some((chapter, verse)) => self.open_passage_in_new_tab(chapter, verse, cx),
                None => window.dispatch_action(row.action(), cx),
            }
        }
        self.close_palette(&ClosePalette, window, cx);
    }

    /// Restart the palette scoped to a testament or book: "in:nt ".
    fn search_in(&mut self, action: &SearchIn, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_handed_off = self.palette_open;
        self.show_palette(&format!("in:{} ", action.scope), window, cx);
    }

    /// Suggested searches for this opening of the palette.
    fn palette_suggestions(&self) -> Vec<&'static str> {
        suggestions::pick(self.palette_opens + self.suggestion_seed, SUGGESTIONS_SHOWN)
    }

    /// Replace the palette's text with a suggested search.
    fn set_palette_query(
        &mut self,
        action: &SetPaletteQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_handed_off = self.palette_open;
        let opens = self.palette_opens;
        self.show_palette(&action.query, window, cx);
        self.palette_opens = opens;
    }

    fn refresh_palette(&mut self, cx: &mut Context<Self>) {
        let query = self.palette.read(cx).query(cx).to_string();
        let settings = cx.global::<Settings>();
        let library = cx.global::<Library>();
        let translation = current_translation(cx);
        let search = |q: &str, books| {
            library.search(
                translation,
                &bible_store::SearchRequest {
                    query: q,
                    mode: bible_store::MatchMode::All,
                    books,
                    limit: SEARCH_LIMIT,
                },
            )
        };
        let context = palette::Context {
            current: self.chapter(),
            active_tab: self.active,
            tabs: self.tabs.iter().map(|t| self.tab_title(t)).collect(),
            split: self.tabs[self.active].is_split(),
            recent: &settings.recent,
            translations: sources(cx),
            translation: &self.tab().translation,
            appearance: settings.appearance,
            font_pairing: settings.font_pairing,
            active_pinned: self.tabs[self.active].pinned,
            search: &search,
            suggestions: self.palette_suggestions(),
        };
        self.palette_groups = palette::build(&query, &context);
        cx.notify();
    }

    fn render_toolbar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let library = cx.global::<Library>();
        let settings = cx.global::<Settings>();
        let appearance = settings.appearance;
        let sidebar_open = settings.sidebar_open;
        // Searchable once there are more than a handful of translations.
        let versions = Select::new(&self.translation_select)
            .id("translation")
            .small()
            .w(px(96.))
            .menu_width(px(300.))
            .search_placeholder("Search translations…")
            .accessibility_label("Translation");
        let _ = library;

        let icon_button = |id: &'static str, icon: IconName, tooltip: &'static str| {
            Button::new(id).icon(icon).ghost().small().tooltip(tooltip)
        };

        let left = div()
            .flex()
            .items_center()
            .gap_1()
            .child(crate::app_icon::app_icon(26.).mr_1())
            .child(
                icon_button("sidebar", IconName::PanelLeft, "Toggle sidebar (⌃⌘S)")
                    .selected(sidebar_open)
                    .on_click(cx.listener(|_, _, window, cx| {
                        window.dispatch_action(Box::new(ToggleSidebar), cx)
                    })),
            )
            .child(
                icon_button("back", IconName::ArrowLeft, "Back (⌃⌘←)")
                    .disabled(!self.tab().can_go_back())
                    .on_click(cx.listener(|this, _, window, cx| this.go_back(&GoBack, window, cx))),
            )
            .child(
                icon_button("forward", IconName::ArrowRight, "Forward (⌃⌘→)")
                    .disabled(!self.tab().can_go_forward())
                    .on_click(
                        cx.listener(|this, _, window, cx| this.go_forward(&GoForward, window, cx)),
                    ),
            )
            .child(
                icon_button("new-tab", IconName::Plus, "New tab (⌘T)")
                    .on_click(cx.listener(|this, _, window, cx| this.new_tab(&NewTab, window, cx))),
            );

        let center = div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                icon_button("prev", IconName::ChevronLeft, "Previous chapter (⌘[)")
                    .disabled(self.chapter().prev().is_none())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.prev_chapter(&PrevChapter, window, cx)
                    })),
            )
            .child(
                Button::new("passage")
                    .icon(IconName::Search)
                    .label(self.chapter().to_string())
                    .outline()
                    .small()
                    .w(px(200.))
                    .tooltip("Go to passage or search (⌘K)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_palette(&OpenPalette, window, cx)
                    })),
            )
            .child(
                icon_button("next", IconName::ChevronRight, "Next chapter (⌘])")
                    .disabled(self.chapter().next().is_none())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.next_chapter(&NextChapter, window, cx)
                    })),
            );

        let right = div()
            .flex()
            .items_center()
            .gap_1()
            .when(!self.tabs[self.active].is_split(), |d| {
                d.child(versions).child(div().w(px(6.)))
            })
            .child(
                Popover::new("text-size-popover")
                    .anchor(Anchor::TopRight)
                    .trigger(icon_button("text-size", IconName::ALargeSmall, "Text size"))
                    .content(|_, _, cx| {
                        // A popover (not a menu) so it stays open for repeated clicks.
                        let current = cx.global::<Settings>().font_pairing;
                        let theme = cx.theme().clone();
                        let row =
                            |id: &'static str, label: &'static str, action: Box<dyn Action>| {
                                div()
                                    .id(id)
                                    .px_2()
                                    .py_1()
                                    .rounded(theme.radius)
                                    .cursor_pointer()
                                    .text_sm()
                                    .hover(|d| d.bg(theme.accent))
                                    .child(label)
                                    .on_click(move |_, window, cx| {
                                        window.dispatch_action(action.boxed_clone(), cx)
                                    })
                            };
                        let mut list = div()
                            .flex()
                            .flex_col()
                            .min_w(px(240.))
                            .child(row("font-up", "Larger  ⌘+", Box::new(IncreaseFont)))
                            .child(row("font-down", "Smaller  ⌘−", Box::new(DecreaseFont)))
                            .child(row("font-reset", "Actual Size  ⌘0", Box::new(ResetFont)))
                            .child(div().h(px(1.)).my_1().bg(theme.border))
                            .child(row(
                                "ls-up",
                                "Looser Line Spacing",
                                Box::new(IncreaseLineSpacing),
                            ))
                            .child(row(
                                "ls-down",
                                "Tighter Line Spacing",
                                Box::new(DecreaseLineSpacing),
                            ))
                            .child(row(
                                "ls-reset",
                                "Default Line Spacing",
                                Box::new(ResetLineSpacing),
                            ))
                            .child(div().h(px(1.)).my_1().bg(theme.border));
                        for pairing in FontPairing::ALL {
                            let action = SetFontPairing { pairing };
                            list = list.child(
                                div()
                                    .id(pairing.description())
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_2()
                                    .py_1()
                                    .rounded(theme.radius)
                                    .cursor_pointer()
                                    .text_sm()
                                    .hover(|d| d.bg(theme.accent))
                                    .child(div().w(px(14.)).when(pairing == current, |d| {
                                        d.child(Icon::new(IconName::Check).size_3())
                                    }))
                                    .child(pairing.description())
                                    .on_click(move |_, window, cx| {
                                        window.dispatch_action(action.boxed_clone(), cx)
                                    }),
                            );
                        }
                        list
                    }),
            )
            .child(
                icon_button(
                    "theme",
                    match appearance {
                        Appearance::Dark => IconName::Moon,
                        Appearance::Sepia => IconName::Palette,
                        Appearance::Light => IconName::Sun,
                        Appearance::System if theme.mode.is_dark() => IconName::Moon,
                        Appearance::System => IconName::Sun,
                    },
                    "Theme",
                )
                .dropdown_menu(move |mut menu, _, _| {
                    for a in Appearance::ALL {
                        menu = menu.menu_with_check(
                            a.label(),
                            a == appearance,
                            Box::new(SetAppearance { appearance: a }),
                        );
                    }
                    menu
                }),
            )
            .child(
                icon_button("notes", IconName::PanelRight, "Footnotes (⌥⌘N)")
                    .selected(self.inspector_open)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.inspector_open = !this.inspector_open;
                        cx.notify();
                    })),
            );

        TitleBar::new()
            .h(px(TOOLBAR_HEIGHT))
            .bg(theme.background)
            .border_color(theme.border)
            .when(window.is_fullscreen(), |d| d.pl_2())
            .child(
                div()
                    .flex()
                    .items_center()
                    .w_full()
                    .pr_2()
                    .child(div().flex_1().child(left))
                    .child(center)
                    .child(div().flex_1().flex().justify_end().child(right)),
            )
    }

    // MARK: Sidebar keyboard

    /// ⌥⌘B: show the sidebar and put the cursor on the current chapter.
    fn focus_sidebar(&mut self, _: &FocusSidebar, window: &mut Window, cx: &mut Context<Self>) {
        if !cx.global::<Settings>().sidebar_open {
            self.update_settings(cx, |s| s.sidebar_open = true);
        }
        self.sidebar_cursor = self.chapter();
        self.expanded_book = Some(self.sidebar_cursor.book);
        self.scroll_sidebar.set(Some((
            self.sidebar_cursor.book,
            Some(self.sidebar_cursor.chapter),
        )));
        self.sidebar_focus.focus(window, cx);
        cx.notify();
    }

    fn focus_reader(&mut self, _: &FocusReader, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn move_cursor(&mut self, to: ChapterRef, cx: &mut Context<Self>) {
        self.sidebar_cursor = to;
        self.expanded_book = Some(to.book);
        self.scroll_sidebar.set(Some((to.book, Some(to.chapter))));
        cx.notify();
    }

    fn sidebar_left(&mut self, _: &SidebarLeft, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(to) = self.sidebar_cursor.prev() {
            self.move_cursor(to, cx);
        }
    }

    fn sidebar_right(&mut self, _: &SidebarRight, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(to) = self.sidebar_cursor.next() {
            self.move_cursor(to, cx);
        }
    }

    fn sidebar_up(&mut self, _: &SidebarUp, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.sidebar_cursor;
        if c.chapter > GRID_COLUMNS {
            self.move_cursor(ChapterRef::new(c.book, c.chapter - GRID_COLUMNS), cx);
        }
    }

    fn sidebar_down(&mut self, _: &SidebarDown, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.sidebar_cursor;
        if c.chapter + GRID_COLUMNS <= c.book_info().chapters {
            self.move_cursor(ChapterRef::new(c.book, c.chapter + GRID_COLUMNS), cx);
        }
    }

    fn sidebar_prev_book(&mut self, _: &SidebarPrevBook, _: &mut Window, cx: &mut Context<Self>) {
        let book = self.sidebar_cursor.book;
        if book > 1 {
            self.move_cursor(ChapterRef::new(book - 1, 1), cx);
        }
    }

    fn sidebar_next_book(&mut self, _: &SidebarNextBook, _: &mut Window, cx: &mut Context<Self>) {
        let book = self.sidebar_cursor.book;
        if (book as usize) < BOOKS.len() {
            self.move_cursor(ChapterRef::new(book + 1, 1), cx);
        }
    }

    fn sidebar_open(&mut self, _: &SidebarOpen, window: &mut Window, cx: &mut Context<Self>) {
        let action = OpenPassage {
            chapter: self.sidebar_cursor,
            verse: None,
        };
        self.open_passage(&action, window, cx);
        self.focus.focus(window, cx);
    }

    fn render_sidebar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let has_focus = self.sidebar_focus.contains_focused(window, cx);
        let scroll_to = self.scroll_sidebar.take();
        let mut scroll_ix = None;
        // Child count so far: scrolling targets direct children of the list.
        let mut child_ix = 0usize;
        let mut list = div()
            .id("sidebar-books")
            .track_scroll(&self.sidebar_scroll)
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
            child_ix += 1;
            for book in BOOKS.iter().filter(|b| b.testament() == testament) {
                let num = book.num;
                let is_current = self.chapter().book == num;
                let expanded = self.expanded_book == Some(num);
                if scroll_to.is_some_and(|(b, c)| b == num && c.is_none()) {
                    scroll_ix = Some(child_ix);
                }
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
                child_ix += 1;
                if !expanded {
                    continue;
                }
                // One child per grid row, so the keyboard cursor can scroll to its row.
                for first in (1..=book.chapters).step_by(GRID_COLUMNS as usize) {
                    let last = (first + GRID_COLUMNS - 1).min(book.chapters);
                    if scroll_to.is_some_and(|(b, c)| {
                        b == num && c.is_some_and(|c| (first..=last).contains(&c))
                    }) {
                        scroll_ix = Some(child_ix);
                    }
                    let mut row = div().flex().gap_1().px_3().py(px(2.));
                    for chapter in first..=last {
                        let selected = is_current && self.chapter().chapter == chapter;
                        let cursor =
                            has_focus && self.sidebar_cursor == ChapterRef::new(num, chapter);
                        row = row.child(
                            div()
                                .id(("chapter", num as usize * 1000 + chapter as usize))
                                .w(px(30.))
                                .h(px(26.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_sm()
                                .border_1()
                                .border_color(if cursor {
                                    theme.ring
                                } else {
                                    transparent_black()
                                })
                                .text_xs()
                                .cursor_pointer()
                                .when(selected, |d| {
                                    d.bg(theme.primary).text_color(theme.primary_foreground)
                                })
                                .when(!selected, |d| d.hover(|d| d.bg(theme.sidebar_accent)))
                                .child(chapter.to_string())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    let action = OpenPassage {
                                        chapter: ChapterRef::new(num, chapter),
                                        verse: None,
                                    };
                                    this.open_passage(&action, window, cx)
                                })),
                        );
                    }
                    list = list.child(row);
                    child_ix += 1;
                }
            }
        }
        if let Some(ix) = scroll_ix {
            self.sidebar_scroll.scroll_to_item(ix);
        }
        div()
            .id("sidebar")
            .key_context("Sidebar")
            .track_focus(&self.sidebar_focus)
            .on_action(cx.listener(Self::sidebar_left))
            .on_action(cx.listener(Self::sidebar_right))
            .on_action(cx.listener(Self::sidebar_up))
            .on_action(cx.listener(Self::sidebar_down))
            .on_action(cx.listener(Self::sidebar_prev_book))
            .on_action(cx.listener(Self::sidebar_next_book))
            .on_action(cx.listener(Self::sidebar_open))
            .on_action(cx.listener(Self::focus_reader))
            .w(px(230.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(theme.sidebar_border)
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            .child(list)
    }

    /// One pane's chapter text. `available` is the width it can use.
    fn render_pane(&self, right: bool, available: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let settings = cx.global::<Settings>();
        let dark = theme.mode.is_dark();
        let font_size = settings.font_size;
        let line_height = settings.line_height;
        let pane = self.pane(right).expect("pane exists");
        let no_content = Content::default();
        let content = self.content.get(&pane.id).unwrap_or(&no_content);
        let pane_chapter = pane.location.chapter;
        let scroll = pane.scroll.clone();
        // Block ids and selection order stay apart between panes.
        let order_base = if right { 1_000_000 } else { 0 };
        // The column scales with font size (so bigger text keeps its characters
        // per line) and widens at breakpoints when the window has room.
        let breakpoint = match available {
            a if a >= 1500. => 1.5,
            a if a >= 1100. => 1.25,
            _ => 1.,
        };
        let wanted = settings.reading_width * (settings.font_size / 19.).max(1.) * breakpoint;
        let reading_width = px(wanted.min(available.max(320.)));
        let reading_font = SharedString::from(settings.font_pairing.body());
        let display_font = SharedString::from(settings.font_pairing.display());
        let number_font = SharedString::from(settings.font_pairing.numbers());
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
            italic_added: pane.translation == "KJV",
            target: pane.location.verse,
        };
        let book = pane_chapter.book;
        let chapter = pane_chapter.chapter;
        let note_ids: Vec<u32> = content.notes.iter().map(|n| n.id).collect();
        // Body text uses explicit runs, so it can't inherit from the parent div.
        let body_style = TextStyle {
            font_family: reading_font.clone(),
            font_size: px(font_size).into(),
            line_height: relative(line_height),
            color: theme.foreground,
            ..Default::default()
        };
        let note_index = |id: u32| note_ids.iter().position(|n| *n == id).unwrap_or(0);

        let mut column = div()
            .id(pane_id("chapter-scroll", right))
            .track_scroll(&scroll)
            .overflow_y_scroll()
            .size_full()
            .flex()
            .flex_col()
            .items_center();

        // Child 0: chapter title (go_to relies on this offset).
        column = column.child(
            div()
                .w_full()
                .max_w(reading_width)
                .px_6()
                .pt_4()
                .pb_4()
                .text_center()
                .font_family(".SystemUIFont")
                .text_color(theme.muted_foreground)
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .pb_4()
                        .child(match pane_chapter.prev() {
                            Some(prev) => Button::new(pane_id("top-prev", right))
                                .label(format!("‹ {prev}"))
                                .ghost()
                                .small()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.prev_chapter(&PrevChapter, window, cx)
                                }))
                                .into_any_element(),
                            None => div().into_any_element(),
                        })
                        .child(match pane_chapter.next() {
                            Some(next) => Button::new(pane_id("top-next", right))
                                .label(format!("{next} ›"))
                                .ghost()
                                .small()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.next_chapter(&NextChapter, window, cx)
                                }))
                                .into_any_element(),
                            None => div().into_any_element(),
                        }),
                )
                .child(
                    div()
                        .font_family(display_font.clone())
                        .text_sm()
                        .child(pane_chapter.book_info().name.to_uppercase()),
                )
                .child(
                    div()
                        .font_family(display_font.clone())
                        .text_size(px(font_size * 2.4))
                        .text_color(theme.foreground)
                        .child(chapter.to_string()),
                ),
        );

        if content.load != Load::Ready {
            column = column.child(self.render_pane_status(
                right,
                &content.load,
                &pane.translation,
                pane_chapter,
                reading_width,
                font_size,
                cx,
            ));
        }

        // Verse still running at the start of each block (poetry lines,
        // paragraph breaks mid-verse), so the jump target is highlighted whole.
        let mut verse_in_progress = None;
        for (ix, block) in content.blocks.iter().enumerate() {
            let rendered = render_block(
                block,
                verse_in_progress,
                |v| VerseId::new(book, chapter, v),
                &note_index,
                &palette,
                &options,
            );
            if let Some(last) = rendered.verses.last() {
                verse_in_progress = Some(*last);
            }
            let note_ids_for_click: Vec<u32> = rendered.notes.iter().map(|(_, id)| *id).collect();
            let ranges: Vec<_> = rendered.notes.iter().map(|(r, _)| r.clone()).collect();
            // Footnote text for the hover popover, by marker range.
            let tips: Vec<(std::ops::Range<usize>, SharedString)> = rendered
                .notes
                .iter()
                .filter_map(|(range, id)| {
                    let note = content.notes.iter().find(|n| n.id == *id)?;
                    let label = match note.kind {
                        NoteKind::Footnote => "",
                        NoteKind::CrossReference => "Cross-ref · ",
                    };
                    let text = format!("{} {label}{}", note_marker(note_index(*id)), note.text);
                    Some((range.clone(), text.into()))
                })
                .collect();
            let styled = if block.kind.is_heading() {
                StyledText::new(rendered.text.clone()).with_highlights(rendered.highlights.clone())
            } else {
                let mut block_style = body_style.clone();
                if block.kind == BlockKind::PsalmTitle {
                    block_style.font_size = px(font_size * 0.85).into();
                    block_style.font_style = FontStyle::Italic;
                    block_style.color = theme.muted_foreground;
                }
                let runs = chapter_view::build_runs(&rendered, &block_style, Some(&number_font));
                StyledText::new(rendered.text.clone()).with_runs(runs)
            };
            let text_layout = styled.layout().clone();
            let plain = rendered.text.clone();
            let entity = cx.entity().downgrade();
            let text = InteractiveText::new(("block", ix), styled)
                .on_click(ranges, move |range_ix, _, cx| {
                    if let Some(id) = note_ids_for_click.get(range_ix).copied() {
                        entity
                            .update(cx, |this, cx| {
                                this.selected_note = Some(id);
                                this.inspector_open = true;
                                cx.notify();
                            })
                            .ok();
                    }
                })
                .tooltip(move |ix, window, cx| {
                    let (_, text) = tips.iter().find(|(range, _)| range.contains(&ix))?;
                    let text = text.clone();
                    Some(
                        Tooltip::element(move |_, _| div().max_w(px(340.)).child(text.clone()))
                            .build(window, cx),
                    )
                });
            let text = crate::selectable::SelectableBlock::new(
                ("select", ix),
                order_base + ix as u64,
                plain.into(),
                text_layout,
                text,
            );

            let base = div().w_full().max_w(reading_width).px_6();
            let el = match &block.kind {
                BlockKind::MajorHeading => base
                    .pt_6()
                    .pb_2()
                    .text_center()
                    .font_family(display_font.clone())
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
                    .font_family(display_font.clone())
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
                    .font_family(display_font.clone())
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
                    let next_is_poetry = content
                        .blocks
                        .get(ix + 1)
                        .is_some_and(|b| chapter_view::block_is_poetry(&b.kind));
                    base.when(!next_is_poetry, |d| d.pb(px(font_size * 0.6)))
                        .child(text)
                }
            };
            column = column.child(el);
        }

        if let Some(bible) = saved_bible(settings, &pane.translation)
            && content.load == Load::Ready
        {
            // YouVersion requires the version's attribution next to its text.
            column = column.child(
                div()
                    .w_full()
                    .max_w(reading_width)
                    .px_6()
                    .pt_8()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .font_family(".SystemUIFont")
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("{} · {}", bible.abbrev, bible.name))
                    .when(!bible.copyright.trim().is_empty(), |d| {
                        d.child(bible.copyright.trim().to_string())
                    })
                    .child("Provided by YouVersion"),
            );
        }

        let next = pane_chapter.next();
        column = column.child(
            div()
                .w_full()
                .max_w(reading_width)
                .px_6()
                .pt_10()
                .pb_16()
                .flex()
                .justify_between()
                .font_family(".SystemUIFont")
                .child(
                    Button::new(pane_id("bottom-prev", right))
                        .label(
                            pane_chapter
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
                    Button::new(pane_id("bottom-next", right))
                        .label(next.map(|c| format!("{c} ›")).unwrap_or_default())
                        .ghost()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.next_chapter(&NextChapter, window, cx)
                        })),
                ),
        );

        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .font_family(reading_font)
            .text_size(px(font_size))
            .line_height(relative(line_height))
            .text_color(theme.foreground)
            .child(column)
    }

    /// What a YouVersion pane shows instead of text: placeholder lines while
    /// the chapter loads, or why it couldn't.
    #[allow(clippy::too_many_arguments)]
    fn render_pane_status(
        &self,
        right: bool,
        load: &Load,
        key: &str,
        chapter: ChapterRef,
        reading_width: Pixels,
        font_size: f32,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let label = source_label(cx, key);
        let wrap = div()
            .id(pane_id("pane-status", right))
            .w_full()
            .max_w(reading_width)
            .px_6()
            .pb_4()
            .font_family(".SystemUIFont");
        match load {
            Load::Ready => div().into_any_element(),
            Load::Loading => {
                let line = |w: f32| {
                    Skeleton::new()
                        .h(px(font_size * 0.9))
                        .w(relative(w))
                        .rounded_md()
                };
                wrap.flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(Spinner::new().small())
                            .child(format!("Loading {chapter} ({label}) from YouVersion…")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .pt_2()
                            .children([0.97, 0.92, 0.99, 0.86, 0.58].map(line)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .pt_2()
                            .children([0.95, 0.99, 0.9, 0.96, 0.42].map(line)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .pt_2()
                            .children([0.98, 0.9, 0.94, 0.7].map(line)),
                    )
                    .into_any_element()
            }
            Load::Failed { message, setup } => {
                let fallback = cx.global::<Library>().translations[0].info.abbrev.clone();
                let read_in = fallback.clone();
                wrap.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .p_4()
                        .rounded_lg()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.sidebar)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(Icon::new(IconName::TriangleAlert).text_color(theme.warning))
                                .child(format!("Couldn't load {chapter} ({label})")),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(message.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_2()
                                .child(
                                    Button::new(pane_id("retry", right))
                                        .icon(IconName::RefreshCw)
                                        .label("Try again")
                                        .small()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.retry_pane(right, cx)
                                        })),
                                )
                                .when(*setup, |row| {
                                    row.child(
                                        Button::new(pane_id("open-sources", right))
                                            .icon(IconName::Settings)
                                            .label("Bible sources settings")
                                            .small()
                                            .on_click(|_, _, cx| {
                                                crate::settings_window::open_sources(cx)
                                            }),
                                    )
                                })
                                .child(
                                    Button::new(pane_id("read-local", right))
                                        .label(format!("Read in {fallback} instead"))
                                        .ghost()
                                        .small()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.focus_pane(right, cx);
                                            this.set_translation(read_in.clone(), cx);
                                        })),
                                ),
                        ),
                )
                .into_any_element()
            }
        }
    }

    /// Header above a pane of a split tab: translation, passage, close.
    fn render_pane_header(&self, right: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let tab = &self.tabs[self.active];
        let pane = self.pane(right).expect("pane exists");
        let focused = tab.focus_side == right;
        let select = if right {
            &self.side_select
        } else {
            &self.translation_select
        };
        let versions = Select::new(select)
            .id(pane_id("translation", right))
            .small()
            .w(px(96.))
            .menu_width(px(300.))
            .search_placeholder("Search translations…")
            .accessibility_label("Translation");
        let label = pane.title();
        let ghost = pane.pane_label();
        div()
            .id(pane_id("pane-header", right))
            .flex()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .h(px(32.))
            .pl_2()
            .pr_1()
            .border_b_1()
            .border_color(theme.border)
            .relative()
            .bg(if focused {
                theme.background
            } else {
                theme.sidebar
            })
            // Accent line marks the pane that navigation applies to.
            .when(focused, |d| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(2.))
                        .bg(theme.primary),
                )
            })
            .child(div().flex_shrink_0().w(px(96.)).child(versions))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(!focused, |d| d.text_color(theme.muted_foreground))
                    .child(label),
            )
            .child(
                Button::new(pane_id("close-pane", right))
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .tooltip("Close pane (⇧⌘\\)")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.close_pane_at(right, cx)
                    })),
            )
            .on_drag(DraggedPane { right }, move |_, _, _, cx| {
                cx.new(|_| TabGhost {
                    label: ghost.clone(),
                })
            })
    }

    /// A pane with its header (when split). Pressing anywhere in it, text
    /// selection included, gives it focus.
    fn render_pane_frame(
        &self,
        right: bool,
        available: f32,
        header: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .capture_any_mouse_down(cx.listener(move |this, _, _, cx| this.focus_pane(right, cx)))
            .when(header, |d| d.child(self.render_pane_header(right, cx)))
            .child(self.render_pane(right, available, cx))
    }

    /// Everything right of the sidebar and left of the notes: one pane, or
    /// two side by side (one at a time in a narrow window).
    fn render_panes(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = cx.global::<Settings>();
        let viewport = f32::from(window.viewport_size().width);
        let available = viewport
            - if settings.sidebar_open { 230. } else { 0. }
            - if self.inspector_open { 280. } else { 0. };
        let theme = cx.theme();
        let (border, accent, primary, popover) =
            (theme.border, theme.accent, theme.primary, theme.popover);
        let split = self.tabs[self.active].is_split();
        let collapsed = split && self.collapsed(available);

        let body = if !split {
            self.render_pane_frame(false, available, false, cx)
                .into_any_element()
        } else if collapsed {
            let tab = &self.tabs[self.active];
            let mut switcher = div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap_1()
                .p_1()
                .border_b_1()
                .border_color(border);
            for right in [false, true] {
                let pane = self.pane(right).expect("split has both panes");
                let selected = tab.focus_side == right;
                switcher = switcher.child(
                    Button::new(pane_id("pane-switch", right))
                        .label(pane.pane_label())
                        .small()
                        .ghost()
                        .selected(selected)
                        .on_click(cx.listener(move |this, _, _, cx| this.focus_pane(right, cx))),
                );
            }
            let right = tab.focus_side;
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(switcher)
                .child(self.render_pane_frame(right, available, true, cx))
                .into_any_element()
        } else {
            let half = available / 2.;
            let panel = |right: bool, cx: &mut Context<Self>| {
                resizable_panel()
                    .size_range(px(280.)..gpui_kit::Pixels::MAX)
                    .child(
                        div()
                            .size_full()
                            .flex()
                            .child(self.render_pane_frame(right, half, true, cx)),
                    )
            };
            h_resizable("split-panes")
                .with_state(&self.split_state)
                .child(panel(false, cx))
                .child(panel(true, cx))
                .into_any_element()
        };

        let zone = self.drop_zone;
        div()
            .id("panes")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .on_drag_move::<DraggedTab>(cx.listener(
                |this, event: &DragMoveEvent<DraggedTab>, _, cx| {
                    let from = event.drag(cx).from;
                    let inside = event.bounds.contains(&event.event.position);
                    let zone = (inside && this.can_split_with(from))
                        .then(|| event.event.position.x > event.bounds.center().x);
                    if this.drop_zone != zone {
                        this.drop_zone = zone;
                        cx.notify();
                    }
                },
            ))
            .on_drop(cx.listener(|this, dragged: &DraggedTab, _, cx| {
                if let Some(right) = this.drop_zone {
                    this.drop_tab_on_panes(dragged.from, right, cx)
                }
            }))
            .child(body)
            .when_some(zone, |d, right| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .w(relative(0.5))
                        .when(right, |d| d.right_0())
                        .when(!right, |d| d.left_0())
                        .m_2()
                        .rounded_lg()
                        .border_2()
                        .border_color(primary)
                        .bg(accent.opacity(0.25))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(popover)
                                .text_sm()
                                .child(if right {
                                    "Open on the right"
                                } else {
                                    "Open on the left"
                                }),
                        ),
                )
            })
    }

    fn render_inspector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let abbrev = source_label(cx, &self.tab().translation);
        let no_content = Content::default();
        let notes = &self.focused_content().unwrap_or(&no_content).notes;
        let mut list = div()
            .id("notes")
            .flex()
            .flex_col()
            .gap_2()
            .overflow_y_scroll()
            .size_full()
            .p_3();
        if let Some(related) = self.render_related_verses(cx) {
            list = list.child(related);
        }
        if notes.is_empty() {
            list = list.child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("No notes in this chapter."),
            );
        }
        for (ix, note) in notes.iter().enumerate() {
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
                        let Some(id) = this
                            .focused_content()
                            .and_then(|c| c.notes.get(ix))
                            .map(|n| n.id)
                        else {
                            return;
                        };
                        this.selected_note = Some(id);
                        this.tab_mut().location.verse = Some(verse);
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
                    .flex()
                    .items_center()
                    .justify_between()
                    .pl_3()
                    .pr_2()
                    .py_1()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .border_b_1()
                    .border_color(theme.border)
                    .child(format!("NOTES · {abbrev}"))
                    .child(
                        Button::new("close-notes")
                            .icon(IconName::Close)
                            .ghost()
                            .xsmall()
                            .tooltip("Close notes (⌥⌘N)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.inspector_open = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(list)
    }

    /// Cross-references for the verse the reader jumped to, above the notes.
    /// `None` when there is no verse or nothing related.
    fn render_related_verses(&self, cx: &mut Context<Self>) -> Option<Div> {
        let theme = cx.theme().clone();
        let verse = self.target()?;
        let passages = cx.global::<Library>().study.as_ref()?.related(verse, 6);
        if passages.is_empty() {
            return None;
        }
        let translation = current_translation(cx);
        let rows: Vec<(Passage, String)> = passages
            .into_iter()
            .map(|p| {
                let text = translation.passage_text(&p, 2).unwrap_or_default();
                (p, text)
            })
            .collect();
        let mut section = div()
            .flex()
            .flex_col()
            .gap_1()
            .pb_2()
            .mb_1()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(format!("RELATED · {verse}")),
            );
        for (ix, (passage, text)) in rows.into_iter().enumerate() {
            let target = passage.start;
            section = section.child(
                div()
                    .id(("related", ix))
                    .p_2()
                    .rounded_md()
                    .text_sm()
                    .cursor_pointer()
                    .hover(|d| d.bg(theme.muted))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.go_to(target.chapter_ref(), Some(target), cx);
                    }))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.primary)
                            .child(passage.label()),
                    )
                    .child(div().line_clamp(3).child(text)),
            );
        }
        Some(section)
    }

    fn render_palette(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let view = cx.entity().downgrade();
        let close = {
            let view = view.clone();
            move |window: &mut Window, cx: &mut App| {
                view.update(cx, |this, cx| this.close_palette(&ClosePalette, window, cx))
                    .ok();
            }
        };
        // Commands that take over the palette (rename, scoped search) set
        // `palette_handed_off`; closing would steal their focus.
        let close_on_confirm = {
            let view = view.clone();
            move |window: &mut Window, cx: &mut App| {
                view.update(cx, |this, cx| {
                    if std::mem::take(&mut this.palette_handed_off) {
                        cx.notify();
                    } else {
                        this.close_palette(&ClosePalette, window, cx);
                    }
                })
                .ok();
            }
        };

        let mut command = Command::new(&self.palette)
            .filterable(false)
            .placeholder(format!(
                "Go to a passage (jn 3:16), or search. Try “{}”",
                self.palette_suggestions()
                    .first()
                    .copied()
                    .unwrap_or("pride")
            ))
            .max_h(px(440.))
            .on_query({
                let view = view.clone();
                move |_, _, cx| {
                    view.update(cx, |this, cx| this.refresh_palette(cx)).ok();
                }
            })
            .on_confirm(move |_, window, cx| close_on_confirm(window, cx))
            .on_cancel(close.clone())
            .empty(|_, _, cx| {
                div()
                    .p_4()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No matches")
            })
            .footer(|_, _, cx| {
                let hint = |key: &str, label: &'static str| {
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(Kbd::new(Keystroke::parse(key).expect("valid keystroke")))
                        .child(label)
                };
                div()
                    .flex()
                    .gap_4()
                    .px_3()
                    .py_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(hint("up", "").child(hint("down", "Navigate")))
                    .child(hint("enter", "Open"))
                    .child(hint("cmd-enter", "New tab"))
                    .child(hint("alt-enter", "Beside"))
                    .child(hint("escape", "Close"))
                    .child(hint("shift-cmd-p", "Commands"))
            });

        for group in &self.palette_groups {
            let items = group.rows.iter().map(|row| {
                let item = CommandItem::new().label(row.label()).action(row.action());
                match row {
                    Row::Goto(_) => item.icon(IconName::BookOpen),
                    Row::Tab { .. } => item.icon(IconName::GalleryVerticalEnd),
                    Row::Recent(_) => item.icon(IconName::FileText),
                    Row::Command { .. } => item,
                    Row::Topic { .. } => item.icon(IconName::BookOpen),
                    Row::SeeAll { .. } => item.icon(IconName::Search),
                    Row::Suggestion(_) => item.icon(IconName::Search),
                    Row::Verse(hit) => {
                        let reference = hit.verse.to_string();
                        let text = hit.text.clone();
                        item.child(move |_, cx| {
                            div()
                                .w_full()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .py_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(cx.theme().muted_foreground)
                                        .child(reference.clone()),
                                )
                                .child(div().text_sm().line_clamp(2).child(text.clone()))
                        })
                    }
                }
            });
            command = command.group(
                CommandGroup::new()
                    .label(group.heading.clone())
                    .items(items),
            );
        }

        div()
            .id("palette-backdrop")
            .occlude()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(72.))
            .bg(hsla(0., 0., 0., 0.25))
            .on_click(
                cx.listener(|this, _, window, cx| this.close_palette(&ClosePalette, window, cx)),
            )
            .child(
                div()
                    .id("palette")
                    .key_context("Palette")
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .w(px(600.))
                    .rounded_lg()
                    .bg(theme.popover)
                    .shadow_lg()
                    .font_family(".SystemUIFont")
                    .child(command),
            )
    }

    fn render_rename(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .id("rename-backdrop")
            .occlude()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(72.))
            .bg(hsla(0., 0., 0., 0.25))
            .on_click(
                cx.listener(|this, _, window, cx| this.close_palette(&ClosePalette, window, cx)),
            )
            .child(
                div()
                    .id("rename")
                    .key_context("Palette")
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .w(px(420.))
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .rounded_lg()
                    .bg(theme.popover)
                    .shadow_lg()
                    .font_family(".SystemUIFont")
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Rename tab"),
                    )
                    .child(Input::new(&self.rename_input))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Enter to save. A named tab keeps its place as a saved view."),
                    ),
            )
    }

    fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (accent, muted) = (theme.accent, theme.muted);
        let last = self.tabs.len() - 1;
        let mut strip = div().id("tab-strip").flex().min_w_0().overflow_x_scroll();
        for (ix, tab) in self.tabs.iter().enumerate() {
            let selected = ix == self.active;
            let pinned = tab.pinned;
            let split = tab.is_split();
            let label = self.tab_title(tab);
            let ghost = label.clone();
            strip = strip.child(
                div()
                    .id(("tab", ix))
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .gap_1()
                    .h(px(31.))
                    .pl_3()
                    .pr(px(if pinned { 12. } else { 6. }))
                    .text_sm()
                    .cursor_pointer()
                    .border_r_1()
                    .border_color(theme.border)
                    .when(selected, |d| {
                        d.bg(theme.background).text_color(theme.foreground)
                    })
                    .when(!selected, |d| {
                        d.text_color(theme.muted_foreground)
                            .hover(move |d| d.bg(muted))
                    })
                    .when(pinned, |d| {
                        d.child(Icon::new(IconName::Star).small().text_color(theme.primary))
                    })
                    .child(
                        div()
                            .max_w(px(if pinned {
                                120.
                            } else if split {
                                360.
                            } else {
                                220.
                            }))
                            .truncate()
                            .child(label),
                    )
                    .when(!pinned, |d| {
                        d.child(
                            Button::new(("close-tab", ix))
                                .icon(IconName::Close)
                                .ghost()
                                .xsmall()
                                .tooltip("Close tab (⌘W)")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.close_tab_at(ix, window, cx);
                                })),
                        )
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.activate_tab(&ActivateTab { index: ix }, window, cx)
                    }))
                    .on_drag(DraggedTab { from: ix }, move |_, _, _, cx| {
                        cx.new(|_| TabGhost {
                            label: ghost.clone(),
                        })
                    })
                    .drag_over::<DraggedTab>(move |style, _, _, _| style.bg(accent))
                    .on_drop(cx.listener(move |this, dragged: &DraggedTab, _, cx| {
                        this.move_tab(dragged.from, ix, cx)
                    }))
                    .drag_over::<DraggedPane>(move |style, _, _, _| style.bg(accent))
                    .on_drop(cx.listener(|this, dragged: &DraggedPane, _, cx| {
                        this.pane_to_tab(dragged.right, true, cx)
                    }))
                    .context_menu(move |menu, _, _| {
                        menu.menu(
                            if pinned { "Unpin Tab" } else { "Pin Tab" },
                            Box::new(PinTab { index: ix }),
                        )
                        .menu("Rename Tab…", Box::new(RenameTab { index: ix }))
                        .menu(
                            if split {
                                "Unsplit (Separate Tabs)"
                            } else {
                                "Split Right"
                            },
                            Box::new(ToggleSplit),
                        )
                        .separator()
                        .menu("Close Tab", Box::new(CloseTabAt { index: ix }))
                    }),
            );
        }
        div()
            .flex()
            .items_center()
            .h(px(32.))
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(strip)
            // Dropping on the empty space moves the tab to the end.
            .child(
                div()
                    .id("tab-bar-end")
                    .flex_1()
                    .h_full()
                    .drag_over::<DraggedTab>(move |style, _, _, _| style.bg(accent))
                    .on_drop(cx.listener(move |this, dragged: &DraggedTab, _, cx| {
                        this.move_tab(dragged.from, last, cx)
                    }))
                    .drag_over::<DraggedPane>(move |style, _, _, _| style.bg(accent))
                    .on_drop(cx.listener(|this, dragged: &DraggedPane, _, cx| {
                        this.pane_to_tab(dragged.right, true, cx)
                    })),
            )
            .child(
                Button::new("tab-bar-new")
                    .icon(IconName::Plus)
                    .ghost()
                    .xsmall()
                    .tooltip("New tab (⌘T)")
                    .on_click(cx.listener(|this, _, window, cx| this.new_tab(&NewTab, window, cx))),
            )
            .pr_2()
    }
}

#[cfg(feature = "screenshot")]
impl ReaderView {
    /// Apply a scripted UI step for offscreen screenshots.
    pub fn debug_action(&mut self, action: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (name, arg) = action.split_once(':').unwrap_or((action, ""));
        match name {
            "palette" => self.show_palette(arg, window, cx),
            "search" => {
                let query = arg.replace('~', " ");
                self.open_search_page(&crate::palette::OpenSearchPage { query }, window, cx);
            }
            "commands" => self.show_palette(&format!("{COMMAND_PREFIX}{arg}"), window, cx),
            "notes" => {
                self.inspector_open = true;
                self.selected_note = self
                    .focused_content()
                    .and_then(|c| c.notes.first())
                    .map(|n| n.id);
            }
            // YouVersion fixtures (see remote::demo): a fake Bible with cached text.
            "yvdemo" => {
                crate::remote::demo::seed(cx);
                self.set_translation(crate::remote::demo::key(), cx);
            }
            "yvloading" | "yverror" => {
                let id = self.tab().id;
                let load = if name == "yvloading" {
                    Load::Loading
                } else {
                    Load::Failed {
                        message: bible_youversion::Error::RateLimited {
                            retry_after: Some(1800),
                        }
                        .to_string(),
                        setup: false,
                    }
                };
                self.content.insert(
                    id,
                    Content {
                        load,
                        ..Content::default()
                    },
                );
            }
            "yvstate" => crate::remote::demo::sign_in(arg, cx),
            "yvsearch" => {
                if let Some(page) = self.search.as_mut() {
                    let verses = [
                        (
                            43,
                            3,
                            16,
                            "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.",
                        ),
                        (
                            46,
                            13,
                            4,
                            "Charity suffereth long, and is kind; charity envieth not; charity vaunteth not itself, is not puffed up,",
                        ),
                    ];
                    page.yv.bible = Some(crate::remote::demo::ID);
                    page.yv.entries = verses
                        .iter()
                        .map(|(b, c, v, t)| search_page::YvEntry {
                            verse: VerseId::new(*b, *c, *v),
                            text: (*t).to_string(),
                        })
                        .collect();
                    page.yv.next_token = Some("more".into());
                    page.yv.requests_used = 3;
                    page.yv.state = search_page::YvState::Done;
                }
            }
            "nosidebar" => self.update_settings(cx, |s| s.sidebar_open = false),
            "dark" | "light" | "sepia" | "system" => {
                let appearance = match name {
                    "dark" => Appearance::Dark,
                    "light" => Appearance::Light,
                    "sepia" => Appearance::Sepia,
                    _ => Appearance::System,
                };
                crate::set_appearance(appearance, cx);
            }
            "font" => {
                if let Some(pairing) = FontPairing::from_key(arg) {
                    self.update_settings(cx, |s| s.font_pairing = pairing);
                }
            }
            "back" => self.go_back(&GoBack, window, cx),
            "forward" => self.go_forward(&GoForward, window, cx),
            "goto" => {
                if let Some(r) = bible_core::parse_reference(arg) {
                    let action = OpenPassage {
                        chapter: r.chapter_ref(),
                        verse: r.first_verse(),
                    };
                    self.open_passage(&action, window, cx);
                }
            }
            "newtab" => self.new_tab(&NewTab, window, cx),
            "closetab" => self.close_tab(&CloseTab, window, cx),
            "tab" => {
                if let Ok(n) = arg.parse::<usize>() {
                    self.activate_tab(
                        &ActivateTab {
                            index: n.saturating_sub(1),
                        },
                        window,
                        cx,
                    );
                }
            }
            "version" => self.set_translation(arg.to_uppercase(), cx),
            "split" => {
                if !self.tabs[self.active].is_split() {
                    self.toggle_split(&ToggleSplit, window, cx);
                }
                if let Some(r) = bible_core::parse_reference(arg) {
                    self.tab_mut().navigate(Location {
                        chapter: r.chapter_ref(),
                        verse: r.first_verse(),
                    });
                    self.show_active(cx);
                }
            }
            "unsplit" => {
                if self.tabs[self.active].is_split() {
                    self.toggle_split(&ToggleSplit, window, cx);
                }
            }
            "closepane" => self.close_pane(&ClosePane, window, cx),
            "focus" => self.focus_pane(arg == "right", cx),
            "narrow" => self.force_collapsed = true,
            "dropzone" => self.drop_zone = Some(arg != "left"),
            "pin" => self.pin_tab_at(self.active, cx),
            "rename" => {
                self.tabs[self.active].rename(arg);
                self.save_tabs(cx);
            }
            "sidebar" => self.focus_sidebar(&FocusSidebar, window, cx),
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
        self.sync_translation_items(window, cx);
        self.sync_translation_select(window, cx);
        let settings = cx.global::<Settings>();
        let sidebar_open = settings.sidebar_open;
        let theme = cx.theme();

        div()
            .id("reader")
            .key_context("Reader")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::next_chapter))
            .on_action(cx.listener(Self::prev_chapter))
            .on_action(cx.listener(Self::open_passage))
            .on_action(cx.listener(Self::open_palette))
            .on_action(cx.listener(Self::open_commands))
            .on_action(cx.listener(Self::close_palette))
            .on_action(cx.listener(Self::select_translation))
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::reopen_closed_tab))
            .on_action(cx.listener(Self::activate_tab))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::prev_tab))
            .on_action(cx.listener(Self::pin_tab))
            .on_action(cx.listener(Self::rename_tab))
            .on_action(cx.listener(Self::close_tab_action))
            .on_action(cx.listener(Self::open_in_new_tab))
            .on_action(cx.listener(Self::open_in_split))
            .on_action(cx.listener(Self::toggle_split))
            .on_action(cx.listener(Self::close_pane))
            .on_action(cx.listener(Self::focus_left_pane))
            .on_action(cx.listener(Self::focus_right_pane))
            .on_action(cx.listener(Self::copy_verse))
            .on_action(cx.listener(Self::copy_chapter))
            .on_action(cx.listener(Self::search_in))
            .on_action(cx.listener(Self::open_search_page))
            .on_action(cx.listener(Self::set_palette_query))
            .on_action(cx.listener(Self::focus_sidebar))
            .on_action(cx.listener(|this, _: &IncreaseFont, _, cx| {
                this.update_settings(cx, |s| {
                    s.font_size = (s.font_size + 1.).min(FONT_SIZE_RANGE.1)
                })
            }))
            .on_action(cx.listener(|this, _: &DecreaseFont, _, cx| {
                this.update_settings(cx, |s| {
                    s.font_size = (s.font_size - 1.).max(FONT_SIZE_RANGE.0)
                })
            }))
            .on_action(cx.listener(|this, _: &ResetFont, _, cx| {
                this.update_settings(cx, |s| s.font_size = Settings::default().font_size)
            }))
            .on_action(cx.listener(|this, _: &IncreaseLineSpacing, _, cx| {
                this.update_settings(cx, |s| {
                    s.line_height =
                        ((s.line_height + 0.1).min(LINE_HEIGHT_RANGE.1) * 100.).round() / 100.
                })
            }))
            .on_action(cx.listener(|this, _: &DecreaseLineSpacing, _, cx| {
                this.update_settings(cx, |s| {
                    s.line_height =
                        ((s.line_height - 0.1).max(LINE_HEIGHT_RANGE.0) * 100.).round() / 100.
                })
            }))
            .on_action(cx.listener(|this, _: &ResetLineSpacing, _, cx| {
                this.update_settings(cx, |s| s.line_height = Settings::default().line_height)
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
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::go_forward))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.drop_zone.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(self.render_toolbar(window, cx))
            .when(
                self.tabs.len() > 1 || self.tabs[self.active].is_split(),
                |d| d.child(self.render_tab_bar(cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(sidebar_open, |d| d.child(self.render_sidebar(window, cx)))
                    .child(if self.search.is_some() {
                        self.render_search_page(cx).into_any_element()
                    } else {
                        self.render_panes(window, cx).into_any_element()
                    })
                    .when(self.inspector_open, |d| d.child(self.render_inspector(cx))),
            )
            .when(self.palette_open, |d| d.child(self.render_palette(cx)))
            .when(self.renaming.is_some(), |d| d.child(self.render_rename(cx)))
    }
}

/// Headless UI tests: real views and click dispatch, no pixels. They need the
/// built translations (`cargo run --release -p bible-import`) and skip without them.
#[cfg(test)]
mod tests {
    // `super::*` globs in `gpui_kit::test`; the test macro expands to `#[test]`.
    use core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{TestAppContext, WindowBounds, WindowOptions, base::Root, size};

    use super::*;

    fn open(cx: &mut TestAppContext) -> Option<(WindowHandle<Root>, Entity<ReaderView>)> {
        open_with(cx, |_| {})
    }

    /// Like `open`, with `setup` run after the globals exist and before the
    /// window does (to add a YouVersion source, say).
    fn open_with(
        cx: &mut TestAppContext,
        setup: impl FnOnce(&mut App),
    ) -> Option<(WindowHandle<Root>, Entity<ReaderView>)> {
        let library = Library::load(&crate::library::data_dir()).ok()?;
        Some(cx.update(|cx| {
            gpui_kit::init(cx);
            crate::bind_keys(cx);
            cx.set_global(library);
            cx.set_global(Settings::default());
            setup(cx);
            let (window, view) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1100.), px(800.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| ReaderView::new(window, cx)),
            )
            .expect("open window");
            (window.downcast::<Root>().expect("Root"), view)
        }))
    }

    /// Run `f` with the window after laying out a fresh frame.
    fn with_window(
        window: WindowHandle<Root>,
        cx: &mut TestAppContext,
        f: impl FnOnce(&mut Window, &mut App),
    ) {
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            f(window, cx);
            window.render_frame(cx);
        })
        .unwrap();
        cx.run_until_parked();
    }

    fn chapter(view: &Entity<ReaderView>, cx: &mut TestAppContext) -> String {
        view.read_with(cx, |v, _| v.chapter().to_string())
    }

    #[gpui_kit::test]
    fn toolbar_navigation_and_history(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            eprintln!("skipping: no translations in data/build");
            return;
        };
        let click = |id: &'static str, cx: &mut TestAppContext| {
            with_window(window, cx, |window, cx| window.click(id, cx));
        };

        assert_eq!(chapter(&view, cx), "John 1");
        click("next", cx);
        click("next", cx);
        assert_eq!(chapter(&view, cx), "John 3");
        click("back", cx);
        click("back", cx);
        assert_eq!(chapter(&view, cx), "John 1");
        assert!(!view.read_with(cx, |v, _| v.tab().can_go_back()));
        click("forward", cx);
        assert_eq!(chapter(&view, cx), "John 2");

        let recent = cx.read(|cx| cx.global::<Settings>().recent.clone());
        assert_eq!(recent.first(), Some(&(43, 2)));
    }

    #[gpui_kit::test]
    fn palette_jumps_with_keyboard(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        with_window(window, cx, |window, cx| window.press("cmd-k", cx));
        assert!(view.read_with(cx, |v, _| v.palette_open));

        with_window(window, cx, |window, cx| window.input("jn 3:16", cx));
        view.read_with(cx, |v, _| {
            assert_eq!(v.palette_groups[0].heading, "Go to");
        });
        with_window(window, cx, |window, cx| window.press("enter", cx));
        view.read_with(cx, |v, _| {
            assert!(!v.palette_open);
            assert_eq!(v.chapter().to_string(), "John 3");
            assert_eq!(v.target(), Some(VerseId::new(43, 3, 16)));
        });
    }

    #[gpui_kit::test]
    fn copy_verse_and_chapter_put_citation_on_clipboard(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        with_window(window, cx, |window, cx| {
            window.press("cmd-k", cx);
        });
        with_window(window, cx, |window, cx| window.input("jn 3:16", cx));
        with_window(window, cx, |window, cx| window.press("enter", cx));
        with_window(window, cx, |window, cx| window.press("cmd-shift-c", cx));
        let verse = cx.read_from_clipboard().and_then(|c| c.text()).unwrap();
        assert!(verse.starts_with('“'), "{verse}");
        assert!(verse.contains("John 3:16 (BSB)"), "{verse}");
        assert!(verse.contains("eternal life"), "{verse}");

        with_window(window, cx, |window, cx| window.press("cmd-alt-c", cx));
        let chapter = cx.read_from_clipboard().and_then(|c| c.text()).unwrap();
        assert!(chapter.starts_with("1 "), "{chapter}");
        assert!(chapter.contains("\n16 "), "{chapter}");
        assert!(chapter.ends_with("John 3 (BSB)"), "{chapter}");
        view.read_with(cx, |v, _| assert_eq!(v.chapter().to_string(), "John 3"));
    }

    #[gpui_kit::test]
    fn dragging_over_chapter_text_selects_it(cx: &mut TestAppContext) {
        let Some((window, _view)) = open(cx) else {
            return;
        };
        with_window(window, cx, |window, cx| {
            window.render_frame(cx);
            window.drag(point(px(600.), px(200.)), point(px(900.), px(500.)), cx);
        });
        let mut selected = String::new();
        with_window(window, cx, |window, cx| {
            selected = gpui_kit::base::TextSelection::selected_text(window, cx);
        });
        assert!(selected.contains("In the beginning"), "got {selected:?}");
    }

    #[gpui_kit::test]
    fn palette_runs_commands(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        with_window(window, cx, |window, cx| window.press("cmd-shift-p", cx));
        with_window(window, cx, |window, cx| window.input("new tab", cx));
        with_window(window, cx, |window, cx| window.press("enter", cx));
        view.read_with(cx, |v, _| {
            assert!(!v.palette_open);
            assert_eq!(v.tabs.len(), 2);
        });
    }

    #[gpui_kit::test]
    fn translation_dropdown_is_searchable(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        with_window(window, cx, |window, cx| window.click("translation", cx));
        with_window(window, cx, |window, cx| window.input("king", cx));
        with_window(window, cx, |window, cx| window.press("enter", cx));
        view.read_with(cx, |v, _| assert_eq!(v.tab().translation, "KJV"));
        let saved = cx.read(|cx| cx.global::<Settings>().translation.clone());
        assert_eq!(saved, "KJV");
    }

    #[gpui_kit::test]
    fn tabs_keep_their_own_place_and_translation(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        let press = |key: &'static str, cx: &mut TestAppContext| {
            with_window(window, cx, |window, cx| window.press(key, cx));
        };

        // Tab 1: Psalm 23 in KJV.
        view.update(cx, |v, cx| v.go_to(ChapterRef::new(19, 23), None, cx));
        press("cmd-2", cx);
        // Tab 2: duplicate, then move to Romans 8 in BSB.
        press("cmd-t", cx);
        view.update(cx, |v, cx| v.go_to(ChapterRef::new(45, 8), None, cx));
        press("cmd-1", cx);
        cx.run_until_parked();
        view.read_with(cx, |v, _| {
            assert_eq!(v.tabs.len(), 2);
            assert_eq!(v.active, 1);
            assert_eq!(v.tab_title(v.tab()), "Romans 8 · BSB");
        });

        // The tab bar appears with two tabs; clicking the first switches back.
        with_window(window, cx, |window, cx| {
            window.dispatch_action(Box::new(ActivateTab { index: 0 }), cx)
        });
        view.read_with(cx, |v, _| {
            assert_eq!(v.active, 0);
            assert_eq!(v.tab_title(v.tab()), "Psalms 23 · KJV");
        });

        // Close it, then bring it back with ⇧⌘T.
        press("cmd-w", cx);
        view.read_with(cx, |v, _| assert_eq!(v.tabs.len(), 1));
        press("cmd-shift-t", cx);
        view.read_with(cx, |v, _| {
            assert_eq!(v.tabs.len(), 2);
            assert_eq!(v.tab_title(v.tab()), "Psalms 23 · KJV");
        });

        // Tabs are saved for the next launch.
        let saved = cx.read(|cx| cx.global::<Settings>().tabs.clone());
        assert_eq!(saved.len(), 2);
    }

    #[gpui_kit::test]
    fn arrow_keys_turn_chapters_and_cmd_b_toggles_the_sidebar(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        let press = |key: &'static str, cx: &mut TestAppContext| {
            with_window(window, cx, |window, cx| window.press(key, cx));
        };
        assert_eq!(chapter(&view, cx), "John 1");
        press("right", cx);
        press("right", cx);
        assert_eq!(chapter(&view, cx), "John 3");
        press("left", cx);
        assert_eq!(chapter(&view, cx), "John 2");

        let open = |cx: &mut TestAppContext| cx.read(|cx| cx.global::<Settings>().sidebar_open);
        assert!(open(cx));
        press("cmd-b", cx);
        assert!(!open(cx));
        press("cmd-b", cx);
        assert!(open(cx));

        // The prev / next buttons at the top of the chapter work too.
        with_window(window, cx, |window, cx| window.click("top-next", cx));
        assert_eq!(chapter(&view, cx), "John 3");
        with_window(window, cx, |window, cx| window.click("top-prev", cx));
        assert_eq!(chapter(&view, cx), "John 2");
    }

    #[gpui_kit::test]
    fn sidebar_chapter_picker_works_from_the_keyboard(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        let press = |key: &'static str, cx: &mut TestAppContext| {
            with_window(window, cx, |window, cx| window.press(key, cx));
        };
        press("cmd-alt-b", cx);
        // Arrows move the cursor without navigating; Enter opens.
        press("right", cx);
        press("down", cx);
        assert_eq!(chapter(&view, cx), "John 1");
        view.read_with(cx, |v, _| {
            assert_eq!(v.sidebar_cursor, ChapterRef::new(43, 8));
        });
        press("enter", cx);
        assert_eq!(chapter(&view, cx), "John 8");
        // Focus is back on the reader, so arrows turn chapters again.
        press("right", cx);
        assert_eq!(chapter(&view, cx), "John 9");

        press("cmd-alt-b", cx);
        press("alt-down", cx);
        press("escape", cx);
        view.read_with(cx, |v, _| {
            assert_eq!(v.sidebar_cursor, ChapterRef::new(44, 1));
            assert_eq!(v.chapter().to_string(), "John 9");
        });
        press("left", cx);
        assert_eq!(chapter(&view, cx), "John 8");
    }

    #[gpui_kit::test]
    fn palette_cmd_enter_opens_in_a_new_tab(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        with_window(window, cx, |window, cx| window.press("cmd-k", cx));
        with_window(window, cx, |window, cx| window.input("ps 23", cx));
        with_window(window, cx, |window, cx| window.press("cmd-enter", cx));
        view.read_with(cx, |v, _| {
            assert!(!v.palette_open);
            assert_eq!(v.tabs.len(), 2);
            assert_eq!(v.active, 1);
            assert_eq!(v.chapter().to_string(), "Psalms 23");
            assert_eq!(v.tabs[0].location.chapter.to_string(), "John 1");
        });
    }

    #[gpui_kit::test]
    fn palette_suggests_searches_and_offers_topics(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        let study = cx.update(|cx| cx.global::<Library>().study.is_some());
        with_window(window, cx, |window, cx| window.press("cmd-k", cx));
        view.read_with(cx, |v, _| {
            let group = v
                .palette_groups
                .iter()
                .find(|g| g.heading == "Try searching")
                .expect("suggestions on an empty palette");
            assert_eq!(group.rows.len(), SUGGESTIONS_SHOWN);
        });
        // Picking one fills in the query and searches.
        let suggestion = view.read_with(cx, |v, _| {
            let group = v
                .palette_groups
                .iter()
                .find(|g| g.heading == "Try searching")
                .unwrap();
            group.rows[0].action()
        });
        with_window(window, cx, |window, cx| {
            window.dispatch_action(suggestion, cx)
        });
        view.read_with(cx, |v, cx| {
            assert!(v.palette_open);
            assert!(!v.palette.read(cx).query(cx).is_empty());
            assert!(v.palette_groups.iter().any(|g| g.heading == "Verses"));
        });
        with_window(window, cx, |window, cx| {
            window.press("cmd-a", cx);
            window.input("pride", cx);
        });
        view.read_with(cx, |v, _| {
            let topics = v.palette_groups.iter().find(|g| g.heading == "Topics");
            if study {
                assert_eq!(
                    topics.expect("topic rows").rows[0]
                        .label()
                        .split(" · ")
                        .next(),
                    Some("Topic: pride")
                );
            }
        });
    }

    #[gpui_kit::test]
    fn search_page_filters_and_opens_results(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        let study = cx.update(|cx| cx.global::<Library>().study.is_some());
        let open_page = |query: &str| crate::palette::OpenSearchPage {
            query: query.to_string(),
        };
        with_window(window, cx, |window, cx| {
            window.dispatch_action(Box::new(open_page("pride")), cx)
        });
        view.read_with(cx, |v, _| {
            let page = v.search.as_ref().expect("page opens");
            assert_eq!(page.text, "pride");
            assert!(page.entries.len() > 5);
            if study {
                assert_eq!(page.topic.as_deref(), Some("pride"));
                assert!(page.entries[0].key);
            }
        });

        // Typing re-runs the search; scope and mode filters narrow it.
        with_window(window, cx, |window, cx| {
            window.click(("sp-scope", 2usize), cx);
        });
        view.read_with(cx, |v, _| {
            let page = v.search.as_ref().unwrap();
            assert!(page.entries.iter().all(|e| e.verse.book() >= 40));
        });

        // Down moves the selection; Enter opens it here and closes the page.
        with_window(window, cx, |window, cx| window.press("down", cx));
        let second = view.read_with(cx, |v, _| {
            let page = v.search.as_ref().unwrap();
            assert_eq!(page.selected, 1);
            page.entries[1].verse
        });
        with_window(window, cx, |window, cx| window.press("cmd-enter", cx));
        view.read_with(cx, |v, _| {
            // A new tab opened behind the page, which stays up.
            assert!(v.search.is_some());
            assert_eq!(v.tabs.len(), 2);
            assert_eq!(v.active, 0);
            assert_eq!(v.tabs[1].location.verse, Some(second));
            assert!(v.search.as_ref().unwrap().opened.contains(&second));
        });
        with_window(window, cx, |window, cx| window.press("enter", cx));
        view.read_with(cx, |v, _| {
            assert!(v.search.is_none());
            assert_eq!(v.tabs.len(), 2);
            assert_eq!(v.tab().location.verse, Some(second));
        });
    }

    #[gpui_kit::test]
    fn search_page_closes_with_escape_and_when_navigating(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        with_window(window, cx, |window, cx| window.press("cmd-shift-f", cx));
        view.read_with(cx, |v, _| {
            let page = v.search.as_ref().expect("cmd-shift-f opens the page");
            assert!(page.text.is_empty());
            assert_eq!(page.suggestions.len(), 8);
        });
        with_window(window, cx, |window, cx| window.press("escape", cx));
        view.read_with(cx, |v, _| assert!(v.search.is_none()));

        with_window(window, cx, |window, cx| {
            window.dispatch_action(
                Box::new(crate::palette::OpenSearchPage {
                    query: "in:john love".into(),
                }),
                cx,
            )
        });
        view.read_with(cx, |v, _| {
            let page = v.search.as_ref().unwrap();
            assert_eq!(page.text, "love");
            assert_eq!(page.scope.as_ref().unwrap().label, "John");
            assert!(page.entries.iter().all(|e| e.verse.book() == 43));
        });
        with_window(window, cx, |window, cx| window.click("next", cx));
        view.read_with(cx, |v, _| {
            assert!(v.search.is_none(), "navigating closes it")
        });
    }

    #[gpui_kit::test]
    fn palette_search_can_be_scoped(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        with_window(window, cx, |window, cx| window.press("cmd-shift-p", cx));
        with_window(window, cx, |window, cx| window.input("new testament", cx));
        with_window(window, cx, |window, cx| window.press("enter", cx));
        view.read_with(cx, |v, cx| {
            // The palette stays open, restarted as a scoped search.
            assert!(v.palette_open);
            assert_eq!(v.palette.read(cx).query(cx).trim(), "in:nt");
        });
        with_window(window, cx, |window, cx| window.input("shepherd", cx));
        view.read_with(cx, |v, _| {
            let group = &v.palette_groups[0];
            assert_eq!(group.heading, "Verses in New Testament");
            assert!(group.rows.iter().all(|r| match r {
                Row::Verse(hit) => hit.verse.book() >= 40,
                Row::SeeAll { query, .. } => query.contains("in:nt"),
                _ => false,
            }));
        });
    }

    #[gpui_kit::test]
    fn tabs_pin_reorder_and_rename(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        let press = |key: &'static str, cx: &mut TestAppContext| {
            with_window(window, cx, |window, cx| window.press(key, cx));
        };
        // Three tabs: John 1, John 2, John 3 (the last is active).
        press("cmd-t", cx);
        press("cmd-]", cx);
        press("cmd-t", cx);
        press("cmd-]", cx);
        let titles = |cx: &mut TestAppContext| {
            view.read_with(cx, |v, _| {
                v.tabs.iter().map(|t| t.title()).collect::<Vec<_>>()
            })
        };
        assert_eq!(titles(cx), ["John 1", "John 2", "John 3"]);

        // Drag the last tab to the front.
        view.update(cx, |v, cx| v.move_tab(2, 0, cx));
        assert_eq!(titles(cx), ["John 3", "John 1", "John 2"]);
        view.read_with(cx, |v, _| assert_eq!(v.active, 0, "active follows its tab"));

        // Pin John 2: it jumps to the front; the active tab is unchanged.
        view.update(cx, |v, cx| {
            v.pin_tab_at(2, cx);
        });
        assert_eq!(titles(cx), ["John 2", "John 3", "John 1"]);
        view.read_with(cx, |v, _| assert_eq!(v.tab().title(), "John 3"));

        // Dragging an unpinned tab over a pinned one can't pass the pin.
        view.update(cx, |v, cx| v.move_tab(2, 0, cx));
        assert_eq!(titles(cx), ["John 2", "John 1", "John 3"]);

        // Rename through the palette command, then type a name.
        with_window(window, cx, |window, cx| window.press("cmd-shift-p", cx));
        with_window(window, cx, |window, cx| window.input("rename tab", cx));
        with_window(window, cx, |window, cx| window.press("enter", cx));
        view.read_with(cx, |v, _| {
            assert_eq!(v.renaming, Some(v.active));
            assert!(!v.palette_open);
        });
        with_window(window, cx, |window, cx| window.input("Born again", cx));
        with_window(window, cx, |window, cx| window.press("enter", cx));
        view.read_with(cx, |v, _| {
            assert_eq!(v.renaming, None);
            assert_eq!(v.tab().label(), "Born again");
        });

        // Pins and names are saved.
        let saved = cx.read(|cx| cx.global::<Settings>().tabs.clone());
        assert!(saved[0].pinned);
        assert_eq!(saved[2].name.as_deref(), Some("Born again"));
    }

    #[gpui_kit::test]
    fn escape_cancels_a_rename(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        with_window(window, cx, |window, cx| {
            window.dispatch_action(Box::new(RenameTab { index: 0 }), cx)
        });
        view.read_with(cx, |v, _| assert_eq!(v.renaming, Some(0)));
        with_window(window, cx, |window, cx| window.input("nope", cx));
        with_window(window, cx, |window, cx| window.press("escape", cx));
        view.read_with(cx, |v, _| {
            assert_eq!(v.renaming, None);
            assert_eq!(v.tab().name, None);
        });
    }

    fn press(window: WindowHandle<Root>, key: &'static str, cx: &mut TestAppContext) {
        with_window(window, cx, |window, cx| window.press(key, cx));
    }

    #[gpui_kit::test]
    fn split_panes_navigate_and_switch_version_independently(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        view.update(cx, |v, cx| v.go_to(ChapterRef::new(43, 3), None, cx));
        press(window, "cmd-\\", cx);
        view.read_with(cx, |v, _| {
            assert!(v.tabs[0].is_split());
            assert_eq!(v.tabs.len(), 1);
            assert!(v.tabs[0].focus_side, "the new pane takes focus");
        });

        // Navigation and version changes go to the focused (right) pane only.
        view.update(cx, |v, cx| v.go_to(ChapterRef::new(45, 8), None, cx));
        view.update(cx, |v, cx| v.set_translation("KJV".into(), cx));
        view.read_with(cx, |v, _| {
            assert_eq!(v.tabs[0].label(), "John 3 · BSB  |  Romans 8 · KJV");
            assert_eq!(v.chapter().to_string(), "Romans 8");
        });
        let current = cx.read(|cx| cx.global::<Settings>().translation.clone());
        assert_eq!(current, "KJV");

        // ⌘⌥← moves focus; everything now acts on the left pane.
        press(window, "cmd-alt-left", cx);
        view.read_with(cx, |v, _| assert_eq!(v.chapter().to_string(), "John 3"));
        let current = cx.read(|cx| cx.global::<Settings>().translation.clone());
        assert_eq!(current, "BSB");
        press(window, "cmd-]", cx);
        view.read_with(cx, |v, _| {
            assert_eq!(v.tabs[0].label(), "John 4 · BSB  |  Romans 8 · KJV");
            assert!(v.content.contains_key(&v.tab().id));
            assert_eq!(v.content.len(), 2, "both panes keep their text");
        });

        // The split is saved.
        let saved = cx.read(|cx| cx.global::<Settings>().tabs.clone());
        let side = saved[0].side.as_ref().expect("side saved");
        assert_eq!(
            (side.book, side.chapter, side.translation.as_str()),
            (45, 8, "KJV")
        );
    }

    #[gpui_kit::test]
    fn clicking_a_pane_focuses_it(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        press(window, "cmd-\\", cx);
        press(window, "cmd-alt-left", cx);
        view.read_with(cx, |v, _| assert!(!v.tabs[0].focus_side));
        // The right pane's own "next" button both focuses the pane and acts on it.
        with_window(window, cx, |window, cx| window.click("top-next-right", cx));
        view.read_with(cx, |v, _| {
            assert!(v.tabs[0].focus_side);
            assert_eq!(v.tabs[0].label(), "John 1 · BSB  |  John 2 · BSB");
        });
    }

    #[gpui_kit::test]
    fn dropping_a_tab_on_the_text_splits_beside_it(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        press(window, "cmd-t", cx);
        view.update(cx, |v, cx| v.go_to(ChapterRef::new(19, 23), None, cx));
        press(window, "cmd-t", cx);
        view.update(cx, |v, cx| v.go_to(ChapterRef::new(45, 8), None, cx));
        view.read_with(cx, |v, _| {
            assert_eq!(v.tabs.len(), 3);
            assert!(v.can_split_with(1));
            assert!(!v.can_split_with(v.active), "not onto itself");
        });

        // Tab 1 (Psalms 23) joins the active tab (Romans 8) on its left.
        view.update(cx, |v, cx| v.drop_tab_on_panes(1, false, cx));
        view.read_with(cx, |v, _| {
            assert_eq!(v.tabs.len(), 2);
            assert_eq!(
                v.tabs[v.active].label(),
                "Psalms 23 · BSB  |  Romans 8 · BSB"
            );
            assert!(!v.can_split_with(0), "a split tab takes no third pane");
        });

        // Single-pane tabs keep working next to the split one.
        with_window(window, cx, |window, cx| {
            window.dispatch_action(Box::new(ActivateTab { index: 0 }), cx)
        });
        view.read_with(cx, |v, _| {
            assert!(!v.tabs[v.active].is_split());
            assert_eq!(v.chapter().to_string(), "John 1");
        });
    }

    #[gpui_kit::test]
    fn unsplit_and_close_pane(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        press(window, "cmd-\\", cx);
        view.update(cx, |v, cx| v.go_to(ChapterRef::new(45, 8), None, cx));

        // Dragging a pane out (or ⌘\) makes it a separate tab; focus stays left.
        press(window, "cmd-\\", cx);
        view.read_with(cx, |v, _| {
            assert_eq!(v.tabs.len(), 2);
            assert!(!v.tabs[0].is_split());
            assert_eq!(v.tabs[1].label(), "Romans 8 · BSB");
            assert_eq!(v.active, 0);
        });

        // Split again, then close the focused pane: the other stays, and ⇧⌘T
        // brings the closed one back as a tab.
        press(window, "cmd-\\", cx);
        press(window, "cmd-shift-\\", cx);
        view.read_with(cx, |v, _| {
            assert!(!v.tabs[v.active].is_split());
            assert_eq!(v.tabs.len(), 2);
        });
        press(window, "cmd-shift-t", cx);
        view.read_with(cx, |v, _| assert_eq!(v.tabs.len(), 3));
    }

    #[gpui_kit::test]
    fn palette_opens_a_passage_beside(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        press(window, "cmd-k", cx);
        with_window(window, cx, |window, cx| window.input("rom 8", cx));
        press(window, "alt-enter", cx);
        view.read_with(cx, |v, _| {
            assert!(!v.palette_open);
            assert_eq!(v.tabs[0].label(), "John 1 · BSB  |  Romans 8 · BSB");
            assert_eq!(v.chapter().to_string(), "Romans 8");
        });

        // Split already: ⌥Enter fills the other pane.
        press(window, "cmd-k", cx);
        with_window(window, cx, |window, cx| window.input("ps 23", cx));
        press(window, "alt-enter", cx);
        view.read_with(cx, |v, _| {
            assert_eq!(v.tabs[0].label(), "Psalms 23 · BSB  |  Romans 8 · BSB");
        });

        // And a plain Enter replaces what the focused pane shows.
        press(window, "cmd-k", cx);
        with_window(window, cx, |window, cx| window.input("jn 1", cx));
        press(window, "enter", cx);
        view.read_with(cx, |v, _| {
            assert_eq!(v.tabs[0].label(), "John 1 · BSB  |  Romans 8 · BSB");
        });
    }

    #[gpui_kit::test]
    fn narrow_windows_show_one_pane(cx: &mut TestAppContext) {
        let Some((_window, view)) = open(cx) else {
            return;
        };
        view.read_with(cx, |v, _| {
            assert!(v.collapsed(700.));
            assert!(!v.collapsed(900.));
        });
    }

    #[gpui_kit::test]
    fn dragging_a_tab_onto_the_text_opens_it_on_that_side(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        view.update(cx, |v, cx| v.go_to(ChapterRef::new(19, 23), None, cx));
        press(window, "cmd-t", cx);
        view.update(cx, |v, cx| v.go_to(ChapterRef::new(45, 8), None, cx));
        // Real pointer drag: first tab (top left) to the right half of the text.
        with_window(window, cx, |window, cx| {
            window.drag(point(px(40.), px(60.)), point(px(900.), px(400.)), cx)
        });
        view.read_with(cx, |v, _| {
            assert_eq!(v.tabs.len(), 1);
            assert_eq!(
                v.tabs[0].label(),
                "Romans 8 · BSB  |  Psalms 23 · BSB",
                "dropped on the right half"
            );
            assert_eq!(v.drop_zone, None);
        });
    }

    #[gpui_kit::test]
    fn selecting_text_in_a_pane_focuses_it(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        press(window, "cmd-\\", cx);
        press(window, "cmd-alt-left", cx);
        view.read_with(cx, |v, _| assert!(!v.tabs[0].focus_side));
        with_window(window, cx, |window, cx| {
            window.drag(point(px(750.), px(300.)), point(px(1000.), px(500.)), cx)
        });
        view.read_with(cx, |v, _| assert!(v.tabs[0].focus_side));
        let mut selected = String::new();
        with_window(window, cx, |window, cx| {
            selected = gpui_kit::base::TextSelection::selected_text(window, cx);
        });
        assert!(!selected.is_empty(), "text in the right pane is selected");
    }

    #[gpui_kit::test]
    fn dragging_a_pane_header_to_the_tab_strip_makes_a_tab(cx: &mut TestAppContext) {
        let Some((window, view)) = open(cx) else {
            return;
        };
        press(window, "cmd-\\", cx);
        view.update(cx, |v, cx| v.go_to(ChapterRef::new(45, 8), None, cx));
        with_window(window, cx, |window, cx| {
            window.drag(point(px(850.), px(95.)), point(px(700.), px(60.)), cx)
        });
        view.read_with(cx, |v, _| {
            assert_eq!(v.tabs.len(), 2);
            assert!(!v.tabs[0].is_split());
            assert_eq!(v.tab().label(), "Romans 8 · BSB");
            assert_eq!(v.active, 1);
        });
    }

    // MARK: YouVersion sources

    const YV_ID: u32 = 777;

    /// Synthetic chapter in YouVersion's markup (public-domain wording).
    const YV_JOHN_3: &str = r#"{"id":"JHN.3","content":"<div><div class=\"s1\">Heading</div><div class=\"p\"><span class=\"yv-v\" v=\"16\"></span><span class=\"yv-vlbl\">16</span>For God so loved the world.</div></div>"}"#;

    fn yv_scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("br-yv-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Add Bible 777 to the user's list and point the YouVersion client at
    /// `host`. An unroutable host makes any real request fail fast, offline.
    fn add_yv_bible(cx: &mut App, dir: &std::path::Path, host: &str) -> bible_youversion::Cache {
        let cache = bible_youversion::Cache::new(dir);
        let client = bible_youversion::Client::with_host("test-key", host);
        cx.set_global(remote::Remote::with_source(bible_youversion::Source::new(
            client,
            cache.clone(),
        )));
        cx.update_global::<Settings, _>(|s, _| {
            s.youversion.bibles.push(crate::library::SavedBible {
                id: YV_ID,
                abbrev: "YVT".into(),
                name: "YouVersion Test".into(),
                copyright: "© Example Publisher".into(),
            });
        });
        cache
    }

    #[gpui_kit::test]
    fn youversion_chapter_loads_from_cache_without_a_request(cx: &mut TestAppContext) {
        let dir = yv_scratch("cached");
        let d = dir.clone();
        let Some((_, view)) = open_with(cx, move |cx| {
            let cache = add_yv_bible(cx, &d, "127.0.0.1:9");
            cache.put("passage:777:JHN.3:html", YV_JOHN_3).unwrap();
        }) else {
            return;
        };
        view.update(cx, |v, cx| {
            v.go_to(ChapterRef::new(43, 3), None, cx);
            v.set_translation(crate::library::remote_key(YV_ID), cx);
        });
        cx.run_until_parked();
        view.read_with(cx, |v, _| {
            let content = v.focused_content().expect("content");
            assert!(content.load == Load::Ready);
            assert_eq!(content.blocks.len(), 2);
        });
        let requests = cx.read(|cx| cx.global::<remote::Remote>().requests_made());
        assert_eq!(requests, 0, "a cached chapter must not touch the network");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[gpui_kit::test]
    fn youversion_chapter_shows_loading_then_a_clear_error_offline(cx: &mut TestAppContext) {
        let dir = yv_scratch("offline");
        let d = dir.clone();
        let Some((_, view)) = open_with(cx, move |cx| {
            add_yv_bible(cx, &d, "127.0.0.1:9");
        }) else {
            return;
        };
        view.update(cx, |v, cx| {
            v.go_to(ChapterRef::new(43, 3), None, cx);
            v.set_translation(crate::library::remote_key(YV_ID), cx);
            // Not cached: the pane is loading until the fetch answers.
            assert!(v.focused_content().expect("content").load == Load::Loading);
        });
        cx.run_until_parked();
        view.read_with(cx, |v, _| {
            match &v.focused_content().expect("content").load {
                Load::Failed { message, setup } => {
                    assert!(message.contains("Couldn't reach YouVersion"), "{message}");
                    assert!(!setup);
                }
                _ => panic!("expected a failed load"),
            }
        });
        // Try again goes back to loading.
        view.update(cx, |v, cx| {
            v.retry_pane(false, cx);
            assert!(v.focused_content().expect("content").load == Load::Loading);
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[gpui_kit::test]
    fn removing_a_youversion_bible_moves_its_panes_back(cx: &mut TestAppContext) {
        let dir = yv_scratch("remove");
        let d = dir.clone();
        let Some((_, view)) = open_with(cx, move |cx| {
            let cache = add_yv_bible(cx, &d, "127.0.0.1:9");
            cache.put("passage:777:JHN.1:html", YV_JOHN_3).unwrap();
        }) else {
            return;
        };
        view.update(cx, |v, cx| {
            v.set_translation(crate::library::remote_key(YV_ID), cx)
        });
        cx.update(|cx| {
            cx.update_global::<Settings, _>(|s, _| s.youversion.bibles.clear());
        });
        cx.run_until_parked();
        let first = cx.read(|cx| cx.global::<Library>().translations[0].info.abbrev.clone());
        view.read_with(cx, |v, _| assert_eq!(v.tab().translation, first));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[gpui_kit::test]
    fn copy_from_youversion_cites_the_version_and_its_copyright(cx: &mut TestAppContext) {
        let dir = yv_scratch("copy");
        let d = dir.clone();
        let Some((_, view)) = open_with(cx, move |cx| {
            let cache = add_yv_bible(cx, &d, "127.0.0.1:9");
            cache.put("passage:777:JHN.3:html", YV_JOHN_3).unwrap();
        }) else {
            return;
        };
        view.update(cx, |v, cx| {
            v.go_to(ChapterRef::new(43, 3), None, cx);
            v.set_translation(crate::library::remote_key(YV_ID), cx);
        });
        let text = view.read_with(cx, |v, cx| {
            copy_text(
                cx,
                v.focused_content(),
                &v.tab().translation,
                ChapterRef::new(43, 3),
                Some(VerseId::new(43, 3, 16)),
            )
        });
        let text = text.expect("copy text");
        assert!(
            text.starts_with("“For God so loved the world.” — John 3:16 (YVT)"),
            "{text}"
        );
        assert!(text.contains("© Example Publisher"));
        assert!(text.contains("YouVersion"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[gpui_kit::test]
    fn youversion_search_is_opt_in_and_served_from_cache(cx: &mut TestAppContext) {
        let dir = yv_scratch("search");
        let d = dir.clone();
        let Some((window, view)) = open_with(cx, move |cx| {
            let cache = add_yv_bible(cx, &d, "127.0.0.1:9");
            cache
                .put(
                    "search:777:love:6:",
                    r#"{"verses":[{"reference":"JHN.3.16"}],"next_page_token":null}"#,
                )
                .unwrap();
            // Verse text comes from the (cached) chapter, not a request per verse.
            cache.put("passage:777:JHN.3:html", YV_JOHN_3).unwrap();
        }) else {
            return;
        };
        with_window(window, cx, |window, cx| {
            view.update(cx, |v, cx| {
                v.open_search_page(
                    &crate::palette::OpenSearchPage {
                        query: "love".into(),
                    },
                    window,
                    cx,
                )
            });
        });
        // Local search ran by itself; YouVersion has not been asked anything.
        view.read_with(cx, |v, _| {
            let page = v.search.as_ref().expect("search page");
            assert!(page.yv.state == search_page::YvState::Idle);
            assert!(
                !page.entries.is_empty(),
                "bundled translations answer first"
            );
        });
        view.update(cx, |v, cx| v.search_yv(false, cx));
        cx.run_until_parked();
        view.read_with(cx, |v, _| {
            let page = v.search.as_ref().expect("search page");
            assert!(page.yv.state == search_page::YvState::Done);
            assert_eq!(page.yv.entries.len(), 1);
            assert_eq!(page.yv.entries[0].text, "For God so loved the world.");
            assert_eq!(page.yv.requests_used, 0, "all from cache");
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[gpui_kit::test]
    fn skimming_past_youversion_chapters_costs_no_requests(cx: &mut TestAppContext) {
        use std::io::{Read, Write};
        // A fake YouVersion that answers every chapter with one short chapter.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let paths = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = paths.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                seen.lock()
                    .unwrap()
                    .push(head.split(' ').nth(1).unwrap_or("").to_string());
                let body = serde_json::json!({
                    "content": "<div class=\"p\"><span class=\"yv-v\" v=\"1\"></span>Only verse.</div>"
                })
                .to_string();
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        let dir = yv_scratch("skim");
        let d = dir.clone();
        let Some((_, view)) = open_with(cx, move |cx| {
            add_yv_bible(cx, &d, &base);
        }) else {
            return;
        };
        // Page through John 1, 2, 3 without waiting: only where we stop is fetched.
        view.update(cx, |v, cx| {
            v.set_translation(crate::library::remote_key(YV_ID), cx);
            v.go_to(ChapterRef::new(43, 2), None, cx);
            v.go_to(ChapterRef::new(43, 3), None, cx);
        });
        cx.run_until_parked();
        view.read_with(cx, |v, _| {
            assert!(v.focused_content().expect("content").load == Load::Ready);
        });
        let log = paths.lock().unwrap().clone();
        assert_eq!(log.len(), 1, "only John 3 should be requested: {log:?}");
        assert!(log[0].contains("/passages/JHN.3?"), "{log:?}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
