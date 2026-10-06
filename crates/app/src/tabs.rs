//! Reading tabs: each has its own passage, translation, back / forward
//! history and scroll position. Tabs persist in settings.json, so they double
//! as saved places to come back to.

use std::sync::atomic::{AtomicU64, Ordering};

use bible_core::{ChapterRef, VerseId};
use gpui_kit::ScrollHandle;

use crate::library::SavedTab;

const HISTORY_LIMIT: usize = 100;

/// A place in the text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Location {
    pub chapter: ChapterRef,
    /// Verse to scroll to and highlight, if any.
    pub verse: Option<VerseId>,
}

impl Location {
    pub fn chapter(chapter: ChapterRef) -> Self {
        Self {
            chapter,
            verse: None,
        }
    }
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub struct Tab {
    /// Stable identity, so the active tab survives reordering.
    pub id: u64,
    pub location: Location,
    pub translation: String,
    /// Pinned tabs sit first, compact and without a close button.
    pub pinned: bool,
    /// Custom name that turns the tab into a named saved view.
    pub name: Option<String>,
    back: Vec<Location>,
    forward: Vec<Location>,
    pub scroll: ScrollHandle,
}

impl Tab {
    pub fn new(location: Location, translation: String) -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            location,
            translation,
            pinned: false,
            name: None,
            back: Vec::new(),
            forward: Vec::new(),
            scroll: ScrollHandle::new(),
        }
    }

    /// A new tab at the same place, with fresh history.
    pub fn duplicate(&self) -> Self {
        Self::new(self.location, self.translation.clone())
    }

    /// Go somewhere new, recording the current place for Back. Returns false
    /// (and records nothing) when already there.
    pub fn navigate(&mut self, to: Location) -> bool {
        let here = self.location;
        let moved = here.chapter != to.chapter || to.verse.is_some_and(|v| here.verse != Some(v));
        if moved {
            self.back.push(here);
            if self.back.len() > HISTORY_LIMIT {
                self.back.remove(0);
            }
            self.forward.clear();
        }
        self.location = to;
        moved
    }

    pub fn go_back(&mut self) -> Option<Location> {
        let prev = self.back.pop()?;
        self.forward.push(self.location);
        self.location = prev;
        Some(prev)
    }

    pub fn go_forward(&mut self) -> Option<Location> {
        let next = self.forward.pop()?;
        self.back.push(self.location);
        self.location = next;
        Some(next)
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// "John 3"
    pub fn title(&self) -> String {
        self.location.chapter.to_string()
    }

    /// What the tab shows: its custom name, or "John 3 · BSB".
    pub fn label(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => format!("{} · {}", self.title(), self.translation),
        }
    }

    /// Set the custom name; blank clears it.
    pub fn rename(&mut self, name: &str) {
        let name = name.trim();
        self.name = (!name.is_empty()).then(|| name.to_string());
    }

    pub fn to_saved(&self) -> SavedTab {
        SavedTab {
            book: self.location.chapter.book,
            chapter: self.location.chapter.chapter,
            verse: self.location.verse.map(|v| v.verse()),
            translation: self.translation.clone(),
            pinned: self.pinned,
            name: self.name.clone(),
        }
    }

    /// Restore a saved tab. Invalid references fall back to John 1.
    pub fn from_saved(saved: &SavedTab) -> Self {
        let valid = bible_core::book_by_num(saved.book)
            .is_some_and(|b| saved.chapter >= 1 && saved.chapter <= b.chapters);
        let chapter = if valid {
            ChapterRef::new(saved.book, saved.chapter)
        } else {
            ChapterRef::new(43, 1)
        };
        let verse = saved
            .verse
            .filter(|_| valid)
            .map(|v| VerseId::new(chapter.book, chapter.chapter, v));
        let mut tab = Self::new(Location { chapter, verse }, saved.translation.clone());
        tab.pinned = saved.pinned;
        tab.name = saved.name.clone();
        tab
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(book: u8, chapter: u16) -> Location {
        Location::chapter(ChapterRef::new(book, chapter))
    }

    #[test]
    fn back_forward() {
        let mut tab = Tab::new(at(43, 1), "BSB".into());
        assert!(tab.navigate(at(43, 2)));
        assert!(tab.navigate(at(19, 23)));
        assert!(!tab.navigate(at(19, 23)), "same place records nothing");

        assert_eq!(tab.go_back(), Some(at(43, 2)));
        assert_eq!(tab.go_back(), Some(at(43, 1)));
        assert_eq!(tab.go_back(), None);
        assert_eq!(tab.go_forward(), Some(at(43, 2)));
        assert!(tab.can_go_forward());

        tab.navigate(at(1, 1));
        assert!(!tab.can_go_forward(), "a new jump clears forward");
        assert_eq!(tab.go_back(), Some(at(43, 2)));
    }

    #[test]
    fn jumping_to_a_verse_in_the_same_chapter_is_recorded() {
        let mut tab = Tab::new(at(43, 3), "BSB".into());
        let verse = Location {
            chapter: ChapterRef::new(43, 3),
            verse: Some(VerseId::new(43, 3, 16)),
        };
        assert!(tab.navigate(verse));
        assert_eq!(tab.go_back(), Some(at(43, 3)));
    }

    #[test]
    fn label_falls_back_to_the_passage() {
        let mut tab = Tab::new(at(43, 3), "BSB".into());
        assert_eq!(tab.label(), "John 3 · BSB");
        tab.rename("Born again");
        assert_eq!(tab.label(), "Born again");
        tab.rename("   ");
        assert_eq!(tab.label(), "John 3 · BSB");
    }

    #[test]
    fn saved_round_trip_and_fallback() {
        let tab = Tab::new(
            Location {
                chapter: ChapterRef::new(19, 23),
                verse: Some(VerseId::new(19, 23, 4)),
            },
            "KJV".into(),
        );
        let mut tab = tab;
        tab.pinned = true;
        tab.rename("  Shepherd psalm ");
        let restored = Tab::from_saved(&tab.to_saved());
        assert!(restored.pinned);
        assert_eq!(restored.label(), "Shepherd psalm");
        assert_eq!(restored.location, tab.location);
        assert_eq!(restored.translation, "KJV");

        let bogus = SavedTab {
            book: 43,
            chapter: 999,
            verse: Some(1),
            translation: "BSB".into(),
            pinned: false,
            name: None,
        };
        assert_eq!(Tab::from_saved(&bogus).location, at(43, 1));
    }
}
