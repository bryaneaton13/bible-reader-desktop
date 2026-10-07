# Bible Reader (desktop): preliminary plan

Status: written 2026-10-06 as the starting plan. Much of it is now built; see the [roadmap](05-roadmap.md) for progress.

## Docs
1. [01-product-ux.md](01-product-ux.md): what good Bible readers do, what changes for desktop, MVP scope
2. [02-menubar.md](02-menubar.md): menu bar companion, lessons from CodexBar, how to build it
3. [03-bible-content.md](03-bible-content.md): versions, licensing, file formats, versification
4. [04-architecture-gpui.md](04-architecture-gpui.md): GPUI + gpui-kit, crate layout, data model, run/test/package
5. [05-roadmap.md](05-roadmap.md): phases and a first spike

## One-paragraph pitch
Calm, book-quality Bible reader for Mac. Offline-first. Fast keyboard-driven navigation (command palette that understands "jn 3:16"). Desktop layout: sidebar, centered reading column, right inspector. Side-by-side versions. Highlights and notes keyed to verse, not to translation. Small menu bar companion for verse of the day, continue reading and quick lookup. Built in Rust with GPUI + gpui-kit.

## Recommended path (short)
- **Stack:** Rust workspace. GPUI through `gpui-kit` (crates.io, no git deps). Domain crates have no GPUI dependency.
- **Data:** USFM/USX import, then a normalized SQLite with FTS5 per translation (read-only). Separate `user.db` for highlights, notes and history.
- **Content order:** ship public domain first (BSB, KJV, WEB). Start NIV/ESV/NLT license talks now, in parallel. They gate release, not development.
- **Menu bar:** in-process `gpui-tray`, plain menu. Fallback: raw `NSStatusItem` via `objc2`.
- **First step:** 1 to 2 week spike to prove GPUI can render Bible typography well (see roadmap). Biggest technical risk lives there.

## Decisions (2026-10-06)
- **Platform:** macOS only. No Windows / Linux work. Free to use `objc2` / AppKit directly where GPUI falls short.
- **Menu bar:** simple plain menu via `gpui-tray`, in-process. No Swift helper, no rich cards.
- **Licensing outreach:** owner handles Biblica (NIV), Tyndale (NLT), Crossway (ESV).
- **Sync:** none in v1. Still use UUIDs + `updated_at` in `user.db` so it can come later.
- **Distribution:** free download, direct (not Mac App Store). Personal use first, maybe shared later.

### What these imply
- No App Store means no sandbox, no App Group. Data lives in `~/Library/Application Support/<bundle id>/`.
- Personal use: ad-hoc signing (`codesign -s -`) is enough on your own Mac. Sharing later needs Developer ID signing + notarization ($99/yr Apple Developer account), or recipients hit Gatekeeper warnings.
- Free / non-commercial: say so in publisher outreach. It usually makes terms easier and cheaper.
- The menu bar only exists while the app process runs. So closing the last window must **not** quit the app (keep running, menu bar stays, Dock icon optional). Quit from menu bar or Cmd+Q.
- `gpui-tray` must be built with the backend feature that matches gpui-kit's gpui (`gpui-pre`). Verify in the spike. Fallback: raw `NSStatusItem` via `objc2-app-kit`.

## More decisions (2026-10-06)
- **Versions now:** BSB, KJV, WEB (all public domain, downloaded as USFM). Licensed ones come later.
- **MVP cuts:** no compare popover, no verse selection (so no highlights or verse notes in v1).
- **gpui-pre:** accepted.
- **Licensed text storage:** SQLite (encrypted SQLCipher only if a publisher requires it).

## Status: Phase 0 spike done (2026-10-06)
GPUI is a go. Working: BSB/KJV/WEB import, reading view, sidebar navigator, ⌘K palette (reference + full-text search), footnotes panel, translation switch, themes, font size, menu bar item via `gpui-tray`. See the root [README](../README.md) to run it and [AGENTS.md](../AGENTS.md) for how to validate changes.

## Status: Phase 1 mostly done (2026-10-06)
Added back / forward history, recents, Sepia theme, title-bar toolbar with icons, a launchable `.app` bundle, headless UI tests and a one-command screenshot script. Remaining Phase 1 items are in [05](05-roadmap.md). Spike findings are in [04](04-architecture-gpui.md#9-spike-findings-2026-10-06).

## Still open
1. **Menu bar icon:** gpui-tray has no template-image option, so the icon is a fixed gray. Fine for now; a small `objc2` call can fix it later.
2. **Continuous scroll** across chapters vs the current next/prev at the end of each chapter.

## Confidence notes
- UX findings: drawn from reviews/blogs, not hands-on testing of every app. Desktop recommendations are partly design judgement.
- GPUI / gpui-kit: versions checked on crates.io (Oct 2026). Code samples taken from READMEs, **not compiled yet**.
- Licensing: quotation limits and API terms checked on publisher sites. Offline license terms are negotiated case by case, so expect surprises.
