use std::fmt;

use serde::{Deserialize, Serialize};

use crate::books::{Book, book_by_num};

/// Canonical verse identifier encoded as `BBCCCVVV` (book, chapter, verse).
///
/// Ids are shared across translations, so annotations keyed by a `VerseId`
/// survive switching versions. Verse 0 is used for unnumbered content such
/// as Psalm titles in versions that don't number them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VerseId(pub u32);

impl VerseId {
    pub fn new(book: u8, chapter: u16, verse: u16) -> Self {
        Self(book as u32 * 1_000_000 + chapter as u32 * 1_000 + verse as u32)
    }

    pub fn book(self) -> u8 {
        (self.0 / 1_000_000) as u8
    }

    pub fn chapter(self) -> u16 {
        (self.0 / 1_000 % 1_000) as u16
    }

    pub fn verse(self) -> u16 {
        (self.0 % 1_000) as u16
    }

    pub fn chapter_ref(self) -> ChapterRef {
        ChapterRef {
            book: self.book(),
            chapter: self.chapter(),
        }
    }
}

impl fmt::Display for VerseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match book_by_num(self.book()) {
            Some(book) => write!(f, "{} {}:{}", book.name, self.chapter(), self.verse()),
            None => write!(f, "{}", self.0),
        }
    }
}

/// A single chapter of a single book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChapterRef {
    pub book: u8,
    pub chapter: u16,
}

impl ChapterRef {
    pub fn new(book: u8, chapter: u16) -> Self {
        Self { book, chapter }
    }

    pub fn book_info(self) -> &'static Book {
        book_by_num(self.book).expect("valid book number")
    }

    /// Following chapter in canonical order, crossing book boundaries.
    pub fn next(self) -> Option<Self> {
        if self.chapter < self.book_info().chapters {
            Some(Self::new(self.book, self.chapter + 1))
        } else {
            book_by_num(self.book + 1).map(|b| Self::new(b.num, 1))
        }
    }

    /// Preceding chapter in canonical order, crossing book boundaries.
    pub fn prev(self) -> Option<Self> {
        if self.chapter > 1 {
            Some(Self::new(self.book, self.chapter - 1))
        } else {
            book_by_num(self.book.checked_sub(1)?).map(|b| Self::new(b.num, b.chapters))
        }
    }
}

impl fmt::Display for ChapterRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.book_info().name, self.chapter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verse_id_round_trip() {
        let id = VerseId::new(43, 3, 16);
        assert_eq!(id.0, 43_003_016);
        assert_eq!((id.book(), id.chapter(), id.verse()), (43, 3, 16));
        assert_eq!(id.to_string(), "John 3:16");
        assert!(VerseId::new(1, 50, 26) < VerseId::new(2, 1, 1));
    }

    #[test]
    fn chapter_navigation_crosses_books() {
        assert_eq!(ChapterRef::new(1, 50).next(), Some(ChapterRef::new(2, 1)));
        assert_eq!(ChapterRef::new(2, 1).prev(), Some(ChapterRef::new(1, 50)));
        assert_eq!(ChapterRef::new(1, 1).prev(), None);
        assert_eq!(ChapterRef::new(66, 22).next(), None);
        assert_eq!(ChapterRef::new(19, 23).to_string(), "Psalms 23");
    }
}
