//! ⌘K / ⌘P palette contents: go to a passage, switch tabs, reopen recent
//! chapters, run commands, and full-text search, grouped like Spotlight.
//!
//! Row building is plain data (`build`) so it can be unit tested. Every row
//! carries the Action it runs; `ReaderView` renders the rows with gpui-kit's
//! `Command` palette, which handles arrow keys, Enter and shortcut hints.

use bible_core::{ChapterRef, Reference, VerseId, parse_reference};
use bible_store::SearchHit;
use gpui_kit::Action;

use crate::library::Appearance;
use crate::{
    CloseTab, DecreaseFont, IncreaseFont, NewTab, ReopenClosedTab, ResetFont, SelectTranslation,
    SetAppearance, ToggleInspector, ToggleRedLetter, ToggleSidebar, ToggleVerseNumbers,
};

pub const SEARCH_LIMIT: usize = 100;
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
}

impl Row {
    pub fn label(&self) -> String {
        match self {
            Row::Goto(r) => format!("Go to {r}"),
            Row::Tab { title, .. } => title.clone(),
            Row::Recent(c) => c.to_string(),
            Row::Command { label, .. } => label.clone(),
            Row::Verse(hit) => format!("{} {}", hit.verse, hit.text),
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
        }
    }
}

pub struct Group {
    pub heading: &'static str,
    pub rows: Vec<Row>,
}

/// What the palette needs to know about the app.
pub struct Context<'a> {
    pub current: ChapterRef,
    pub active_tab: usize,
    /// Titles of all open tabs, e.g. "John 3 · BSB".
    pub tabs: Vec<String>,
    pub recent: &'a [(u8, u16)],
    /// (abbrev, full name) in toolbar order.
    pub translations: Vec<(String, String)>,
    pub translation: &'a str,
    pub appearance: Appearance,
    pub search: &'a dyn Fn(&str) -> Vec<SearchHit>,
}

pub fn build(query: &str, cx: &Context) -> Vec<Group> {
    let query = query.trim();
    let mut groups = Vec::new();
    let mut push = |heading, rows: Vec<Row>| {
        if !rows.is_empty() {
            groups.push(Group { heading, rows });
        }
    };

    if let Some(rest) = query.strip_prefix(COMMAND_PREFIX) {
        push("Commands", commands(rest.trim(), cx));
        return groups;
    }

    if query.is_empty() {
        push("Tabs", tabs(query, cx));
        push(
            "Recent",
            cx.recent
                .iter()
                .map(|(b, c)| ChapterRef::new(*b, *c))
                .filter(|c| *c != cx.current)
                .take(RECENT_SHOWN)
                .map(Row::Recent)
                .collect(),
        );
        push("Commands", commands("", cx));
        return groups;
    }

    let reference = parse_reference(query);
    push("Go to", reference.into_iter().map(Row::Goto).collect());
    push("Tabs", tabs(query, cx));
    push(
        "Commands",
        commands(query, cx).into_iter().take(6).collect(),
    );
    // "jn 3" is a reference, not words to search for; "rom" might be either.
    let looks_like_reference = reference.is_some() && query.chars().any(|c| c.is_ascii_digit());
    if query.chars().count() >= 2 && !looks_like_reference {
        push(
            "Verses",
            (cx.search)(query).into_iter().map(Row::Verse).collect(),
        );
    }
    groups
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
    ];
    for (index, (abbrev, name)) in cx.translations.iter().enumerate() {
        if abbrev != cx.translation {
            all.push((
                format!("Switch to {abbrev} ({name})"),
                &["translation", "version", "bible"],
                Box::new(SelectTranslation { index }),
            ));
        }
    }
    for appearance in Appearance::ALL {
        if appearance != cx.appearance {
            all.push((
                format!("Theme: {}", appearance.label()),
                &["appearance", "color", "mode"],
                Box::new(SetAppearance { appearance }),
            ));
        }
    }
    all.into_iter()
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
    use super::*;

    fn with<R>(query: &str, f: impl FnOnce(Vec<Group>) -> R) -> R {
        let search = |q: &str| {
            vec![SearchHit {
                verse: VerseId::new(46, 13, 4),
                text: format!("hit for {q}"),
            }]
        };
        let recent = [(19, 23), (43, 3)];
        let cx = Context {
            current: ChapterRef::new(43, 3),
            active_tab: 0,
            tabs: vec!["John 3 · BSB".into(), "Psalms 23 · KJV".into()],
            recent: &recent,
            translations: vec![
                ("BSB".into(), "Berean Standard Bible".into()),
                ("KJV".into(), "King James Version".into()),
            ],
            translation: "BSB",
            appearance: Appearance::System,
            search: &search,
        };
        f(build(query, &cx))
    }

    fn headings(groups: &[Group]) -> Vec<&'static str> {
        groups.iter().map(|g| g.heading).collect()
    }

    #[test]
    fn empty_query_shows_tabs_recent_commands() {
        with("", |groups| {
            assert_eq!(headings(&groups), ["Tabs", "Recent", "Commands"]);
            // The active tab and the current chapter are left out.
            assert_eq!(groups[0].rows.len(), 1);
            assert_eq!(groups[1].rows[0].label(), "Psalms 23");
            assert_eq!(groups[1].rows.len(), 1);
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
            assert_eq!(headings(&groups), ["Verses"]);
        });
        with("sepia", |groups| {
            assert_eq!(headings(&groups)[0], "Commands");
            assert_eq!(groups[0].rows[0].label(), "Theme: Sepia");
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
            assert_eq!(labels, ["New Tab", "Reopen Closed Tab", "Close Tab"]);
        });
    }
}
