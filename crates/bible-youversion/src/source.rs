//! The cached API surface the app uses: the Bible catalog, chapters,
//! single verses and verse search. Every GET goes through the disk cache.

use bible_core::{ChapterRef, VerseId, book_by_code};
use serde::{Deserialize, Serialize};

use crate::cache::Cache;
use crate::client::{Client, Error};
use crate::html::{Chapter, parse_chapter};

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

#[derive(Deserialize)]
struct PassageBody {
    #[serde(default)]
    content: String,
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
}

impl Source {
    pub fn new(client: Client, cache: Cache) -> Self {
        Self { client, cache }
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
    pub fn bibles(
        &self,
        languages: &[String],
        all_available: bool,
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
            let key = format!(
                "bibles:{}:{all_available}:{}",
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
        Some(parse_chapter(&passage.content, chapter))
    }

    pub fn chapter(&self, bible: u32, chapter: ChapterRef) -> Result<Fetched<Chapter>, Error> {
        let path = format!("/v1/bibles/{bible}/passages/{}", usfm_chapter(chapter));
        let query = [
            ("format", "html".to_string()),
            ("include_headings", "true".to_string()),
            ("include_notes", "true".to_string()),
        ];
        let (body, from_cache) = self.cached(&Self::chapter_key(bible, chapter), &path, &query)?;
        let passage: PassageBody = Self::parse(&body)?;
        Ok(Fetched {
            value: parse_chapter(&passage.content, chapter),
            from_cache,
        })
    }

    /// Plain text of one verse.
    pub fn verse_text(&self, bible: u32, verse: VerseId) -> Result<Fetched<String>, Error> {
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
        let key = "bibles:en:false:";
        let page = r#"{"data":[{"id":3034,"abbreviation":"BSB","title":"Berean Standard Bible","language_tag":"en","copyright":"Public domain"},{"id":111,"abbreviation":"NIV","title":"New International Version","localized_title":"NIV","language_tag":"en"}],"total_size":2}"#;
        src.cache().put(key, page).unwrap();
        let got = src.bibles(&["en".into()], false).unwrap();
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
}
