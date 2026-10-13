# 01: Product and UX notes

## What the best apps get right
- **YouVersion (Bible App):** huge reach, easy pickers (tap chapter or version label in top bar), menus hide on scroll, verse tap opens action bar (highlight / bookmark / note / share / compare / image). "Compare" shows one verse across your recent versions. iPad has side-by-side with synced scroll.
- **ESV app:** praised most for typography. "Set like a printed page." Quiet chrome. Reviewers say it beats YouVersion for long reading.
- **Blue Letter Bible / ESV:** calmer than YouVersion. Quiet is a feature.
- **Olive Tree:** split screen (Bible + resource) called best in class.
- **Accordance / Logos:** pro panes and saved workspaces. Powerful, but Logos is called dense and overwhelming. Aim between ESV calm and Accordance flexibility.

## Common complaints (avoid)
- Clutter: social feed, plans, challenges crowding out the text.
- Notification overload.
- Weak search (hard to find a verse or keyword).
- Streaming dependence. Must be offline-first.
- Ads (Bible Gateway), confusing purchase tiers (Logos), abandoned apps (NeuBible).

## Desktop is different
- **Layout:** three regions.
  - Left sidebar (collapsible): versions, bookmarks, highlights, notes, history, plans.
  - Center: reading column, max ~60 to 75 characters wide (~680 to 720px), centered. Extra width goes to panes, not longer lines.
  - Right inspector (collapsible): notes, cross-references, footnotes, compare.
- **Keyboard first.** Command palette (Cmd+K or Cmd+L): one box for references ("jn 3:16", "1 cor 13", "ps 23:1-4"), full-text search and commands.
- **Shortcuts (draft):**
  - Cmd+[ / Cmd+]: previous / next chapter
  - Cmd+F: find in chapter
  - Cmd+Shift+F: search the Bible
  - Cmd+D: bookmark
  - Cmd+Opt+N: new note
  - Cmd+1..4: switch version
  - Cmd+Plus / Cmd+Minus: text size
  - Space / Shift+Space: page down / up
- **Hover replaces tap:** hover a footnote or cross-ref marker for a popover. Click verse number to select. Shift-click for ranges. Selection opens a small floating action bar near the text, not a bottom sheet.
- **Parallel:** 2 to 3 version panes, verse-aligned scroll.
- **Mac conventions:** follow system light/dark, native menus, multiple windows or tabs, trackpad swipe for chapters, "Look up in Bible" service later.
- **Mobile-only stuff to drop or shrink:** verse images, social sharing, friends, widgets-as-home. On desktop, "copy with citation" covers most of it.

## Reading view details
- Serif body, generous line height. User controls: font, size, line height, width.
- Verse numbers small superscript, muted color. Toggle to hide.
- Section headings, paragraph breaks and poetry indents preserved from the source.
- Red letter (words of Jesus): toggle.
- Footnotes and cross-refs: small markers, content in popover/inspector. Never inline by default.
- Themes: light, dark, sepia.
- Continuous scroll within a chapter. Next chapter loads below, with stable scroll position.
- Remember last position per window.

## MVP scope

**In v1**
1. Reading view with good typography and themes
2. Book / chapter navigator (keyboard + grid)
3. Command palette with reference parsing
4. Full-text search with version/book/testament filters
5. Footnote / cross-ref panel (popovers later)
6. History / continue reading
7. Menu bar companion (basic)
8. Offline only, all local

Cut from v1 (2026-10-06): verse selection (and with it highlights, verse notes, copy with citation) and the compare popover. Copy with citation came back 2026-10-06; verse selection, highlights and notes came back 2026-10-12 as saved verses ([07](07-saved-verses.md)).

**Later**
- Verse selection: highlight, bookmark, note, copy with citation
- Compare popover + side-by-side parallel panes
- Reading plans with progress (fits menu bar nicely)
- Cross-reference data from openbible.info in inspector
- Sync across Macs
- Audio
- Topic search, Strong's numbers / original language
- Windows / Linux builds

**Never (or very opt-in)**
- Social feed, streaks pressure, ads, push spam

## Open questions for you
- Notes: plain text or Markdown? Attached to one verse or a range?
- Highlights: shared across translations (recommended, keyed by verse id) or per translation?
- How "study" do you want it? Stay a reader, or grow toward Accordance-lite?
