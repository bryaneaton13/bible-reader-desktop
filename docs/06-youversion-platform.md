# 06: YouVersion Platform: plan and recommendations

Status: research and first implementation, 2026-10-11 (branch `youversion-sources`). See [Implementation](#8-implementation-2026-10-11) at the end. Goal: get licensed translations (NIV and friends) into the app without negotiating with each publisher, while keeping the offline-first feel.

## 1. What it is

YouVersion Platform is a free developer program (Life.Church) that gives apps **licensed, official access to 1,000+ Bible versions** through a REST API, plus SDKs and "Sign in with YouVersion". You register an app on platform.youversion.com, get an **App Key**, and optionally accept per-publisher **Express Bible Licenses** (e.g. Biblica for NIV / NIrV). Your Platform Account is already created (see the portal screenshot: Get Started, Apps, Analytics, Licensing, Profiles, Notifications).

Sources: [overview](https://developers.youversion.com/overview), [Bible API](https://developers.youversion.com/api/bibles), [API usage](https://developers.youversion.com/api-usage), [error codes](https://developers.youversion.com/error-codes), [SDK intro](https://developers.youversion.com/sdks/introduction).

### Verified from official docs
| Topic | Fact |
|---|---|
| Base URL | `https://api.youversion.com` (prod), `https://api-dev.youversion.com` (dev) |
| Auth | Header `X-YVP-App-Key: <key>` on every request. The key is an identifier, not a secret (JS SDK says it can ship in client code). |
| List Bibles | `GET /v1/bibles?language_ranges[]=en` (licensed to your app). `all_available=true` returns the full catalog. Paged (`page_size` 1 to 99, `page_token`). |
| Structure | `GET /v1/bibles/{id}/index` gives books, chapters, verses and `text_direction`. Also `/books`, `/chapters`, `/verses`. |
| Text | `GET /v1/bibles/{id}/passages/{USFM}` e.g. `JHN.3.16` or a chapter `JHN.3`. Returns `id`, `content`, `reference`. |
| Format | `format=text` (default) or `format=html`. `include_headings` and `include_notes` only apply to `html`. |
| Bible metadata | `id` (BSB is 3034), `abbreviation`, `title`, `localized_title`, `language_tag`, `copyright`, `organization_id`, `youversion_deep_link`. |
| Licensing gate | Metadata returns 200 for unlicensed Bibles. **Passages return 403** until your org has accepted that publisher's license. |
| Rate limit | 429 with plain-text body and `Retry-After`. Docs advise caching and batching. Conditional requests supported (`If-None-Match` / `If-Modified-Since`, 304). |
| Extras | Verse of the Day (year calendar and per day), Search (verses, topics, suggested queries), Languages, Organizations, Licenses, Fonts (stylesheets), Highlights (needs user sign-in), Permissions. |
| SDKs | Swift (iOS/iPadOS/macOS), Kotlin, JavaScript, React, React Native. **No Rust SDK**; docs say call the HTTP API directly. |
| Display | HTML passages need Bible CSS, the Fonts API stylesheet and the **required version attribution** ("Display Bible HTML" guide). |

### Reported by third parties, not confirmed (verify in the portal before relying on them)
- Default limit is about **200 requests/hour** until you "Make Live" (step 5 in the portal), which raises it. ([GitHub issue](https://github.com/asvirts/aenon/issues/3))
- NIV / NIrV come through license 1 (Biblica). ESV is **not** on the list. ([same issue](https://github.com/asvirts/aenon/issues/3))
- Free for approved apps; commercial terms unclear. ([review](https://learnofchrist.com/resources/youversion-platform-services))
- Per-version rules cover attribution, copy limits, **caching, territories and revocation**. The platform grants **no permanent offline rights**, and publishes no machine-readable compliance rules. ([review](https://learnofchrist.com/resources/youversion-platform-services))

### Not documented anywhere I could reach
Exact cache TTLs / offline storage allowed per version, copy/quote limits per version, the real quota numbers, the "Sign in" OAuth flow for a **native desktop** app (redirect URI, PKCE), and which specific translations your key can read (NLT, CSB, NASB, NKJV, MSG are unconfirmed). The sign-in and display-HTML pages returned empty to my fetcher.

## 2. The core tension: offline-first vs. YouVersion

The app today is offline-first: one read-only SQLite per translation, FTS5 search, topics, cross-references (`bible-store`, `Library::load`). YouVersion content is **online API content with restricted caching**. So:

- Treat YouVersion translations as **online translations with a bounded, expiring cache**, not as importable data. Do not bulk-download whole Bibles into `data/build/*.sqlite`. That likely violates the license and risks the app key.
- Public-domain BSB / KJV / WEB stay the offline core. YouVersion adds "more translations when online".
- Consequences: no local FTS5 or topic search over NIV text, no ranking blend. Search for those translations goes through YouVersion's Search API or runs on BSB and then shows the NIV text for the hits.

## 3. Recommended feature set (ordered by value / risk)

### P1: Remote translations in the reader (the main win)
- New "YouVersion" source: list licensed Bibles for the user's languages, add any to the translation picker (⌘1–4, Select, split panes, tabs all keep working).
- Fetch chapter as `format=html` with headings and notes, convert to the existing `Block` / `Span` model (verse numbers, poetry, red letter, footnote anchors). Same renderer, so typography stays consistent.
- Cache chapters on disk with TTL and `If-None-Match` revalidation; keep a tiny LRU in memory. Prefetch next / previous chapter only.
- Show a required copyright line from `copyright` in the chapter footer, About window, and appended on ⇧⌘C / ⌥⌘C copy.
- Offline behavior: cached chapters open; uncached show a calm "needs connection" state with fallback to BSB for the same passage.

### P2: Settings, onboarding and compliance
- Settings page "Bible sources": enable YouVersion, pick languages, choose which Bibles appear, clear cache, show per-Bible copyright and license status (403 means "license not accepted in your portal").
- App key lives in a bundled config (identifier, not secret) with an env override for dev.
- Graceful 403 / 429 / network handling with `Retry-After`, a request budget and a visible "rate limited" notice.

### P3: Verse of the Day for the menu bar
- Phase 4 menu bar currently uses a curated local list. Swap in the VOTD endpoint (cache the whole year calendar once, then it works offline). Cheap, low risk, good first integration to prove the plumbing.

### P4: Search across remote versions
- YouVersion Search API for verses and topics in a given Bible. Show as a "YouVersion" section on the search page when the active translation is remote. Keep local ranked search for offline translations.
- Cheaper alternative: run the local search on BSB and render the same verse IDs in the remote translation (verse IDs are canonical `BBCCCVVV`; map to USFM `JHN.3.16`).

### P5 (later): Sign in with YouVersion
- Gives access to the user's highlights (and later bookmarks, notes, plans if exposed). Only worth it once highlights/notes exist locally (roadmap Phase 2). Needs a desktop OAuth flow (loopback or custom URL scheme, PKCE) that I could not verify. Adds privacy and data-deletion obligations. **Defer.**

### Not recommended
- Bulk mirroring or exporting YouVersion text into SQLite. Not allowed in spirit, and it breaks if a publisher revokes a license.
- Embedding the Swift SDK. A Rust app with a plain HTTP client is simpler than bridging Swift.
- Building the ESV / NLT path on YouVersion before confirming they are offered. Keep the publisher outreach in the roadmap for those.

## 4. Architecture sketch

```
bible-core    unchanged (USFM id helpers: VerseId <-> "JHN.3.16")
bible-store   add `trait ChapterSource { chapter(ChapterRef) -> Option<Vec<Block>>; info() }`
              Translation (SQLite) implements it
bible-youversion (new crate, no GPUI)
              client.rs   reqwest/ureq + X-YVP-App-Key, retry/backoff, 304 handling
              html.rs     passage HTML -> Vec<Block>, notes -> Note   (unit tests with saved fixtures)
              cache.rs    disk cache (chapter blobs + ETag + fetched_at), TTL, size cap, purge
              catalog.rs  /v1/bibles + /index (book/chapter validity), language filter
app           Library holds Vec<Box<dyn ChapterSource>>; remote ones load async and
              render a loading state; reader/tabs/split already key on translation abbrev
```

- Domain crates stay GPUI-free; the new crate is plain Rust. Needs an HTTP client and async. The app has no HTTP dependency today, so pick one (`ureq` on a background thread keeps it simple; GPUI has its own executor).
- `Translation::open` is synchronous and `Library::load` scans `*.sqlite`; remote Bibles come from a catalog cache instead, so they do not collide with that scan.
- Search, topics, cross-references and notes panel only depend on `Translation` today. Audit each call site (`library.search`, `passage_text`, notes) and degrade cleanly for remote sources.
- Tests: fixture-based HTML parser tests, a fake `ChapterSource` for headless UI tests, and `shot.sh` steps such as `version:niv` with a canned fixture source so screenshots need no network or key.

## 5. Open questions to settle first (in this order)

1. **License terms for caching/offline.** Read the Terms of Use and the per-Bible license for NIV: how long can chapters be cached, is any offline pack allowed, copy limits? Ask support (platform.youversion.com/support) if the portal does not say. This decides how good the offline story can be.
2. **Which Bibles does your key actually get?** In the portal: Licensing → accept Biblica; then call `GET /v1/bibles?language_ranges[]=en` and record the list. Check for NLT, CSB, NASB, NKJV, NET, MSG. Write the answer here.
3. **Rate limit.** Check the quota on your app page; press **Make Live** (step 5) once the app is presentable, since it raises limits and this is a personal / free app.
4. **Distribution.** The plan says personal use first, maybe shared later. Confirm YouVersion terms allow a free downloadable desktop app and whether a key can be shared across installs (it is an identifier, so likely yes).
5. **HTML shape.** Fetch one `format=html` chapter with headings and notes and save it as a test fixture before designing `html.rs`.

## 6. Milestones

| # | Work | Size | Exit check |
|---|---|---|---|
| 0 | Portal: register app, accept Biblica license, record available Bibles, save HTML fixture, read terms | 0.5 day | Questions 1 to 3 and 5 answered above |
| 1 | `bible-youversion` crate: client, 403/429/304, catalog; CLI example that prints a chapter | 1 to 2 days | `cargo run -p bible-youversion --example chapter -- 111 JHN.3` works |
| 2 | HTML to `Block` converter + tests | 2 days | John 3 and Psalm 23 render like BSB (poetry, headings, red letter, notes) |
| 3 | `ChapterSource` abstraction, remote translation in picker, tabs, split | 2 to 3 days | Open NIV beside BSB; restart restores tabs |
| 4 | Disk cache + offline/error states + attribution + copy citation | 2 days | Airplane mode opens cached chapters; copy appends copyright |
| 5 | Settings "Bible sources" page, VOTD in menu bar | 1 to 2 days | Menu bar shows VOTD offline after first sync |
| 6 | Remote search section | 1 to 2 days | Search page shows YouVersion matches for the active remote Bible |
| later | Sign in + highlights | open | After local highlights exist |

Add to the roadmap under Phase 5 ("Package + licensed texts") once questions 1 and 2 are answered; it replaces the "Import NIV / ESV / NLT when files arrive" line for any version YouVersion carries.

## 7. Risks

- **Offline promise weakens** for licensed versions. Mitigate: BSB / KJV / WEB stay fully offline; clear UI for "online" Bibles.
- **Platform dependency:** keys, terms, endpoints and licenses can change or be revoked. Mitigate: isolate in one crate behind `ChapterSource`; never persist licensed text beyond the cache policy.
- **Compliance drift:** per-version copy limits and attribution are on you. Mitigate: one place (`copyright` string + copy path) enforces them.
- **Rate limits** at 200/hour (if accurate) are tight. Mitigate: chapter-level fetches only, ETag revalidation, small prefetch, then Make Live.
- **Unverified assumptions** (section 1, last two lists) could change the plan; resolve them in milestone 0 before writing code.

## 8. Implementation (2026-10-11)

Decisions from the owner: cache every request for **30 days**; never commit anything fetched from the API (the cache lives in `~/Library/Application Support/BibleReader/cache/youversion/`, tests and screenshots use synthetic public-domain wording); a good loading state; **Sign in with YouVersion is optional**; **no Verse of the Day** from YouVersion; **remote search is opt-in**, local search by default, with the request cost shown.

### What was built
- `crates/bible-youversion` (no GPUI): `client` (ureq, `X-YVP-App-Key`, bearer token when signed in, 401/403 -> `NotPermitted`, 429 -> `RateLimited{retry_after}`), `cache` (30-day TTL, expired entries never served, purged at launch, stats, clear), `html` (passage HTML -> `Block`/`Note`, tolerant parser), `source` (catalog, chapter, verse text, verse search, all cached), `auth` (PKCE; Keychain session).
- App: `remote.rs` (global state + background `request`), `sources_page.rs` (Settings ▸ Bible sources), YouVersion Bibles in the translation dropdowns, palette, tabs and split view (key `yv:<id>`), loading skeleton and error card with retry in every pane, attribution footer, copy with copyright, and the search page's "Search with YouVersion" card (1 request to search + up to 6 for verse text; cached results cost nothing).
- Tests: 24 in the crate, 5 headless UI tests (cache hit makes no request, loading then an offline error, removing a Bible falls back, copy attribution, opt-in search). Screenshots: `yvdemo`, `yvloading`, `yverror`, `yvsearch`, `yvstate:*`, `SHOT_WINDOW=sources`.

### Sign-in flow (from YouVersion's Swift SDK, not the docs site)
Authorize at `https://api.youversion.com/auth/authorize` (`response_type=code`, `client_id=<app key>`, `redirect_uri=youversionauth://callback`, `state`, `nonce`, S256 `code_challenge`, `scope=openid profile`). The redirect carries `state`; the client then GETs `/auth/callback` with the same query (no redirect following) and reads `code` from the 302 `Location`, then POSTs `/auth/token` (`grant_type=authorization_code`, `code_verifier`) and reads the ID token claims. Refresh uses `grant_type=refresh_token`. The custom URL scheme is registered in `Info.plist` by `scripts/bundle.sh`, so sign-in only works from the bundled app.

### Still unverified (needs a real key; do these first)
1. **HTML shape.** The parser follows the class names YouVersion's Swift SDK renders (`yv-v`, `yv-vlbl`, `yv-n f|x`, USFM block classes like `p`, `q1`, `s1`, `d`, `b`). Fetch one real chapter and compare; `html::tests` hold the fixtures to extend.
2. **Does `youversionauth://callback` work for your app key** (it is the SDK's default, but redirect URIs may need registering in the portal)? And does signing in actually add Bibles? The Settings page reports "Signing in added N Bibles" by comparing the catalog before and after, so this answers itself.
3. **Search response and caching terms**: `/v1/search-verses` parameters follow the Swift SDK; cache rules per Bible are still undocumented, so 30 days is our own choice. Confirm against the Terms of Use.
4. **Rate limit** (reported as 200 requests/hour until the app is live). Settings shows "N API requests this session".
