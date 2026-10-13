//! Picking verses in the text and saving them as highlights, the bar that
//! offers it, and the note and tags editor. Child module of `reader` so it
//! can use the view's private state.
//!
//! Click a verse number to select that verse (again to unselect it),
//! shift-click to select a range, ⌥-click anywhere in a verse's text when
//! numbers are hidden. ⌘D saves the selection.

use super::*;

/// Tag suggestions shown under the tags field.
const TAG_SUGGESTIONS: usize = 8;

/// The one highlight color, tuned per theme so text stays readable.
pub(super) fn highlight_color(cx: &App) -> Hsla {
    let dark = cx.theme().mode.is_dark();
    match cx.global::<Settings>().appearance {
        _ if dark => hsla(46. / 360., 0.55, 0.36, 0.55),
        Appearance::Sepia => hsla(42. / 360., 0.85, 0.6, 0.38),
        _ => hsla(50. / 360., 1., 0.62, 0.42),
    }
}

impl ReaderView {
    /// Drop the selection once its pane no longer shows its chapter.
    pub(super) fn keep_selection_if_visible(&mut self) {
        let Some(sel) = &self.selection else { return };
        let tab = &self.tabs[self.active];
        let visible = !tab.is_saved_page()
            && [Some(tab), tab.side.as_deref()]
                .into_iter()
                .flatten()
                .any(|p| p.id == sel.pane && p.location.chapter == sel.chapter);
        if !visible {
            self.selection = None;
            self.saved_page.removed.clear();
        }
    }

    /// A click on a verse number (`first..=last` for merged verses).
    pub(super) fn click_verse(
        &mut self,
        pane: u64,
        chapter: ChapterRef,
        (first, last): (u16, u16),
        shift: bool,
        cx: &mut Context<Self>,
    ) {
        self.saved_page.removed.clear();
        match self.selection.as_mut() {
            Some(sel) if sel.pane == pane && sel.chapter == chapter => {
                if shift {
                    let (a, b) = (sel.anchor.min(first), sel.anchor.max(last));
                    sel.verses.extend(a..=b);
                } else {
                    let on = sel.verses.contains(first);
                    for v in first..=last {
                        if sel.verses.contains(v) == on {
                            sel.verses.toggle(v);
                        }
                    }
                    sel.anchor = first;
                }
                if sel.verses.is_empty() {
                    self.selection = None;
                }
            }
            _ => {
                // Clicking a highlighted verse picks up its whole item, so
                // the bar can edit or remove it.
                let saved = cx.global::<Saved>();
                let verses = match saved.in_chapter(chapter).find(|i| i.verses.contains(first)) {
                    Some(item) if !shift => item.verses.clone(),
                    _ => VerseSet::new(first..=last),
                };
                self.selection = Some(Selection {
                    pane,
                    chapter,
                    verses,
                    anchor: first,
                });
            }
        }
        cx.notify();
    }

    pub(super) fn clear_selection(
        &mut self,
        _: &ClearSelection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selection.take().is_some() {
            self.saved_page.removed.clear();
            cx.notify();
        } else {
            cx.propagate();
        }
    }

    /// The selection's verse texts, version label and notice.
    fn selection_texts(&self, cx: &App) -> Option<(Selection, String, String, String, String)> {
        let sel = self.selection.clone()?;
        let pane = self.find_pane(sel.pane)?;
        let key = pane.translation.clone();
        let (texts, version, notice) =
            chapter_texts(cx, self.content.get(&sel.pane), &key, sel.chapter);
        let text = saved::join_verse_texts(&sel.verses, &texts);
        Some((sel, key, version, text, notice))
    }

    /// Save the selection as a highlight. Returns the saved item's id.
    pub(super) fn save_selection(&mut self, cx: &mut Context<Self>) -> Option<i64> {
        let (sel, translation, version, text, notice) = self.selection_texts(cx)?;
        let id = cx.update_global::<Saved, _>(|saved, _| {
            saved.add(NewSaved {
                chapter: Some(sel.chapter),
                verses: sel.verses.clone(),
                translation,
                version,
                text,
                notice,
            })
        });
        self.selection = None;
        self.saved_page.removed.clear();
        cx.notify();
        id
    }

    /// ⌘D: highlight the selected verses, or the verse jumped to.
    pub(super) fn save_selection_action(
        &mut self,
        _: &SaveSelection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs[self.active].is_saved_page() {
            return;
        }
        if self.selection.is_none()
            && let Some(verse) = self.target()
        {
            self.selection = Some(Selection {
                pane: self.tab().id,
                chapter: verse.chapter_ref(),
                verses: VerseSet::new([verse.verse()]),
                anchor: verse.verse(),
            });
        }
        self.save_selection(cx);
    }

    /// Remove every highlight that touches the selection; the selection
    /// stays so Undo (or Highlight) can bring it back.
    fn remove_selected_highlights(&mut self, cx: &mut Context<Self>) {
        let Some(sel) = self.selection.clone() else {
            return;
        };
        let ids: Vec<i64> = cx
            .global::<Saved>()
            .in_chapter(sel.chapter)
            .filter(|i| i.verses.iter().any(|v| sel.verses.contains(v)))
            .map(|i| i.id)
            .collect();
        let removed: Vec<SavedVerse> = cx.update_global::<Saved, _>(|saved, _| {
            ids.iter().filter_map(|id| saved.remove(*id)).collect()
        });
        self.saved_page.removed = removed;
        cx.notify();
    }

    pub(super) fn undo_remove(&mut self, cx: &mut Context<Self>) {
        let removed = std::mem::take(&mut self.saved_page.removed);
        cx.update_global::<Saved, _>(|saved, _| {
            for item in &removed {
                saved.restore(item);
            }
        });
        cx.notify();
    }

    pub(super) fn copy_selection(&mut self, cx: &mut Context<Self>) {
        if let Some((sel, _, version, text, notice)) = self.selection_texts(cx)
            && !text.is_empty()
        {
            let label = sel.verses.label(sel.chapter);
            cx.write_to_clipboard(ClipboardItem::new_string(cite(
                &text, &label, &version, &notice, true,
            )));
        }
    }

    // MARK: Note editor

    pub(super) fn open_note_editor(
        &mut self,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = cx.global::<Saved>().get(id).cloned() else {
            return;
        };
        self.palette_open = false;
        self.renaming = None;
        self.note_editor = Some(id);
        let tags = match item.tags.is_empty() {
            true => String::new(),
            false => format!("{}, ", item.tags.join(", ")),
        };
        self.tags_input
            .update(cx, |input, cx| input.set_value(tags, window, cx));
        self.note_input.update(cx, |input, cx| {
            input.set_value(item.note.clone(), window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn save_note_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.note_editor.take() else {
            return;
        };
        let note = self.note_input.read(cx).value().to_string();
        let tags = saved::parse_tags(&self.tags_input.read(cx).value());
        cx.update_global::<Saved, _>(|saved, _| saved.set_note(id, &note, &tags));
        self.refocus_after_editor(window, cx);
    }

    pub(super) fn close_note_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.note_editor.take().is_some() {
            self.refocus_after_editor(window, cx);
        }
    }

    fn refocus_after_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs[self.active].is_saved_page() {
            self.saved_query
                .update(cx, |input, cx| input.focus(window, cx));
        } else {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    /// ⌘Enter in the note saves (plain Enter is a new line).
    pub(super) fn on_note_input(
        &mut self,
        _: &Entity<TextareaState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let InputEvent::PressEnter {
            secondary: true, ..
        } = event
        {
            self.save_note_editor(window, cx);
        }
    }

    pub(super) fn on_tags_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::PressEnter { .. } => self.save_note_editor(window, cx),
            InputEvent::Change => cx.notify(),
            _ => {}
        }
    }

    fn render_note_editor(&self, id: i64, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let saved = cx.global::<Saved>();
        let Some(item) = saved.get(id) else {
            return div().into_any_element();
        };
        let typed = self.tags_input.read(cx).value().to_string();
        let suggestions = saved::suggest_tags(&typed, &saved.tags(), TAG_SUGGESTIONS);
        let title = format!("{} ({})", item.label(), item.version);
        let chips = suggestions.into_iter().enumerate().map(|(ix, tag)| {
            Button::new(("tag-suggestion", ix))
                .label(tag.clone())
                .xsmall()
                .outline()
                .on_click(cx.listener(move |this, _, window, cx| {
                    let value = saved::complete_tag(&this.tags_input.read(cx).value(), &tag);
                    this.tags_input.update(cx, |input, cx| {
                        input.set_value(value, window, cx);
                        input.focus(window, cx);
                    });
                }))
        });
        div()
            .id("note-backdrop")
            .occlude()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(72.))
            .bg(hsla(0., 0., 0., 0.25))
            .on_click(cx.listener(|this, _, window, cx| this.close_note_editor(window, cx)))
            .child(
                div()
                    .id("note-editor")
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key != "escape" {
                            return;
                        }
                        this.close_note_editor(window, cx);
                        cx.stop_propagation();
                    }))
                    .w(px(480.))
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .shadow_lg()
                    .font_family(".SystemUIFont")
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().size(px(10.)).rounded_full().bg(highlight_color(cx)))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .line_clamp(3)
                            .child(item.text.clone()),
                    )
                    .child(
                        div()
                            .pt_1()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Note"),
                    )
                    .child(Textarea::new(&self.note_input))
                    .child(
                        div()
                            .pt_1()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Tags"),
                    )
                    .child(Input::new(&self.tags_input))
                    .child(div().flex().flex_wrap().gap_1().children(chips))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .pt_2()
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child("⌘Enter to save, Esc to cancel"),
                            )
                            .child(
                                Button::new("note-cancel")
                                    .label("Cancel")
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_note_editor(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("note-save")
                                    .label("Save")
                                    .small()
                                    .primary()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_note_editor(window, cx)
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    // MARK: Selection bar

    /// The floating bar over the text while verses are selected.
    pub(super) fn render_selection_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let sel = self.selection.as_ref()?;
        let theme = cx.theme().clone();
        let saved = cx.global::<Saved>();
        let exact = saved.find_exact(sel.chapter, &sel.verses).map(|i| i.id);
        let touches = saved
            .in_chapter(sel.chapter)
            .any(|i| i.verses.iter().any(|v| sel.verses.contains(v)));
        let removed = !self.saved_page.removed.is_empty();
        let label = sel.verses.label(sel.chapter);
        let swatch = highlight_color(cx);

        let mut bar = div()
            .id("selection-bar")
            .occlude()
            .flex()
            .items_center()
            .gap_1()
            .pl_3()
            .pr_1()
            .py_1()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            // Opaque, so the text it floats over doesn't show through.
            .bg(theme.background)
            .text_color(theme.foreground)
            .shadow_lg()
            .font_family(".SystemUIFont")
            .text_sm()
            .child(
                div()
                    .pr_2()
                    .font_weight(FontWeight::SEMIBOLD)
                    .whitespace_nowrap()
                    .child(label),
            );
        bar = match exact {
            Some(id) => bar
                .child(
                    Button::new("sel-note")
                        .label("Note & tags…")
                        .small()
                        .ghost()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.selection = None;
                            this.open_note_editor(id, window, cx);
                        })),
                )
                .child(
                    Button::new("sel-remove")
                        .label("Remove highlight")
                        .small()
                        .ghost()
                        .on_click(
                            cx.listener(|this, _, _, cx| this.remove_selected_highlights(cx)),
                        ),
                ),
            None => bar
                .child(
                    Button::new("sel-highlight")
                        .child(div().size(px(10.)).rounded_full().bg(swatch))
                        .label("Highlight")
                        .small()
                        .ghost()
                        .tooltip("Highlight and save (⌘D)")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.save_selection(cx);
                        })),
                )
                .child(
                    Button::new("sel-add-note")
                        .label("Add note…")
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(id) = this.save_selection(cx) {
                                this.open_note_editor(id, window, cx);
                            }
                        })),
                )
                .when(touches, |bar| {
                    bar.child(
                        Button::new("sel-remove")
                            .label("Remove highlight")
                            .small()
                            .ghost()
                            .on_click(
                                cx.listener(|this, _, _, cx| this.remove_selected_highlights(cx)),
                            ),
                    )
                })
                .when(removed, |bar| {
                    bar.child(
                        Button::new("sel-undo")
                            .icon(IconName::Undo2)
                            .label("Undo")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| this.undo_remove(cx))),
                    )
                }),
        };
        bar = bar
            .child(
                Button::new("sel-copy")
                    .icon(IconName::Copy)
                    .small()
                    .ghost()
                    .tooltip("Copy with citation (⇧⌘C)")
                    .on_click(cx.listener(|this, _, _, cx| this.copy_selection(cx))),
            )
            .child(
                Button::new("sel-clear")
                    .icon(IconName::Close)
                    .small()
                    .ghost()
                    .tooltip("Clear selection (Esc)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.clear_selection(&ClearSelection, window, cx)
                    })),
            );
        Some(
            div()
                .absolute()
                .bottom_4()
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(bar)
                .into_any_element(),
        )
    }

    /// "Saved in this chapter" at the top of the notes panel.
    pub(super) fn render_saved_in_chapter(&self, cx: &mut Context<Self>) -> Option<Div> {
        let theme = cx.theme().clone();
        let chapter = self.chapter();
        let items: Vec<SavedVerse> = cx.global::<Saved>().in_chapter(chapter).cloned().collect();
        if items.is_empty() {
            return None;
        }
        let swatch = highlight_color(cx);
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
                    .child("SAVED IN THIS CHAPTER"),
            );
        for item in items {
            let id = item.id;
            section =
                section.child(
                    div()
                        .id(("saved-here", id as usize))
                        .flex()
                        .gap_2()
                        .p_2()
                        .rounded_md()
                        .text_sm()
                        .cursor_pointer()
                        .hover(|d| d.bg(theme.muted))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_note_editor(id, window, cx)
                        }))
                        .child(div().w(px(3.)).flex_shrink_0().rounded_sm().bg(swatch))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(format!("{} · {}", item.label(), item.version)),
                                )
                                .child(match item.note.trim().is_empty() {
                                    true => div()
                                        .text_color(theme.muted_foreground)
                                        .child("Add a note…"),
                                    false => div().line_clamp(4).child(item.note.clone()),
                                })
                                .when(!item.tags.is_empty(), |d| {
                                    d.child(
                                        div().text_xs().text_color(theme.muted_foreground).child(
                                            item.tags
                                                .iter()
                                                .map(|t| format!("#{t}"))
                                                .collect::<Vec<_>>()
                                                .join(" "),
                                        ),
                                    )
                                }),
                        ),
                );
        }
        Some(section)
    }

    pub(super) fn render_overlays(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.note_editor.map(|id| self.render_note_editor(id, cx))
    }
}
