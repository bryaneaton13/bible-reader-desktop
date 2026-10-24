# Changelog

All notable changes to Bible Reader are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/) (`0.MINOR.PATCH` until 1.0).

## [Unreleased]

### Added
- A simple website focused on native performance, with light-first app previews, Mac downloads, GitHub and build details, and optional YouVersion setup information.
- Privacy-friendly Vercel Web Analytics on the website, with no analytics added to the Mac app.

## [0.3.0] - 2026-10-23

### Added
- **Downloadable Mac app.** An Apple Silicon ZIP is available on GitHub Releases, with the offline Bibles included. No Rust or command-line setup is needed. This build is ad-hoc signed, not Apple-notarized; first launch requires approval in macOS Privacy & Security settings.
- **Highlight verses.** Click a verse number to select it, shift-click to select a range, or ⌥-click a verse's text. Then choose Highlight in the bar that appears, or press ⌘D. Select as many verses in a chapter as you like. Highlights use one color that suits each theme, and show in every translation.
- **Notes and tags on highlights.** Add a note and comma-separated tags to any highlight. The tag field suggests tags you've already used. The notes panel lists what you've saved in the current chapter.
- **Saved verses tab (⇧⌘D).** Every highlight in one list, with its text, note and tags. Search it (including by passage, like "rom 8"), filter by tag, testament or "with notes", sort it, or drag (or ⌥↑ / ⌥↓) items into your own order. Open any item in a new tab with one click.
- **Export saved verses as Markdown (⇧⌘E).** Exports the list as you're viewing it, filters and order included, to a `.md` file or the clipboard, with each version's copyright notice.

### Changed
- The highlight for the verse you jumped to is now a neutral gray, so it isn't mistaken for a saved highlight.
- ⇧⌘C copies the selected verses when there are any.
- Saved verses keep a copy of their text in the translation they were saved from, including YouVersion Bibles such as the NIV. They're stored in `user.db` next to your settings.

## [0.2.0] - 2026-10-12

Bible Reader can now read the NIV and other licensed translations from YouVersion, using your own app key. The bundled translations and the offline experience are unchanged.

### Added
- **YouVersion Platform Bibles (bring your own key).** Paste an app key in Settings ▸ Bible sources and press Connect, then add Bibles such as the NIV, NIrV and NIV (Anglicised), or public domain Bibles in other languages. Setup steps are in the README.
- **Remote Bibles everywhere.** YouVersion Bibles work in the translation pickers, ⌘K palette, tabs and split view. Headings, poetry, red letter, footnotes and cross-references come through, and the publisher's copyright notice and link show with the text and on copy.
- **Loading and error states.** Chapters show a skeleton while loading, and a card with Retry when offline, rate limited or not licensed.
- **Optional Sign in with YouVersion** (installed app only), which can add Bibles for your account.
- **Opt-in YouVersion search.** A card on the search page searches with YouVersion and shows its request cost first. Local search stays the default.
- **Settings ▸ Bible sources.** Language, a Bible finder, a switch for Bibles that need a license, per-session request count, and cache size with a clear button.
- **Version shown in tabs**, so two translations of one chapter are easy to tell apart.
- App icon, bundled font credits and clickable credits in Settings ▸ About.
- Keyboard Shortcuts window (⌘/).

### Changed
- **Fewer API requests.** Chapters are cached for 30 days, duplicate requests are shared, and a fetch is dropped if you move on within 200 ms. Verse search results reuse whole cached chapters. The API serves one chapter per request, so there is no batching or prefetching.
- Search page uses virtualized lists and line spacing is adjustable.
- The app is no longer strictly offline: it makes network calls only if you add a YouVersion key.
- NIV use is governed by Biblica's license and YouVersion's terms through your own platform account. The 30-day plain-text cache and whole-chapter copy follow the choices recorded in `docs/06-youversion-platform.md`; revisit them before sharing the app widely.
- Rebuild data after pulling if you build from source: `cargo run --release -p bible-import`.

### Fixed
- Alternate verse numbers no longer leak into text for some Bibles.
- Transient YouVersion 5xx errors are retried twice.

## [0.1.0]

First release: BSB, KJV and WEB offline, topical search, cross-references, tabs, split view, themes and font pairings.

[Unreleased]: https://github.com/bryaneaton13/bible-reader-desktop/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/bryaneaton13/bible-reader-desktop/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/bryaneaton13/bible-reader-desktop/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/bryaneaton13/bible-reader-desktop/releases/tag/v0.1.0
