//! Builds translation databases from USFM sources.
//!
//! Usage: bible-import [SOURCES_DIR] [OUT_DIR] [ABBREV...]
//! Defaults: data/sources data/build, all known translations plus study.db
//! (topics and cross-references). Name "study" among the abbreviations to
//! build only that.

mod study;
mod usfm;

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use bible_core::BOOKS;
use bible_store::{TranslationInfo, TranslationWriter};

struct Source {
    abbrev: &'static str,
    dir: &'static str,
    name: &'static str,
    copyright: &'static str,
}

const SOURCES: &[Source] = &[
    Source {
        abbrev: "BSB",
        dir: "bsb",
        name: "Berean Standard Bible",
        copyright: "The Holy Bible, Berean Standard Bible, BSB. Dedicated to the public domain.",
    },
    Source {
        abbrev: "KJV",
        dir: "kjv",
        name: "King James Version",
        copyright: "King James Version (1769). Public domain (outside the UK).",
    },
    Source {
        abbrev: "WEB",
        dir: "web",
        name: "World English Bible",
        copyright: "World English Bible. Public domain. \"World English Bible\" is a trademark of eBible.org.",
    },
];

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sources_dir = PathBuf::from(args.first().map_or("data/sources", String::as_str));
    let out_dir = PathBuf::from(args.get(1).map_or("data/build", String::as_str));
    let only: Vec<String> = args.iter().skip(2).map(|s| s.to_uppercase()).collect();
    std::fs::create_dir_all(&out_dir)?;

    if only.is_empty() || only.iter().any(|a| a == "STUDY") {
        study::import(&sources_dir.join("openbible"), &out_dir.join("study.db"))
            .context("importing study data")?;
    }
    for source in SOURCES {
        if !only.is_empty() && !only.iter().any(|a| a == source.abbrev) {
            continue;
        }
        let out = out_dir.join(format!("{}.sqlite", source.abbrev.to_lowercase()));
        import(source, &sources_dir.join(source.dir), &out)
            .with_context(|| format!("importing {}", source.abbrev))?;
    }
    Ok(())
}

fn import(source: &Source, dir: &Path, out: &Path) -> Result<()> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext == "usfm" || ext == "sfm")
        })
        .collect();
    files.sort();

    let info = TranslationInfo {
        abbrev: source.abbrev.into(),
        name: source.name.into(),
        copyright: source.copyright.into(),
    };
    let writer = TranslationWriter::create(out, &info)?;
    let mut next_note_id = 1;
    let mut books_seen = Vec::new();
    let mut verse_total = 0;
    let mut warnings = Vec::new();

    for file in &files {
        let src = std::fs::read_to_string(file)?;
        let src = src.trim_start_matches('\u{feff}');
        let Some(book) = usfm::parse_book(src, &mut next_note_id)
            .with_context(|| format!("parsing {}", file.display()))?
        else {
            continue;
        };
        if book.chapters.len() != book.book.chapters as usize {
            warnings.push(format!(
                "{}: {} chapters, expected {}",
                book.book.name,
                book.chapters.len(),
                book.book.chapters
            ));
        }
        for (chapter, blocks) in &book.chapters {
            writer.add_chapter(*chapter, blocks)?;
        }
        for (id, (end, text)) in &book.verses {
            if text.is_empty() {
                continue;
            }
            writer.add_verse(*id, *end, text)?;
            verse_total += 1;
        }
        for note in &book.notes {
            writer.add_note(note)?;
        }
        books_seen.push(book.book.num);
    }
    writer.finish()?;

    for b in BOOKS.iter().filter(|b| !books_seen.contains(&b.num)) {
        warnings.push(format!("missing book {}", b.name));
    }
    println!(
        "{}: {} books, {} verses, {} notes -> {}",
        source.abbrev,
        books_seen.len(),
        verse_total,
        next_note_id - 1,
        out.display()
    );
    for w in &warnings {
        println!("  warning: {w}");
    }
    if books_seen.is_empty() {
        bail!("no books found in {}", dir.display());
    }
    Ok(())
}
