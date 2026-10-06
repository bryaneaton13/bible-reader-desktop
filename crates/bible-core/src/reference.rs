//! Parses user-typed references like "jn 3:16", "1 cor 13", "ps 23:1-4".

use std::fmt;

use crate::books::{BOOKS, Book};
use crate::verse::{ChapterRef, VerseId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reference {
    pub book: u8,
    pub chapter: u16,
    pub verse_start: Option<u16>,
    pub verse_end: Option<u16>,
}

impl Reference {
    pub fn chapter_ref(&self) -> ChapterRef {
        ChapterRef::new(self.book, self.chapter)
    }

    pub fn first_verse(&self) -> Option<VerseId> {
        self.verse_start
            .map(|v| VerseId::new(self.book, self.chapter, v))
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.chapter_ref())?;
        match (self.verse_start, self.verse_end) {
            (Some(s), Some(e)) if e != s => write!(f, ":{s}-{e}"),
            (Some(s), _) => write!(f, ":{s}"),
            _ => Ok(()),
        }
    }
}

/// Lowercase and drop everything except letters and digits.
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Resolve a book name or abbreviation.
///
/// Exact matches on name, USFM code or alias win. Otherwise the first book in
/// canonical order whose name starts with the input is chosen, so "ph" gives
/// Philippians and "jo" gives Joshua.
pub fn find_book(input: &str) -> Option<&'static Book> {
    let key = normalize(input);
    if key.is_empty() {
        return None;
    }
    let exact = BOOKS.iter().find(|b| {
        normalize(b.name) == key
            || b.code.eq_ignore_ascii_case(&key)
            || b.aliases.contains(&key.as_str())
    });
    exact.or_else(|| BOOKS.iter().find(|b| normalize(b.name).starts_with(&key)))
}

/// Parse a reference. Returns `None` when the book is unknown or the
/// chapter/verse numbers are out of range for that book.
pub fn parse_reference(input: &str) -> Option<Reference> {
    let input = input.trim();
    // Split into book part and numeric tail. A leading digit belongs to the
    // book ("1 cor"), so look for the first digit after some letters.
    let bytes = input.as_bytes();
    let mut seen_letter = false;
    let mut split = input.len();
    for (i, &b) in bytes.iter().enumerate() {
        if b.is_ascii_alphabetic() {
            seen_letter = true;
        } else if b.is_ascii_digit() && seen_letter {
            split = i;
            break;
        }
    }
    let (book_part, tail) = input.split_at(split);
    let book = find_book(book_part)?;

    let numbers: Vec<u16> = tail
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().ok())
        .collect::<Option<_>>()?;
    let has_verse_sep = tail.contains(':') || tail.contains('.');

    let (chapter, verse_start, verse_end) = match (numbers.as_slice(), has_verse_sep) {
        ([], _) => (1, None, None),
        // Single-chapter books: "jude 5" means verse 5.
        ([v], false) if book.chapters == 1 => (1, Some(*v), None),
        ([v, e], false) if book.chapters == 1 => (1, Some(*v), Some(*e)),
        ([c], _) => (*c, None, None),
        ([c, v], _) => (*c, Some(*v), None),
        ([c, v, e, ..], _) => (*c, Some(*v), Some(*e)),
    };

    if chapter == 0 || chapter > book.chapters {
        return None;
    }
    if verse_start == Some(0) {
        return None;
    }
    let verse_end = match (verse_start, verse_end) {
        (Some(s), Some(e)) if e < s => None,
        (_, e) => e,
    };
    Some(Reference {
        book: book.num,
        chapter,
        verse_start,
        verse_end,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Option<String> {
        parse_reference(s).map(|r| r.to_string())
    }

    #[test]
    fn common_forms() {
        assert_eq!(p("jn 3:16").as_deref(), Some("John 3:16"));
        assert_eq!(p("John 3:16").as_deref(), Some("John 3:16"));
        assert_eq!(p("jhn3.16").as_deref(), Some("John 3:16"));
        assert_eq!(p("1 cor 13").as_deref(), Some("1 Corinthians 13"));
        assert_eq!(p("1cor13:4-7").as_deref(), Some("1 Corinthians 13:4-7"));
        assert_eq!(p("ps 23:1-4").as_deref(), Some("Psalms 23:1-4"));
        assert_eq!(p("Psalm 119").as_deref(), Some("Psalms 119"));
        assert_eq!(p("song of solomon 2").as_deref(), Some("Song of Songs 2"));
        assert_eq!(p("rev").as_deref(), Some("Revelation 1"));
        assert_eq!(p("Gen 1:1–3").as_deref(), Some("Genesis 1:1-3"));
    }

    #[test]
    fn single_chapter_books() {
        assert_eq!(p("jude 5").as_deref(), Some("Jude 1:5"));
        assert_eq!(p("phm 1:6").as_deref(), Some("Philemon 1:6"));
        assert_eq!(p("3 jn 14").as_deref(), Some("3 John 1:14"));
    }

    #[test]
    fn prefix_matching_prefers_canonical_order() {
        assert_eq!(p("phil 4:13").as_deref(), Some("Philippians 4:13"));
        assert_eq!(p("philem 1").as_deref(), Some("Philemon 1:1"));
        assert_eq!(p("judg 6").as_deref(), Some("Judges 6"));
        assert_eq!(p("lam 3").as_deref(), Some("Lamentations 3"));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(p(""), None);
        assert_eq!(p("xyz 1"), None);
        assert_eq!(p("gen 51"), None);
        assert_eq!(p("gen 0"), None);
        assert_eq!(p("gen 1:0"), None);
    }
}
