//! The Saved verses tab (⇧⌘D): every highlight with its note and tags, to
//! search, filter, sort, drag into your own order, open in a new tab, and
//! export as Markdown. Child module of `reader` so it can use the view's
//! private state.

use std::path::PathBuf;

use super::*;
use crate::saved::{Filter, Sort};

/// Pick how the Saved verses page is sorted.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = bible_reader, no_json)]
pub struct SetSavedSort {
    pub sort: Sort,
}

/// Payload while a saved item is dragged to a new place in the list.
#[derive(Clone)]
struct DraggedSaved {
    id: i64,
}

#[derive(Default)]
pub(super) struct SavedPage {
    pub filter: Filter,
    /// Position in the shown list of the keyboard selection.
    pub selected: usize,
    /// Items just removed, for Undo (here or in the selection bar).
    pub removed: Vec<SavedVerse>,
    /// What the last export or copy did.
    pub status: Option<String>,
    pub exported: Option<PathBuf>,
    /// Items opened in a background tab from this page.
    pub opened: HashSet<i64>,
    pub scroll: ScrollHandle,
}

impl ReaderView {
    /// ⇧⌘D: go to the Saved verses tab, opening it beside this one if needed.
    pub(super) fn show_saved(
        &mut self,
        _: &ShowSaved,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_handed_off = self.palette_open;
        self.palette_open = false;
        match self.tabs.iter().position(|t| t.is_saved_page()) {
            Some(ix) => self.active = ix,
            None => {
                let here = self.tab();
                let page = Tab::saved_page(here.location, here.translation.clone());
                self.insert_tab_after_active(page);
            }
        }
        self.show_active(cx);
        self.saved_query
            .update(cx, |input, cx| input.focus(window, cx));
    }

    /// Indexes into `Saved::items` of the rows shown, in order.
    pub(super) fn shown_saved(&self, cx: &App) -> Vec<usize> {
        self.saved_page.filter.apply(&cx.global::<Saved>().items)
    }

    fn selected_saved(&self, cx: &App) -> Option<SavedVerse> {
        let shown = self.shown_saved(cx);
        let ix = *shown.get(self.saved_page.selected)?;
        cx.global::<Saved>().items.get(ix).cloned()
    }

    pub(super) fn on_saved_changed(&mut self, cx: &mut Context<Self>) {
        let count = self.shown_saved(cx).len();
        self.saved_page.selected = self.saved_page.selected.min(count.saturating_sub(1));
        cx.notify();
    }

    pub(super) fn on_saved_query(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let value = input.read(cx).value().to_string();
                if self.saved_page.filter.query != value {
                    self.saved_page.filter.query = value;
                    self.saved_page.selected = 0;
                    cx.notify();
                }
            }
            InputEvent::PressEnter { secondary, .. } => {
                if let Some(item) = self.selected_saved(cx) {
                    self.open_saved(item.id, *secondary, window, cx);
                }
            }
            _ => {}
        }
    }

    fn set_saved_filter(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Filter)) {
        change(&mut self.saved_page.filter);
        self.saved_page.selected = 0;
        cx.notify();
    }

    fn set_saved_sort(&mut self, action: &SetSavedSort, _: &mut Window, cx: &mut Context<Self>) {
        let sort = action.sort;
        self.set_saved_filter(cx, |f| f.sort = sort);
    }

    fn saved_select(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.shown_saved(cx).len();
        if count == 0 {
            return;
        }
        let next =
            (self.saved_page.selected as isize + delta).clamp(0, count as isize - 1) as usize;
        self.saved_page.selected = next;
        self.saved_page.scroll.scroll_to_item(next);
        cx.notify();
    }

    /// ⌥↑ / ⌥↓: move the selected item up or down in your own order.
    pub(super) fn move_saved(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.saved_page.filter.sort != Sort::Manual {
            return;
        }
        let shown = self.shown_saved(cx);
        let at = self.saved_page.selected;
        let to = at as isize + delta;
        if to < 0 || to as usize >= shown.len() {
            return;
        }
        let to = to as usize;
        self.drop_saved_at(shown[at], shown[to], cx);
        self.saved_page.selected = to;
        self.saved_page.scroll.scroll_to_item(to);
    }

    /// Put the item at `from` (an index into all items) where the item at
    /// `to` is: after it when moving down, before it when moving up.
    fn drop_saved_at(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from == to {
            return;
        }
        cx.update_global::<Saved, _>(|saved, _| {
            let id = saved.items[from].id;
            let before = if from < to {
                saved.items.get(to + 1).map(|i| i.id)
            } else {
                Some(saved.items[to].id)
            };
            saved.move_item(id, before);
        });
        cx.notify();
    }

    fn drop_saved(&mut self, dragged: i64, onto: i64, cx: &mut Context<Self>) {
        let items = &cx.global::<Saved>().items;
        let from = items.iter().position(|i| i.id == dragged);
        let to = items.iter().position(|i| i.id == onto);
        if let (Some(from), Some(to)) = (from, to) {
            self.drop_saved_at(from, to, cx);
        }
    }

    /// Open a saved item in a new tab, in the translation it was saved from.
    /// `background` keeps this page in front.
    pub(super) fn open_saved(
        &mut self,
        id: i64,
        background: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = cx.global::<Saved>().get(id).cloned() else {
            return;
        };
        let translation = if is_available(cx, &item.translation) {
            item.translation.clone()
        } else {
            self.tab().translation.clone()
        };
        let verse = item.verses.verse_ids(item.chapter).next();
        let tab = Tab::new(
            Location {
                chapter: item.chapter,
                verse,
            },
            translation,
        );
        if background {
            let keep = self.tabs[self.active].id;
            self.insert_tab_after_active(tab);
            self.active = self.tabs.iter().position(|t| t.id == keep).unwrap_or(0);
            self.saved_page.opened.insert(id);
            self.save_tabs(cx);
            cx.notify();
        } else {
            self.insert_tab_after_active(tab);
            self.show_active(cx);
            self.focus.focus(window, cx);
        }
    }

    fn remove_saved(&mut self, id: i64, cx: &mut Context<Self>) {
        let removed = cx.update_global::<Saved, _>(|saved, _| saved.remove(id));
        self.saved_page.removed = removed.into_iter().collect();
        self.saved_page.status = None;
        cx.notify();
    }

    fn copy_saved_item(&mut self, id: i64, cx: &mut Context<Self>) {
        if let Some(item) = cx.global::<Saved>().get(id) {
            let text = cite(&item.text, &item.label(), &item.version, &item.notice, true);
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.saved_page.status = Some(format!("Copied {}.", item.label()));
            self.saved_page.exported = None;
            cx.notify();
        }
    }

    /// What the filters narrow the list to, for the export's heading and name.
    fn saved_filter_heading(&self) -> Option<String> {
        let f = &self.saved_page.filter;
        let mut parts = Vec::new();
        if let Some(tag) = &f.tag {
            parts.push(format!("Tag: {tag}"));
        }
        match f.testament {
            Some(Testament::Old) => parts.push("Old Testament".into()),
            Some(Testament::New) => parts.push("New Testament".into()),
            None => {}
        }
        if f.with_note {
            parts.push("With notes".into());
        }
        if !f.query.trim().is_empty() {
            parts.push(format!("Matching “{}”", f.query.trim()));
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }

    /// The shown list, in the shown order, as Markdown.
    pub(super) fn saved_markdown(&self, cx: &App) -> (String, usize) {
        let saved = cx.global::<Saved>();
        let items: Vec<&SavedVerse> = self
            .shown_saved(cx)
            .into_iter()
            .map(|ix| &saved.items[ix])
            .collect();
        let date = saved::long_date(saved::now());
        let heading = self.saved_filter_heading();
        (
            saved::markdown(&items, heading.as_deref(), &date),
            items.len(),
        )
    }

    pub(super) fn copy_saved_markdown(
        &mut self,
        _: &CopySavedMarkdown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (text, count) = self.saved_markdown(cx);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.saved_page.status = Some(format!("Copied {count} as Markdown."));
        self.saved_page.exported = None;
        cx.notify();
    }

    /// Ask where to save the shown list as a Markdown file, then write it.
    pub(super) fn export_saved(&mut self, _: &ExportSaved, _: &mut Window, cx: &mut Context<Self>) {
        let (text, count) = self.saved_markdown(cx);
        let name = match &self.saved_page.filter.tag {
            Some(tag) => format!("Saved verses - {tag}.md"),
            None => "Saved verses.md".to_string(),
        };
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let downloads = home.join("Downloads");
        let dir = if downloads.is_dir() { downloads } else { home };
        let chosen = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            let written = std::fs::write(&path, text);
            this.update(cx, |this, cx| {
                let file = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                match written {
                    Ok(()) => {
                        this.saved_page.status = Some(format!("Exported {count} to {file}."));
                        this.saved_page.exported = Some(path);
                    }
                    Err(err) => {
                        this.saved_page.status = Some(format!("Couldn't write {file}: {err}"));
                        this.saved_page.exported = None;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn render_saved_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let saved = cx.global::<Saved>();
        let total = saved.items.len();
        let tags = saved.tags();
        let shown = self.shown_saved(cx);
        let filter = self.saved_page.filter.clone();
        let sort = filter.sort;

        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Saved verses"),
            )
            .child(div().text_sm().text_color(theme.muted_foreground).child(
                match (filter.is_narrowed(), total) {
                    (true, _) => format!("{} of {total}", shown.len()),
                    (false, 1) => "1 passage".to_string(),
                    (false, n) => format!("{n} passages"),
                },
            ))
            .child(div().flex_1())
            .child(
                Button::new("saved-sort")
                    .icon(IconName::SortAscending)
                    .label(sort.label())
                    .small()
                    .ghost()
                    .dropdown_menu(move |mut menu, _, _| {
                        for option in Sort::ALL {
                            menu = menu.menu_with_check(
                                option.label(),
                                option == sort,
                                Box::new(SetSavedSort { sort: option }),
                            );
                        }
                        menu
                    }),
            )
            .child(
                Button::new("saved-export")
                    .icon(IconName::FileText)
                    .label("Export")
                    .small()
                    .outline()
                    .disabled(shown.is_empty())
                    .dropdown_menu(|menu, _, _| {
                        menu.menu("Export as Markdown… (⇧⌘E)", Box::new(ExportSaved))
                            .menu("Copy as Markdown", Box::new(CopySavedMarkdown))
                    }),
            );

        let chip = |id: SharedString, label: String, on: bool| {
            Button::new(id).label(label).xsmall().outline().selected(on)
        };
        let mut chips = div().flex().flex_wrap().items_center().gap_1().child(
            chip("saved-tag-all".into(), "All".into(), filter.tag.is_none()).on_click(
                cx.listener(|this, _, _, cx| this.set_saved_filter(cx, |f| f.tag = None)),
            ),
        );
        for (ix, (tag, count)) in tags.iter().enumerate() {
            let on = filter
                .tag
                .as_ref()
                .is_some_and(|t| t.eq_ignore_ascii_case(tag));
            let pick = tag.clone();
            chips = chips.child(
                chip(
                    format!("saved-tag-{ix}").into(),
                    format!("#{tag} {count}"),
                    on,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let pick = pick.clone();
                    this.set_saved_filter(cx, |f| {
                        f.tag = if on { None } else { Some(pick) };
                    })
                })),
            );
        }
        chips = chips.child(div().w(px(1.)).h(px(16.)).mx_1().bg(theme.border));
        for (id, label, testament) in [
            ("saved-ot", "Old Testament", Testament::Old),
            ("saved-nt", "New Testament", Testament::New),
        ] {
            let on = filter.testament == Some(testament);
            chips = chips.child(chip(id.into(), label.into(), on).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.set_saved_filter(cx, |f| {
                        f.testament = if on { None } else { Some(testament) };
                    })
                },
            )));
        }
        chips = chips.child(
            chip(
                "saved-with-note".into(),
                "With notes".into(),
                filter.with_note,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.set_saved_filter(cx, |f| f.with_note = !f.with_note)
            })),
        );

        let banner = self.render_saved_banner(cx);

        let body =
            if total == 0 {
                div()
                .flex()
                .flex_col()
                .gap_2()
                .p_6()
                .items_center()
                .text_center()
                .child(Icon::new(IconName::Heart).size_6().text_color(theme.muted_foreground))
                .child(div().font_weight(FontWeight::SEMIBOLD).child("No saved verses yet"))
                .child(
                    div()
                        .max_w(px(420.))
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(
                            "In any chapter, click a verse number to select it (shift-click for a \
                             range), then choose Highlight or press ⌘D. Add a note and tags to \
                             find it again here.",
                        ),
                )
                .into_any_element()
            } else if shown.is_empty() {
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_6()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("Nothing saved matches these filters."),
                    )
                    .child(
                        Button::new("saved-clear-filters")
                            .label("Clear filters")
                            .small()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.saved_query
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                                this.set_saved_filter(cx, |f| {
                                    *f = Filter {
                                        sort: f.sort,
                                        ..Filter::default()
                                    }
                                });
                            })),
                    )
                    .into_any_element()
            } else {
                let rows = shown
                    .iter()
                    .enumerate()
                    .map(|(row, ix)| self.render_saved_row(row, *ix, cx).into_any_element())
                    .collect::<Vec<_>>();
                div()
                    .id("saved-list")
                    .track_scroll(&self.saved_page.scroll)
                    .overflow_y_scroll()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .pb_6()
                    .children(rows)
                    .into_any_element()
            };

        div()
            .id("saved-page")
            .key_context("SavedPage")
            .on_action(cx.listener(Self::set_saved_sort))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                // The search field would otherwise keep the arrows for itself.
                let keys = &event.keystroke;
                let plain = !keys.modifiers.modified();
                match keys.key.as_str() {
                    "down" if plain => this.saved_select(1, cx),
                    "up" if plain => this.saved_select(-1, cx),
                    "down" if keys.modifiers.alt => this.move_saved(1, cx),
                    "up" if keys.modifiers.alt => this.move_saved(-1, cx),
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
                    .child(
                        Input::new(&self.saved_query)
                            .prefix(Icon::new(IconName::Search).size_4())
                            .cleanable(true),
                    )
                    .child(chips)
                    .children(banner)
                    .child(body),
            )
    }

    /// Undo for a removal, or what the last export did.
    fn render_saved_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let page = &self.saved_page;
        let row = div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_1()
            .rounded_md()
            .bg(theme.muted)
            .text_sm();
        if let Some(first) = page.removed.first() {
            return Some(
                row.child(div().flex_1().child(format!("Removed {}.", first.label())))
                    .child(
                        Button::new("saved-undo")
                            .icon(IconName::Undo2)
                            .label("Undo")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| this.undo_remove(cx))),
                    )
                    .into_any_element(),
            );
        }
        let status = page.status.clone()?;
        let exported = page.exported.clone();
        Some(
            row.child(div().flex_1().child(status))
                .when_some(exported, |row, path| {
                    row.child(
                        Button::new("saved-reveal")
                            .label("Show in Finder")
                            .xsmall()
                            .ghost()
                            .on_click(move |_, _, cx| cx.reveal_path(&path)),
                    )
                })
                .into_any_element(),
        )
    }

    /// One saved item. `row` is its place in the shown list, `ix` in all items.
    fn render_saved_row(&self, row: usize, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let settings = cx.global::<Settings>();
        let reading_font = SharedString::from(settings.font_pairing.body());
        let font_size = settings.font_size;
        let item = cx.global::<Saved>().items[ix].clone();
        let id = item.id;
        let selected = row == self.saved_page.selected;
        let manual = self.saved_page.filter.sort == Sort::Manual;
        let opened = self.saved_page.opened.contains(&id);
        let (accent, accent_foreground) = (theme.accent, theme.accent_foreground);
        let swatch = highlight_color(cx);
        let label = item.label();
        let ghost = label.clone();

        let actions = div()
            .flex()
            .gap_1()
            .child(
                Button::new(("saved-open", row))
                    .icon(IconName::ExternalLink)
                    .label("Open")
                    .xsmall()
                    .ghost()
                    .tooltip("Open in a new tab (Enter; ⌘Enter keeps this list in front)")
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        let background = event.modifiers().secondary();
                        this.open_saved(id, background, window, cx);
                    })),
            )
            .child(
                Button::new(("saved-copy", row))
                    .icon(IconName::Copy)
                    .xsmall()
                    .ghost()
                    .tooltip("Copy with citation")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.copy_saved_item(id, cx);
                    })),
            )
            .child(
                Button::new(("saved-note", row))
                    .icon(IconName::FileText)
                    .xsmall()
                    .ghost()
                    .tooltip("Note and tags")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open_note_editor(id, window, cx);
                    })),
            )
            .child(
                Button::new(("saved-remove", row))
                    .icon(IconName::Delete)
                    .xsmall()
                    .ghost()
                    .tooltip("Remove")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.remove_saved(id, cx);
                    })),
            );

        let tags = item.tags.iter().enumerate().map(|(n, tag)| {
            let pick = tag.clone();
            Button::new(("saved-row-tag", row * 100 + n))
                .label(format!("#{tag}"))
                .xsmall()
                .ghost()
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    let pick = pick.clone();
                    this.set_saved_filter(cx, |f| f.tag = Some(pick));
                }))
        });

        div()
            .id(("saved-row", row))
            .flex()
            .gap_3()
            .p_3()
            .rounded_md()
            .cursor_pointer()
            .when(selected, |d| d.bg(accent).text_color(accent_foreground))
            .when(!selected, |d| d.hover(|d| d.bg(theme.muted)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.saved_page.selected = row;
                cx.notify();
            }))
            .when(manual, |d| {
                d.on_drag(DraggedSaved { id }, move |_, _, _, cx| {
                    cx.new(|_| TabGhost {
                        label: ghost.clone(),
                    })
                })
                .drag_over::<DraggedSaved>(move |style, _, _, _| style.bg(accent))
                .on_drop(cx.listener(
                    move |this, dragged: &DraggedSaved, _, cx| this.drop_saved(dragged.id, id, cx),
                ))
            })
            .child(div().w(px(4.)).flex_shrink_0().rounded_sm().bg(swatch))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(label),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(item.version.clone()),
                            )
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
                            .child(actions),
                    )
                    .child(
                        div()
                            .font_family(reading_font)
                            .text_size(px((font_size * 0.85).max(14.)))
                            .line_height(relative(1.45))
                            .child(item.text.clone()),
                    )
                    .when(!item.note.trim().is_empty(), |d| {
                        d.child(
                            div()
                                .text_sm()
                                .italic()
                                .text_color(theme.muted_foreground)
                                .child(item.note.trim().to_string()),
                        )
                    })
                    .when(!item.tags.is_empty(), |d| {
                        d.child(div().flex().flex_wrap().gap_1().children(tags))
                    }),
            )
    }
}
