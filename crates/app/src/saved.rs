//! Saved verses: the `user.db` store as a global, the Saved verses page's
//! filtering and sorting, tag suggestions, and Markdown export.
//!
//! Everything but the global is plain data so it can be unit tested.

use bible_core::{ChapterRef, Testament, VerseSet, book_by_num, parse_reference};
use bible_store::{NewSaved, SavedVerse, UserStore};
use gpui_kit::Global;

use crate::library::support_dir;

/// The user's saved verses, loaded once and kept in step with `user.db`.
pub struct Saved {
    store: Option<UserStore>,
    /// Every item, in manual order.
    pub items: Vec<SavedVerse>,
}

impl Global for Saved {}

impl Saved {
    /// `~/Library/Application Support/BibleReader/user.db`. Falls back to
    /// memory (nothing kept) if the file can't be opened.
    pub fn load() -> Self {
        let store = support_dir()
            .map(|dir| dir.join("user.db"))
            .and_then(|path| match UserStore::open(&path) {
                Ok(store) => Some(store),
                Err(err) => {
                    eprintln!(
                        "warning: saved verses unavailable ({}): {err:#}",
                        path.display()
                    );
                    None
                }
            })
            .or_else(|| UserStore::open_in_memory().ok());
        Self::with_store(store)
    }

    /// Nothing written to disk (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn in_memory() -> Self {
        Self::with_store(UserStore::open_in_memory().ok())
    }

    fn with_store(store: Option<UserStore>) -> Self {
        let mut saved = Self {
            store,
            items: Vec::new(),
        };
        saved.refresh();
        saved
    }

    fn refresh(&mut self) {
        if let Some(store) = &self.store {
            match store.all() {
                Ok(items) => self.items = items,
                Err(err) => eprintln!("warning: reading saved verses: {err:#}"),
            }
        }
    }

    fn write(&mut self, f: impl FnOnce(&UserStore) -> anyhow::Result<()>) {
        if let Some(store) = &self.store
            && let Err(err) = f(store)
        {
            eprintln!("warning: saving verses: {err:#}");
        }
        self.refresh();
    }

    pub fn get(&self, id: i64) -> Option<&SavedVerse> {
        self.items.iter().find(|i| i.id == id)
    }

    /// Items in this chapter.
    pub fn in_chapter(&self, chapter: ChapterRef) -> impl Iterator<Item = &SavedVerse> {
        self.items.iter().filter(move |i| i.chapter == chapter)
    }

    /// Every saved verse of the chapter, for highlighting.
    pub fn verses_in(&self, chapter: ChapterRef) -> VerseSet {
        let mut set = VerseSet::default();
        for item in self.in_chapter(chapter) {
            set.extend(item.verses.iter());
        }
        set
    }

    /// The item saved with exactly these verses.
    pub fn find_exact(&self, chapter: ChapterRef, verses: &VerseSet) -> Option<&SavedVerse> {
        self.in_chapter(chapter).find(|i| i.verses == *verses)
    }

    /// Save verses, or return the existing item with exactly these verses.
    pub fn add(&mut self, new: NewSaved) -> Option<i64> {
        let chapter = new.chapter?;
        if new.verses.is_empty() {
            return None;
        }
        if let Some(existing) = self.find_exact(chapter, &new.verses) {
            return Some(existing.id);
        }
        let mut id = None;
        self.write(|store| {
            id = Some(store.insert(&new)?.id);
            Ok(())
        });
        id
    }

    pub fn set_note(&mut self, id: i64, note: &str, tags: &[String]) {
        self.write(|store| store.update_note(id, note, tags));
    }

    /// Remove an item, returning it for Undo.
    pub fn remove(&mut self, id: i64) -> Option<SavedVerse> {
        let item = self.get(id)?.clone();
        self.write(|store| store.remove(id));
        Some(item)
    }

    pub fn restore(&mut self, item: &SavedVerse) {
        self.write(|store| store.restore(item));
    }

    /// Move `id` to just before `before` (to the end when `None`).
    pub fn move_item(&mut self, id: i64, before: Option<i64>) {
        let mut ids: Vec<i64> = self
            .items
            .iter()
            .map(|i| i.id)
            .filter(|i| *i != id)
            .collect();
        let at = before
            .and_then(|b| ids.iter().position(|i| *i == b))
            .unwrap_or(ids.len());
        ids.insert(at, id);
        self.write(|store| store.reorder(&ids));
    }

    /// Every tag in use with how many items have it, most used first.
    pub fn tags(&self) -> Vec<(String, usize)> {
        tag_counts(&self.items)
    }
}

pub fn tag_counts(items: &[SavedVerse]) -> Vec<(String, usize)> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for tag in items.iter().flat_map(|i| &i.tags) {
        match counts.iter_mut().find(|(t, _)| t.eq_ignore_ascii_case(tag)) {
            Some((_, n)) => *n += 1,
            None => counts.push((tag.clone(), 1)),
        }
    }
    counts.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    counts
}

/// Tags typed into the tag field: comma separated.
pub fn parse_tags(input: &str) -> Vec<String> {
    bible_store::user::normalize_tags(&input.split(',').map(str::to_string).collect::<Vec<_>>())
}

/// Existing tags to offer while typing `input`: those matching the word
/// being typed (after the last comma), or the most used ones when it's
/// empty. Tags already entered are left out.
pub fn suggest_tags(input: &str, known: &[(String, usize)], limit: usize) -> Vec<String> {
    let (done, partial) = match input.rsplit_once(',') {
        Some((done, partial)) => (parse_tags(done), partial.trim().to_lowercase()),
        None => (Vec::new(), input.trim().to_lowercase()),
    };
    let partial = partial.trim_start_matches('#');
    let mut starts = Vec::new();
    let mut contains = Vec::new();
    for (tag, _) in known {
        let lower = tag.to_lowercase();
        if done.iter().any(|d| d.eq_ignore_ascii_case(tag)) || lower == partial {
            continue;
        }
        if lower.starts_with(partial) {
            starts.push(tag.clone());
        } else if lower.contains(partial) {
            contains.push(tag.clone());
        }
    }
    starts.extend(contains);
    starts.truncate(limit);
    starts
}

/// Replace the word being typed in the tag field with `tag`.
pub fn complete_tag(input: &str, tag: &str) -> String {
    let done = match input.rsplit_once(',') {
        Some((done, _)) => parse_tags(done),
        None => Vec::new(),
    };
    let mut tags = done;
    tags.push(tag.to_string());
    format!("{}, ", tags.join(", "))
}

/// How the Saved verses page orders items.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Sort {
    /// The user's own order (drag to change).
    #[default]
    Manual,
    Newest,
    Oldest,
    /// Genesis to Revelation.
    Bible,
}

impl Sort {
    pub const ALL: [Sort; 4] = [Sort::Manual, Sort::Newest, Sort::Oldest, Sort::Bible];

    pub fn label(self) -> &'static str {
        match self {
            Sort::Manual => "My order",
            Sort::Newest => "Newest first",
            Sort::Oldest => "Oldest first",
            Sort::Bible => "Bible order",
        }
    }
}

/// What the Saved verses page is showing.
#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub query: String,
    pub tag: Option<String>,
    pub testament: Option<Testament>,
    pub with_note: bool,
    pub sort: Sort,
}

impl Filter {
    pub fn is_narrowed(&self) -> bool {
        !self.query.trim().is_empty()
            || self.tag.is_some()
            || self.testament.is_some()
            || self.with_note
    }

    /// Indexes into `items` of what to show, in display order.
    pub fn apply(&self, items: &[SavedVerse]) -> Vec<usize> {
        let query = self.query.trim();
        // "jn 3" or "ps 23:4" means a passage; other text is words.
        let reference = query
            .chars()
            .any(|c| c.is_ascii_digit())
            .then(|| parse_reference(query))
            .flatten();
        let words: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let mut shown: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if let Some(tag) = &self.tag
                    && !item.tags.iter().any(|t| t.eq_ignore_ascii_case(tag))
                {
                    return false;
                }
                if let Some(testament) = self.testament
                    && book_by_num(item.chapter.book).map(|b| b.testament()) != Some(testament)
                {
                    return false;
                }
                if self.with_note && item.note.trim().is_empty() {
                    return false;
                }
                match &reference {
                    Some(r) => {
                        item.chapter == r.chapter_ref()
                            && r.first_verse()
                                .is_none_or(|v| item.verses.contains(v.verse()))
                    }
                    None => {
                        let haystack = format!(
                            "{} {} {} {} {}",
                            item.label(),
                            item.version,
                            item.text,
                            item.note,
                            item.tags.join(" ")
                        )
                        .to_lowercase();
                        words
                            .iter()
                            .all(|w| haystack.contains(w.trim_start_matches('#')))
                    }
                }
            })
            .map(|(ix, _)| ix)
            .collect();
        match self.sort {
            Sort::Manual => {}
            Sort::Newest => {
                shown.sort_by_key(|ix| std::cmp::Reverse((items[*ix].created_at, items[*ix].id)))
            }
            Sort::Oldest => shown.sort_by_key(|ix| (items[*ix].created_at, items[*ix].id)),
            Sort::Bible => shown.sort_by_key(|ix| (items[*ix].chapter, items[*ix].verses.first())),
        }
        shown
    }
}

/// Text for saved verses: runs joined with spaces, gaps marked with "…".
pub fn join_verse_texts(verses: &VerseSet, texts: &[(u16, String)]) -> String {
    let text_of = |v: u16| texts.iter().find(|(n, _)| *n == v).map(|(_, t)| t.trim());
    verses
        .ranges()
        .iter()
        .map(|(a, b)| {
            (*a..=*b)
                .filter_map(text_of)
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|run| !run.is_empty())
        .collect::<Vec<_>>()
        .join(" … ")
}

/// The list as a Markdown document. `heading` says what was exported
/// ("Tag: Memory"), `date` is when.
pub fn markdown(items: &[&SavedVerse], heading: Option<&str>, date: &str) -> String {
    let mut out = String::from("# Saved verses\n\n");
    let count = match items.len() {
        1 => "1 passage".to_string(),
        n => format!("{n} passages"),
    };
    let mut meta = vec![count, format!("exported {date}")];
    if let Some(heading) = heading {
        meta.insert(0, heading.to_string());
    }
    out.push_str(&format!("{}\n", meta.join(" · ")));
    for item in items {
        out.push_str(&format!("\n## {} ({})\n\n", item.label(), item.version));
        for line in item.text.lines() {
            out.push_str(&format!("> {line}\n"));
        }
        if !item.note.trim().is_empty() {
            out.push('\n');
            out.push_str(item.note.trim());
            out.push('\n');
        }
        if !item.tags.is_empty() {
            let tags: Vec<String> = item
                .tags
                .iter()
                .map(|t| format!("#{}", t.replace(' ', "-")))
                .collect();
            out.push_str(&format!("\n{}\n", tags.join(" ")));
        }
    }
    // One notice per version that needs one, in order of first use.
    let mut notices: Vec<(&str, &str)> = Vec::new();
    for item in items {
        let notice = item.notice.trim();
        if !notice.is_empty()
            && !notice.to_lowercase().contains("public domain")
            && !notices.iter().any(|(v, _)| *v == item.version)
        {
            notices.push((&item.version, notice));
        }
    }
    if !notices.is_empty() {
        out.push_str("\n---\n\n");
        for (version, notice) in notices {
            out.push_str(&format!("{version}: {notice}\n\n"));
        }
        out.pop();
    }
    out
}

/// "October 12, 2026" for unix seconds (UTC).
pub fn long_date(secs: i64) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    // Howard Hinnant's days-to-civil.
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{} {day}, {year}", MONTHS[(month - 1) as usize])
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: i64, book: u8, chapter: u16, verses: &str, created: i64) -> SavedVerse {
        SavedVerse {
            id,
            chapter: ChapterRef::new(book, chapter),
            verses: VerseSet::parse(verses),
            translation: "BSB".into(),
            version: "BSB".into(),
            text: format!("Text of {id}."),
            notice: String::new(),
            note: String::new(),
            tags: Vec::new(),
            position: id,
            created_at: created,
            updated_at: created,
        }
    }

    fn sample() -> Vec<SavedVerse> {
        let mut a = item(1, 45, 8, "28", 30);
        a.tags = vec!["Memory".into(), "Hope".into()];
        a.text = "And we know that God works all things together for good.".into();
        let mut b = item(2, 19, 23, "1-3", 10);
        b.tags = vec!["Memory".into()];
        b.note = "Read at the hospital.".into();
        let c = item(3, 43, 3, "16", 20);
        vec![a, b, c]
    }

    #[test]
    fn filters_and_sorts() {
        let items = sample();
        let ids = |f: &Filter| {
            f.apply(&items)
                .iter()
                .map(|ix| items[*ix].id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&Filter::default()), vec![1, 2, 3]);
        let tag = Filter {
            tag: Some("memory".into()),
            ..Filter::default()
        };
        assert_eq!(ids(&tag), vec![1, 2]);
        let nt = Filter {
            testament: Some(Testament::New),
            ..Filter::default()
        };
        assert_eq!(ids(&nt), vec![1, 3]);
        let notes = Filter {
            with_note: true,
            ..Filter::default()
        };
        assert_eq!(ids(&notes), vec![2]);
        let words = Filter {
            query: "god WORKS".into(),
            ..Filter::default()
        };
        assert_eq!(ids(&words), vec![1]);
        let book = Filter {
            query: "rom".into(),
            ..Filter::default()
        };
        assert_eq!(ids(&book), vec![1]);
        let reference = Filter {
            query: "ps 23:2".into(),
            ..Filter::default()
        };
        assert_eq!(ids(&reference), vec![2]);
        let miss = Filter {
            query: "ps 23:5".into(),
            ..Filter::default()
        };
        assert!(ids(&miss).is_empty());
        let hashtag = Filter {
            query: "#hope".into(),
            ..Filter::default()
        };
        assert_eq!(ids(&hashtag), vec![1]);

        let sorted = |sort| {
            ids(&Filter {
                sort,
                ..Filter::default()
            })
        };
        assert_eq!(sorted(Sort::Newest), vec![1, 3, 2]);
        assert_eq!(sorted(Sort::Oldest), vec![2, 3, 1]);
        assert_eq!(sorted(Sort::Bible), vec![2, 3, 1]);
    }

    #[test]
    fn tag_suggestions() {
        let known = tag_counts(&sample());
        assert_eq!(known[0], ("Memory".to_string(), 2));
        assert_eq!(suggest_tags("", &known, 5), vec!["Memory", "Hope"]);
        assert_eq!(suggest_tags("ho", &known, 5), vec!["Hope"]);
        assert_eq!(suggest_tags("Memory, ", &known, 5), vec!["Hope"]);
        assert_eq!(suggest_tags("hope, mor", &known, 5), vec!["Memory"]);
        assert!(
            suggest_tags("memory", &known, 5).is_empty(),
            "already typed in full"
        );
        assert_eq!(complete_tag("hope, mem", "Memory"), "hope, Memory, ");
        assert_eq!(
            parse_tags(" hope,, #Memory , hope "),
            vec!["hope", "Memory"]
        );
    }

    #[test]
    fn verse_texts_join_with_gaps() {
        let texts: Vec<(u16, String)> = (1..=5).map(|v| (v, format!("V{v}."))).collect();
        assert_eq!(
            join_verse_texts(&VerseSet::parse("1-2,4"), &texts),
            "V1. V2. … V4."
        );
        assert_eq!(join_verse_texts(&VerseSet::parse("9"), &texts), "");
    }

    #[test]
    fn markdown_export() {
        let mut items = sample();
        items[0].version = "NIV".into();
        items[0].notice = "Copyright © Biblica, Inc.".into();
        items[1].notice = "Public domain".into();
        let refs: Vec<&SavedVerse> = items.iter().take(2).collect();
        let md = markdown(&refs, Some("Tag: Memory"), "October 12, 2026");
        assert_eq!(
            md,
            "# Saved verses\n\n\
             Tag: Memory · 2 passages · exported October 12, 2026\n\n\
             ## Romans 8:28 (NIV)\n\n\
             > And we know that God works all things together for good.\n\n\
             #Memory #Hope\n\n\
             ## Psalms 23:1–3 (BSB)\n\n\
             > Text of 2.\n\n\
             Read at the hospital.\n\n\
             #Memory\n\n\
             ---\n\n\
             NIV: Copyright © Biblica, Inc.\n"
        );
    }

    #[test]
    fn dates() {
        assert_eq!(long_date(0), "January 1, 1970");
        assert_eq!(long_date(1_791_763_200), "October 12, 2026");
    }
}
