//! SQLite storage for translations.
//!
//! One read-only database file per translation, produced by `bible-import`.

use std::ops::RangeInclusive;
use std::path::Path;

use anyhow::{Context as _, Result};
use bible_core::{Block, ChapterRef, Note, NoteKind, VerseId};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

pub const SCHEMA_VERSION: &str = "1";

const SCHEMA: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE chapter(
    book INTEGER NOT NULL,
    chapter INTEGER NOT NULL,
    content TEXT NOT NULL,               -- JSON Vec<Block>
    PRIMARY KEY(book, chapter)
) WITHOUT ROWID;
CREATE TABLE verse(
    id INTEGER PRIMARY KEY,              -- VerseId (BBCCCVVV)
    end_verse INTEGER,                   -- set for merged ranges
    text TEXT NOT NULL
);
CREATE TABLE note(
    id INTEGER PRIMARY KEY,
    verse_id INTEGER NOT NULL,
    kind TEXT NOT NULL,                  -- footnote | cross_reference
    text TEXT NOT NULL
);
CREATE VIRTUAL TABLE verse_fts USING fts5(text, content='verse', content_rowid='id');
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationInfo {
    pub abbrev: String,
    pub name: String,
    pub copyright: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub verse: VerseId,
    pub text: String,
}

/// Writes a new translation database. Used only by the importer.
pub struct TranslationWriter {
    conn: Connection,
}

impl TranslationWriter {
    pub fn create(path: &Path, info: &TranslationInfo) -> Result<Self> {
        if path.exists() {
            std::fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;")?;
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch("BEGIN")?;
        for (key, value) in [
            ("schema_version", SCHEMA_VERSION),
            ("abbrev", info.abbrev.as_str()),
            ("name", info.name.as_str()),
            ("copyright", info.copyright.as_str()),
        ] {
            conn.execute("INSERT INTO meta VALUES (?1, ?2)", params![key, value])?;
        }
        Ok(Self { conn })
    }

    pub fn add_chapter(&self, chapter: ChapterRef, blocks: &[Block]) -> Result<()> {
        let json = serde_json::to_string(blocks)?;
        self.conn.execute(
            "INSERT INTO chapter VALUES (?1, ?2, ?3)",
            params![chapter.book, chapter.chapter, json],
        )?;
        Ok(())
    }

    pub fn add_verse(&self, id: VerseId, end_verse: Option<u16>, text: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO verse VALUES (?1, ?2, ?3)",
            params![id.0, end_verse, text],
        )?;
        Ok(())
    }

    pub fn add_note(&self, note: &Note) -> Result<()> {
        let kind = match note.kind {
            NoteKind::Footnote => "footnote",
            NoteKind::CrossReference => "cross_reference",
        };
        self.conn.execute(
            "INSERT INTO note VALUES (?1, ?2, ?3, ?4)",
            params![note.id, note.verse.0, kind, note.text],
        )?;
        Ok(())
    }

    pub fn finish(self) -> Result<()> {
        self.conn.execute_batch(
            "INSERT INTO verse_fts(verse_fts) VALUES ('rebuild');
             COMMIT;
             VACUUM;",
        )?;
        Ok(())
    }
}

/// Read-only handle to one translation database.
pub struct Translation {
    conn: Connection,
    pub info: TranslationInfo,
}

impl Translation {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("opening {}", path.display()))?;
        let meta = |key: &str| -> Result<String> {
            conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
                .with_context(|| format!("missing meta key {key}"))
        };
        let info = TranslationInfo {
            abbrev: meta("abbrev")?,
            name: meta("name")?,
            copyright: meta("copyright")?,
        };
        Ok(Self { conn, info })
    }

    pub fn chapter(&self, chapter: ChapterRef) -> Result<Option<Vec<Block>>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT content FROM chapter WHERE book = ?1 AND chapter = ?2",
                params![chapter.book, chapter.chapter],
                |r| r.get(0),
            )
            .optional()?;
        json.map(|j| serde_json::from_str(&j).map_err(Into::into))
            .transpose()
    }

    pub fn verse_text(&self, id: VerseId) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT text FROM verse WHERE id = ?1", [id.0], |r| r.get(0))
            .optional()?)
    }

    pub fn notes_for_chapter(&self, chapter: ChapterRef) -> Result<Vec<Note>> {
        let lo = VerseId::new(chapter.book, chapter.chapter, 0).0;
        let hi = VerseId::new(chapter.book, chapter.chapter, 999).0;
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, verse_id, kind, text FROM note WHERE verse_id BETWEEN ?1 AND ?2 ORDER BY id",
        )?;
        let rows = stmt.query_map([lo, hi], |r| {
            let kind: String = r.get(2)?;
            Ok(Note {
                id: r.get(0)?,
                verse: VerseId(r.get(1)?),
                kind: if kind == "cross_reference" {
                    NoteKind::CrossReference
                } else {
                    NoteKind::Footnote
                },
                text: r.get(3)?,
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(Into::into)
    }

    /// Full-text search. Each whitespace-separated word must appear;
    /// the last word matches as a prefix so results show while typing.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        self.search_in(query, 1..=u8::MAX, limit)
    }

    /// Like [`search`](Self::search), limited to books `books` (canonical
    /// numbers, inclusive). Verse ids sort by book, so this is a range scan.
    pub fn search_in(
        &self,
        query: &str,
        books: RangeInclusive<u8>,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let Some(fts_query) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let lo = VerseId::new(*books.start(), 0, 0).0;
        let hi = VerseId::new(*books.end(), 999, 999).0;
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, text FROM verse
             WHERE id BETWEEN ?2 AND ?3
               AND id IN (SELECT rowid FROM verse_fts WHERE verse_fts MATCH ?1)
             ORDER BY id LIMIT ?4",
        )?;
        let rows = stmt.query_map(params![fts_query, lo, hi, limit as i64], |r| {
            Ok(SearchHit {
                verse: VerseId(r.get(0)?),
                text: r.get(1)?,
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(Into::into)
    }

    pub fn verse_count(&self) -> Result<u32> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM verse", [], |r| r.get(0))?)
    }
}

/// Turn free text into a safe FTS5 query: quoted terms, last one as prefix.
fn fts_query(input: &str) -> Option<String> {
    let words: Vec<String> = input
        .split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric() || *c == '\'')
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect();
    let (last, rest) = words.split_last()?;
    let mut parts: Vec<String> = rest.iter().map(|w| format!("\"{w}\"")).collect();
    parts.push(format!("\"{last}\"*"));
    Some(parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bible_core::{BlockKind, Span, TextStyle};

    fn sample_db(dir: &Path) -> Translation {
        let path = dir.join("t.sqlite");
        let info = TranslationInfo {
            abbrev: "TST".into(),
            name: "Test".into(),
            copyright: "Public domain".into(),
        };
        let w = TranslationWriter::create(&path, &info).unwrap();
        let mut block = Block::new(BlockKind::Paragraph);
        block.spans.push(Span::Verse { num: 16, end: None });
        block.push_text("For God so loved the world", TextStyle::default());
        w.add_chapter(ChapterRef::new(43, 3), &[block]).unwrap();
        w.add_verse(VerseId::new(43, 3, 16), None, "For God so loved the world")
            .unwrap();
        w.add_verse(VerseId::new(1, 1, 1), None, "In the beginning God created")
            .unwrap();
        w.add_note(&Note {
            id: 1,
            kind: NoteKind::Footnote,
            verse: VerseId::new(43, 3, 16),
            text: "Or only begotten".into(),
        })
        .unwrap();
        w.finish().unwrap();
        Translation::open(&path).unwrap()
    }

    #[test]
    fn round_trip_and_search() {
        let dir = std::env::temp_dir().join(format!("bible-store-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let t = sample_db(&dir);

        assert_eq!(t.info.abbrev, "TST");
        let blocks = t.chapter(ChapterRef::new(43, 3)).unwrap().unwrap();
        assert_eq!(blocks[0].kind, BlockKind::Paragraph);
        assert!(t.chapter(ChapterRef::new(43, 4)).unwrap().is_none());

        let hits = t.search("god lov", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].verse, VerseId::new(43, 3, 16));
        assert_eq!(t.search("god", 10).unwrap().len(), 2);
        assert!(t.search("  \"*", 10).unwrap().is_empty());
        // Scoped to the New Testament (books 40..=66) or one book.
        assert_eq!(t.search_in("god", 40..=66, 10).unwrap().len(), 1);
        assert_eq!(t.search_in("god", 1..=1, 10).unwrap().len(), 1);
        assert!(t.search_in("god", 2..=39, 10).unwrap().is_empty());

        let notes = t.notes_for_chapter(ChapterRef::new(43, 3)).unwrap();
        assert_eq!(notes.len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
