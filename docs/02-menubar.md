# 02: Menu bar companion

## CodexBar: what it is and what to copy
CodexBar (github.com/steipete/CodexBar, MIT, Swift 6 / macOS 14+) shows AI usage limits in the menu bar.

Notes from its README and `docs/ui.md` / `docs/architecture.md`. Source files not read yet.
- **Shape:** AppKit `NSStatusItem` created in an `AppDelegate`, SwiftUI used for "cards" inside the menu. Docs point to NSStatusItem, not SwiftUI `MenuBarExtra`.
- **Icon:** 18x18 template image (auto light/dark). Fill level = state. Dimmed when stale or errored. Small overlays for alerts.
- **Menu:** rich card at top (headline number, progress bar, countdown), plain menu rows for actions below.
- **Modes:** one status item per provider, or "merge" into one item with a switcher.
- **Pace tracking:** "ahead / behind" vs expected. Maps straight onto reading plans.
- **Refresh:** background loop with sensible defaults. Never triggers permission prompts from background work.
- **Storage:** SQLite with WAL.
- **Extras:** settings window, launch at login, Sparkle updates, Homebrew cask, bundled CLI, WidgetKit widgets.
- **Lesson:** dense but scannable. Good defaults. Few settings. Optional fields in the bar text.

## What ours shows
- **Icon:** template book glyph. Optional small dot when today's plan reading not done. Optional text ("John 3", "Day 12/90").
- **Top card:** verse of the day (2 to 3 lines, bold reference). Actions: Copy, Open in app.
- **Continue reading:** "Continue: Romans 8" opens main window at that spot.
- **Plan card (later):** "Day 12 of 90", today's passages, progress bar, Mark done.
- **Quick lookup:** type "jn 3:16", see text, Return opens in app, Cmd+C copies. Maybe global hotkey for this.
- **Footer:** Open Bible Reader, Settings…, Launch at login, Quit.
- **Refresh:** data is static. Update at midnight and on wake. No polling.

## How to build it (options)
GPUI core has **no** native tray / status item support. Zed closed a PR for it (Dec 2025).

1. **`gpui-tray` crate, in-process**
   - Pros: same process, same state, GPUI actions, cross-platform.
   - Cons: menu only (no rich cards or text field), young crate, must match GPUI version.
2. **Raw `NSStatusItem` via `objc2-app-kit`, in-process**
   - Pros: full control, can attach custom views or an `NSPopover`.
   - Cons: unsafe FFI, macOS only, more code.
3. **`tray-icon` + `muda` (Tauri team)**
   - Pros: mature.
   - Cons: wants its own main-thread event loop, clashes with GPUI. Not recommended.
4. **Separate SwiftUI helper app (CodexBar style)**
   - Pros: best native look, rich cards, text field. Runs when main app closed. Launch at login via `SMAppService`.
   - Cons: second binary to sign / notarize / bundle. Needs IPC. macOS only.
   - IPC: helper reads shared SQLite (App Group container, read-only). Commands to main app via URL scheme (`biblereader://open?ref=JHN.3.16`).
5. **Quick lookup as a GPUI popup window** (works with 1 or 2): status item click opens a small borderless GPUI window positioned under the icon. Gets rich UI while staying in Rust. Needs a spike to check window positioning and focus behaviour.

## Decision (2026-10-06): option 1, `gpui-tray`, plain menu
Menu contents for v1:
```
[book icon]
  "For God so loved the world…"   (verse of the day, truncated, disabled row)
  John 3:16 (BSB)
  Copy Verse of the Day
  ─────────
  Continue: Romans 8
  Recent ▸   (last 5 passages)
  ─────────
  Open Bible Reader
  Launch at Login   ✓
  Quit
```
- Each row dispatches a GPUI action. Rebuild the menu with `refresh_menu()` when verse of the day changes (midnight / wake) or history changes.
- No quick-lookup text field (menus can't host one). Substitute: "Open Bible Reader" focuses the main window with the command palette open. Option 5 (a GPUI popup window) remains a later upgrade.
- App keeps running when the last window closes, so the menu bar stays alive. Cmd+Q / menu Quit exits.
- Launch at login: `SMAppService.mainApp` via `objc2` (needs a bundled `.app`), or skip for v1.
- Spike check: `gpui-tray` builds against gpui-kit's `gpui-pre`. If not, fall back to option 2 (raw `NSStatusItem` + `NSMenu` via `objc2-app-kit`). The menu is small, so fallback is cheap.
- No Swift helper. CodexBar-style rich cards are out of scope.
