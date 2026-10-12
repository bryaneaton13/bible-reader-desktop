//! Check that YouVersion's chapter HTML turns into the right blocks, by
//! comparing Bible 3034 (the Berean Standard Bible, public domain, which is
//! also bundled with the app) against the local copy, verse by verse.
//!
//!   set -a; source agent.env; set +a
//!   cargo run -p bible-youversion --example verify [BIBLE_ID LOCAL_DB]
//!
//! The default compares Bible 3034 with data/build/bsb.sqlite; `206 web` compares
//! the World English Bible with data/build/web.sqlite (red letter and all).
//!
//! Prints markup class counts, block/verse/note counts and any verse whose
//! text differs from the bundled BSB. Needs data/build/bsb.sqlite and about
//! 10 requests. Nothing is written to disk.

use std::collections::BTreeMap;

use bible_core::{BlockKind, ChapterRef, Span, book_by_code};
use bible_store::Translation;
use bible_youversion::{Cache, Client, Source, parse_chapter, usfm_chapter, verse_texts};

fn classes(html: &str) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    let mut rest = html;
    while let Some(i) = rest.find("class=\"") {
        rest = &rest[i + 7..];
        let end = rest.find('"').unwrap_or(rest.len());
        for c in rest[..end].split_whitespace() {
            *out.entry(c.to_string()).or_insert(0) += 1;
        }
        rest = &rest[end..];
    }
    out
}

/// Words only, lower case: tolerant of quote styles, dashes and spacing.
fn norm(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn main() {
    let Ok(key) = std::env::var("YVP_APP_KEY") else {
        eprintln!("set YVP_APP_KEY (source agent.env)");
        std::process::exit(2);
    };
    let bible: u32 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(3034);
    let db = std::env::args().nth(2).unwrap_or_else(|| "bsb".into());
    let local = Translation::open(std::path::Path::new(&format!("data/build/{db}.sqlite")))
        .expect("run `cargo run --release -p bible-import` first");
    let dir = std::env::temp_dir().join(format!("yv-verify-{}", std::process::id()));
    let source = Source::new(Client::new(key), Cache::new(&dir));

    let chapters = [
        "GEN.1", "PSA.23", "PSA.119", "PRO.1", "MAT.5", "JHN.3", "ROM.8", "OBA.1", "3JN.1",
        "REV.22",
    ];
    let mut all_classes: BTreeMap<String, usize> = BTreeMap::new();
    for code in chapters {
        let (book, ch) = code.split_once('.').unwrap();
        let chapter = ChapterRef::new(book_by_code(book).unwrap().num, ch.parse().unwrap());
        let html = match source.probe(bible, &usfm_chapter(chapter)) {
            Ok(h) => h,
            Err(e) => {
                println!("{code}: ERROR {e}");
                continue;
            }
        };
        for (c, n) in classes(&html) {
            *all_classes.entry(c).or_insert(0) += n;
        }
        let parsed = parse_chapter(&html, chapter);
        let remote = verse_texts(&parsed.blocks);
        let bundled = local.chapter(chapter).ok().flatten().unwrap_or_default();
        let bundled = verse_texts(&bundled);

        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        for b in &parsed.blocks {
            *kinds.entry(format!("{:?}", b.kind)).or_insert(0) += 1;
        }
        let red = parsed
            .blocks
            .iter()
            .flat_map(|b| &b.spans)
            .filter(|s| matches!(s, Span::Text { style, .. } if style.jesus))
            .count();
        let headings = parsed
            .blocks
            .iter()
            .filter(|b| b.kind.is_heading() && b.kind != BlockKind::ParallelRefs)
            .count();
        let numbers: Vec<u16> = remote.iter().map(|(n, _)| *n).collect();
        let contiguous = numbers.windows(2).all(|w| w[1] > w[0]);
        println!(
            "\n{code}: {} blocks, {} verses (bundled {}), {} notes, {headings} headings, {red} red-letter runs, verses ascending: {contiguous}",
            parsed.blocks.len(),
            remote.len(),
            bundled.len(),
            parsed.notes.len()
        );
        println!("  blocks: {kinds:?}");
        let mut different = 0;
        for (n, text) in &remote {
            let other = bundled
                .iter()
                .find(|(v, _)| v == n)
                .map(|(_, t)| t.as_str());
            if other.map(norm).as_deref() != Some(norm(text).as_str()) {
                different += 1;
                if different <= 3 {
                    println!(
                        "  verse {n} differs:\n    yv:    {text}\n    local: {}",
                        other.unwrap_or("(missing)")
                    );
                }
            }
        }
        if different == 0 {
            println!(
                "  all {} verses match the bundled translation word for word",
                remote.len()
            );
        } else {
            println!("  {different} verses differ");
        }
        if let Some(n) = parsed.notes.first() {
            println!("  first note (verse {}): {:?}", n.verse.verse(), n.text);
        }
    }
    println!("\nmarkup classes seen across all chapters:");
    for (c, n) in &all_classes {
        println!("  {c:<14} {n}");
    }
    println!("\nrequests used: {}", source.client().requests_made());
    let _ = std::fs::remove_dir_all(dir);
}
