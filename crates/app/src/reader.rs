//! The main reading window: title-bar toolbar, tab bar, book sidebar,
//! chapter text, notes inspector and the ⌘K palette.

use std::cell::Cell;

use bible_core::{BOOKS, Block, BlockKind, ChapterRef, Note, NoteKind, Testament, VerseId};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _};
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, IndexPath, Selectable as _, Sizable as _,
    TitleBar,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::chapter_view::{self, Options, Palette, note_marker, render_block};
use crate::library::{Appearance, FONT_SIZE_RANGE, Library, Settings, current_translation};
use crate::palette::{
    self, ActivateTab, COMMAND_PREFIX, CloseTabAt, OpenPassage, PinTab, RenameTab, Row,
    SEARCH_LIMIT, SearchIn,
};
use crate::tabs::{Location, Tab};
use crate::translation_picker::TranslationItem;
use crate::{
    ClosePalette, CloseTab, DecreaseFont, FocusReader, FocusSidebar, GoBack, GoForward,
    IncreaseFont, NewTab, NextChapter, NextTab, OpenCommands, OpenInNewTab, OpenPalette,
    PrevChapter, PrevTab, ReopenClosedTab, ResetFont, SelectTranslation, SetAppearance,
    SidebarDown, SidebarLeft, SidebarNextBook, SidebarOpen, SidebarPrevBook, SidebarRight,
    SidebarUp, ToggleInspector, ToggleRedLetter, ToggleSidebar, ToggleVerseNumbers,
};

const TOOLBAR_HEIGHT: f32 = 44.;
const CLOSED_TABS_KEPT: usize = 20;
/// Chapter tiles per row in the sidebar grid (keyboard Up / Down step).
const GRID_COLUMNS: u16 = 6;

/// Payload while a tab is being dragged to a new position.
#[derive(Clone)]
struct DraggedTab {
    from: usize,
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
    /// Content of the active tab's chapter.
    blocks: Vec<Block>,
    notes: Vec<Note>,
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
    /// An action run from the palette has taken over, so its on-confirm
    /// callback must not close or refocus anything.
    palette_handed_off: bool,
    /// Tab being renamed, and the field it is typed into.
    renaming: Option<usize>,
    rename_input: Entity<InputState>,
    palette: Entity<CommandState>,
    palette_groups: Vec<palette::Group>,
    translation_select: Entity<TranslationSelect>,
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
        let library = cx.global::<Library>();
        for tab in &mut tabs {
            if library.index_of(&tab.translation).is_none() {
                tab.translation = library.translations[0].info.abbrev.clone();
            }
        }
        let active = settings.active_tab.min(tabs.len() - 1);

        let items: Vec<TranslationItem> = library
            .translations
            .iter()
            .map(TranslationItem::from_translation)
            .collect();
        let selected = library
            .index_of(&tabs[active].translation)
            .map(|ix| IndexPath::default().row(ix));
        let translation_select = cx.new(|cx| {
            SelectState::new(SearchableVec::new(items), selected, window, cx).searchable(true)
        });
        let palette = cx.new(|cx| CommandState::new(window, cx));
        let rename_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Tab name (blank shows the passage)")
        });

        let subscriptions = vec![
            cx.subscribe_in(&translation_select, window, Self::on_translation_selected),
            cx.subscribe_in(&rename_input, window, Self::on_rename_input),
            // Settings window changes and menu actions repaint the reader.
            cx.observe_global::<Settings>(|_, cx| cx.notify()),
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
            expanded_book: Some(tabs[active].location.chapter.book),
            sidebar_scroll: ScrollHandle::new(),
            sidebar_focus: cx.focus_handle(),
            sidebar_cursor: tabs[active].location.chapter,
            sidebar_book: None,
            scroll_sidebar: Cell::new(None),
            palette_handed_off: false,
            renaming: None,
            rename_input,
            tabs,
            active,
            closed: Vec::new(),
            blocks: Vec::new(),
            notes: Vec::new(),
            selected_note: None,
            inspector_open: false,
            palette_open: false,
            palette,
            palette_groups: Vec::new(),
            translation_select,
            _subscriptions: subscriptions,
        };
        this.show_active(cx);
        this
    }

    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn chapter(&self) -> ChapterRef {
        self.tab().location.chapter
    }

    fn target(&self) -> Option<VerseId> {
        self.tab().location.verse
    }

    /// Load the active tab's chapter, scroll to its verse, and persist.
    fn show_active(&mut self, cx: &mut Context<Self>) {
        let tab = &self.tabs[self.active];
        let (location, abbrev) = (tab.location, tab.translation.clone());
        self.expanded_book = Some(location.chapter.book);
        self.sidebar_cursor = location.chapter;
        if self.sidebar_book != Some(location.chapter.book) {
            self.sidebar_book = Some(location.chapter.book);
            self.scroll_sidebar.set(Some((location.chapter.book, None)));
        }

        cx.update_global::<Settings, _>(|s, _| s.translation = abbrev.clone());
        let translation = current_translation(cx);
        self.blocks = translation
            .chapter(location.chapter)
            .ok()
            .flatten()
            .unwrap_or_default();
        self.notes = translation
            .notes_for_chapter(location.chapter)
            .unwrap_or_default();
        self.selected_note = None;

        if let Some(v) = location.verse {
            let ix = self
                .blocks
                .iter()
                .position(|b| {
                    b.spans.iter().any(|s| matches!(s, bible_core::Span::Verse { num, end }
                        if *num == v.verse() || end.is_some_and(|e| *num <= v.verse() && v.verse() <= e)))
                })
                .unwrap_or(0);
            // Child 0 of the scroll container is the chapter title.
            self.tabs[self.active].scroll.scroll_to_top_of_item(ix + 1);
        }

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
        self.tabs[self.active].navigate(Location { chapter, verse });
        self.show_active(cx);
    }

    /// Point the translation dropdown at the active tab's translation. Runs
    /// at render time because changing a `Select` needs the `Window`.
    fn sync_translation_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let abbrev = &self.tab().translation;
        if self.translation_select.read(cx).selected_value() == Some(abbrev) {
            return;
        }
        if let Some(ix) = cx.global::<Library>().index_of(abbrev) {
            self.translation_select.update(cx, |select, cx| {
                select.set_selected_index(Some(IndexPath::default().row(ix)), window, cx)
            });
        }
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        if self.tabs[self.active].go_back().is_some() {
            self.show_active(cx);
        }
    }

    fn go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        if self.tabs[self.active].go_forward().is_some() {
            self.show_active(cx);
        }
    }

    fn open_passage(&mut self, action: &OpenPassage, _: &mut Window, cx: &mut Context<Self>) {
        self.tabs[self.active].navigate(Location {
            chapter: action.chapter,
            verse: action.verse,
        });
        self.show_active(cx);
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

    // MARK: Translation

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

    fn on_translation_selected(
        &mut self,
        _: &Entity<TranslationSelect>,
        event: &SelectEvent<SearchableVec<TranslationItem>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let SelectEvent::Confirm(Some(abbrev)) = event {
            self.set_translation(abbrev.clone(), cx);
        }
        self.focus.focus(window, cx);
    }

    /// Switch the active tab's translation.
    fn set_translation(&mut self, abbrev: String, cx: &mut Context<Self>) {
        self.tabs[self.active].translation = abbrev;
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

    fn refresh_palette(&mut self, cx: &mut Context<Self>) {
        let query = self.palette.read(cx).query(cx).to_string();
        let settings = cx.global::<Settings>();
        let library = cx.global::<Library>();
        let translation = current_translation(cx);
        let search = |q: &str, books| {
            translation
                .search_in(q, books, SEARCH_LIMIT)
                .unwrap_or_default()
        };
        let context = palette::Context {
            current: self.chapter(),
            active_tab: self.active,
            tabs: self.tabs.iter().map(|t| self.tab_title(t)).collect(),
            recent: &settings.recent,
            translations: library
                .translations
                .iter()
                .map(|t| (t.info.abbrev.clone(), t.info.name.clone()))
                .collect(),
            translation: &translation.info.abbrev,
            appearance: settings.appearance,
            active_pinned: self.tab().pinned,
            search: &search,
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
            .child(versions)
            .child(div().w(px(6.)))
            .child(
                icon_button("text-size", IconName::ALargeSmall, "Text size").dropdown_menu(
                    |menu, _, _| {
                        menu.menu("Larger  ⌘+", Box::new(IncreaseFont))
                            .menu("Smaller  ⌘−", Box::new(DecreaseFont))
                            .menu("Actual Size  ⌘0", Box::new(ResetFont))
                    },
                ),
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

    fn render_chapter(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let settings = cx.global::<Settings>();
        let dark = theme.mode.is_dark();
        let font_size = settings.font_size;
        let line_height = settings.line_height;
        let reading_width = px(settings.reading_width);
        let reading_font = SharedString::from(settings.reading_font.clone());
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
            target: self.target(),
        };
        let book = self.chapter().book;
        let chapter = self.chapter().chapter;
        let note_ids: Vec<u32> = self.notes.iter().map(|n| n.id).collect();
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
            .id("chapter-scroll")
            .track_scroll(&self.tab().scroll)
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
                .px_8()
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
                        .child(match self.chapter().prev() {
                            Some(prev) => Button::new("top-prev")
                                .label(format!("‹ {prev}"))
                                .ghost()
                                .small()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.prev_chapter(&PrevChapter, window, cx)
                                }))
                                .into_any_element(),
                            None => div().into_any_element(),
                        })
                        .child(match self.chapter().next() {
                            Some(next) => Button::new("top-next")
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
                        .text_sm()
                        .child(self.chapter().book_info().name.to_uppercase()),
                )
                .child(
                    div()
                        .text_size(px(font_size * 2.4))
                        .text_color(theme.foreground)
                        .child(chapter.to_string()),
                ),
        );

        // Verse still running at the start of each block (poetry lines,
        // paragraph breaks mid-verse), so the jump target is highlighted whole.
        let mut verse_in_progress = None;
        for (ix, block) in self.blocks.iter().enumerate() {
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
                    let note = self.notes.iter().find(|n| n.id == *id)?;
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
                let runs = chapter_view::build_runs(&rendered, &block_style);
                StyledText::new(rendered.text.clone()).with_runs(runs)
            };
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

            let base = div().w_full().max_w(reading_width).px_8();
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

        let next = self.chapter().next();
        column = column.child(
            div()
                .w_full()
                .max_w(reading_width)
                .px_8()
                .pt_10()
                .pb_16()
                .flex()
                .justify_between()
                .font_family(".SystemUIFont")
                .child(
                    Button::new("bottom-prev")
                        .label(
                            self.chapter()
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
            .font_family(reading_font)
            .text_size(px(font_size))
            .line_height(relative(line_height))
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
                        let tab = &mut this.tabs[this.active];
                        tab.location.verse = Some(verse);
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
            .placeholder("Go to a passage (jn 3:16), search words, or type > for commands")
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
                            .max_w(px(if pinned { 120. } else { 220. }))
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
                    .context_menu(move |menu, _, _| {
                        menu.menu(
                            if pinned { "Unpin Tab" } else { "Pin Tab" },
                            Box::new(PinTab { index: ix }),
                        )
                        .menu("Rename Tab…", Box::new(RenameTab { index: ix }))
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
            "commands" => self.show_palette(&format!("{COMMAND_PREFIX}{arg}"), window, cx),
            "notes" => {
                self.inspector_open = true;
                self.selected_note = self.notes.first().map(|n| n.id);
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
            .on_action(cx.listener(Self::search_in))
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
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(self.render_toolbar(window, cx))
            .when(self.tabs.len() > 1, |d| d.child(self.render_tab_bar(cx)))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(sidebar_open, |d| d.child(self.render_sidebar(window, cx)))
                    .child(self.render_chapter(cx))
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
        let library = Library::load(&crate::library::data_dir()).ok()?;
        Some(cx.update(|cx| {
            gpui_kit::init(cx);
            crate::bind_keys(cx);
            cx.set_global(library);
            cx.set_global(Settings::default());
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
}
