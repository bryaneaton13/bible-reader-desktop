//! Study data shared by every translation: topics ("pride" -> verses about
//! it), cross-references ("related verses") and a popularity prior used to
//! rank search results. Built by `bible-import` from the openbible.info data
//! (CC-BY) into `study.db`; passages are keyed by `VerseId`, so one file
//! serves all translations.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context as _, Result};
use bible_core::VerseId;
use bible_core::text::{is_stop_word, stem, words};
use rusqlite::{Connection, OpenFlags, params};

const SCHEMA: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE topic(
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    passages INTEGER NOT NULL
);
CREATE TABLE alias(topic_id INTEGER NOT NULL, alias TEXT NOT NULL);
CREATE TABLE topic_passage(
    topic_id INTEGER NOT NULL,
    start INTEGER NOT NULL,              -- VerseId of the first verse
    end INTEGER NOT NULL,                -- VerseId of the last verse (= start for one verse)
    score INTEGER NOT NULL,              -- higher is a better fit
    PRIMARY KEY(topic_id, start, end)
) WITHOUT ROWID;
CREATE TABLE xref(
    from_verse INTEGER NOT NULL,
    to_start INTEGER NOT NULL,
    to_end INTEGER NOT NULL,
    votes INTEGER NOT NULL,
    PRIMARY KEY(from_verse, to_start, to_end)
) WITHOUT ROWID;
CREATE TABLE popularity(verse INTEGER PRIMARY KEY, weight INTEGER NOT NULL);
";

/// Topic matches at least this strong boost their verses in search results.
pub const BOOST_STRENGTH: u32 = 85;

/// Passages shorter than this many verses count verse by verse in rankings.
const MAX_EXPANDED_RANGE: u32 = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topic {
    pub id: u32,
    pub name: String,
    pub passages: u32,
}

/// A topic that matched a query, with how well (0..=100). Strengths from
/// [`BOOST_STRENGTH`] up are sure enough to reorder search results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicMatch {
    pub topic: Topic,
    pub strength: u32,
}

/// One verse or a run of verses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Passage {
    pub start: VerseId,
    pub end: VerseId,
    /// Topic fit or cross-reference votes; bigger is better.
    pub score: u32,
}

impl Passage {
    /// "John 3:16" or "Exodus 20:1-26" (or "Genesis 1:31-2:3" across chapters).
    pub fn label(&self) -> String {
        if self.start == self.end {
            return self.start.to_string();
        }
        if self.start.chapter_ref() == self.end.chapter_ref() {
            format!("{}-{}", self.start, self.end.verse())
        } else if self.start.book() == self.end.book() {
            format!("{}-{}:{}", self.start, self.end.chapter(), self.end.verse())
        } else {
            format!("{} - {}", self.start, self.end)
        }
    }

    /// Verse ids of the passage, capped for very long runs.
    pub fn verses(&self) -> Vec<VerseId> {
        if self.start.chapter_ref() != self.end.chapter_ref() {
            return vec![self.start];
        }
        let end = self.end.0.min(self.start.0 + MAX_EXPANDED_RANGE - 1);
        (self.start.0..=end).map(VerseId).collect()
    }
}

/// Writes `study.db`. Used only by the importer.
pub struct StudyWriter {
    conn: Connection,
    popularity: HashMap<u32, u32>,
    next_topic: u32,
}

impl StudyWriter {
    pub fn create(path: &Path, attribution: &str) -> Result<Self> {
        if path.exists() {
            std::fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;")?;
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch("BEGIN")?;
        conn.execute("INSERT INTO meta VALUES ('attribution', ?1)", [attribution])?;
        Ok(Self {
            conn,
            popularity: HashMap::new(),
            next_topic: 1,
        })
    }

    /// Add a topic with its passages; returns its id.
    pub fn add_topic(&mut self, name: &str, passages: &[Passage]) -> Result<u32> {
        let id = self.next_topic;
        self.next_topic += 1;
        self.conn.execute(
            "INSERT INTO topic VALUES (?1, ?2, ?3)",
            params![id, name, passages.len() as u32],
        )?;
        for p in passages {
            self.conn.execute(
                "INSERT OR IGNORE INTO topic_passage VALUES (?1, ?2, ?3, ?4)",
                params![id, p.start.0, p.end.0, p.score],
            )?;
            // Verses that many people file under topics are well known.
            *self.popularity.entry(p.start.0).or_default() += p.score * 5;
        }
        Ok(id)
    }

    pub fn add_alias(&self, topic_id: u32, alias: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO alias VALUES (?1, ?2)",
            params![topic_id, alias],
        )?;
        Ok(())
    }

    pub fn add_xref(&mut self, from: VerseId, to: Passage) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO xref VALUES (?1, ?2, ?3, ?4)",
            params![from.0, to.start.0, to.end.0, to.score],
        )?;
        *self.popularity.entry(to.start.0).or_default() += to.score;
        Ok(())
    }

    pub fn finish(self) -> Result<()> {
        let max = self.popularity.values().copied().max().unwrap_or(1);
        self.conn
            .execute("INSERT INTO meta VALUES ('popularity_max', ?1)", [max])?;
        for (verse, weight) in &self.popularity {
            self.conn.execute(
                "INSERT INTO popularity VALUES (?1, ?2)",
                params![verse, weight],
            )?;
        }
        self.conn.execute_batch(
            "CREATE INDEX topic_passage_by_score ON topic_passage(topic_id, score DESC);
             COMMIT;
             VACUUM;",
        )?;
        Ok(())
    }
}

/// A topic name or alias reduced to comparable stems.
struct Label {
    words: Vec<String>,
    /// Aliases rank a little below the topic's own name.
    alias: bool,
}

struct Entry {
    topic: Topic,
    labels: Vec<Label>,
}

/// Read-only handle to `study.db`. Topic names live in memory (a few
/// thousand), so matching a query is a plain loop.
pub struct Study {
    conn: Connection,
    entries: Vec<Entry>,
    popularity_max: f32,
    pub attribution: String,
}

/// Words that make a query a question ("what does the bible say about X")
/// rather than part of a topic's name.
const QUESTION_WORDS: &[&str] = &[
    "what",
    "does",
    "do",
    "did",
    "say",
    "says",
    "about",
    "bible",
    "biblical",
    "scripture",
    "scriptures",
    "verse",
    "verses",
    "regarding",
    "how",
    "why",
    "should",
    "christian",
    "christians",
    "according",
    "tell",
    "me",
    "is",
    "there",
    "any",
    "find",
];

fn label_words(text: &str) -> Vec<String> {
    words(text)
        .iter()
        .filter(|w| !is_stop_word(w))
        .map(|w| stem(w))
        .collect()
}

impl Study {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("opening {}", path.display()))?;
        let meta = |key: &str| -> Option<String> {
            conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
                .ok()
        };
        let attribution = meta("attribution").unwrap_or_default();
        let popularity_max = meta("popularity_max")
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(1.)
            .max(1.);

        let mut entries: Vec<Entry> = Vec::new();
        let mut by_id: HashMap<u32, usize> = HashMap::new();
        {
            let mut stmt = conn.prepare("SELECT id, name, passages FROM topic")?;
            let rows = stmt.query_map([], |r| {
                Ok(Topic {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    passages: r.get(2)?,
                })
            })?;
            for topic in rows {
                let topic = topic?;
                by_id.insert(topic.id, entries.len());
                entries.push(Entry {
                    labels: vec![Label {
                        words: label_words(&topic.name),
                        alias: false,
                    }],
                    topic,
                });
            }
            let mut stmt = conn.prepare("SELECT topic_id, alias FROM alias")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows {
                let (id, alias) = row?;
                if let Some(ix) = by_id.get(&id) {
                    entries[*ix].labels.push(Label {
                        words: label_words(&alias),
                        alias: true,
                    });
                }
            }
        }
        Ok(Self {
            conn,
            entries,
            popularity_max,
            attribution,
        })
    }

    pub fn topic_count(&self) -> usize {
        self.entries.len()
    }

    pub fn topic(&self, id: u32) -> Option<&Topic> {
        self.entries
            .iter()
            .find(|e| e.topic.id == id)
            .map(|e| &e.topic)
    }

    /// Topics a query is about, best first. Matches the topic name or one of
    /// its aliases: exactly ("pride"), as a typed prefix ("prid"), with extra
    /// words in the topic ("fruit" -> "fruit of the spirit"), or as a question
    /// that contains it ("what does the bible say about pride").
    pub fn topics(&self, query: &str, limit: usize) -> Vec<TopicMatch> {
        let mut q = label_words(query);
        // Drop question framing, but never reduce a query to nothing.
        let framing = |w: &String| QUESTION_WORDS.iter().any(|f| stem(f) == *w);
        if q.len() >= 2 && q.iter().any(|w| !framing(w)) {
            q.retain(|w| !framing(w));
        }
        if q.is_empty() {
            return Vec::new();
        }
        let mut found: Vec<TopicMatch> = self
            .entries
            .iter()
            .filter_map(|e| {
                let strength = e
                    .labels
                    .iter()
                    .filter_map(|l| label_strength(&q, l))
                    .max()?;
                (strength >= 50).then(|| TopicMatch {
                    topic: e.topic.clone(),
                    strength,
                })
            })
            .collect();
        found.sort_by(|a, b| {
            b.strength
                .cmp(&a.strength)
                .then(b.topic.passages.cmp(&a.topic.passages))
                .then(a.topic.name.cmp(&b.topic.name))
        });
        found.truncate(limit);
        found
    }

    /// A topic's passages, best fit first.
    pub fn topic_passages(&self, id: u32, limit: usize) -> Vec<Passage> {
        let Ok(mut stmt) = self.conn.prepare_cached(
            "SELECT start, end, score FROM topic_passage WHERE topic_id = ?1
             ORDER BY score DESC, start LIMIT ?2",
        ) else {
            return Vec::new();
        };
        stmt.query_map(params![id, limit as i64], |r| {
            Ok(Passage {
                start: VerseId(r.get(0)?),
                end: VerseId(r.get(1)?),
                score: r.get(2)?,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    /// Cross-references from a verse, most agreed-on first.
    pub fn related(&self, verse: VerseId, limit: usize) -> Vec<Passage> {
        let Ok(mut stmt) = self.conn.prepare_cached(
            "SELECT to_start, to_end, votes FROM xref WHERE from_verse = ?1
             ORDER BY votes DESC, to_start LIMIT ?2",
        ) else {
            return Vec::new();
        };
        stmt.query_map(params![verse.0, limit as i64], |r| {
            Ok(Passage {
                start: VerseId(r.get(0)?),
                end: VerseId(r.get(1)?),
                score: r.get(2)?,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    /// How well known a verse is, 0.0..=1.0 (log scale).
    pub fn popularity(&self, verse: VerseId) -> f32 {
        let weight: u32 = self
            .conn
            .prepare_cached("SELECT weight FROM popularity WHERE verse = ?1")
            .and_then(|mut s| s.query_row([verse.0], |r| r.get(0)))
            .unwrap_or(0);
        ((1. + weight as f32).ln() / (1. + self.popularity_max).ln()).min(1.)
    }
}

/// How well one name / alias fits the query stems, if at all.
fn label_strength(q: &[String], label: &Label) -> Option<u32> {
    let name = &label.words;
    if name.is_empty() {
        return None;
    }
    let penalty = if label.alias { 8 } else { 0 };
    let same = name.len() == q.len() && name.iter().all(|w| q.contains(w));
    if same {
        return Some(100 - penalty);
    }
    // Every query word is in the name; the last one may still be a prefix.
    let last = q.len() - 1;
    let covered = q.iter().enumerate().all(|(i, w)| {
        name.iter().any(|n| {
            n == w || (i == last && !label.alias && w.len() >= 3 && n.starts_with(w.as_str()))
        })
    });
    if covered && q.len() <= name.len() {
        let coverage = q.len() as f32 / name.len() as f32;
        // Words still being typed rank below whole-word matches.
        let exact = q.iter().all(|w| name.contains(w));
        let typing = if exact { 0 } else { 12 };
        return Some(70 + (coverage * 20.) as u32 - penalty - typing);
    }
    // The name is inside a longer question or sentence.
    let inside = name.iter().all(|n| q.contains(n));
    if inside && q.len() <= 8 && name.iter().map(|n| n.len()).sum::<usize>() >= 4 {
        let share = name.len() as f32 / q.len() as f32;
        return Some(55 + (share * 20.) as u32 - penalty);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn study(dir: &Path) -> Study {
        let path = dir.join("study.db");
        let mut w = StudyWriter::create(&path, "test").unwrap();
        let p = |v: u32, score| Passage {
            start: VerseId(v),
            end: VerseId(v),
            score,
        };
        let pride = w
            .add_topic("pride", &[p(20_016_018, 3), p(20_011_002, 2)])
            .unwrap();
        w.add_alias(pride, "arrogant").unwrap();
        w.add_topic("fruit of the spirit", &[p(48_005_022, 4)])
            .unwrap();
        w.add_topic("proud", &[p(59_004_006, 9)]).unwrap();
        w.add_xref(VerseId(20_016_018), p(59_004_006, 10)).unwrap();
        w.finish().unwrap();
        Study::open(&path).unwrap()
    }

    #[test]
    fn matches_topics_by_name_alias_prefix_and_question() {
        let dir = std::env::temp_dir().join(format!("bible-study-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let s = study(&dir);
        let names =
            |q: &str| -> Vec<String> { s.topics(q, 5).into_iter().map(|m| m.topic.name).collect() };
        assert_eq!(names("pride"), ["pride"]);
        assert_eq!(names("Arrogant")[0], "pride");
        assert_eq!(names("pri")[0], "pride");
        assert_eq!(names("fruit"), ["fruit of the spirit"]);
        assert_eq!(names("what does the bible say about pride")[0], "pride");
        assert!(names("zebra").is_empty());

        let passages = s.topic_passages(1, 10);
        assert_eq!(passages[0].start, VerseId(20_016_018));
        assert_eq!(
            s.related(VerseId(20_016_018), 5)[0].start,
            VerseId(59_004_006)
        );
        assert!(s.popularity(VerseId(59_004_006)) > s.popularity(VerseId(1_001_001)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn passage_labels() {
        let v = |b, c, n| VerseId::new(b, c, n);
        let one = Passage {
            start: v(43, 3, 16),
            end: v(43, 3, 16),
            score: 1,
        };
        assert_eq!(one.label(), "John 3:16");
        let run = Passage {
            start: v(2, 20, 1),
            end: v(2, 20, 17),
            score: 1,
        };
        assert_eq!(run.label(), "Exodus 20:1-17");
        assert_eq!(run.verses().len(), 17);
        let across = Passage {
            start: v(1, 1, 31),
            end: v(1, 2, 3),
            score: 1,
        };
        assert_eq!(across.label(), "Genesis 1:31-2:3");
    }
}
