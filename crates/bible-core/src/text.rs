//! Word handling shared by search, topics and highlighting.

use std::ops::Range;

/// Words too common to help match a topic or phrase.
const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "in", "is", "it", "of", "on",
    "or", "the", "to", "with",
];

pub fn is_stop_word(word: &str) -> bool {
    STOP_WORDS.contains(&word)
}

/// Lowercase words made of letters, digits and apostrophes.
pub fn words(text: &str) -> Vec<String> {
    word_spans(text)
        .into_iter()
        .map(|r| text[r].to_lowercase())
        .collect()
}

/// Byte ranges of the words in `text`, in order.
pub fn word_spans(text: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        let in_word = c.is_alphanumeric() || c == '\'' || c == '’';
        match (in_word, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                spans.push(s..i);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        spans.push(s..text.len());
    }
    spans
}

/// A light English stemmer: enough to treat "love", "loved", "loveth" and
/// "loving" alike, and "forgive" with "forgiveness". Not linguistically exact;
/// it only has to be consistent between queries, topics and highlights.
pub fn stem(word: &str) -> String {
    let mut s: String = word
        .chars()
        .filter(|c| *c != '’' && *c != '\'')
        .flat_map(char::to_lowercase)
        .collect();
    if let Some(base) = s.strip_suffix("ies")
        && base.len() >= 2
    {
        return format!("{base}y");
    }
    for suffix in ["ness", "ing", "eth", "ed", "es", "ly", "s"] {
        if s.len() >= suffix.len() + 3 && s.ends_with(suffix) {
            s.truncate(s.len() - suffix.len());
            break;
        }
    }
    if s.len() > 3 && s.ends_with('e') {
        s.pop();
    }
    s
}

/// Stemmed words of `text` without stop words.
pub fn stems(text: &str) -> Vec<String> {
    words(text)
        .iter()
        .filter(|w| !is_stop_word(w))
        .map(|w| stem(w))
        .collect()
}

/// Where the words of `query` show in `text`: every word whose stem equals
/// (or, for the last word being typed, starts with) a query word's stem.
/// Ranges are byte offsets into `text`, sorted and non-overlapping.
pub fn highlight_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    let wanted: Vec<String> = stems(query);
    if wanted.is_empty() {
        return Vec::new();
    }
    let last = wanted.len() - 1;
    word_spans(text)
        .into_iter()
        .filter(|r| {
            let s = stem(&text[r.clone()]);
            wanted
                .iter()
                .enumerate()
                .any(|(i, w)| s == *w || (i == last && s.starts_with(w.as_str())))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_collapse_inflections() {
        for w in ["love", "loved", "loves", "loving", "loveth"] {
            assert_eq!(stem(w), "lov", "{w}");
        }
        assert_eq!(stem("forgiveness"), stem("forgive"));
        assert_eq!(stem("glories"), "glory");
        assert_eq!(stem("God"), "god");
        assert_eq!(stem("is"), "is");
    }

    #[test]
    fn highlights_inflected_words() {
        let text = "For God so loved the world";
        let hits = highlight_ranges(text, "god loving");
        let found: Vec<&str> = hits.iter().map(|r| &text[r.clone()]).collect();
        assert_eq!(found, ["God", "loved"]);
        // The last word matches as a prefix while typing.
        let hits = highlight_ranges(text, "wor");
        assert_eq!(&text[hits[0].clone()], "world");
    }
}
