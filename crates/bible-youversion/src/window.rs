//! Deciding how many chapters one request should cover.
//!
//! A cache miss on chapter N is a chance to get its neighbours in the same
//! request, if the API accepts a multi-chapter passage (`JHN.3-5`). That turns
//! the likely next page turns into cache hits instead of more requests. The
//! window never crosses a book and stays under a verse budget, because the
//! API may cap how much one passage returns.

use bible_core::ChapterRef;

/// Which way the reader is moving, from how they arrived at the chapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// They came from the previous chapter: read on.
    Forward,
    /// They came from the next chapter: reading backwards.
    Backward,
    /// They jumped here: they may go either way.
    Jump,
}

impl Direction {
    /// Compare where the reader was with where they are.
    pub fn from_move(previous: Option<ChapterRef>, now: ChapterRef) -> Self {
        match previous {
            Some(p) if Some(p) == now.prev() && p.book == now.book => Self::Forward,
            Some(p) if Some(p) == now.next() && p.book == now.book => Self::Backward,
            _ => Self::Jump,
        }
    }
}

/// Most chapters one request may cover.
pub const MAX_CHAPTERS: u16 = 3;
/// Most verses one request may cover. The comparable API.Bible service
/// truncates passages at 200 verses; stay well under.
pub const MAX_VERSES: u32 = 140;

/// The chapters of `chapter`'s book to request together, as `first..=last`
/// (always containing `chapter`). `verses` says how long a chapter is; it can
/// be approximate (another translation's count is fine).
pub fn plan(
    chapter: ChapterRef,
    direction: Direction,
    max_chapters: u16,
    verses: impl Fn(ChapterRef) -> u32,
) -> (u16, u16) {
    let book_len = chapter.book_info().chapters;
    let at = chapter.chapter;
    let max_chapters = max_chapters.clamp(1, MAX_CHAPTERS);
    let ahead = (at + 1)..=book_len;
    let behind = (1..at).rev();
    // Neighbours in the order we would like them. Each extends the window by
    // one, so it stays contiguous.
    let wanted: Vec<u16> = match direction {
        Direction::Forward => ahead.collect(),
        Direction::Backward => behind.collect(),
        Direction::Jump => {
            let (mut a, mut b) = (ahead, behind);
            let mut order = Vec::new();
            loop {
                match (a.next(), b.next()) {
                    (None, None) => break,
                    (x, y) => order.extend(x.into_iter().chain(y)),
                }
            }
            order
        }
    };
    let (mut first, mut last) = (at, at);
    let mut budget = MAX_VERSES.saturating_sub(verses(chapter));
    for next in wanted {
        if last - first + 1 >= max_chapters {
            break;
        }
        let cost = verses(ChapterRef::new(chapter.book, next));
        if cost > budget {
            break;
        }
        budget -= cost;
        first = first.min(next);
        last = last.max(next);
    }
    (first, last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn flat(_: ChapterRef) -> u32 {
        30
    }

    #[test]
    fn direction_comes_from_how_the_reader_arrived() {
        let john3 = ChapterRef::new(43, 3);
        assert_eq!(
            Direction::from_move(Some(ChapterRef::new(43, 2)), john3),
            Direction::Forward
        );
        assert_eq!(
            Direction::from_move(Some(ChapterRef::new(43, 4)), john3),
            Direction::Backward
        );
        assert_eq!(
            Direction::from_move(Some(ChapterRef::new(1, 1)), john3),
            Direction::Jump
        );
        assert_eq!(Direction::from_move(None, john3), Direction::Jump);
        // Crossing a book boundary is not "reading on" within the book.
        assert_eq!(
            Direction::from_move(Some(ChapterRef::new(42, 24)), ChapterRef::new(43, 1)),
            Direction::Jump
        );
    }

    #[test]
    fn reading_forward_looks_ahead() {
        assert_eq!(
            plan(ChapterRef::new(43, 3), Direction::Forward, 3, flat),
            (3, 5)
        );
    }

    #[test]
    fn reading_backward_looks_behind() {
        assert_eq!(
            plan(ChapterRef::new(43, 5), Direction::Backward, 3, flat),
            (3, 5)
        );
    }

    #[test]
    fn a_jump_takes_both_neighbours() {
        assert_eq!(
            plan(ChapterRef::new(43, 5), Direction::Jump, 3, flat),
            (4, 6)
        );
    }

    #[test]
    fn never_leaves_the_book() {
        // John has 21 chapters.
        assert_eq!(
            plan(ChapterRef::new(43, 21), Direction::Forward, 3, flat),
            (21, 21)
        );
        assert_eq!(
            plan(ChapterRef::new(43, 20), Direction::Forward, 3, flat),
            (20, 21)
        );
        assert_eq!(
            plan(ChapterRef::new(43, 1), Direction::Backward, 3, flat),
            (1, 1)
        );
        assert_eq!(
            plan(ChapterRef::new(43, 1), Direction::Jump, 3, flat),
            (1, 3)
        );
    }

    #[test]
    fn stays_under_the_verse_budget() {
        // Psalm 119 alone is over budget: just that chapter.
        let long = |c: ChapterRef| if c.chapter == 119 { 176 } else { 10 };
        assert_eq!(
            plan(ChapterRef::new(19, 119), Direction::Forward, 3, long),
            (119, 119)
        );
        assert_eq!(
            plan(ChapterRef::new(19, 118), Direction::Forward, 3, long),
            (118, 118)
        );
        // Long chapters shrink the window instead of failing it.
        let medium = |_: ChapterRef| 60;
        assert_eq!(
            plan(ChapterRef::new(43, 3), Direction::Forward, 3, medium),
            (3, 4)
        );
    }

    #[test]
    fn a_window_of_one_is_just_the_chapter() {
        assert_eq!(
            plan(ChapterRef::new(43, 3), Direction::Jump, 1, flat),
            (3, 3)
        );
    }
}
