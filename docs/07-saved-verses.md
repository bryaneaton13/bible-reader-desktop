# 07: Saved verses (highlights, notes, tags, a list tab, export)

Status: built 2026-10-12 (branch `saved-verses`). This brings back the verse selection, highlights and verse notes that [01](01-product-ux.md) cut from v1, and adds a tab to manage them. The first plan had more in it; section 6 lists what was dropped and why.

## 1. What it does

You select one or more verses in a chapter and highlight them. You can add a note and tags. Every saved item goes into one list, the **Saved verses** tab, where you can:
- search, filter and sort it
- drag items into your own order
- open any item in a new tab
- export it, or a filtered part of it, as Markdown

Use cases:
- collecting memory verses (tag them `Memory`, filter, export)
- collecting verses you like over time

## 2. Decisions

| Topic | Decision |
|---|---|
| What a saved item is | One chapter plus any set of its verses (`Romans 8:1–4, 28`, a `VerseSet`), with a note and free-text tags. Items that cross chapters are not supported. |
| Key | Book, chapter and verse numbers, not the translation, so a highlight shows in every translation ([03](03-bible-content.md)). |
| Text | Copied into `user.db` when the item is saved, in the translation it was saved from, along with that version's label and copyright notice. This includes YouVersion Bibles like the NIV, at any length. The owner chose this for personal use (2026-10-12). It is separate from the 30-day chapter cache in [06](06-youversion-platform.md). Revisit it before sharing the app. |
| Color | One highlight color, chosen per theme: a soft yellow highlighter in Light, amber in Sepia, muted gold in Dark (`highlights::highlight_color`). The jump-to-verse wash used to be yellow. It is now a neutral gray so it doesn't look like a highlight. Selected verses use the theme's info color. |
| Storage | `~/Library/Application Support/BibleReader/user.db`, SQLite in WAL mode, via `bible-store::user::UserStore` (no GPUI). The `Saved` global in `app/src/saved.rs` keeps it in memory and notifies the views. |
| The list page | A real tab (`TabKind::Saved`). There is at most one. It can be pinned and is restored at launch. It can't be split. |
| Export | Markdown only (file or clipboard). |

## 3. Reader

- **Select verses:**
  - Click a verse number to select or unselect that verse.
  - Shift-click selects a range from the last click.
  - ⌥-click anywhere in a verse's text works the same way, which helps when verse numbers are hidden.
  - Clicking an already highlighted verse with nothing selected picks up the whole saved item.
  - Esc clears the selection.
- **Selection bar:** a floating pill near the bottom of the text, showing the reference.
  - For a new selection: **Highlight**, **Add note…**, Copy, and Close. **Remove highlight** also appears if the selection touches a saved item.
  - For a selection that matches a saved item exactly: **Note & tags…** and **Remove highlight**.
  - After a removal it offers **Undo**.
- **Shortcuts:**
  - ⌘D highlights the selection, or the verse you jumped to.
  - ⇧⌘C copies the selection with its citation.
  - ⇧⌘D opens Saved verses.
  - ⇧⌘E exports.
  - These are also in the menus and the palette, and listed in the Shortcuts window.
- **Note and tags editor:** an overlay with a multi-line note and a tags field.
  - The tags field is comma separated, and suggests existing tags that match the word being typed (`saved::suggest_tags`).
  - Saving: ⌘Enter, or Enter in the tags field.
  - Esc cancels.
- **Notes panel:** a "Saved in this chapter" section with each item's note and tags. Click an item to edit it.

## 4. The Saved verses tab

- **Header:** a count, a sort menu (My order, Newest first, Oldest first, Bible order) and an Export menu (Export as Markdown…, Copy as Markdown).
- **Search:** matches words in the reference, version, text, note and tags. `#tag` matches a tag. A query with a digit is read as a passage first (`ps 23`, `rom 8:28`).
- **Filter chips:** All, one chip per tag with its count, Old Testament, New Testament, With notes.
- **Rows:** a highlight bar, the reference and version, the saved text in the reading font, the note, and the tags (click a tag to filter by it). Actions:
  - **Open** opens a new tab at the passage, in the translation it was saved from, and switches to it. ⌘-click (or ⌘Enter) opens it behind the list instead.
  - Copy with citation.
  - Note and tags.
  - Remove, with Undo.
- **Keyboard:**
  - ↑↓ select a row.
  - Enter opens it.
  - ⌥↑ / ⌥↓ move it within My order.
  - In My order you can also drag rows.
- **Navigation:** going anywhere from this tab (sidebar, ⌘K, a related verse) opens a reading tab beside it, so the list stays put.

## 5. Export format

Export uses the current view: the current filters, search and order. The save panel is the native macOS one (`cx.prompt_for_new_path`) and defaults to `~/Downloads/Saved verses.md`, or `Saved verses - <tag>.md` when filtered by a tag. After saving, a **Show in Finder** button appears.

```markdown
# Saved verses

Tag: Memory · 2 passages · exported October 12, 2026

## Romans 8:28 (NIV)

> And we know that in all things God works for the good of those who love him…

For hard days.

#Memory #Hope

---

NIV: <copyright notice> Provided by YouVersion.
```

Each version's notice appears once at the end. Public domain versions get no notice.

## 6. Dropped from the first plan

- Several highlight colors: replaced by one color per theme.
- Word (.docx) export: may come later. `docx-rs` is the candidate.
- A "learned" flag and any memory-verse UI: tags cover memory lists for now.
- UUID ids: the ids are SQLite integers. Sync would need a migration.
- A right-click verse menu: ⌥-click covers hidden verse numbers.

## 7. Code map

- `bible-core/src/verse_set.rs`: `VerseSet` (parse `1-4,28`, display `1–4, 28`, toggle, ranges).
- `bible-store/src/user.rs`: `user.db` schema, `UserStore` (insert, update note and tags, remove, restore, reorder).
- `app/src/saved.rs`: the `Saved` global, `Filter` / `Sort`, tag suggestions, `join_verse_texts`, `markdown`.
- `app/src/highlights.rs`: selection, selection bar, note editor, notes panel section.
- `app/src/saved_page.rs`: the tab.
- `app/src/chapter_view.rs`: saved and selected backgrounds per verse segment (`RenderedBlock.segments`, `verse_at`).
- `app/src/tabs.rs`: `TabKind`.
- Tests:
  - unit tests in each module
  - headless UI tests in `reader.rs`: selecting and saving, the note editor and tag suggestions, the Saved tab (filter, reorder, open), and restoring the tab after a restart
- Screenshot steps: see `AGENTS.md` (`select`, `highlight`, `note`, `noteeditor`, `saved`, `savedsearch`).
