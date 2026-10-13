//! The user's own data: saved verses with notes and tags, in `user.db`.
//!
//! Read-write (unlike the translation databases). Items are keyed by chapter
//! and verse numbers, so highlights show in every translation. The text is a
//! copy made when the verse was saved, in the translation it was saved from.

use std::path::Path;

use anyhow::Result;
use bible_core::{ChapterRef, VerseSet};
use rusqlite::{Connection, OptionalExtension, params};

const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS saved(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    book INTEGER NOT NULL,
    chapter INTEGER NOT NULL,
    verses TEXT NOT NULL,                -- \"1-4,28\"
    translation TEXT NOT NULL,           -- \"BSB\" or \"yv:111\"
    version TEXT NOT NULL,               -- label shown: \"NIV\"
    text TEXT NOT NULL,                  -- copy of the verses when saved
    notice TEXT NOT NULL DEFAULT '',     -- copyright notice for exports
    note TEXT NOT NULL DEFAULT '',
    position INTEGER NOT NULL,           -- manual order, ascending
    created_at INTEGER NOT NULL,         -- unix seconds
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS saved_chapter ON saved(book, chapter);
CREATE TABLE IF NOT EXISTS saved_tag(
    saved_id INTEGER NOT NULL REFERENCES saved(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY(saved_id, tag)
);
";

/// One saved item: any set of verses in one chapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedVerse {
    pub id: i64,
    pub chapter: ChapterRef,
    pub verses: VerseSet,
    /// Translation key it was saved from.
    pub translation: String,
    /// Short version label ("NIV").
    pub version: String,
    pub text: String,
    /// Copyright notice to print with the text; empty for public domain.
    pub notice: String,
    pub note: String,
    pub tags: Vec<String>,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl SavedVerse {
    /// "Romans 8:1–4, 28"
    pub fn label(&self) -> String {
        self.verses.label(self.chapter)
    }
}

/// What the reader knows when the user saves verses.
#[derive(Debug, Clone, Default)]
pub struct NewSaved {
    pub chapter: Option<ChapterRef>,
    pub verses: VerseSet,
    pub translation: String,
    pub version: String,
    pub text: String,
    pub notice: String,
}

pub struct UserStore {
    conn: Connection,
}

impl UserStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL;")?;
        Self::init(conn)
    }

    /// For tests and when there is no home directory.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        conn.execute(
            "INSERT OR IGNORE INTO meta(key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;
        Ok(Self { conn })
    }

    /// Every item in manual order.
    pub fn all(&self) -> Result<Vec<SavedVerse>> {
        let mut tags = self
            .conn
            .prepare("SELECT saved_id, tag FROM saved_tag ORDER BY rowid")?;
        let mut by_item: std::collections::HashMap<i64, Vec<String>> = Default::default();
        for row in tags.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (id, tag) = row?;
            by_item.entry(id).or_default().push(tag);
        }
        let mut stmt = self.conn.prepare(
            "SELECT id, book, chapter, verses, translation, version, text, notice, note,
                    position, created_at, updated_at
             FROM saved ORDER BY position, id",
        )?;
        let items = stmt
            .query_map([], |r| {
                Ok(SavedVerse {
                    id: r.get(0)?,
                    chapter: ChapterRef::new(r.get(1)?, r.get(2)?),
                    verses: VerseSet::parse(&r.get::<_, String>(3)?),
                    translation: r.get(4)?,
                    version: r.get(5)?,
                    text: r.get(6)?,
                    notice: r.get(7)?,
                    note: r.get(8)?,
                    tags: Vec::new(),
                    position: r.get(9)?,
                    created_at: r.get(10)?,
                    updated_at: r.get(11)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(items
            .into_iter()
            .map(|mut item| {
                item.tags = by_item.remove(&item.id).unwrap_or_default();
                item
            })
            .collect())
    }

    /// Save new verses at the top of the manual order.
    pub fn insert(&self, new: &NewSaved) -> Result<SavedVerse> {
        let chapter = new.chapter.ok_or_else(|| anyhow::anyhow!("no chapter"))?;
        let top: Option<i64> = self
            .conn
            .query_row("SELECT MIN(position) FROM saved", [], |r| r.get(0))
            .optional()?
            .flatten();
        let position = top.map_or(0, |p| p - 1);
        let now = now();
        self.conn.execute(
            "INSERT INTO saved(book, chapter, verses, translation, version, text, notice,
                               position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                chapter.book,
                chapter.chapter,
                new.verses.to_storage(),
                new.translation,
                new.version,
                new.text,
                new.notice,
                position,
                now
            ],
        )?;
        Ok(SavedVerse {
            id: self.conn.last_insert_rowid(),
            chapter,
            verses: new.verses.clone(),
            translation: new.translation.clone(),
            version: new.version.clone(),
            text: new.text.clone(),
            notice: new.notice.clone(),
            note: String::new(),
            tags: Vec::new(),
            position,
            created_at: now,
            updated_at: now,
        })
    }

    /// Write back the note and tags of an item.
    pub fn update_note(&self, id: i64, note: &str, tags: &[String]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE saved SET note = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, note.trim(), now()],
        )?;
        tx.execute("DELETE FROM saved_tag WHERE saved_id = ?1", params![id])?;
        for tag in normalize_tags(tags) {
            tx.execute(
                "INSERT OR IGNORE INTO saved_tag(saved_id, tag) VALUES (?1, ?2)",
                params![id, tag],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM saved WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Put a removed item back exactly as it was (Undo).
    pub fn restore(&self, item: &SavedVerse) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO saved(id, book, chapter, verses, translation, version, text,
                                          notice, note, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                item.id,
                item.chapter.book,
                item.chapter.chapter,
                item.verses.to_storage(),
                item.translation,
                item.version,
                item.text,
                item.notice,
                item.note,
                item.position,
                item.created_at,
                item.updated_at
            ],
        )?;
        for tag in &item.tags {
            tx.execute(
                "INSERT OR IGNORE INTO saved_tag(saved_id, tag) VALUES (?1, ?2)",
                params![item.id, tag],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Set the manual order: `ids` first to last. Ids not listed keep their
    /// place after them.
    pub fn reorder(&self, ids: &[i64]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for (position, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE saved SET position = ?2 WHERE id = ?1",
                params![id, position as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

/// Trimmed, single-spaced, without empty or repeated (ignoring case) tags.
pub fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag.split_whitespace().collect::<Vec<_>>().join(" ");
        let tag = tag.trim_start_matches('#').to_string();
        if !tag.is_empty() && !out.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
            out.push(tag);
        }
    }
    out
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(book: u8, chapter: u16, verses: &str) -> NewSaved {
        NewSaved {
            chapter: Some(ChapterRef::new(book, chapter)),
            verses: VerseSet::parse(verses),
            translation: "yv:111".into(),
            version: "NIV".into(),
            text: "Some words.".into(),
            notice: "Copyright Biblica.".into(),
        }
    }

    #[test]
    fn insert_note_tags_and_order() {
        let store = UserStore::open_in_memory().unwrap();
        let a = store.insert(&new(45, 8, "28")).unwrap();
        let b = store.insert(&new(50, 4, "6-7")).unwrap();
        // Newest first.
        let all = store.all().unwrap();
        assert_eq!(
            all.iter().map(|i| i.label()).collect::<Vec<_>>(),
            vec!["Philippians 4:6–7", "Romans 8:28"]
        );
        assert_eq!(all[0].version, "NIV");
        assert_eq!(all[0].notice, "Copyright Biblica.");

        store
            .update_note(
                a.id,
                "  For hard days ",
                &["Memory".into(), " #memory".into(), "  Hope   now ".into()],
            )
            .unwrap();
        let all = store.all().unwrap();
        let saved_a = all.iter().find(|i| i.id == a.id).unwrap();
        assert_eq!(saved_a.note, "For hard days");
        assert_eq!(saved_a.tags, vec!["Memory", "Hope now"]);

        store.reorder(&[a.id, b.id]).unwrap();
        let order: Vec<i64> = store.all().unwrap().iter().map(|i| i.id).collect();
        assert_eq!(order, vec![a.id, b.id]);
    }

    #[test]
    fn remove_and_restore() {
        let store = UserStore::open_in_memory().unwrap();
        let a = store.insert(&new(43, 3, "16")).unwrap();
        store.update_note(a.id, "note", &["Gospel".into()]).unwrap();
        let before = store.all().unwrap();
        store.remove(a.id).unwrap();
        assert!(store.all().unwrap().is_empty());
        store.restore(&before[0]).unwrap();
        assert_eq!(store.all().unwrap(), before);
    }

    #[test]
    fn reopening_keeps_data() {
        let dir = std::env::temp_dir().join(format!("bible-user-{}", std::process::id()));
        let path = dir.join("user.db");
        let _ = std::fs::remove_file(&path);
        {
            let store = UserStore::open(&path).unwrap();
            store.insert(&new(19, 23, "1-3")).unwrap();
        }
        let store = UserStore::open(&path).unwrap();
        assert_eq!(store.all().unwrap()[0].label(), "Psalms 23:1–3");
        let _ = std::fs::remove_dir_all(dir);
    }
}
