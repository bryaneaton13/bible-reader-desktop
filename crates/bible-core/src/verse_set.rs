//! A set of verses in one chapter, such as Romans 8:1–4, 28.

use std::fmt;

use crate::verse::{ChapterRef, VerseId};

/// Verse numbers within one chapter, sorted and without duplicates.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct VerseSet(Vec<u16>);

impl VerseSet {
    pub fn new(verses: impl IntoIterator<Item = u16>) -> Self {
        let mut v: Vec<u16> = verses.into_iter().filter(|n| *n > 0).collect();
        v.sort_unstable();
        v.dedup();
        Self(v)
    }

    /// Parse the storage form, `"1-4,28"`. Bad pieces are skipped.
    pub fn parse(s: &str) -> Self {
        let mut verses = Vec::new();
        for piece in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match piece.split_once(['-', '–']) {
                Some((a, b)) => {
                    if let (Ok(a), Ok(b)) = (a.trim().parse::<u16>(), b.trim().parse::<u16>())
                        && a <= b
                    {
                        verses.extend(a..=b);
                    }
                }
                None => verses.extend(piece.parse::<u16>().ok()),
            }
        }
        Self::new(verses)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn first(&self) -> Option<u16> {
        self.0.first().copied()
    }

    pub fn contains(&self, verse: u16) -> bool {
        self.0.binary_search(&verse).is_ok()
    }

    pub fn iter(&self) -> impl Iterator<Item = u16> + '_ {
        self.0.iter().copied()
    }

    /// Add the verse if missing, remove it if present.
    pub fn toggle(&mut self, verse: u16) {
        match self.0.binary_search(&verse) {
            Ok(ix) => {
                self.0.remove(ix);
            }
            Err(ix) => self.0.insert(ix, verse),
        }
    }

    pub fn extend(&mut self, verses: impl IntoIterator<Item = u16>) {
        *self = Self::new(self.0.iter().copied().chain(verses));
    }

    /// Runs of consecutive verses: `[(1, 4), (28, 28)]`.
    pub fn ranges(&self) -> Vec<(u16, u16)> {
        let mut out: Vec<(u16, u16)> = Vec::new();
        for &v in &self.0 {
            match out.last_mut() {
                Some((_, end)) if *end + 1 == v => *end = v,
                _ => out.push((v, v)),
            }
        }
        out
    }

    /// The storage form, `"1-4,28"`.
    pub fn to_storage(&self) -> String {
        self.join("-", ",")
    }

    /// "Romans 8:1–4, 28"
    pub fn label(&self, chapter: ChapterRef) -> String {
        match self.is_empty() {
            true => chapter.to_string(),
            false => format!("{chapter}:{self}"),
        }
    }

    pub fn verse_ids(&self, chapter: ChapterRef) -> impl Iterator<Item = VerseId> + '_ {
        self.0
            .iter()
            .map(move |v| VerseId::new(chapter.book, chapter.chapter, *v))
    }

    fn join(&self, dash: &str, comma: &str) -> String {
        self.ranges()
            .iter()
            .map(|(a, b)| match a == b {
                true => a.to_string(),
                false => format!("{a}{dash}{b}"),
            })
            .collect::<Vec<_>>()
            .join(comma)
    }
}

/// "1–4, 28"
impl fmt::Display for VerseSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.join("–", ", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_display_and_storage() {
        let set = VerseSet::parse("28, 1-4,3,x,9-7");
        assert_eq!(set.iter().collect::<Vec<_>>(), vec![1, 2, 3, 4, 28]);
        assert_eq!(set.to_string(), "1–4, 28");
        assert_eq!(set.to_storage(), "1-4,28");
        assert_eq!(VerseSet::parse(&set.to_storage()), set);
        assert_eq!(set.label(ChapterRef::new(45, 8)), "Romans 8:1–4, 28");
        assert_eq!(
            VerseSet::new([16]).label(ChapterRef::new(43, 3)),
            "John 3:16"
        );
    }

    #[test]
    fn toggle_and_extend() {
        let mut set = VerseSet::new([5, 3]);
        set.toggle(4);
        assert_eq!(set.ranges(), vec![(3, 5)]);
        set.toggle(4);
        assert_eq!(set.ranges(), vec![(3, 3), (5, 5)]);
        set.extend(7..=8);
        assert_eq!(set.to_string(), "3, 5, 7–8");
        assert!(set.contains(7) && !set.contains(6));
        assert_eq!(set.first(), Some(3));
        assert_eq!(set.len(), 4);
    }
}
