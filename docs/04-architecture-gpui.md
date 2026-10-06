# 04: Architecture: GPUI, gpui-kit, data, run / test / package

Versions checked on crates.io on 2026-10-06. Code below comes from upstream READMEs and has **not been compiled yet**. Verify during the spike.

## 1. The stack

### GPUI
- Zed's Rust GPU UI framework. Metal on macOS (best supported). Linux (Vulkan, Wayland/X11) and Windows 10+ work. WASM is newer.
- **Model:**
  - `Application` owns all state.
  - `Entity<T>` is a handle to state (`cx.new(...)`, `entity.update(cx, ...)`).
  - Views implement `Render`. `render(&mut self, &mut Window, &mut Context<Self>) -> impl IntoElement`. Call `cx.notify()` to re-render.
  - `RenderOnce` for stateless components.
- **Elements:** `div()` with Tailwind-like builders (`.flex().flex_col().gap_2().p_4().bg(..)`), Taffy layout. Also `uniform_list` (fixed-height virtual list), `list` (variable height), `img`, `svg`, `canvas`.
- **Actions + keys:** `actions!(...)` or `#[derive(Action)]`. Register with `cx.bind_keys([KeyBinding::new("cmd-]", NextChapter, Some("Reader"))])`. Handle with `.on_action(cx.listener(...))`. Dispatch follows focus tree + `key_context`.
- **Async:** `cx.spawn` (main thread), `cx.background_spawn` (thread pool). Drop `Task` to cancel.
- **Text:** CoreText shaping on macOS. Styled text runs exist (`StyledText`, `InteractiveText` for clickable ranges).
- **Publishing:** official `gpui` on crates.io is 0.2.2 (Oct 2025) and stale vs Zed `main` (which split out `gpui_platform`). Direct use means a git dependency pinned to a Zed rev.

### gpui-kit (Longbridge)
- What you called "gpui-kit" is real. `gpui-kit` 0.7.1 on crates.io, repo `github.com/longbridge/gpui-kit`. Successor / umbrella of `gpui-component` (also 0.7.1). Apache-2.0. Used in Longbridge Pro. Releases weekly.
- **Layers:**
  - `gpui-kit`: facade
  - `gpui-base`: unstyled behaviour (input, tree, virtual list, dock model)
  - `gpui-component`: styled components, shadcn-like, Lucide icons
- **Components we want:**
  - dock / resizable panels, sidebar, tabs
  - input / textarea, list / virtual list, tree
  - popover, menus, dialogs, toasts
  - theme (light/dark)
  - markdown view (handy for notes)
  - test helpers
- **Underlying gpui:** depends on `gpui-pre` (=0.3.8): weekly snapshot of Zed gpui published by the gpui-kit maintainer. So: no git deps needed, but a single-maintainer supply chain. Pin exact versions and commit `Cargo.lock`.
- Docs: gpui-kit.com/docs (much better than upstream gpui docs).
- Needs Rust 1.92+.

### Alternatives (not recommended now)
- adabraka-ui / adabraka-gpui fork: has tray + global hotkeys built in. Fork risk.
- Raw Zed gpui git dependency + build our own components. More work, more control.

## 2. Workspace layout

```
bible_reader_app/
  Cargo.toml                # [workspace], resolver = "2"
  rust-toolchain.toml       # pin, e.g. channel = "1.92" or newer
  crates/
    bible-core/             # no GPUI. VerseId, Reference, book table,
                            #   reference parser ("jn 3:16"), versification
    bible-store/            # no GPUI. rusqlite: open translation DBs, user.db,
                            #   queries, FTS search, migrations
    bible-import/           # bin, no GPUI. USFM/USX -> translation .sqlite
    app/                    # bin. GPUI + gpui-kit. Views, actions, theme, menu bar
  data/
    sources/                # raw USFM/USX (gitignored for licensed texts)
    build/                  # generated *.sqlite
  docs/
```

**Rule:** all logic that can be tested without a window lives in `bible-core` / `bible-store`. `app` stays thin. This also lets a future Swift menu bar helper or CLI reuse the same data.

## 3. Data design

**Translation DB:** one file per version (e.g. `bsb.sqlite`, `niv.sqlite`). Read-only at runtime. Encrypted (SQLCipher) for licensed versions.
```sql
meta(key TEXT PRIMARY KEY, value TEXT)       -- abbrev, name, copyright notice, versification
book(id INTEGER PRIMARY KEY, code TEXT, name TEXT, chapters INTEGER)
verse(id INTEGER PRIMARY KEY, text TEXT)     -- id = BBCCCVVV, plain text
block(chapter_key INTEGER, seq INTEGER, kind TEXT, verse_id INTEGER,
      text TEXT, attrs TEXT,                 -- kind: heading|para|poetry|verse_num|text|wj|note_ref
      PRIMARY KEY(chapter_key, seq))
note(id INTEGER PRIMARY KEY, verse_id INTEGER, kind TEXT, body TEXT)  -- footnote / xref
CREATE VIRTUAL TABLE verse_fts USING fts5(text, content='verse', content_rowid='id');
```
- Rendering a chapter is one query on `block` by `chapter_key`: an ordered list of styled runs.
- Search is FTS5 on `verse_fts`, run across the attached DBs.

**User DB:** `user.sqlite` in `~/Library/Application Support/<bundle id>/`. Read-write, WAL.
```sql
highlight(id, start_verse, end_verse, color, created_at)
bookmark(id, verse_id, label, created_at)
note(id, start_verse, end_verse, body_md, created_at, updated_at)
history(id, translation, verse_id, opened_at)
settings(key, value)
plan_progress(plan_id, day, done_at)          -- later
```
- Keyed on canonical verse ids, so annotations survive version switches.
- Use UUIDs + `updated_at` from day one, so sync can be added later without migration pain.

**Reference parser** (`bible-core`): alias table, then tokens, then `Reference { book, chapter, verse_start, verse_end }`. Pure function, heavy unit + property tests.

## 4. App structure (GPUI side)
- `ReaderWindow` (root view): gpui-kit dock / resizable layout.
  - `Sidebar`: navigator, bookmarks, history
  - `ReaderPane`: one per version in parallel mode
    - Chapter content rendered from blocks into a `list` (variable height, virtualized). Paragraph = one element with styled runs.
  - `Inspector`: notes, footnotes, compare
  - `CommandPalette`: modal input. Parser first, FTS fallback.
- **State:** `AppState` entity holds open DB handles, current position, settings. Views subscribe.
- **Actions:** `NextChapter`, `PrevChapter`, `OpenPalette`, `ToggleSidebar`, `ToggleInspector`, `Bookmark`, `Highlight(color)`, `IncreaseFont`...
- **Menu bar:** see 02. Starts as `gpui-tray` or `objc2` NSStatusItem in the same process.

### Typography risks: the spike must prove these
1. Mixed runs in one paragraph: serif body, smaller raised verse numbers, red-letter color, footnote markers. GPUI has styled runs. **True superscript / baseline shift is unverified.** Fallback: smaller font + top alignment, or inline element.
2. Hover / click on a specific run (verse number, footnote marker). `InteractiveText` looks like the fit.
3. **Text selection + copy of arbitrary text.** GPUI non-editor text likely has no free drag selection. Plan: verse-level selection (click / shift-click) as the primary model, which matches every Bible app anyway. Check whether gpui-kit offers selectable text.
4. Smooth scroll across long chapters (Psalm 119 = 176 verses) and chapter-to-chapter continuous scroll.
5. Parallel panes with verse-aligned scroll.

## 5. Setup and run

**Prereqs (macOS)**
```sh
xcode-select --install          # CLT. Full Xcode may be needed for Metal toolchain (verify)
curl https://sh.rustup.rs -sSf | sh
rustup show                      # picks up rust-toolchain.toml
```

**Cargo** (`crates/app/Cargo.toml`)
```toml
[dependencies]
gpui-kit = "=0.7.1"
bible-core = { path = "../bible-core" }
bible-store = { path = "../bible-store" }

[dev-dependencies]
gpui-kit = { version = "=0.7.1", features = ["test-support"] }
```
`bible-store`: `rusqlite = { version = "*", features = ["bundled"] }`. Bundled SQLite includes FTS5. Use `bundled-sqlcipher` when licensed texts arrive.

**Hello world** (from gpui-kit README, uncompiled)
```rust
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;

pub struct HelloWorld;
impl Render for HelloWorld {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().v_flex().gap_2().size_full().items_center().justify_center()
            .child("Hello, World!")
            .child(Button::new("ok").primary().label("Let's Go!")
                .on_click(|_, _, _| println!("Clicked!")))
    }
}

fn main() {
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| HelloWorld))
            .expect("Failed to open window");
    });
}
```

**Commands**
```sh
cargo run -p bible-import -- data/sources/bsb --out data/build/bsb.sqlite   # build a translation
cargo run -p app                    # run app (first build takes minutes)
cargo test --workspace              # all tests
cargo build --release -p app        # release binary (not a .app bundle)
```
Speed-ups: `sccache`. `cargo build --timings` to find slow crates.

## 6. Testing
- **bible-core:** plain `#[test]`. Reference parser tables, ids, versification edge cases (missing verses, merged ranges). `proptest` for parser round-trips.
- **bible-import:** golden / snapshot tests (`insta`). Small USFM fixtures (heading, poetry, `\wj`, footnote), then expected block rows. Also full-book sanity counts (verses per chapter matches canon table).
- **bible-store:** integration tests on temp DBs. Search results, highlight CRUD, migrations.
- **app (GPUI):** with `test-support`, `#[gpui_kit::test]` (or `#[gpui::test]`) using `TestAppContext`.
  - `cx.add_window_view(...)`, `simulate_keystrokes("cmd-]")`, assert reader moved to next chapter. `run_until_parked()` to flush tasks.
  - gpui-kit `TestWindowExt` finds elements by `ElementId`. Call `window.render_frame(cx)` first.
  - Pixel / screenshot tests: offscreen Metal renderer, macOS only. Use sparingly (typography regressions).
- **CI:** GitHub Actions `macos-latest`. `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace --locked`. Cache target dir.
- **Manual QA list:** long chapters, poetry books, red-letter Gospels, omitted verses in parallel view, dark mode, font scaling.

## 7. Packaging (later)
Decided: direct free download, no App Store. Personal builds: ad-hoc sign. Sharing: Developer ID + notarize.

- `cargo build --release` gives only a binary. gpui-kit docs describe a manual `.app`: `Info.plist` + binary in `Contents/MacOS` + resources (translation DBs) in `Contents/Resources`.
- Sign with Developer ID (`codesign --options runtime`, nested first). Notarize with `xcrun notarytool`. DMG via `hdiutil`.
- `cargo-packager` or `cargo-bundle` could automate. Unverified with GPUI.
- Updates: Sparkle (like CodexBar) for direct distribution. Mac App Store needs sandboxing, which affects the menu bar helper + App Group design.
- Notifications only work from a bundled `.app`.

## 8. Risks
| Risk | Mitigation |
|---|---|
| GPUI / gpui-kit API churn (weekly releases, pre-1.0) | Exact pins, upgrade on purpose, thin `app` crate |
| `gpui-pre` single-maintainer snapshot | Accept for now. Escape hatch: Zed git rev |
| Typography gaps (superscript, selection) | Spike first, before committing |
| Sparse upstream docs | Read Zed + gpui-kit source / examples. Apps to study: Zed, Vellum / Zorite (markdown editors), Hummingbird, Loungy |
| Menu bar not native in GPUI | gpui-tray / objc2, Swift helper fallback |
| Licensing delays | Build on public domain texts. Licensed ones drop in through the same importer |

## Sources
- crates.io/crates/gpui, crates.io/crates/gpui-kit, crates.io/crates/gpui-component, crates.io/crates/gpui-pre
- github.com/longbridge/gpui-kit, gpui-kit.com/docs (getting-started, installation, test, packaging)
- github.com/zed-industries/zed/tree/main/crates/gpui, github.com/zed-industries/awesome-gpui
- github.com/domenkozar/gpui-tray, zed PR #44047

## 9. Spike findings (2026-10-06)
The code now exists, so this section replaces the guesses above where they differ.

- **Versions:** `gpui-kit = "=0.7.1"` resolves to `gpui-pre 0.3.8`. `gpui-tray 0.1.4` with features `gpui-kit, menu-state` builds against the same gpui, so there's one copy and no type clashes.
- **No full Xcode needed:** gpui-kit turns on gpui's `runtime_shaders`, so Command Line Tools are enough.
- **Toolchain:** Rust 1.99, edition 2024.
- **Data model, simplified:** each chapter is stored as one JSON blob (`Vec<Block>`, `bible-core::content`) instead of per-block rows. That's one query per chapter, and import of all three Bibles takes about 15 s. FTS5 search over 31k verses responds instantly.
- **Import results:**
  - BSB: 31,086 verses, 4,854 notes.
  - KJV: 31,102 verses, 6,959 notes. KJV `¶` pilcrows become paragraph breaks.
  - WEB: 31,098 verses, 1,470 notes. Deuterocanon is skipped.
- **Typography answers:**
  - GPUI text runs can't change font size, so there's no inline size change for superscripts. Verse numbers are ASCII digits with the OpenType **`sups`** feature set on their run. Iowan Old Style supports it, and the result looks like real superscripts.
  - Unicode superscript digits (¹⁷) were tried first and rejected. GPUI's line wrapper treats them as non-word characters, so it broke lines in the middle of "17". ASCII digits avoid that.
  - Body text uses explicit `TextRun`s (`chapter_view::build_runs`), so it does not inherit font from the parent div. The base `TextStyle` is passed in.
  - Footnote markers are Unicode superscript letters, clickable via `InteractiveText::on_click`. They open the notes panel.
  - Red letter, italic added words (KJV only), poetry indents, headings, psalm titles and parallel-passage refs all render.
- **Free text selection:** not built. gpui-base has `SelectableText` with a `TextSelectionLayer`, but it takes plain text (no runs). Revisit when verse selection comes back.
- **Menu bar:** `gpui-tray` works in-process. The menu is rebuilt with `refresh_menu` on navigation, and `QuitMode::Explicit` keeps the app alive with no windows. There is no template-image API, so the icon is a fixed mid-gray.
- **Headless UI tests (added in Phase 1):** `gpui-kit`'s `test-support` gives `#[gpui_kit::test]` plus `TestWindowExt` (`click`, `input`, `press` by element id) on gpui's test platform. These tests check behavior, not pixels. See [AGENTS.md](../AGENTS.md#2-headless-ui-tests-behavior-no-pixels).
- **Icons need explicit assets:** `gpui_kit::application()` registers no `AssetSource`. Call `.with_assets(gpui_kit::assets::Assets)`, or icons render blank.
- **Visual testing:** the `screenshot` cargo feature uses `Window::render_to_image()` from `test-support` to save a PNG without screen-recording permission. Scripted UI actions must be applied before the first frame. A window that isn't key doesn't reliably repaint before capture.
