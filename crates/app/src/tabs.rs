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
    /// Second pane when this tab is split side by side. It is itself a `Tab`
    /// so it gets its own place, translation, history and scroll; only the
    /// outer tab's `pinned` / `name` count.
    pub side: Option<Box<Tab>>,
    /// Navigation targets the right pane instead of the left one.
    pub focus_side: bool,
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
            side: None,
            focus_side: false,
        }
    }

    /// The chapter this pane was showing just before the current one, for
    /// guessing which way the reader is heading (see `Direction`). After a
    /// Back, that is the chapter it came back from.
    pub fn previous_chapter(&self) -> Option<ChapterRef> {
        self.back.last().or(self.forward.last()).map(|l| l.chapter)
    }

    pub fn is_split(&self) -> bool {
        self.side.is_some()
    }

    /// The pane navigation applies to.
    pub fn focused(&self) -> &Tab {
        match &self.side {
            Some(side) if self.focus_side => side,
            _ => self,
        }
    }

    pub fn focused_mut(&mut self) -> &mut Tab {
        if self.focus_side && self.side.is_some() {
            self.side.as_deref_mut().expect("checked")
        } else {
            self
        }
    }

    /// Swap the reading state (not pinned / name / split) of this tab's own
    /// pane with its side pane's.
    fn swap_panes(&mut self) {
        if let Some(side) = self.side.as_mut() {
            std::mem::swap(&mut self.id, &mut side.id);
            std::mem::swap(&mut self.location, &mut side.location);
            std::mem::swap(&mut self.translation, &mut side.translation);
            std::mem::swap(&mut self.back, &mut side.back);
            std::mem::swap(&mut self.forward, &mut side.forward);
            std::mem::swap(&mut self.scroll, &mut side.scroll);
        }
    }

    /// Show `other` beside this tab's pane: on the right, or on the left
    /// when `right` is false. The new pane takes focus. Does nothing when
    /// already split or when `other` is itself split.
    pub fn split_with(&mut self, mut other: Tab, right: bool) -> bool {
        if self.is_split() || other.is_split() {
            return false;
        }
        other.pinned = false;
        other.name = None;
        self.side = Some(Box::new(other));
        if !right {
            self.swap_panes();
        }
        self.focus_side = right;
        true
    }

    /// Remove one pane (the right one when `right`) and return it as a tab.
    /// The other pane stays in this tab, which is no longer split.
    pub fn take_pane(&mut self, right: bool) -> Option<Tab> {
        self.side.as_ref()?;
        if !right {
            self.swap_panes();
        }
        let taken = self.side.take().map(|t| *t);
        self.focus_side = false;
        taken
    }

    /// A new tab at the same place, with fresh history. A split tab
    /// duplicates its focused pane.
    pub fn duplicate(&self) -> Self {
        let pane = self.focused();
        Self::new(pane.location, pane.translation.clone())
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
        match (&self.name, &self.side) {
            (Some(name), _) => name.clone(),
            (None, Some(side)) => format!("{}  |  {}", self.pane_label(), side.pane_label()),
            (None, None) => self.pane_label(),
        }
    }

    /// "John 3 · BSB"
    pub fn pane_label(&self) -> String {
        format!("{} · {}", self.title(), self.translation)
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
            side: self.side.as_ref().map(|s| Box::new(s.to_saved())),
            side_focused: self.focus_side && self.side.is_some(),
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
        if let Some(side) = &saved.side {
            let mut side = Self::from_saved(side);
            side.side = None;
            tab.side = Some(Box::new(side));
            tab.focus_side = saved.side_focused;
        }
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
            side: None,
            side_focused: false,
        };
        assert_eq!(Tab::from_saved(&bogus).location, at(43, 1));
    }

    #[test]
    fn split_keeps_each_pane_independent() {
        let mut tab = Tab::new(at(43, 3), "BSB".into());
        assert!(tab.split_with(Tab::new(at(45, 8), "KJV".into()), true));
        assert!(tab.is_split());
        assert_eq!(tab.label(), "John 3 · BSB  |  Romans 8 · KJV");
        // The new pane has focus; navigating moves only it.
        assert_eq!(tab.focused().translation, "KJV");
        tab.focused_mut().navigate(at(45, 9));
        assert_eq!(tab.location, at(43, 3));
        assert_eq!(tab.side.as_ref().unwrap().location, at(45, 9));
        // No nesting, no second split.
        assert!(!tab.split_with(Tab::new(at(1, 1), "BSB".into()), true));

        let restored = Tab::from_saved(&tab.to_saved());
        assert_eq!(restored.label(), "John 3 · BSB  |  Romans 9 · KJV");
        assert!(restored.focus_side);
    }

    #[test]
    fn split_left_and_take_pane() {
        let mut tab = Tab::new(at(43, 3), "BSB".into());
        assert!(tab.split_with(Tab::new(at(45, 8), "KJV".into()), false));
        // The dropped tab is now the left pane.
        assert_eq!(tab.label(), "Romans 8 · KJV  |  John 3 · BSB");
        assert!(!tab.focus_side || tab.focused().translation == "BSB");

        // Taking the left pane leaves the right one behind.
        let left = tab.take_pane(false).unwrap();
        assert_eq!(left.label(), "Romans 8 · KJV");
        assert_eq!(tab.label(), "John 3 · BSB");
        assert!(!tab.is_split());
    }
}
