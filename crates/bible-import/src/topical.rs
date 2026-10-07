//! Nave's Topical Bible and Torrey's New Topical Textbook (public domain),
//! read from CCEL's ThML files, plus the helpers that decide which of their
//! entries are worth keeping: topic naming, merging keys and proper-name
//! detection.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{Context as _, Result};
use bible_core::text::{stems, word_spans};
use bible_core::{VerseId, parse_osis_range};

use crate::usfm;

/// One entry of a topical Bible: a term and where it is cited.
#[derive(Debug, Default)]
pub struct Entry {
    pub term: String,
    /// Targets of "See X" cross-references.
    pub see: Vec<String>,
    /// First and last verse of each citation, with repeats kept: how often an
    /// entry cites a verse says how central the verse is to it.
    pub passages: Vec<(VerseId, VerseId)>,
}

pub fn parse(xml: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<term ") {
        rest = &rest[i..];
        let (Some(open_end), Some(close)) = (rest.find('>'), rest.find("</term>")) else {
            break;
        };
        let term = decode(&strip_tags(&rest[open_end + 1..close]));
        rest = &rest[close + "</term>".len()..];
        let Some(def_end) = rest.find("</def>") else {
            break;
        };
        let body = &rest[..def_end];
        rest = &rest[def_end..];

        let mut entry = Entry {
            term: term.trim().to_string(),
            ..Entry::default()
        };
        let mut cursor = body;
        while let Some(i) = cursor.find("osisRef=\"") {
            cursor = &cursor[i + "osisRef=\"".len()..];
            let Some(end) = cursor.find('"') else { break };
            for part in cursor[..end].split(',') {
                let osis = part.trim().trim_start_matches("Bible:");
                if let Some(range) = parse_osis_range(osis) {
                    entry.passages.push(range);
                }
            }
            cursor = &cursor[end..];
        }
        let mut cursor = body;
        while let Some(i) = cursor.find("?term=") {
            cursor = &cursor[i..];
            let (Some(open_end), Some(close)) = (cursor.find('>'), cursor.find("</a>")) else {
                break;
            };
            if open_end < close {
                entry
                    .see
                    .push(decode(&strip_tags(&cursor[open_end + 1..close])));
            }
            cursor = &cursor[close..];
        }
        if !entry.term.is_empty() {
            entries.push(entry);
        }
    }
    entries
}

pub fn load(path: &Path) -> Result<Vec<Entry>> {
    let xml =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(parse(&xml))
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// A searchable topic name from a book's heading: "FAITH, JUSTIFICATION BY"
/// becomes "justification by faith", "MAN, NATURAL" "natural man".
pub fn topic_name(term: &str) -> String {
    let lower = term.to_lowercase();
    let mut parts = lower.splitn(2, ',').map(str::trim);
    let head = parts.next().unwrap_or_default();
    let name = match parts.next() {
        Some(tail) if !tail.is_empty() => format!("{tail} {head}"),
        _ => head.to_string(),
    };
    name.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-' || *c == '\'')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Same key for "the power of the tongue" and "tongue, power of": the sorted
/// stems of the content words.
pub fn merge_key(name: &str) -> String {
    let mut stems = stems(name);
    stems.sort();
    stems.dedup();
    stems.join(" ")
}

/// Which words the Bible text treats as names (capitalized mid-sentence).
pub struct Vocab {
    /// word -> (capitalized, lowercase) counts.
    counts: HashMap<String, (u32, u32)>,
    verses: HashMap<VerseId, String>,
}

/// Words that, around a name, still make the topic about that name
/// ("the tribe of Gad", "Mount Sinai", "ancient Samaria").
const GENERIC: &[&str] = &[
    "tribe",
    "tribes",
    "city",
    "cities",
    "land",
    "mount",
    "mountain",
    "valley",
    "river",
    "sea",
    "king",
    "queen",
    "prophet",
    "prophetess",
    "children",
    "son",
    "sons",
    "daughter",
    "daughters",
    "house",
    "people",
    "ancient",
    "wilderness",
    "brook",
    "plain",
    "empire",
    "gate",
    "well",
    "town",
    "village",
    "country",
    "kingdom",
];

/// Words that are capitalized but are topics worth having.
const NAME_TOPICS: &[&str] = &["god", "jesus", "christ", "lord", "satan", "yahweh"];

impl Vocab {
    /// Counts words in a translation's USFM sources.
    pub fn from_usfm(dir: &Path) -> Result<Self> {
        let mut counts: HashMap<String, (u32, u32)> = HashMap::new();
        let mut verses = HashMap::new();
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "usfm" || x == "sfm"))
            .collect();
        files.sort();
        let mut note_id = 1;
        for file in files {
            let src = std::fs::read_to_string(&file)?;
            let src = src.trim_start_matches('\u{feff}');
            let Some(book) = usfm::parse_book(src, &mut note_id)? else {
                continue;
            };
            for (id, (_, text)) in &book.verses {
                verses.insert(*id, text.clone());
                let mut sentence_start = true;
                for span in word_spans(text) {
                    let word = &text[span.clone()];
                    let before = text[..span.start].trim_end();
                    sentence_start = sentence_start
                        || before.is_empty()
                        || before.ends_with(['.', '?', '!', ':', '“', '"', '‘']);
                    let capital = word.chars().next().is_some_and(char::is_uppercase);
                    let slot = counts.entry(word.to_lowercase()).or_default();
                    if capital && !sentence_start {
                        slot.0 += 1;
                    } else if !capital {
                        slot.1 += 1;
                    }
                    sentence_start = false;
                }
            }
        }
        Ok(Self { counts, verses })
    }

    fn is_proper(&self, word: &str) -> bool {
        self.counts
            .get(word)
            .is_some_and(|(cap, low)| *cap > 0 && *cap as f32 >= 0.8 * (*cap + *low) as f32)
    }

    /// A person, place or other name: every content word is capitalized in
    /// the text. Topics about God himself are the exception.
    pub fn is_name(&self, name: &str) -> bool {
        // "hazar-enan": the text lowercases the second half, so judge the first.
        let words: Vec<String> = name
            .split_whitespace()
            .map(|w| w.split('-').next().unwrap_or(w).to_lowercase())
            .filter(|w| !bible_core::text::is_stop_word(w))
            .collect();
        if words.iter().any(|w| NAME_TOPICS.contains(&w.as_str())) {
            return false;
        }
        let core: Vec<&String> = words
            .iter()
            .filter(|w| !GENERIC.contains(&w.as_str()))
            .collect();
        !core.is_empty() && core.iter().all(|w| self.is_proper(w))
    }

    /// How many of the passages say one of the topic's words outright. A topic
    /// whose verses all use its word adds little over a plain text search.
    pub fn literal_share(&self, name: &str, passages: &[(VerseId, VerseId)]) -> f32 {
        let wanted = stems(name);
        let mut total = 0;
        let mut literal = 0;
        for &(start, end) in passages {
            let Some(text) = self.verses.get(&start) else {
                continue;
            };
            let _ = end;
            total += 1;
            if stems(text).iter().any(|s| {
                wanted
                    .iter()
                    .any(|w| s == w || (w.len() >= 5 && s.starts_with(w.as_str())))
            }) {
                literal += 1;
            }
        }
        if total == 0 {
            0.
        } else {
            literal as f32 / total as f32
        }
    }
}

/// Group an entry's citations by first verse, counting repeats.
pub fn citations(entry: &Entry, max_verses: u16) -> BTreeMap<(VerseId, VerseId), u32> {
    let mut map = BTreeMap::new();
    for &(start, end) in &entry.passages {
        let span = if start.chapter_ref() == end.chapter_ref() {
            end.verse().saturating_sub(start.verse()) + 1
        } else {
            u16::MAX
        };
        if end >= start && span <= max_verses {
            *map.entry((start, end)).or_insert(0) += 1;
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_naturally() {
        assert_eq!(
            topic_name("FAITH, JUSTIFICATION BY"),
            "justification by faith"
        );
        assert_eq!(topic_name("MAN, NATURAL"), "natural man");
        assert_eq!(topic_name("Access to God"), "access to god");
        assert_eq!(
            merge_key("the power of the tongue"),
            merge_key("TONGUE, POWER OF")
        );
    }

    #[test]
    fn parses_entries() {
        let xml = r#"<term id="x">PRIDE &amp; VANITY</term><def type="x">
<p class="index2"><scripRef osisRef="Bible:Prov.16.18">Pr 16:18</scripRef>; <scripRef osisRef="Bible:Isa.14.12-Isa.14.15">x</scripRef>
<scripRef osisRef="Bible:Exod.28">Ex 28</scripRef></p>
<p class="index3">.See <a href="bible.p.html?term=humility" id="q">HUMILITY</a></p></def>"#;
        let entries = parse(xml);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].term, "PRIDE & VANITY");
        assert_eq!(entries[0].passages.len(), 2);
        assert_eq!(entries[0].see, ["HUMILITY"]);
    }
}
