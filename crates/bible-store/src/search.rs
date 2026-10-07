//! Ranked verse search: full-text matches (stemmed, scored by bm25) blended
//! with topic membership, exact-phrase matches and how well known a verse is.
//! A search for "pride" returns verses that say "pride" and also verses the
//! topic lists file under it, with the best of both first.

use std::collections::HashMap;
use std::ops::RangeInclusive;

use bible_core::VerseId;
use bible_core::text::{is_stop_word, words};

use crate::study::{BOOST_STRENGTH, Study, TopicMatch};
use crate::{SearchHit, Translation};

/// How several words in a query combine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatchMode {
    /// Every word must appear (the last one may be a prefix).
    #[default]
    All,
    /// Any word may appear; verses with more of them rank higher.
    Any,
    /// The words must appear together, in order.
    Phrase,
}

impl MatchMode {
    pub const ALL: [MatchMode; 3] = [Self::All, Self::Any, Self::Phrase];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All words",
            Self::Any => "Any word",
            Self::Phrase => "Exact phrase",
        }
    }
}

pub struct SearchRequest<'a> {
    pub query: &'a str,
    pub mode: MatchMode,
    /// Canonical book numbers, inclusive.
    pub books: RangeInclusive<u8>,
    pub limit: usize,
}

impl<'a> SearchRequest<'a> {
    pub fn new(query: &'a str, limit: usize) -> Self {
        Self {
            query,
            mode: MatchMode::All,
            books: 1..=u8::MAX,
            limit,
        }
    }
}

#[derive(Debug, Default)]
pub struct Results {
    /// Topics the query is about, best first (empty without study data).
    pub topics: Vec<TopicMatch>,
    /// Best first, at most `limit`.
    pub hits: Vec<SearchHit>,
    /// Matching verses before the limit was applied.
    pub total: usize,
}

/// A query broken into what must match, what must not, and phrases.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedQuery {
    /// Words and quoted phrases (several words) that should match.
    pub terms: Vec<Vec<String>>,
    /// Words after a minus sign that must not match.
    pub excluded: Vec<String>,
}

impl ParsedQuery {
    pub fn parse(query: &str) -> Self {
        let mut parsed = Self::default();
        let mut rest = query;
        while let Some(open) = rest.find('"') {
            let (before, after) = rest.split_at(open);
            parsed.add_words(before);
            let after = &after[1..];
            match after.find('"') {
                Some(close) => {
                    let phrase = words(&after[..close]);
                    if !phrase.is_empty() {
                        parsed.terms.push(phrase);
                    }
                    rest = &after[close + 1..];
                }
                None => {
                    // An unclosed quote still means "this phrase".
                    let phrase = words(after);
                    if !phrase.is_empty() {
                        parsed.terms.push(phrase);
                    }
                    rest = "";
                }
            }
        }
        parsed.add_words(rest);
        parsed
    }

    fn add_words(&mut self, text: &str) {
        for raw in text.split_whitespace() {
            if let Some(excluded) = raw.strip_prefix('-') {
                self.excluded.extend(words(excluded));
            } else {
                self.terms.extend(words(raw).into_iter().map(|w| vec![w]));
            }
        }
    }

    /// The positive words, space separated, for highlighting and topics.
    pub fn plain(&self) -> String {
        self.terms
            .iter()
            .map(|t| t.join(" "))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn has_phrase(&self) -> bool {
        self.terms.iter().any(|t| t.len() > 1)
    }

    /// FTS5 expression, or `None` when there is nothing to search for.
    fn fts(&self, mode: MatchMode, drop_stop_words: bool) -> Option<String> {
        let mut terms: Vec<Vec<String>> = self.terms.clone();
        if drop_stop_words && terms.iter().any(|t| t.len() == 1 && !is_stop_word(&t[0])) {
            terms.retain(|t| t.len() > 1 || !is_stop_word(&t[0]));
        }
        if terms.is_empty() {
            return None;
        }
        let quote = |w: &str| format!("\"{}\"", w.replace('"', ""));
        let last = terms.len() - 1;
        let parts: Vec<String> = if mode == MatchMode::Phrase {
            let all: Vec<String> = terms.iter().flatten().cloned().collect();
            vec![format!("\"{}\" *", all.join(" ").replace('"', ""))]
        } else {
            terms
                .iter()
                .enumerate()
                .map(|(i, t)| match t.as_slice() {
                    [w] if i == last => format!("({} OR {}*)", quote(w), quote(w)),
                    [w] => quote(w),
                    many => format!("\"{}\"", many.join(" ").replace('"', "")),
                })
                .collect()
        };
        let joiner = if mode == MatchMode::Any {
            " OR "
        } else {
            " AND "
        };
        let mut expr = parts.join(joiner);
        if !self.excluded.is_empty() {
            let out: Vec<String> = self.excluded.iter().map(|w| quote(w)).collect();
            expr = format!("({expr}) NOT ({})", out.join(" OR "));
        }
        Some(expr)
    }
}

/// How many text matches are fetched for re-ranking.
const CANDIDATES: usize = 1500;
/// Fewer exact (all-words) matches than this pulls in partial matches.
const MIN_FULL_MATCHES: usize = 5;
/// Topic verses added beyond the text matches, per search.
const TOPIC_EXTRAS: usize = 30;
/// Topics blended into the ranking.
const TOPICS_USED: usize = 2;

struct Scored {
    hit: SearchHit,
    text_score: f32,
    full: bool,
}

pub fn search(
    translation: &Translation,
    study: Option<&Study>,
    request: &SearchRequest,
) -> Results {
    let parsed = ParsedQuery::parse(request.query);
    let mut results = Results::default();
    if parsed.terms.is_empty() {
        return results;
    }

    // Topics: not for quoted phrases or exact-phrase mode, which ask for text.
    let topic_query = parsed.plain();
    if let Some(study) = study
        && !parsed.has_phrase()
        && request.mode != MatchMode::Phrase
    {
        results.topics = study.topics(&topic_query, 5);
    }
    let strong: Vec<&TopicMatch> = results
        .topics
        .iter()
        .filter(|t| t.strength >= BOOST_STRENGTH)
        .take(TOPICS_USED)
        .collect();

    // Verse -> how well the strongest topics fit it (0..=1).
    let mut topic_fit: HashMap<VerseId, f32> = HashMap::new();
    let mut topic_order: Vec<VerseId> = Vec::new();
    if let Some(study) = study {
        for m in &strong {
            let passages = study.topic_passages(m.topic.id, 200);
            let best = passages.iter().map(|p| p.score).max().unwrap_or(1).max(1) as f32;
            for p in passages {
                let fit = (0.4 + 0.6 * p.score as f32 / best) * m.strength as f32 / 100.;
                for verse in p.verses() {
                    if request.books.contains(&verse.book()) {
                        let slot = topic_fit.entry(verse).or_insert_with(|| {
                            topic_order.push(verse);
                            0.
                        });
                        *slot = slot.max(fit);
                    }
                }
            }
        }
    }

    // Text matches.
    let mut scored: HashMap<VerseId, Scored> = HashMap::new();
    let collect = |expr: &str, full: bool, scored: &mut HashMap<VerseId, Scored>| {
        for (hit, rank) in translation
            .fts_candidates(expr, request.books.clone(), CANDIDATES)
            .unwrap_or_default()
        {
            scored.entry(hit.verse).or_insert(Scored {
                text_score: -rank as f32,
                hit,
                full,
            });
        }
    };
    if let Some(expr) = parsed.fts(request.mode, false) {
        collect(&expr, true, &mut scored);
    }
    let multi_word = parsed.terms.len() > 1 && request.mode == MatchMode::All;
    if multi_word
        && scored.len() < MIN_FULL_MATCHES
        && let Some(expr) = parsed.fts(MatchMode::Any, true)
    {
        collect(&expr, false, &mut scored);
    }

    // Topic verses that don't contain the words ("pride" -> Proverbs 16:18
    // is in, but so is "God opposes the proud").
    let mut extras = 0;
    for verse in &topic_order {
        if extras >= TOPIC_EXTRAS {
            break;
        }
        if scored.contains_key(verse) {
            continue;
        }
        if let Ok(Some(text)) = translation.verse_text(*verse) {
            scored.insert(
                *verse,
                Scored {
                    hit: SearchHit {
                        verse: *verse,
                        text,
                        score: 0.,
                        in_topic: true,
                    },
                    text_score: 0.,
                    full: false,
                },
            );
            extras += 1;
        }
    }

    results.total = scored.len();
    let max_text = scored
        .values()
        .map(|s| s.text_score)
        .fold(f32::MIN_POSITIVE, f32::max);
    let phrase = words(&parsed.plain()).join(" ");
    let plain_words: Vec<String> = parsed.terms.iter().flatten().cloned().collect();

    let mut ranked: Vec<SearchHit> = scored
        .into_values()
        .map(|s| {
            let Scored {
                mut hit,
                text_score,
                full,
            } = s;
            let mut score = text_score / max_text;
            if full {
                score += 0.5;
            }
            let verse_words = words(&hit.text);
            let joined = verse_words.join(" ");
            if plain_words.len() > 1 && joined.contains(&phrase) {
                score += 0.6;
            }
            if !plain_words.is_empty() && plain_words.iter().all(|w| verse_words.contains(w)) {
                score += 0.25;
            }
            if let Some(fit) = topic_fit.get(&hit.verse) {
                score += 0.9 * fit;
                hit.in_topic = true;
            }
            if let Some(study) = study {
                score += 0.3 * study.popularity(hit.verse);
            }
            hit.score = score;
            hit
        })
        .collect();
    ranked.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.verse.cmp(&b.verse)));
    ranked.truncate(request.limit);
    results.hits = ranked;
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(q: &str) -> Vec<String> {
        ParsedQuery::parse(q)
            .terms
            .iter()
            .map(|t| t.join(" "))
            .collect()
    }

    #[test]
    fn parses_words_phrases_and_exclusions() {
        assert_eq!(terms("love your enemies"), ["love", "your", "enemies"]);
        assert_eq!(terms("\"fear not\" angel"), ["fear not", "angel"]);
        let p = ParsedQuery::parse("grace -works");
        assert_eq!(p.excluded, ["works"]);
        assert_eq!(p.plain(), "grace");
        // An unclosed quote is still a phrase.
        assert_eq!(terms("\"be still"), ["be still"]);
        assert!(ParsedQuery::parse("  \"*  ").terms.is_empty());
    }

    #[test]
    fn builds_fts_expressions() {
        let p = ParsedQuery::parse("god lov");
        assert_eq!(
            p.fts(MatchMode::All, false).unwrap(),
            "\"god\" AND (\"lov\" OR \"lov\"*)"
        );
        assert_eq!(
            p.fts(MatchMode::Any, false).unwrap(),
            "\"god\" OR (\"lov\" OR \"lov\"*)"
        );
        assert_eq!(p.fts(MatchMode::Phrase, false).unwrap(), "\"god lov\" *");
        let p = ParsedQuery::parse("the love -hate");
        assert_eq!(
            p.fts(MatchMode::All, true).unwrap(),
            "((\"love\" OR \"love\"*)) NOT (\"hate\")"
        );
    }
}
