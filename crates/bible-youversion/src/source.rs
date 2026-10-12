//! The cached API surface the app uses: the Bible catalog, chapters,
//! single verses and verse search. Every GET goes through the disk cache.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use bible_core::{ChapterRef, VerseId, book_by_code};
use serde::{Deserialize, Serialize};

use crate::cache::Cache;
use crate::client::{Client, Error};
use crate::html::{Chapter, parse_chapter, parse_chapters, verse_texts};

/// A Bible in YouVersion's catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BibleInfo {
    pub id: u32,
    #[serde(default)]
    pub abbreviation: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub localized_title: Option<String>,
    #[serde(default)]
    pub language_tag: String,
    #[serde(default)]
    pub copyright: Option<String>,
}

impl BibleInfo {
    /// Short label for pickers and tabs. Falls back to the id.
    pub fn label(&self) -> String {
        if self.abbreviation.trim().is_empty() {
            format!("#{}", self.id)
        } else {
            self.abbreviation.trim().to_string()
        }
    }

    pub fn name(&self) -> &str {
        self.localized_title
            .as_deref()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or(&self.title)
    }
}

/// A value and whether it came from the disk cache (no API request used).
#[derive(Debug, Clone, PartialEq)]
pub struct Fetched<T> {
    pub value: T,
    pub from_cache: bool,
}

#[derive(Deserialize)]
struct BiblePage {
    #[serde(default)]
    data: Vec<BibleInfo>,
    #[serde(default)]
    next_page_token: Option<String>,
}

/// A cached chapter: the API's own response (`content` is HTML), or a
/// chapter we cut out of a multi-chapter response (`parsed`).
#[derive(Deserialize)]
struct PassageBody {
    #[serde(default)]
    content: String,
    #[serde(default)]
    parsed: Option<Chapter>,
}

#[derive(Serialize)]
struct ParsedBody<'a> {
    parsed: &'a Chapter,
}

impl PassageBody {
    fn into_chapter(self, chapter: ChapterRef) -> Chapter {
        match self.parsed {
            Some(parsed) => parsed,
            None => parse_chapter(&self.content, chapter),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One page of verse search results.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchPage {
    pub verses: Vec<VerseId>,
    pub did_you_mean: Vec<String>,
    pub search_instead_for: Option<String>,
    pub next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct SearchBody {
    #[serde(default)]
    verses: Vec<SearchVerse>,
    #[serde(default)]
    did_you_mean: Vec<String>,
    #[serde(default)]
    search_instead_for: Option<String>,
    #[serde(default)]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct SearchVerse {
    reference: String,
}

/// "JHN.3.16" -> John 3:16. Ranges ("JHN.3.16-18") start at the first verse.
pub fn parse_usfm_verse(reference: &str) -> Option<VerseId> {
    let mut parts = reference.trim().split('.');
    let book = book_by_code(parts.next()?)?;
    let chapter = parts.next()?.parse().ok()?;
    let verse = parts.next()?.split(['-', '+', ',']).next()?.parse().ok()?;
    Some(VerseId::new(book.num, chapter, verse))
}

pub fn usfm_chapter(chapter: ChapterRef) -> String {
    format!("{}.{}", chapter.book_info().code, chapter.chapter)
}

pub fn usfm_verse(verse: VerseId) -> String {
    format!(
        "{}.{}.{}",
        book_code(verse.book()),
        verse.chapter(),
        verse.verse()
    )
}

fn book_code(book: u8) -> &'static str {
    bible_core::book_by_num(book).map_or("GEN", |b| b.code)
}

/// A client plus the disk cache.
#[derive(Clone)]
pub struct Source {
    client: Client,
    cache: Cache,
    /// Chapters being fetched right now, so two callers asking for the same
    /// one (both panes, a skim past it, search) share a single request.
    inflight: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl Source {
    pub fn new(client: Client, cache: Cache) -> Self {
        Self {
            client,
            cache,
            inflight: Arc::default(),
        }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    /// Cached body, or fetch and cache it.
    fn cached(
        &self,
        key: &str,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<(String, bool), Error> {
        if let Some(body) = self.cache.get(key) {
            return Ok((body, true));
        }
        let body = self.client.get(path, query)?;
        // A full disk shouldn't fail the read.
        let _ = self.cache.put(key, &body);
        Ok((body, false))
    }

    fn parse<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, Error> {
        serde_json::from_str(body).map_err(|e| Error::Invalid(e.to_string()))
    }

    /// Bibles in `languages` ("en", "es", or "*" for all). With
    /// `all_available` the list includes Bibles this app isn't licensed for.
    /// `audience` ("anon" / "user") keeps a signed-out list from being served
    /// after sign in.
    pub fn bibles(
        &self,
        languages: &[String],
        all_available: bool,
        audience: &str,
    ) -> Result<Fetched<Vec<BibleInfo>>, Error> {
        let mut langs: Vec<String> = languages.to_vec();
        langs.sort();
        let mut all = Vec::new();
        let mut token: Option<String> = None;
        let mut from_cache = true;
        // The catalog is bounded; cap the loop anyway.
        for _ in 0..40 {
            let mut query: Vec<(&str, String)> = langs
                .iter()
                .map(|l| ("language_ranges[]", l.clone()))
                .collect();
            query.push(("page_size", "99".into()));
            if all_available {
                query.push(("all_available", "true".into()));
            }
            if let Some(t) = &token {
                query.push(("page_token", t.clone()));
            }
            // Who is asking is part of the key: signing in can change the list.
            let key = format!(
                "bibles:{audience}:{}:{all_available}:{}",
                langs.join(","),
                token.as_deref().unwrap_or("")
            );
            let (body, cached) = self.cached(&key, "/v1/bibles", &query)?;
            from_cache &= cached;
            let page: BiblePage = Self::parse(&body)?;
            all.extend(page.data);
            match page.next_page_token.filter(|t| !t.is_empty()) {
                Some(next) => token = Some(next),
                None => break,
            }
        }
        Ok(Fetched {
            value: all,
            from_cache,
        })
    }

    fn chapter_key(bible: u32, chapter: ChapterRef) -> String {
        format!("passage:{bible}:{}:html", usfm_chapter(chapter))
    }

    /// The chapter from disk only: no network, so it's safe on the UI thread.
    pub fn chapter_cached(&self, bible: u32, chapter: ChapterRef) -> Option<Chapter> {
        let body = self.cache.get(&Self::chapter_key(bible, chapter))?;
        let passage: PassageBody = serde_json::from_str(&body).ok()?;
        Some(passage.into_chapter(chapter))
    }

    fn window_key(bible: u32) -> String {
        format!("cap:window:{bible}")
    }

    /// Most chapters one request has been seen to return for this Bible.
    fn window_cap(&self, bible: u32) -> u16 {
        self.cache
            .get(&Self::window_key(bible))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(crate::window::MAX_CHAPTERS)
    }

    fn set_window_cap(&self, bible: u32, cap: u16) {
        let _ = self.cache.put(&Self::window_key(bible), &cap.to_string());
    }

    /// One chapter, one request.
    pub fn chapter(&self, bible: u32, chapter: ChapterRef) -> Result<Fetched<Chapter>, Error> {
        self.chapter_in(bible, chapter, (chapter.chapter, chapter.chapter))
    }

    /// `chapter`, and on a cache miss also its neighbours `window.0..=window.1`
    /// in the same request when the API allows it. Neighbours go to the cache,
    /// so turning the page is free. If the API doesn't return a clean split,
    /// the window shrinks (for 30 days) and we fall back to one chapter.
    pub fn chapter_in(
        &self,
        bible: u32,
        chapter: ChapterRef,
        window: (u16, u16),
    ) -> Result<Fetched<Chapter>, Error> {
        let target = Self::chapter_key(bible, chapter);
        let mut window = window;
        loop {
            if let Some(cached) = self.chapter_cached(bible, chapter) {
                return Ok(Fetched {
                    value: cached,
                    from_cache: true,
                });
            }
            let keys: Vec<String> = (window.0..=window.1)
                .map(|c| Self::chapter_key(bible, ChapterRef::new(chapter.book, c)))
                .collect();
            // Someone else may be fetching this chapter: wait, then look again.
            let mutex = Arc::new(Mutex::new(()));
            let guard = lock(&mutex);
            let busy = {
                let mut registry = lock(&self.inflight);
                match registry.get(&target).cloned() {
                    Some(other) => Some(other),
                    None => {
                        for key in &keys {
                            registry.insert(key.clone(), mutex.clone());
                        }
                        None
                    }
                }
            };
            if let Some(other) = busy {
                drop(guard);
                drop(lock(&other));
                continue;
            }
            let result = self.fetch_window(bible, chapter, &mut window);
            {
                let mut registry = lock(&self.inflight);
                for key in &keys {
                    registry.remove(key);
                }
            }
            drop(guard);
            return result.map(|value| Fetched {
                value,
                from_cache: false,
            });
        }
    }

    fn fetch_window(
        &self,
        bible: u32,
        chapter: ChapterRef,
        window: &mut (u16, u16),
    ) -> Result<Chapter, Error> {
        // Respect what this Bible has been seen to allow, trimming the side
        // furthest from the chapter being read.
        let cap = self.window_cap(bible);
        while window.1 - window.0 + 1 > cap {
            if chapter.chapter - window.0 > window.1 - chapter.chapter {
                window.0 += 1;
            } else {
                window.1 -= 1;
            }
        }
        let count = usize::from(window.1 - window.0) + 1;
        if count > 1 {
            let range = format!("{}.{}-{}", chapter.book_info().code, window.0, window.1);
            let attempt = self.fetch_passage(bible, &range).and_then(|content| {
                parse_chapters(&content, chapter.book, window.0, count)
                    .ok_or_else(|| Error::Invalid("multi-chapter response".into()))
            });
            match attempt {
                Ok(chapters) => {
                    let mut wanted = None;
                    for (i, parsed) in chapters.into_iter().enumerate() {
                        let this = ChapterRef::new(chapter.book, window.0 + i as u16);
                        if let Ok(json) = serde_json::to_string(&ParsedBody { parsed: &parsed }) {
                            let _ = self.cache.put(&Self::chapter_key(bible, this), &json);
                        }
                        if this == chapter {
                            wanted = Some(parsed);
                        }
                    }
                    if let Some(parsed) = wanted {
                        return Ok(parsed);
                    }
                }
                // Real failures (offline, rate limit, license) won't be fixed
                // by asking for one chapter.
                Err(
                    err @ (Error::Offline(_)
                    | Error::RateLimited { .. }
                    | Error::NotPermitted
                    | Error::MissingKey),
                ) => return Err(err),
                // The API refused or mangled the range: try a smaller window
                // next time, and get this chapter on its own now.
                Err(_) => self.set_window_cap(bible, (count as u16 - 1).max(1)),
            }
        }
        let content = self.fetch_passage(bible, &crate::source::usfm_chapter(chapter))?;
        Ok(parse_chapter(&content, chapter))
    }

    /// One passage's HTML, straight from the API: no cache read or write.
    /// For `examples/probe.rs`, which measures what the API accepts.
    pub fn probe(&self, bible: u32, passage: &str) -> Result<String, Error> {
        let path = format!("/v1/bibles/{bible}/passages/{passage}");
        let query = [
            ("format", "html".to_string()),
            ("include_headings", "true".to_string()),
            ("include_notes", "true".to_string()),
        ];
        let body = self.client.get(&path, &query)?;
        Ok(Self::parse::<PassageBody>(&body)?.content)
    }

    /// One passage's HTML, cached under its own key when it is a whole chapter.
    fn fetch_passage(&self, bible: u32, passage: &str) -> Result<String, Error> {
        let path = format!("/v1/bibles/{bible}/passages/{passage}");
        let query = [
            ("format", "html".to_string()),
            ("include_headings", "true".to_string()),
            ("include_notes", "true".to_string()),
        ];
        let body = self.client.get(&path, &query)?;
        let parsed: PassageBody = Self::parse(&body)?;
        // A single chapter keeps the API's own response in the cache.
        if let Some((code, chapter)) = passage.split_once('.')
            && let Ok(chapter) = chapter.parse::<u16>()
            && let Some(book) = book_by_code(code)
        {
            let _ = self.cache.put(
                &Self::chapter_key(bible, ChapterRef::new(book.num, chapter)),
                &body,
            );
        }
        Ok(parsed.content)
    }

    /// Plain text of one verse.
    pub fn verse_text(&self, bible: u32, verse: VerseId) -> Result<Fetched<String>, Error> {
        // A cached chapter already has it: no request.
        if let Some(chapter) = self.chapter_cached(bible, verse.chapter_ref())
            && let Some((_, text)) = verse_texts(&chapter.blocks)
                .into_iter()
                .find(|(v, _)| *v == verse.verse())
        {
            return Ok(Fetched {
                value: text,
                from_cache: true,
            });
        }
        let usfm = usfm_verse(verse);
        let path = format!("/v1/bibles/{bible}/passages/{usfm}");
        let key = format!("passage:{bible}:{usfm}:text");
        let (body, from_cache) = self.cached(&key, &path, &[("format", "text".to_string())])?;
        let passage: PassageBody = Self::parse(&body)?;
        Ok(Fetched {
            value: passage.content.trim().to_string(),
            from_cache,
        })
    }

    /// Verse search in one Bible. `page_token` continues a previous page.
    pub fn search(
        &self,
        bible: u32,
        query: &str,
        page_size: u32,
        page_token: Option<&str>,
    ) -> Result<Fetched<SearchPage>, Error> {
        let mut params = vec![
            ("query", query.trim().to_string()),
            ("bible_id", bible.to_string()),
            ("user_intent", "text".to_string()),
            ("page_size", page_size.to_string()),
        ];
        if let Some(t) = page_token {
            params.push(("page_token", t.to_string()));
        }
        let key = format!(
            "search:{bible}:{}:{page_size}:{}",
            query.trim().to_lowercase(),
            page_token.unwrap_or("")
        );
        let (body, from_cache) = self.cached(&key, "/v1/search-verses", &params)?;
        let parsed: SearchBody = Self::parse(&body)?;
        Ok(Fetched {
            value: SearchPage {
                verses: parsed
                    .verses
                    .iter()
                    .filter_map(|v| parse_usfm_verse(&v.reference))
                    .collect(),
                did_you_mean: parsed.did_you_mean,
                search_instead_for: parsed.search_instead_for.filter(|s| !s.is_empty()),
                next_page_token: parsed.next_page_token.filter(|t| !t.is_empty()),
            },
            from_cache,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn source(name: &str) -> Source {
        let dir = std::env::temp_dir().join(format!("bible-yv-src-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // An empty key means any request that isn't served from cache fails
        // with MissingKey, so a passing test proves no request was made.
        Source::new(Client::new(""), Cache::new(dir))
    }

    #[test]
    fn usfm_references_round_trip() {
        let john = VerseId::new(43, 3, 16);
        assert_eq!(usfm_verse(john), "JHN.3.16");
        assert_eq!(parse_usfm_verse("JHN.3.16"), Some(john));
        assert_eq!(parse_usfm_verse("JHN.3.16-18"), Some(john));
        assert_eq!(parse_usfm_verse("jhn.3.16"), Some(john));
        assert_eq!(parse_usfm_verse("XXX.1.1"), None);
        assert_eq!(parse_usfm_verse("JHN.3"), None);
        assert_eq!(usfm_chapter(ChapterRef::new(19, 23)), "PSA.23");
    }

    #[test]
    fn chapter_is_served_from_cache_without_a_request() {
        let src = source("chapter");
        let chapter = ChapterRef::new(19, 23);
        assert!(src.chapter_cached(3034, chapter).is_none());
        assert_eq!(src.chapter(3034, chapter), Err(Error::MissingKey));
        let body = r#"{"id":"PSA.23","reference":"Psalm 23","content":"<div><div class=\"q1\"><span class=\"yv-v\" v=\"1\"></span>The <span class=\"nd\">Lord</span> is my shepherd;</div></div>"}"#;
        src.cache()
            .put(&Source::chapter_key(3034, chapter), body)
            .unwrap();
        let cached = src.chapter_cached(3034, chapter).unwrap();
        assert_eq!(cached.blocks.len(), 1);
        let fetched = src.chapter(3034, chapter).unwrap();
        assert!(fetched.from_cache);
        assert_eq!(fetched.value, cached);
        assert_eq!(src.client().requests_made(), 0);
        let _ = std::fs::remove_dir_all(src.cache().dir());
    }

    #[test]
    fn bible_catalog_pages_come_from_cache() {
        let src = source("bibles");
        let key = "bibles:anon:en:false:";
        let page = r#"{"data":[{"id":3034,"abbreviation":"BSB","title":"Berean Standard Bible","language_tag":"en","copyright":"Public domain"},{"id":111,"abbreviation":"NIV","title":"New International Version","localized_title":"NIV","language_tag":"en"}],"total_size":2}"#;
        src.cache().put(key, page).unwrap();
        let got = src.bibles(&["en".into()], false, "anon").unwrap();
        assert!(got.from_cache);
        assert_eq!(got.value.len(), 2);
        assert_eq!(got.value[0].label(), "BSB");
        assert_eq!(got.value[1].name(), "NIV");
        let _ = std::fs::remove_dir_all(src.cache().dir());
    }

    #[test]
    fn search_results_parse_to_verse_ids() {
        let src = source("search");
        let key = "search:3034:love:6:";
        let body = r#"{"verses":[{"reference":"JHN.3.16"},{"reference":"1CO.13.4"},{"reference":"???"}],"did_you_mean":["lov"],"next_page_token":"tok"}"#;
        src.cache().put(key, body).unwrap();
        let got = src.search(3034, "  Love ", 6, None).unwrap();
        assert!(got.from_cache);
        assert_eq!(
            got.value.verses,
            [VerseId::new(43, 3, 16), VerseId::new(46, 13, 4)]
        );
        assert_eq!(got.value.did_you_mean, ["lov"]);
        assert_eq!(got.value.next_page_token.as_deref(), Some("tok"));
        let _ = std::fs::remove_dir_all(src.cache().dir());
    }

    // MARK: Against a fake server

    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A tiny HTTP server that answers each request with `handler(path)` and
    /// counts them. Returns its base URL and the request log.
    fn serve(
        handler: impl Fn(&str) -> (u16, String) + Send + 'static,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let log = Arc::new(Mutex::new(Vec::new()));
        let seen = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head
                    .lines()
                    .next()
                    .and_then(|l| l.split(' ').nth(1))
                    .unwrap_or("")
                    .to_string();
                lock(&seen).push(path.clone());
                let (status, body) = handler(&path);
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        (base, log)
    }

    fn passage(html: &str) -> String {
        serde_json::json!({ "content": html }).to_string()
    }

    /// `n` short chapters in one passage: verses restart at 1 in each.
    fn chapters_html(n: u16) -> String {
        (1..=n)
            .map(|c| {
                format!(
                    "<div class=\"p\"><span class=\"yv-v\" v=\"1\"></span>Chapter {c} one. <span class=\"yv-v\" v=\"2\"></span>Chapter {c} two.</div>"
                )
            })
            .collect()
    }

    fn fake(name: &str, base: &str) -> Source {
        let dir = std::env::temp_dir().join(format!("bible-yv-fake-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Source::new(Client::with_host("key", base), Cache::new(dir))
    }

    #[test]
    fn one_request_fills_the_cache_for_the_next_chapters() {
        let (base, log) = serve(|path| {
            assert!(path.contains("/passages/JHN.3-5"), "{path}");
            (200, passage(&chapters_html(3)))
        });
        let src = fake("window", &base);
        let john3 = ChapterRef::new(43, 3);
        let got = src.chapter_in(111, john3, (3, 5)).unwrap();
        assert!(!got.from_cache);
        assert_eq!(lock(&log).len(), 1, "three chapters, one request");
        // The neighbours are now free.
        for c in [4, 5] {
            let next = src.chapter(111, ChapterRef::new(43, c)).unwrap();
            assert!(next.from_cache, "John {c} should be cached");
        }
        assert_eq!(lock(&log).len(), 1);
        let john4 = src.chapter_cached(111, ChapterRef::new(43, 4)).unwrap();
        let text = verse_texts(&john4.blocks);
        assert_eq!(text[0].1, "Chapter 2 one.");
        let _ = std::fs::remove_dir_all(src.cache().dir());
    }

    #[test]
    fn a_range_the_api_mangles_shrinks_the_window_and_still_returns_the_chapter() {
        // The server ignores the range and returns just one chapter.
        let (base, log) = serve(|_| (200, passage(&chapters_html(1))));
        let src = fake("mangled", &base);
        let got = src.chapter_in(111, ChapterRef::new(43, 3), (3, 5)).unwrap();
        assert_eq!(got.value.blocks.len(), 1);
        // One wasted range request, then the chapter on its own.
        assert_eq!(lock(&log).len(), 2);
        assert!(lock(&log)[0].contains("JHN.3-5"));
        assert!(lock(&log)[1].contains("/passages/JHN.3?"));
        // Remembered: the next miss asks for two chapters, not three.
        assert_eq!(src.window_cap(111), 2);
        let _ = src.chapter_in(111, ChapterRef::new(43, 8), (8, 10));
        assert!(lock(&log)[2].contains("JHN.8-9"), "{:?}", lock(&log));
        let _ = std::fs::remove_dir_all(src.cache().dir());
    }

    #[test]
    fn real_failures_are_not_retried_chapter_by_chapter() {
        let (base, log) = serve(|_| (403, "{}".into()));
        let src = fake("forbidden", &base);
        let err = src
            .chapter_in(111, ChapterRef::new(43, 3), (3, 5))
            .unwrap_err();
        assert_eq!(err, Error::NotPermitted);
        assert_eq!(lock(&log).len(), 1);
        assert_eq!(src.window_cap(111), crate::window::MAX_CHAPTERS);
        let _ = std::fs::remove_dir_all(src.cache().dir());
    }

    #[test]
    fn simultaneous_requests_for_a_chapter_share_one_fetch() {
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let (base, _) = serve(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(200));
            (200, passage(&chapters_html(1)))
        });
        let src = fake("flight", &base);
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let src = src.clone();
                std::thread::spawn(move || src.chapter(111, ChapterRef::new(43, 3)).unwrap())
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(src.cache().dir());
    }

    #[test]
    fn verse_text_comes_from_a_cached_chapter() {
        let src = source("verse-from-chapter");
        let body = r#"{"content":"<div class=\"p\"><span class=\"yv-v\" v=\"16\"></span>For God so loved the world.</div>"}"#;
        src.cache()
            .put(&Source::chapter_key(111, ChapterRef::new(43, 3)), body)
            .unwrap();
        let got = src.verse_text(111, VerseId::new(43, 3, 16)).unwrap();
        assert!(got.from_cache);
        assert_eq!(got.value, "For God so loved the world.");
        assert_eq!(src.client().requests_made(), 0);
        let _ = std::fs::remove_dir_all(src.cache().dir());
    }
}
