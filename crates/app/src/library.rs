//! Shared app-wide state: open translations, persisted settings, last position.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use bible_core::{ChapterRef, VerseId};
use bible_store::Translation;
use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};

/// All translations found in the data directory, in display order.
pub struct Library {
    pub translations: Vec<Translation>,
}

impl Global for Library {}

const PREFERRED_ORDER: &[&str] = &["BSB", "KJV", "WEB"];

impl Library {
    pub fn load(dir: &Path) -> Result<Self> {
        let mut translations = Vec::new();
        for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "sqlite") {
                translations.push(Translation::open(&path)?);
            }
        }
        if translations.is_empty() {
            bail!(
                "no translations in {}. Run `cargo run -p bible-import` first.",
                dir.display()
            );
        }
        translations.sort_by_key(|t| {
            PREFERRED_ORDER
                .iter()
                .position(|a| *a == t.info.abbrev)
                .unwrap_or(usize::MAX)
        });
        Ok(Self { translations })
    }

    pub fn index_of(&self, abbrev: &str) -> Option<usize> {
        self.translations
            .iter()
            .position(|t| t.info.abbrev == abbrev)
    }
}

/// Where translation databases live. `BIBLE_READER_DATA` overrides; a bundled
/// app uses `Contents/Resources/data`; dev builds use `data/build` in the repo.
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("BIBLE_READER_DATA") {
        return dir.into();
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(resources) = exe
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("Resources/data"))
        && resources.is_dir()
    {
        return resources;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/build")
}

/// Settings and reading position persisted between launches.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub translation: String,
    pub book: u8,
    pub chapter: u16,
    pub font_size: f32,
    pub red_letter: bool,
    pub show_verse_numbers: bool,
    pub appearance: Appearance,
    /// Pre-`appearance` settings files stored this; read once to migrate.
    #[serde(skip_serializing)]
    dark: Option<bool>,
    pub sidebar_open: bool,
    /// Recently read chapters, newest first, as (book, chapter).
    pub recent: Vec<(u8, u16)>,
    /// Open reading tabs, restored on launch. `book` / `chapter` /
    /// `translation` above mirror the active one (the menu bar reads them).
    pub tabs: Vec<SavedTab>,
    pub active_tab: usize,
}

/// One reading tab as stored in settings.json.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTab {
    pub book: u8,
    pub chapter: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verse: Option<u16>,
    pub translation: String,
}

/// Reading theme. `System` follows macOS light / dark.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Sepia,
    Dark,
}

impl Appearance {
    pub const ALL: [Appearance; 4] = [Self::System, Self::Light, Self::Sepia, Self::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Match System",
            Self::Light => "Light",
            Self::Sepia => "Sepia",
            Self::Dark => "Dark",
        }
    }
}

const RECENT_LIMIT: usize = 12;

impl Default for Settings {
    fn default() -> Self {
        Self {
            translation: "BSB".into(),
            book: 43,
            chapter: 1,
            font_size: 19.0,
            red_letter: true,
            show_verse_numbers: true,
            appearance: Appearance::System,
            dark: None,
            sidebar_open: true,
            recent: Vec::new(),
            tabs: Vec::new(),
            active_tab: 0,
        }
    }
}

impl Global for Settings {}

impl Settings {
    fn path() -> Option<PathBuf> {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Library/Application Support/BibleReader/settings.json"))
    }

    pub fn load() -> Self {
        let mut settings: Self = Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if let Some(dark) = settings.dark.take()
            && settings.appearance == Appearance::System
        {
            settings.appearance = if dark {
                Appearance::Dark
            } else {
                Appearance::Light
            };
        }
        settings
    }

    /// Move `chapter` to the front of the recent list.
    pub fn push_recent(&mut self, chapter: ChapterRef) {
        let key = (chapter.book, chapter.chapter);
        self.recent.retain(|r| *r != key);
        self.recent.insert(0, key);
        self.recent.truncate(RECENT_LIMIT);
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    pub fn chapter_ref(&self) -> ChapterRef {
        let candidate = ChapterRef::new(self.book, self.chapter);
        match bible_core::book_by_num(self.book) {
            Some(b) if self.chapter >= 1 && self.chapter <= b.chapters => candidate,
            _ => ChapterRef::new(43, 1),
        }
    }
}

/// Curated verse-of-the-day list; one is picked per calendar day.
const VERSES_OF_THE_DAY: &[(u8, u16, u16)] = &[
    (43, 3, 16),
    (19, 23, 1),
    (20, 3, 5),
    (45, 8, 28),
    (50, 4, 13),
    (24, 29, 11),
    (23, 40, 31),
    (6, 1, 9),
    (40, 11, 28),
    (45, 12, 2),
    (48, 5, 22),
    (19, 46, 1),
    (49, 2, 8),
    (43, 14, 6),
    (58, 11, 1),
    (60, 5, 7),
    (19, 119, 105),
    (25, 3, 22),
    (40, 6, 33),
    (45, 15, 13),
    (47, 5, 17),
    (59, 1, 5),
    (62, 4, 19),
    (19, 37, 4),
    (23, 41, 10),
    (50, 4, 6),
    (40, 5, 16),
    (43, 16, 33),
    (19, 27, 1),
    (5, 31, 6),
    (46, 13, 4),
    (51, 3, 23),
    (55, 1, 7),
    (58, 12, 1),
    (20, 16, 3),
    (19, 139, 14),
    (33, 6, 8),
    (45, 5, 8),
    (43, 1, 1),
    (19, 121, 1),
    (23, 26, 3),
    (40, 28, 20),
];

pub fn verse_of_the_day() -> VerseId {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0);
    let (b, c, v) = VERSES_OF_THE_DAY[(days as usize) % VERSES_OF_THE_DAY.len()];
    VerseId::new(b, c, v)
}

pub fn current_translation(cx: &App) -> &Translation {
    let library = cx.global::<Library>();
    let settings = cx.global::<Settings>();
    let ix = library.index_of(&settings.translation).unwrap_or(0);
    &library.translations[ix]
}
