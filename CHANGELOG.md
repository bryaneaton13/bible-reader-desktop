# Changelog

## 0.2.0 (2026-10-12)

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

### Fixed
- Alternate verse numbers no longer leak into text for some Bibles.
- Transient YouVersion 5xx errors are retried twice.

### Notes
- NIV use is governed by Biblica's license and YouVersion's terms through your own platform account. The 30-day plain-text cache and whole-chapter copy follow the choices recorded in `docs/06-youversion-platform.md`; revisit them before sharing the app widely.
- Rebuild data after pulling if you build from source: `cargo run --release -p bible-import`.

## 0.1.0

First release: BSB, KJV and WEB offline, topical search, cross-references, tabs, split view, themes and font pairings.
