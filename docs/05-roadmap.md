# 05: Roadmap (rough)

Sizes are guesses for one developer working with AI help.

## Phase 0: Spike. DONE 2026-10-06, GPUI is a go
Built more than planned: all three Bibles imported, plus palette search, notes panel and the menu bar item. Verse selection dropped from MVP. Findings: [04 §9](04-architecture-gpui.md#9-spike-findings-2026-10-06).

Original plan:
- Workspace skeleton (4 crates), gpui-kit hello world builds and runs.
- `bible-import`: BSB USFM for one book (John + Psalms), into SQLite blocks.
- Render John 3 and Psalm 23 in GPUI:
  - serif body, superscript-ish verse numbers, heading, poetry indent, red letter
  - click verse number, select verse
  - hover footnote marker, popover
  - scroll through Psalm 119
- Prototype status item (gpui-tray or objc2) with 2 menu rows.
- **Exit check:** does it read like a book? If typography is a fight, reconsider (e.g. gpui fork, or SwiftUI shell + Rust core).

## Phase 1: Reader core (2 to 3 wk). Mostly done
Done 2026-10-06: back / forward history + recent chapters in the palette, theme picker with sepia (System / Light / Sepia / Dark), multi-block verse highlight on jump, icon toolbar merged into the title bar, real `.app` bundle (`scripts/bundle.sh`), headless UI tests and `scripts/shot.sh`.
Also done 2026-10-06: tabs (per-tab passage, translation and history; restored on launch; ⌘T / ⌘W / ⇧⌘T), ⌘K / ⌘P palette rebuilt on gpui-kit `Command` (groups, arrow keys, commands with shortcut hints, ⇧⌘P), searchable translation dropdown (gpui-kit `Select`).
Remaining: chapter grid by keyboard, settings window (font / line height / width), sidebar auto-scroll to the current book.

- Full import for BSB, KJV, WEB. Verse-count validation.
- Navigator (sidebar + book/chapter grid), prev / next chapter, history.
- Themes, font / size / width settings.
- Command palette with reference parser.

## Phase 2: Search + annotations (2 wk). Search done; annotations deferred with verse selection
- FTS5 search with filters, result list, jump to verse.
- Highlights, bookmarks, notes in `user.db`. Inspector pane.
- Copy with citation (with version notice).

## Phase 3: Parallel (1 to 2 wk). Compare popover cut from MVP
- Compare popover for a verse across versions.
- 2 to 3 pane parallel with aligned scroll, gaps for omitted verses.

## Phase 4: Menu bar (3 to 5 days). Basic version done (no launch-at-login yet)
- `gpui-tray` plain menu: verse of the day (curated list, local), copy, continue reading, recent, open app, launch at login, quit. See 02.
- Close-last-window keeps app alive.

## Phase 5: Package + licensed texts (time depends on publishers)
- ~~`.app` bundle script (Info.plist + binary + DBs in Resources). Ad-hoc sign for personal use.~~ Done: `scripts/bundle.sh`.
- When sharing: Developer ID signing, notarization, DMG. Sparkle optional.
- SQLCipher for licensed DBs. Import NIV / ESV / NLT when files + permission arrive.

## Later
Reading plans (+ menu bar progress), openbible cross-refs, sync, audio, Strong's.

## Not planned
Windows / Linux, Mac App Store, Swift menu bar helper.

## Start in parallel today
- Licensing emails to Biblica, Tyndale, Crossway (owner). Mention: free, non-commercial, offline macOS app, personal use first.
- Pick 5th / 6th version.
- Pull BSB USFM (github.com/BSB-publishing/bsb2usfm) and KJV / WEB USFM (ebible.org).
