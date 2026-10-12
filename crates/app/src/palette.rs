//! ⌘K / ⌘P palette contents: go to a passage, switch tabs, reopen recent
//! chapters, run commands, and full-text search, grouped like Spotlight.
//!
//! Row building is plain data (`build`) so it can be unit tested. Every row
//! carries the Action it runs; `ReaderView` renders the rows with gpui-kit's
//! `Command` palette, which handles arrow keys, Enter and shortcut hints.

use std::ops::RangeInclusive;

use bible_core::{ChapterRef, Reference, VerseId, find_book, parse_reference};
use bible_store::{Results, SearchHit};
use gpui_kit::Action;

use crate::library::{Appearance, FontPairing, SourceEntry};
use crate::{
    ClosePane, CloseTab, CopyChapter, CopyVerse, DecreaseFont, DecreaseLineSpacing, FocusLeftPane,
    FocusRightPane, IncreaseFont, IncreaseLineSpacing, NewTab, OpenSettings, OpenShortcuts,
    ReopenClosedTab, ResetFont, ResetLineSpacing, SelectTranslation, SetAppearance, SetFontPairing,
    ToggleInspector, ToggleRedLetter, ToggleSidebar, ToggleSplit, ToggleVerseNumbers,
};

/// Verses fetched for the palette; the search page asks for more.
pub const SEARCH_LIMIT: usize = 60;
/// Verse rows shown in the palette before "See all".
const VERSES_SHOWN: usize = 20;
const TOPICS_SHOWN: usize = 3;
/// Suggested searches shown on an empty palette.
pub const SUGGESTIONS_SHOWN: usize = 4;
const RECENT_SHOWN: usize = 8;
/// A query starting with this shows commands only (⇧⌘P opens with it).
pub const COMMAND_PREFIX: char = '>';

/// Open a passage in the active tab.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = bible_reader, no_json)]
pub struct OpenPassage {
    pub chapter: ChapterRef,
    pub verse: Option<VerseId>,
}

/// Switch to the tab at this index.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = bible_reader, no_json)]
pub struct ActivateTab {
    pub index: usize,
}

/// Pin or unpin the tab at this index.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = bible_reader, no_json)]
pub struct PinTab {
    pub index: usize,
}

/// Ask for a new name for the tab at this index.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = bible_reader, no_json)]
pub struct RenameTab {
    pub index: usize,
}

/// Close the tab at this index.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = bible_reader, no_json)]
pub struct CloseTabAt {
    pub index: usize,
}

/// Put `in:<scope> ` in the palette so the next words search only there.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = bible_reader, no_json)]
pub struct SearchIn {
    pub scope: String,
}

/// Open the search results page. `query` may carry an `in:` scope.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = bible_reader, no_json)]
pub struct OpenSearchPage {
    pub query: String,
}

/// Put this text in the palette (used by suggested searches).
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = bible_reader, no_json)]
pub struct SetPaletteQuery {
    pub query: String,
}

pub enum Row {
    Goto(Reference),
    Tab {
        index: usize,
        title: String,
    },
    Recent(ChapterRef),
    Command {
        label: String,
        action: Box<dyn Action>,
    },
    Verse(SearchHit),
    /// A topic the query is about; opens the results page on it.
    Topic {
        name: String,
        passages: u32,
    },
    /// Open every result for the query on the search page.
    SeeAll {
        query: String,
        total: usize,
    },
    /// A suggested search; fills the palette with it.
    Suggestion(String),
}

impl Row {
    pub fn label(&self) -> String {
        match self {
            Row::Goto(r) => format!("Go to {r}"),
            Row::Tab { title, .. } => title.clone(),
            Row::Recent(c) => c.to_string(),
            Row::Command { label, .. } => label.clone(),
            Row::Verse(hit) => format!("{} {}", hit.verse, hit.text),
            Row::Topic { name, passages } => format!("Topic: {name} · {passages} passages"),
            Row::SeeAll { query, total } => format!("See all {total} results for “{query}”"),
            Row::Suggestion(query) => query.clone(),
        }
    }

    /// The passage this row opens, for ⌘Enter (open in a new tab).
    pub fn passage(&self) -> Option<(ChapterRef, Option<VerseId>)> {
        match self {
            Row::Goto(r) => Some((r.chapter_ref(), r.first_verse())),
            Row::Recent(c) => Some((*c, None)),
            Row::Verse(hit) => Some((hit.verse.chapter_ref(), Some(hit.verse))),
            Row::Tab { .. }
            | Row::Command { .. }
            | Row::Topic { .. }
            | Row::SeeAll { .. }
            | Row::Suggestion(_) => None,
        }
    }

    pub fn action(&self) -> Box<dyn Action> {
        match self {
            Row::Goto(r) => Box::new(OpenPassage {
                chapter: r.chapter_ref(),
                verse: r.first_verse(),
            }),
            Row::Tab { index, .. } => Box::new(ActivateTab { index: *index }),
            Row::Recent(c) => Box::new(OpenPassage {
                chapter: *c,
                verse: None,
            }),
            Row::Command { action, .. } => action.boxed_clone(),
            Row::Verse(hit) => Box::new(OpenPassage {
                chapter: hit.verse.chapter_ref(),
                verse: Some(hit.verse),
            }),
            Row::Topic { name, .. } => Box::new(OpenSearchPage {
                query: name.clone(),
            }),
            Row::SeeAll { query, .. } => Box::new(OpenSearchPage {
                query: query.clone(),
            }),
            Row::Suggestion(query) => Box::new(SetPaletteQuery {
                query: query.clone(),
            }),
        }
    }
}

pub struct Group {
    pub heading: String,
    pub rows: Vec<Row>,
}

/// What the palette needs to know about the app.
pub struct Context<'a> {
    pub current: ChapterRef,
    pub active_tab: usize,
    /// Titles of all open tabs, e.g. "John 3 · BSB".
    pub tabs: Vec<String>,
    pub recent: &'a [(u8, u16)],
    /// Bundled translations, then the user's YouVersion Bibles, in toolbar order.
    pub translations: Vec<SourceEntry>,
    /// Key of the focused pane's translation.
    pub translation: &'a str,
    pub appearance: Appearance,
    pub font_pairing: FontPairing,
    pub active_pinned: bool,
    /// The active tab shows two panes side by side.
    pub split: bool,
    /// Ranked search over the given books (canonical numbers).
    pub search: &'a dyn Fn(&str, RangeInclusive<u8>) -> Results,
    /// Suggested searches for the empty palette.
    pub suggestions: Vec<&'static str>,
}

/// Where a search is limited to, from an `in:nt` / `in:john` word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub label: String,
    pub books: RangeInclusive<u8>,
}

/// Pull `in:<scope>` words out of the query: `in:ot`, `in:nt`, or a book name
/// (`in:john`, `in:1cor`). Unknown scopes stay in the query as plain words.
pub fn split_scope(query: &str) -> (Option<Scope>, String) {
    let mut scope = None;
    let mut rest = Vec::new();
    for word in query.split_whitespace() {
        let target = word
            .get(..3)
            .filter(|p| p.eq_ignore_ascii_case("in:"))
            .map(|_| &word[3..])
            .filter(|t| !t.is_empty());
        let resolved = target.and_then(|t| match t.to_lowercase().as_str() {
            "ot" | "old" => Some(Scope {
                label: "Old Testament".into(),
                books: 1..=39,
            }),
            "nt" | "new" => Some(Scope {
                label: "New Testament".into(),
                books: 40..=66,
            }),
            _ => find_book(t).map(|b| Scope {
                label: b.name.into(),
                books: b.num..=b.num,
            }),
        });
        match resolved {
            Some(found) if scope.is_none() => scope = Some(found),
            _ => rest.push(word),
        }
    }
    (scope, rest.join(" "))
}

pub fn build(query: &str, cx: &Context) -> Vec<Group> {
    let query = query.trim();
    let mut groups = Vec::new();
    let mut push = |heading: String, rows: Vec<Row>| {
        if !rows.is_empty() {
            groups.push(Group { heading, rows });
        }
    };

    if let Some(rest) = query.strip_prefix(COMMAND_PREFIX) {
        push("Commands".into(), commands(rest.trim(), cx));
        push("Appearance".into(), appearance(rest.trim(), cx));
        return groups;
    }

    let raw = query;
    let (scope, query) = split_scope(query);
    let query = query.as_str();

    if query.is_empty() && scope.is_none() {
        push("Tabs".into(), tabs(query, cx));
        push(
            "Recent".into(),
            cx.recent
                .iter()
                .map(|(b, c)| ChapterRef::new(*b, *c))
                .filter(|c| *c != cx.current)
                .take(RECENT_SHOWN)
                .map(Row::Recent)
                .collect(),
        );
        push(
            "Try searching".into(),
            cx.suggestions
                .iter()
                .map(|s| Row::Suggestion(s.to_string()))
                .collect(),
        );
        push("Commands".into(), commands("", cx));
        push("Appearance".into(), appearance("", cx));
        return groups;
    }

    let reference = parse_reference(query);
    push(
        "Go to".into(),
        reference.into_iter().map(Row::Goto).collect(),
    );
    if scope.is_none() {
        push("Tabs".into(), tabs(query, cx));
        push(
            "Commands".into(),
            commands(query, cx).into_iter().take(6).collect(),
        );
    }
    // "jn 3" is a reference, not words to search for; "rom" might be either.
    let looks_like_reference = reference.is_some() && query.chars().any(|c| c.is_ascii_digit());
    if query.chars().count() >= 2 && !looks_like_reference {
        let (heading, books) = match &scope {
            Some(scope) => (format!("Verses in {}", scope.label), scope.books.clone()),
            None => ("Verses".to_string(), 1..=u8::MAX),
        };
        let found = (cx.search)(query, books);
        if scope.is_none() {
            push(
                "Topics".into(),
                found
                    .topics
                    .iter()
                    .filter(|t| t.strength >= 60)
                    .take(TOPICS_SHOWN)
                    .map(|t| Row::Topic {
                        name: t.topic.name.clone(),
                        passages: t.topic.passages,
                    })
                    .collect(),
            );
        }
        let mut rows: Vec<Row> = found
            .hits
            .into_iter()
            .take(VERSES_SHOWN)
            .map(Row::Verse)
            .collect();
        if !rows.is_empty() {
            let full_query = match &scope {
                Some(_) => format!("{} {}", query, raw_scope(raw)),
                None => query.to_string(),
            };
            rows.push(Row::SeeAll {
                query: full_query.trim().to_string(),
                total: found.total,
            });
        }
        push(heading, rows);
    }
    if scope.is_none() {
        push(
            "Appearance".into(),
            appearance(query, cx).into_iter().take(6).collect(),
        );
    }
    groups
}

/// The `in:` words of a raw query, to carry a scope over to the search page.
fn raw_scope(raw: &str) -> String {
    raw.split_whitespace()
        .filter(|w| w.len() > 3 && w[..3].eq_ignore_ascii_case("in:"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn tabs(query: &str, cx: &Context) -> Vec<Row> {
    if cx.tabs.len() < 2 {
        return Vec::new();
    }
    cx.tabs
        .iter()
        .enumerate()
        .filter(|(ix, title)| *ix != cx.active_tab && matches(query, title, &[]))
        .map(|(index, title)| Row::Tab {
            index,
            title: title.clone(),
        })
        .collect()
}

fn commands(query: &str, cx: &Context) -> Vec<Row> {
    let mut all: Vec<(String, &[&str], Box<dyn Action>)> = vec![
        ("New Tab".into(), &["open", "window"], Box::new(NewTab)),
        (
            "Reopen Closed Tab".into(),
            &["undo", "restore"],
            Box::new(ReopenClosedTab),
        ),
        ("Close Tab".into(), &[], Box::new(CloseTab)),
    ];
    if cx.split {
        all.push((
            "Unsplit (Separate Tabs)".into(),
            &["side", "compare", "parallel", "columns"],
            Box::new(ToggleSplit),
        ));
        all.push(("Close Pane".into(), &["split"], Box::new(ClosePane)));
        all.push((
            "Focus Left Pane".into(),
            &["split", "side"],
            Box::new(FocusLeftPane),
        ));
        all.push((
            "Focus Right Pane".into(),
            &["split", "side"],
            Box::new(FocusRightPane),
        ));
    } else {
        all.push((
            "Split Right".into(),
            &["side", "compare", "parallel", "columns", "two"],
            Box::new(ToggleSplit),
        ));
    }
    let rest: Vec<(String, &[&str], Box<dyn Action>)> = vec![
        (
            if cx.active_pinned {
                "Unpin Tab".into()
            } else {
                "Pin Tab".into()
            },
            &["keep", "lock"],
            Box::new(PinTab {
                index: cx.active_tab,
            }),
        ),
        (
            "Rename Tab…".into(),
            &["name", "save", "view", "title"],
            Box::new(RenameTab {
                index: cx.active_tab,
            }),
        ),
        (
            "Search Old Testament…".into(),
            &["find", "filter", "in:ot"],
            Box::new(SearchIn { scope: "ot".into() }),
        ),
        (
            "Search New Testament…".into(),
            &["find", "filter", "in:nt"],
            Box::new(SearchIn { scope: "nt".into() }),
        ),
        (
            format!("Search in {}…", cx.current.book_info().name),
            &["find", "filter", "book"],
            Box::new(SearchIn {
                scope: cx.current.book_info().code.to_lowercase(),
            }),
        ),
        (
            "Settings…".into(),
            &["preferences", "font", "line", "width", "options"],
            Box::new(OpenSettings),
        ),
        (
            "Keyboard Shortcuts".into(),
            &["keys", "hotkeys", "help", "bindings", "cheat sheet"],
            Box::new(OpenShortcuts),
        ),
        (
            "Copy Verse with Citation".into(),
            &["clipboard", "quote"],
            Box::new(CopyVerse),
        ),
        ("Copy Chapter".into(), &["clipboard"], Box::new(CopyChapter)),
        (
            "Toggle Sidebar".into(),
            &["books", "navigator"],
            Box::new(ToggleSidebar),
        ),
        (
            "Toggle Footnotes".into(),
            &["notes", "inspector"],
            Box::new(ToggleInspector),
        ),
        (
            "Toggle Red Letter".into(),
            &["jesus", "words"],
            Box::new(ToggleRedLetter),
        ),
        (
            "Toggle Verse Numbers".into(),
            &[],
            Box::new(ToggleVerseNumbers),
        ),
        (
            "Larger Text".into(),
            &["font", "size", "zoom"],
            Box::new(IncreaseFont),
        ),
        (
            "Smaller Text".into(),
            &["font", "size", "zoom"],
            Box::new(DecreaseFont),
        ),
        (
            "Actual Size Text".into(),
            &["font", "size", "reset"],
            Box::new(ResetFont),
        ),
        (
            "Looser Line Spacing".into(),
            &["line", "height", "leading", "spacing"],
            Box::new(IncreaseLineSpacing),
        ),
        (
            "Tighter Line Spacing".into(),
            &["line", "height", "leading", "spacing"],
            Box::new(DecreaseLineSpacing),
        ),
        (
            "Default Line Spacing".into(),
            &["line", "height", "leading", "spacing", "reset"],
            Box::new(ResetLineSpacing),
        ),
    ];
    all.extend(rest);
    for (index, entry) in cx.translations.iter().enumerate() {
        if entry.key != cx.translation {
            let (abbrev, name) = (&entry.label, &entry.name);
            let source = if entry.remote { " · YouVersion" } else { "" };
            all.push((
                format!("Switch to {abbrev} ({name}){source}"),
                &["translation", "version", "bible"],
                Box::new(SelectTranslation { index }),
            ));
        }
    }
    all.into_iter()
        .filter(|(label, keywords, _)| matches(query, label, keywords))
        .map(|(label, _, action)| Row::Command { label, action })
        .collect()
}

/// Theme and font pairing switches. Shown last, after Bible results.
fn appearance(query: &str, cx: &Context) -> Vec<Row> {
    let themes = Appearance::ALL
        .into_iter()
        .filter(|a| *a != cx.appearance)
        .map(|appearance| {
            (
                format!("Theme: {}", appearance.label()),
                &["appearance", "color", "mode"][..],
                Box::new(SetAppearance { appearance }) as Box<dyn Action>,
            )
        });
    let fonts = FontPairing::ALL
        .into_iter()
        .filter(|p| *p != cx.font_pairing)
        .map(|pairing| {
            (
                format!("Font: {}", pairing.description()),
                &["typeface", "pairing", "serif", "text", "style"][..],
                Box::new(SetFontPairing { pairing }) as Box<dyn Action>,
            )
        });
    themes
        .chain(fonts)
        .filter(|(label, keywords, _)| matches(query, label, keywords))
        .map(|(label, _, action)| Row::Command { label, action })
        .collect()
}

/// Every query word appears in the label or keywords (case-insensitive).
fn matches(query: &str, label: &str, keywords: &[&str]) -> bool {
    let haystack = format!("{} {}", label, keywords.join(" ")).to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| haystack.contains(word))
}

#[cfg(test)]
mod tests {
    use bible_store::{Topic, TopicMatch};

    use super::*;

    fn with<R>(query: &str, f: impl FnOnce(Vec<Group>) -> R) -> R {
        let search = |q: &str, books: RangeInclusive<u8>| Results {
            topics: vec![TopicMatch {
                topic: Topic {
                    id: 1,
                    name: q.to_string(),
                    passages: 9,
                },
                strength: if q == "love" { 100 } else { 0 },
            }],
            hits: vec![SearchHit {
                verse: VerseId::new(*books.start(), 13, 4),
                text: format!("hit for {q}"),
                score: 1.,
                in_topic: false,
            }],
            total: 42,
            corrected: None,
        };
        let recent = [(19, 23), (43, 3)];
        let cx = Context {
            current: ChapterRef::new(43, 3),
            active_tab: 0,
            tabs: vec!["John 3 · BSB".into(), "Psalms 23 · KJV".into()],
            recent: &recent,
            translations: vec![
                SourceEntry {
                    key: "BSB".into(),
                    label: "BSB".into(),
                    name: "Berean Standard Bible".into(),
                    remote: false,
                },
                SourceEntry {
                    key: "KJV".into(),
                    label: "KJV".into(),
                    name: "King James Version".into(),
                    remote: false,
                },
            ],
            translation: "BSB",
            appearance: Appearance::System,
            font_pairing: FontPairing::Classic,
            active_pinned: false,
            split: false,
            search: &search,
            suggestions: vec!["pride", "fear not"],
        };
        f(build(query, &cx))
    }

    fn headings(groups: &[Group]) -> Vec<&str> {
        groups.iter().map(|g| g.heading.as_str()).collect()
    }

    #[test]
    fn empty_query_shows_tabs_recent_commands() {
        with("", |groups| {
            assert_eq!(
                headings(&groups),
                ["Tabs", "Recent", "Try searching", "Commands", "Appearance"]
            );
            // The active tab and the current chapter are left out.
            assert_eq!(groups[0].rows.len(), 1);
            assert_eq!(groups[1].rows[0].label(), "Psalms 23");
            assert_eq!(groups[1].rows.len(), 1);
        });
    }

    #[test]
    fn appearance_rows_come_after_verses() {
        with("dark", |groups| {
            let h = headings(&groups);
            assert_eq!(h.last(), Some(&"Appearance"));
            if let Some(v) = h.iter().position(|x| *x == "Verses") {
                assert!(v < h.len() - 1);
            }
        });
        with("lora", |groups| {
            let last = groups.last().unwrap();
            assert_eq!(last.heading, "Appearance");
            assert!(last.rows[0].label().starts_with("Font: Editorial"));
        });
    }

    #[test]
    fn reference_goes_first_and_skips_search() {
        with("jn 3:16", |groups| {
            assert_eq!(headings(&groups)[0], "Go to");
            assert_eq!(groups[0].rows[0].label(), "Go to John 3:16");
            assert!(!headings(&groups).contains(&"Verses"));
        });
    }

    #[test]
    fn words_search_verses_and_commands() {
        with("love", |groups| {
            assert_eq!(headings(&groups), ["Topics", "Verses"]);
            assert_eq!(groups[0].rows[0].label(), "Topic: love · 9 passages");
            // The last verse row opens everything on the search page.
            let last = groups[1].rows.last().unwrap();
            assert_eq!(last.label(), "See all 42 results for “love”");
        });
        with("sepia", |groups| {
            assert_eq!(headings(&groups), ["Verses", "Appearance"]);
            assert_eq!(groups[1].rows[0].label(), "Theme: Sepia");
        });
        with("king james", |groups| {
            assert_eq!(
                groups[0].rows[0].label(),
                "Switch to KJV (King James Version)"
            );
        });
    }

    #[test]
    fn prefix_shows_only_commands() {
        with(">tab", |groups| {
            assert_eq!(headings(&groups), ["Commands"]);
            let labels: Vec<_> = groups[0].rows.iter().map(Row::label).collect();
            assert_eq!(
                labels,
                [
                    "New Tab",
                    "Reopen Closed Tab",
                    "Close Tab",
                    "Pin Tab",
                    "Rename Tab…"
                ]
            );
        });
    }

    #[test]
    fn scope_words_limit_the_search() {
        let (scope, rest) = split_scope("love in:nt other");
        assert_eq!(scope.unwrap().books, 40..=66);
        assert_eq!(rest, "love other");
        let (scope, _) = split_scope("grace IN:1cor");
        let scope = scope.unwrap();
        assert_eq!(
            (scope.label.as_str(), scope.books),
            ("1 Corinthians", 46..=46)
        );
        // Unknown scopes are ordinary words.
        let (scope, rest) = split_scope("in:zzz love");
        assert!(scope.is_none());
        assert_eq!(rest, "in:zzz love");

        with("love in:john", |groups| {
            assert_eq!(headings(&groups), ["Verses in John"]);
            let Row::Verse(hit) = &groups[0].rows[0] else {
                panic!("verse row")
            };
            assert_eq!(hit.verse.book(), 43);
        });
        with("in:nt", |groups| assert!(groups.is_empty()));
    }

    #[test]
    fn tab_rows_open_passages_only_when_they_are_passages() {
        with("jn 3:16", |groups| {
            assert!(groups[0].rows[0].passage().is_some());
        });
        with("", |groups| {
            assert!(groups[0].rows[0].passage().is_none(), "tab row");
            assert!(groups[1].rows[0].passage().is_some(), "recent row");
        });
    }
}
