# 03: Bible content: versions, licensing, formats

## 1. Versions and who to ask

| Version | Rights holder | Offline full text? | Notes |
|---|---|---|---|
| NIV (2011) | Biblica (print via Zondervan / HarperCollins) | Negotiated license | Quote limit 500 verses. Partner APIs (API.Bible, YouVersion) are online only. |
| NLT | Tyndale House | Negotiated license | Permissions questionnaire has an "app" option. api.nlt.to is online, non-commercial. |
| ESV | Crossway | Negotiated license | ESV API terms cap **local storage at 500 verses / half a book**. Full offline needs separate deal. Strict about edits. |
| KJV | Public domain (US). UK Crown patent, not enforced on software in practice | Yes | Use a clean source. Some files carry copyrighted add-ons (Strong's markup, notes). |
| BSB | Public domain since 2023-04-30 | Yes | Modern, readable. Best free default. github.com/BSB-publishing/bsb2usfm |
| WEB | Public domain | Yes | eBible.org USFM. Rename it if you modify the text (trademark). |
| CSB / NASB / NKJV | Holman-Lifeway / Lockman / Thomas Nelson | Negotiated | Candidates for the "1 to 2 others". |
| NET | Biblical Studies Press | Generous free terms (verify) | Notes are the big draw. |

Each licensed version requires its exact copyright notice in the app (About screen + appended on copy).

**Recommendation:** develop and ship with BSB + KJV + WEB. Start NIV / ESV / NLT outreach now. Ask each publisher: (a) what file format they supply, (b) storage / protection requirements, (c) copy-length limits, (d) fees.

## 2. Formats

| Format | Headings | Footnotes | Cross-refs | Red letter | Poetry | Paragraphs |
|---|---|---|---|---|---|---|
| USFM | yes | yes | yes | yes (`\wj`) | yes (`\q1`) | yes (`\p`) |
| USX (XML) | yes | yes | yes | yes | yes | yes |
| OSIS (XML) | yes | yes | yes | yes | yes | yes |
| Zefania XML | weak | weak | weak | mostly no | mostly no | mostly no |
| scrollmapper JSON/SQLite | no, plain verse text | no | no | no | no | no |

**Recommendation:** import USFM / USX (USJ is the JSON form of USX, also fine). Convert at build time into our own SQLite. Never parse USFM at runtime.

Avoid plain-verse community dumps for the reader. They lose headings, paragraphs and poetry, and those are what make it read like a book.

## 3. Internal model (sketch)
- **Verse id:** integer `BBCCCVVV` (book, chapter, verse) on a canonical English / KJV versification. Highlights and notes use this id, so they work across versions.
- **Per translation:** `verse` rows (id, plain text for search) + `block` rows (ordered runs: paragraph start, poetry level, heading, red-letter span, footnote anchor, verse marker).
- Footnotes and cross-refs in their own tables, keyed by verse + offset.
- FTS5 over plain verse text.
- Detail in [04-architecture-gpui.md](04-architecture-gpui.md).

## 4. Versification
- English Protestant translations are almost identical in numbering. Main gap: verses **omitted** in modern texts (Matt 17:21, 18:11, 23:14, Mark 7:16, 9:44, 9:46, Luke 17:36, John 5:4, Acts 8:37, 15:34, 24:7, 28:29, Rom 16:24). Number kept, text missing or in a footnote.
- Edge cases: 3 John 14 / 15 split, merged ranges ("vv. 1-2" in some versions, e.g. NLT combines verses sometimes).
- Model rules: verse may be absent. Verse may be a merged range. Parallel view aligns on canonical id and shows gaps.
- Hebrew numbering (Malachi 3 vs 4 chapters, Psalm titles as verse 1) only matters if non-English-tradition texts are added later. Then add a mapping table (SWORD / Paratext versification schemes are the reference).

## 5. Protection / DRM expectations
Industry pattern, not confirmed with each publisher:
- Licensed text not stored as plain readable files. Usually an encrypted DB (e.g. SQLCipher) with key in app.
- Copy / share caps per action, notice appended.
- No full-text export. Delete on license end.

Plan: public domain texts stored plain. Licensed texts stored as encrypted SQLite, one file per translation. Treat it as good faith, not real security. Tell publishers the approach up front, since it can gate the deal.

## 6. Free supplementary data
- Cross-references: openbible.info (~340k refs with votes, CC-BY).
- Book names / abbreviations: USFM 3-letter codes + our own alias table for the reference parser (Gen, Gn, 1 Sam, 1st Samuel, Ps / Psalm / Psalms, Song / SoS / Song of Solomon...).
- Strong's: KJV+Strong's (SWORD), BSB Strong's variant; OpenScriptures lexicons. Later.
- Reading plans: generate our own (M'Cheyne, canonical, chronological, 90 day). Don't copy branded plans.

## Sources
- biblica.com/permissions, tyndale.com/permissions, crossway.org/permissions, api.esv.org, api.nlt.to
- github.com/BSB-publishing/bsb2usfm, berean.bible/licensing.htm
- ebible.org, crosswire.org/sword/modules, github.com/usfm-bible, github.com/scrollmapper/bible_databases
- openbible.info/labs/cross-references
