//! Shared app-wide state: open translations, persisted settings, last position.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use bible_core::{ChapterRef, VerseId};
use bible_store::{Results, SearchRequest, Study, Translation, search};
use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};

/// All translations found in the data directory, in display order.
pub struct Library {
    pub translations: Vec<Translation>,
    /// Topics and cross-references (`study.db`); search still works without.
    pub study: Option<Study>,
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
        let study = Study::open(&dir.join("study.db"))
            .map_err(|err| eprintln!("warning: no study data ({err:#})"))
            .ok();
        Ok(Self {
            translations,
            study,
        })
    }

    /// Ranked search in one translation, blended with topics when available.
    pub fn search(&self, translation: &Translation, request: &SearchRequest) -> Results {
        search(translation, self.study.as_ref(), request)
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
    /// Title / body typeface pairing.
    pub font_pairing: FontPairing,
    /// Line height as a multiple of the font size.
    pub line_height: f32,
    /// Width of the text column in px.
    pub reading_width: f32,
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
    /// Optional YouVersion Platform Bibles (see `remote.rs`).
    pub youversion: YouVersionSettings,
}

/// A YouVersion Bible the user added. Only the label data lives here; the
/// text is fetched on demand and cached for 30 days outside settings.json.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedBible {
    pub id: u32,
    pub abbrev: String,
    pub name: String,
    #[serde(default)]
    pub copyright: String,
    /// USFM codes of the books it has; empty when unknown (older saves).
    #[serde(default)]
    pub books: Vec<String>,
    /// Publisher name and site, shown as a link with the copyright notice
    /// (Biblica's license requires a link containing "Biblica").
    #[serde(default)]
    pub publisher: String,
    #[serde(default)]
    pub publisher_url: String,
}

impl SavedBible {
    pub fn has_book(&self, usfm: &str) -> bool {
        self.books.is_empty() || self.books.iter().any(|b| b.eq_ignore_ascii_case(usfm))
    }
}

/// Settings for the optional YouVersion source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct YouVersionSettings {
    /// App key pasted in Settings. Empty falls back to `YVP_APP_KEY`.
    pub app_key: String,
    /// Language ranges to list Bibles in ("en", "es", "*" for all).
    pub languages: Vec<String>,
    pub bibles: Vec<SavedBible>,
    /// The user signed in, so the session is in the Keychain. Lets us skip
    /// touching the Keychain for people who never signed in.
    pub signed_in: bool,
    /// List Bibles the app isn't licensed for too.
    pub show_unlicensed: bool,
}

impl Default for YouVersionSettings {
    fn default() -> Self {
        Self {
            app_key: String::new(),
            languages: vec!["en".into()],
            bibles: Vec::new(),
            signed_in: false,
            show_unlicensed: false,
        }
    }
}

/// Tab / settings key of a YouVersion Bible: `yv:<id>`.
const REMOTE_PREFIX: &str = "yv:";

pub fn remote_key(id: u32) -> String {
    format!("{REMOTE_PREFIX}{id}")
}

pub fn remote_id(key: &str) -> Option<u32> {
    key.strip_prefix(REMOTE_PREFIX)?.parse().ok()
}

/// One row of the translation pickers: a bundled translation or a
/// YouVersion Bible the user added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    /// What tabs and settings store: the abbreviation, or `yv:<id>`.
    pub key: String,
    pub label: String,
    pub name: String,
    pub remote: bool,
}

/// Bundled translations first, then the user's YouVersion Bibles.
pub fn sources(cx: &App) -> Vec<SourceEntry> {
    let library = cx.global::<Library>();
    let local = library.translations.iter().map(|t| SourceEntry {
        key: t.info.abbrev.clone(),
        label: t.info.abbrev.clone(),
        name: t.info.name.clone(),
        remote: false,
    });
    let remote = cx
        .global::<Settings>()
        .youversion
        .bibles
        .iter()
        .map(|b| SourceEntry {
            key: remote_key(b.id),
            label: b.abbrev.clone(),
            name: b.name.clone(),
            remote: true,
        });
    local.chain(remote).collect()
}

/// The saved YouVersion Bible behind a `yv:<id>` key.
pub fn saved_bible<'a>(settings: &'a Settings, key: &str) -> Option<&'a SavedBible> {
    let id = remote_id(key)?;
    settings.youversion.bibles.iter().find(|b| b.id == id)
}

/// Can this key be shown right now: bundled, or a YouVersion Bible that is
/// still in the user's list?
pub fn is_available(cx: &App, key: &str) -> bool {
    cx.global::<Library>().index_of(key).is_some()
        || saved_bible(cx.global::<Settings>(), key).is_some()
}

/// Short label for a translation key ("KJV", "NIV").
pub fn source_label(cx: &App, key: &str) -> String {
    match saved_bible(cx.global::<Settings>(), key) {
        Some(b) => b.abbrev.clone(),
        None => key.to_string(),
    }
}

/// One reading tab as stored in settings.json.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTab {
    pub book: u8,
    pub chapter: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verse: Option<u16>,
    pub translation: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
    /// Custom tab name; `None` shows the passage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Second pane when the tab is split side by side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Box<SavedTab>>,
    /// The right pane (not the left) is the one navigation applies to.
    #[serde(default, skip_serializing_if = "is_false")]
    pub side_focused: bool,
    /// The Saved verses page rather than a chapter.
    #[serde(default, skip_serializing_if = "is_false")]
    pub saved_page: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
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

    /// Stable id, as used by the settings dropdown.
    pub fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Sepia => "sepia",
            Self::Dark => "dark",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key() == key)
    }

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

/// A curated title + body typeface pairing. Not a free-for-all: each one is
/// a combination that reads well for long text. All but `Classic` ship inside
/// the app (see `fonts.rs`), so they look the same on every Mac.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontPairing {
    /// Iowan Old Style throughout (ships with macOS).
    #[default]
    Classic,
    /// Playfair Display titles, Lora text.
    Editorial,
    /// Outfit titles, Source Serif text.
    Modern,
    /// Inter titles, Literata text (made for long-form reading on screens).
    Clean,
    /// EB Garamond throughout.
    Garamond,
    /// Outfit titles, Inter text, for a sans-serif page.
    Sans,
}

impl FontPairing {
    pub const ALL: [FontPairing; 6] = [
        Self::Classic,
        Self::Editorial,
        Self::Modern,
        Self::Clean,
        Self::Garamond,
        Self::Sans,
    ];

    /// Stable id, as used by the settings dropdown.
    pub fn key(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Editorial => "editorial",
            Self::Modern => "modern",
            Self::Clean => "clean",
            Self::Garamond => "garamond",
            Self::Sans => "sans",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::Editorial => "Editorial",
            Self::Modern => "Modern",
            Self::Clean => "Clean",
            Self::Garamond => "Garamond",
            Self::Sans => "Sans",
        }
    }

    /// Menu / dropdown text: name plus the two typefaces.
    pub fn description(self) -> &'static str {
        match self {
            Self::Classic => "Classic · Iowan Old Style",
            Self::Editorial => "Editorial · Playfair Display + Lora",
            Self::Modern => "Modern · Outfit + Source Serif",
            Self::Clean => "Clean · Inter + Literata",
            Self::Garamond => "Garamond · EB Garamond",
            Self::Sans => "Sans · Outfit + Inter",
        }
    }

    /// Family for chapter text.
    pub fn body(self) -> &'static str {
        match self {
            Self::Classic => "Iowan Old Style",
            Self::Editorial => "Lora",
            Self::Modern => "Source Serif 4",
            Self::Clean => "Literata",
            Self::Garamond => "EB Garamond",
            Self::Sans => "Inter",
        }
    }

    /// Family for verse numbers. They use the OpenType `sups` feature, and Lora
    /// and Outfit only have superscript glyphs for some digits, so those
    /// pairings borrow a family that has all ten.
    pub fn numbers(self) -> &'static str {
        match self {
            Self::Classic => "Iowan Old Style",
            Self::Editorial => "Playfair Display",
            Self::Modern => "Source Serif 4",
            Self::Clean => "Inter",
            Self::Garamond => "EB Garamond",
            Self::Sans => "Inter",
        }
    }

    /// Family for book and chapter titles and section headings.
    pub fn display(self) -> &'static str {
        match self {
            Self::Classic => "Iowan Old Style",
            Self::Editorial => "Playfair Display",
            Self::Modern => "Outfit",
            Self::Clean => "Inter",
            Self::Garamond => "EB Garamond",
            Self::Sans => "Outfit",
        }
    }
}

pub const FONT_SIZE_RANGE: (f32, f32) = (12., 36.);
pub const LINE_HEIGHT_RANGE: (f32, f32) = (1.2, 2.2);
pub const READING_WIDTH_RANGE: (f32, f32) = (480., 1000.);

impl Default for Settings {
    fn default() -> Self {
        Self {
            translation: "BSB".into(),
            book: 43,
            chapter: 1,
            font_size: 19.0,
            font_pairing: FontPairing::default(),
            line_height: 1.65,
            reading_width: 680.,
            red_letter: true,
            show_verse_numbers: true,
            appearance: Appearance::System,
            dark: None,
            sidebar_open: true,
            recent: Vec::new(),
            tabs: Vec::new(),
            active_tab: 0,
            youversion: YouVersionSettings::default(),
        }
    }
}

impl Global for Settings {}

/// `~/Library/Application Support/BibleReader`, outside the repo and the
/// app bundle. Honors `HOME` so automated runs can use a scratch directory.
pub fn support_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/BibleReader"))
}

impl Settings {
    fn path() -> Option<PathBuf> {
        Some(support_dir()?.join("settings.json"))
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
