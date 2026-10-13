//! Key bindings and the catalog of shortcuts shown in the Shortcuts window.
//!
//! Nothing here is macOS-specific. Bindings use gpui's `secondary` modifier
//! (⌘ on macOS, Ctrl elsewhere), and `Kbd` draws each key for the platform it
//! runs on (⌘⇧P or Ctrl+Shift+P). A port only has to review the `cfg` blocks
//! in `bind` and the `mac_only` rows in `SECTIONS`.
//!
//! The test at the bottom checks that `SECTIONS` and `bind` agree, so the
//! window can't list a shortcut that doesn't work or miss one that does.

use gpui_kit::*;

use crate::palette::OpenSearchPage;
use crate::*;

/// One row of the Shortcuts window.
pub struct Shortcut {
    pub label: &'static str,
    /// Alternatives, in gpui keystroke syntax (`secondary-k`).
    pub keys: &'static [&'static str],
    /// Bound only on macOS (it uses ⌃⌘, which has no equivalent elsewhere).
    pub mac_only: bool,
    /// Handled by a component (text selection, list navigation), not by
    /// `bind`, so the consistency test skips it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub builtin: bool,
}

const fn row(label: &'static str, keys: &'static [&'static str]) -> Shortcut {
    Shortcut {
        label,
        keys,
        mac_only: false,
        builtin: false,
    }
}

const fn mac(label: &'static str, keys: &'static [&'static str]) -> Shortcut {
    Shortcut {
        mac_only: true,
        ..row(label, keys)
    }
}

const fn builtin(label: &'static str, keys: &'static [&'static str]) -> Shortcut {
    Shortcut {
        builtin: true,
        ..row(label, keys)
    }
}

pub struct Section {
    pub title: &'static str,
    pub rows: &'static [Shortcut],
}

impl Shortcut {
    /// Whether this shortcut exists on the platform we're running on.
    pub fn available(&self) -> bool {
        !self.mac_only || cfg!(target_os = "macos")
    }
}

pub const SECTIONS: &[Section] = &[
    Section {
        title: "Find and go to",
        rows: &[
            row(
                "Palette: passages, search, tabs, commands",
                &["secondary-k", "secondary-p", "secondary-l", "secondary-f"],
            ),
            row("Commands only (or type >)", &["secondary-shift-p"]),
            row("Search results page", &["secondary-shift-f"]),
            row("Previous chapter", &["left", "secondary-[", "alt-left"]),
            row("Next chapter", &["right", "secondary-]", "alt-right"]),
            mac("Back in history", &["ctrl-cmd-left"]),
            mac("Forward in history", &["ctrl-cmd-right"]),
        ],
    },
    Section {
        title: "Tabs",
        rows: &[
            row("New tab", &["secondary-t"]),
            row("Close tab", &["secondary-w"]),
            row("Reopen closed tab", &["secondary-shift-t"]),
            row("Next tab", &["ctrl-tab", "secondary-}"]),
            row("Previous tab", &["ctrl-shift-tab", "secondary-{"]),
            row("Close window", &["secondary-shift-w"]),
        ],
    },
    Section {
        title: "Split view",
        rows: &[
            row("Split right, or unsplit", &["secondary-\\"]),
            row("Close pane", &["secondary-shift-\\"]),
            row("Focus left pane", &["secondary-alt-left"]),
            row("Focus right pane", &["secondary-alt-right"]),
        ],
    },
    Section {
        title: "Sidebar",
        rows: &[
            row("Show or hide the sidebar", &["secondary-b"]),
            mac("Show or hide the sidebar (also)", &["cmd-ctrl-s"]),
            row("Focus the sidebar", &["secondary-alt-b"]),
            row(
                "Move through books and chapters",
                &["up", "down", "left", "right"],
            ),
            row("Previous or next book", &["alt-up", "alt-down"]),
            row("Open the chapter", &["enter"]),
            row("Back to the text", &["escape"]),
        ],
    },
    Section {
        title: "Reading",
        rows: &[
            row("Larger text", &["secondary-=", "secondary-+"]),
            row("Smaller text", &["secondary--"]),
            row("Actual size", &["secondary-0"]),
            row(
                "Switch translation",
                &["secondary-1", "secondary-2", "secondary-3", "secondary-4"],
            ),
            row("Show or hide footnotes", &["secondary-alt-n"]),
            row("Dark or light mode", &["secondary-shift-l"]),
        ],
    },
    Section {
        title: "Saved verses",
        rows: &[
            builtin("Select a verse (click its number)", &[]),
            row("Highlight and save the selected verses", &["secondary-d"]),
            row("Clear the verse selection", &["escape"]),
            row("Show saved verses", &["secondary-shift-d"]),
            row("Export saved verses as Markdown", &["secondary-shift-e"]),
            builtin("Move the selected saved verse", &["alt-up", "alt-down"]),
        ],
    },
    Section {
        title: "Copy",
        rows: &[
            builtin("Copy selected text", &["secondary-c"]),
            row(
                "Copy selected verses (or the verse) with citation",
                &["secondary-shift-c"],
            ),
            row("Copy chapter", &["secondary-alt-c"]),
        ],
    },
    Section {
        title: "In the palette and search results",
        rows: &[
            builtin("Move through results", &["up", "down"]),
            builtin("Open", &["enter"]),
            row("Open in a new tab", &["secondary-enter"]),
            row("Open beside the current chapter", &["alt-enter"]),
            row("Close", &["escape"]),
        ],
    },
    Section {
        title: "App",
        rows: &[
            row("Settings", &["secondary-,"]),
            row("Keyboard shortcuts", &["secondary-/"]),
            row("Quit", &["secondary-q"]),
        ],
    },
];

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("secondary-,", OpenSettings, None),
        KeyBinding::new("secondary-/", OpenShortcuts, None),
        KeyBinding::new("secondary-shift-w", CloseWindow, None),
        KeyBinding::new("secondary-t", NewTab, Some("Reader")),
        KeyBinding::new("secondary-w", CloseTab, Some("Reader")),
        KeyBinding::new("secondary-shift-t", ReopenClosedTab, Some("Reader")),
        KeyBinding::new("ctrl-tab", NextTab, Some("Reader")),
        KeyBinding::new("ctrl-shift-tab", PrevTab, Some("Reader")),
        KeyBinding::new("secondary-}", NextTab, Some("Reader")),
        KeyBinding::new("secondary-{", PrevTab, Some("Reader")),
        KeyBinding::new("secondary-p", OpenPalette, Some("Reader")),
        KeyBinding::new("secondary-shift-p", OpenCommands, Some("Reader")),
        KeyBinding::new("secondary-k", OpenPalette, Some("Reader")),
        KeyBinding::new("secondary-l", OpenPalette, Some("Reader")),
        KeyBinding::new("secondary-f", OpenPalette, Some("Reader")),
        KeyBinding::new(
            "secondary-shift-f",
            OpenSearchPage {
                query: String::new(),
            },
            Some("Reader"),
        ),
        KeyBinding::new("escape", CloseSearch, Some("SearchPage")),
        KeyBinding::new("escape", ClosePalette, Some("Palette")),
        KeyBinding::new("secondary-enter", OpenInNewTab, Some("Palette")),
        KeyBinding::new("alt-enter", OpenInSplit, Some("Palette")),
        KeyBinding::new("secondary-\\", ToggleSplit, Some("Reader")),
        KeyBinding::new("secondary-shift-\\", ClosePane, Some("Reader")),
        KeyBinding::new("secondary-alt-left", FocusLeftPane, Some("Reader")),
        KeyBinding::new("secondary-alt-right", FocusRightPane, Some("Reader")),
        KeyBinding::new("secondary-]", NextChapter, Some("Reader")),
        KeyBinding::new("secondary-[", PrevChapter, Some("Reader")),
        KeyBinding::new("alt-right", NextChapter, Some("Reader")),
        KeyBinding::new("alt-left", PrevChapter, Some("Reader")),
        KeyBinding::new("right", NextChapter, Some("Reader")),
        KeyBinding::new("left", PrevChapter, Some("Reader")),
        KeyBinding::new("secondary-b", ToggleSidebar, Some("Reader")),
        KeyBinding::new("secondary-alt-b", FocusSidebar, Some("Reader")),
        // Sidebar chapter picker: arrows move a cursor, Enter opens.
        KeyBinding::new("left", SidebarLeft, Some("Sidebar")),
        KeyBinding::new("right", SidebarRight, Some("Sidebar")),
        KeyBinding::new("up", SidebarUp, Some("Sidebar")),
        KeyBinding::new("down", SidebarDown, Some("Sidebar")),
        KeyBinding::new("alt-up", SidebarPrevBook, Some("Sidebar")),
        KeyBinding::new("alt-down", SidebarNextBook, Some("Sidebar")),
        KeyBinding::new("enter", SidebarOpen, Some("Sidebar")),
        KeyBinding::new("escape", FocusReader, Some("Sidebar")),
        KeyBinding::new("secondary-=", IncreaseFont, Some("Reader")),
        KeyBinding::new("secondary-+", IncreaseFont, Some("Reader")),
        KeyBinding::new("secondary--", DecreaseFont, Some("Reader")),
        KeyBinding::new("secondary-0", ResetFont, Some("Reader")),
        KeyBinding::new("secondary-alt-n", ToggleInspector, Some("Reader")),
        KeyBinding::new("secondary-shift-l", ToggleDark, Some("Reader")),
        KeyBinding::new("secondary-shift-c", CopyVerse, Some("Reader")),
        KeyBinding::new("secondary-alt-c", CopyChapter, Some("Reader")),
        KeyBinding::new("secondary-d", SaveSelection, Some("Reader")),
        KeyBinding::new("escape", ClearSelection, Some("Reader")),
        KeyBinding::new("secondary-shift-d", ShowSaved, Some("Reader")),
        KeyBinding::new("secondary-shift-e", ExportSaved, Some("Reader")),
        KeyBinding::new(
            "secondary-1",
            SelectTranslation { index: 0 },
            Some("Reader"),
        ),
        KeyBinding::new(
            "secondary-2",
            SelectTranslation { index: 1 },
            Some("Reader"),
        ),
        KeyBinding::new(
            "secondary-3",
            SelectTranslation { index: 2 },
            Some("Reader"),
        ),
        KeyBinding::new(
            "secondary-4",
            SelectTranslation { index: 3 },
            Some("Reader"),
        ),
    ]);
    // ⌃⌘ combinations are macOS habits (Safari, Finder); other platforms
    // keep Back / Forward in the menu and palette.
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("ctrl-cmd-left", GoBack, Some("Reader")),
        KeyBinding::new("ctrl-cmd-right", GoForward, Some("Reader")),
        KeyBinding::new("cmd-ctrl-s", ToggleSidebar, Some("Reader")),
    ]);
}

#[cfg(test)]
mod tests {
    use core::prelude::v1::test;

    use super::*;

    fn parse(key: &str) -> Keystroke {
        Keystroke::parse(key).unwrap_or_else(|_| panic!("`{key}` is not a valid keystroke"))
    }

    fn bound_keystrokes(cx: &App) -> Vec<Keystroke> {
        cx.key_bindings()
            .borrow()
            .bindings()
            .filter(|b| b.action().name().starts_with("bible_reader::"))
            .filter_map(|b| b.keystrokes().first().map(|k| k.inner().clone()))
            .collect()
    }

    /// Every key the window shows is bound, and every bound key is shown.
    #[gpui_kit::test]
    fn catalog_matches_bindings(cx: &mut TestAppContext) {
        cx.update(|cx| {
            bind(cx);
            let bound = bound_keystrokes(cx);
            let mut shown = Vec::new();
            for row in SECTIONS
                .iter()
                .flat_map(|s| s.rows)
                .filter(|r| r.available())
            {
                for key in row.keys {
                    let stroke = parse(key);
                    if !row.builtin {
                        assert!(
                            bound.contains(&stroke),
                            "“{}” lists {key}, which isn't bound",
                            row.label
                        );
                    }
                    shown.push(stroke);
                }
            }
            for stroke in &bound {
                assert!(
                    shown.contains(stroke),
                    "{stroke} is bound but missing from SECTIONS"
                );
            }
        });
    }
}
