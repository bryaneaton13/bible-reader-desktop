//! Measure what the YouVersion passages endpoint accepts, so the chapter
//! window in `window.rs` can be set from facts. Prints sizes and counts only;
//! it writes nothing to disk and never prints Bible text.
//!
//!   YVP_APP_KEY=... cargo run -p bible-youversion --example probe -- 3034
//!
//! The argument is a Bible id licensed to your app (BSB is 3034). The probe
//! makes about 8 requests.

use bible_youversion::{Cache, Client, Source, parse_chapters, verse_texts};

fn main() {
    let Ok(key) = std::env::var("YVP_APP_KEY") else {
        eprintln!("set YVP_APP_KEY to your app key");
        std::process::exit(2);
    };
    let bible: u32 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(3034);
    let dir = std::env::temp_dir().join(format!("yv-probe-{}", std::process::id()));
    let source = Source::new(Client::new(key), Cache::new(&dir));

    println!("Bible {bible}; John has 21 chapters and 879 verses.\n");
    // (label, passage id, book, first chapter, chapters expected)
    let cases = [
        ("one chapter", "JHN.3", 43u8, 3u16, 1usize),
        ("two chapters", "JHN.3-4", 43, 3, 2),
        ("three chapters", "JHN.3-5", 43, 3, 3),
        ("five chapters", "JHN.3-7", 43, 3, 5),
        ("ten chapters", "JHN.1-10", 43, 1, 10),
        ("whole book by chapters", "JHN.1-21", 43, 1, 21),
        ("whole book by id", "JHN", 43, 1, 21),
        ("verse range across chapters", "JHN.3.30-4.5", 43, 3, 2),
    ];
    for (label, passage, book, first, expected) in cases {
        let before = source.client().requests_made();
        match source.probe(bible, passage) {
            Err(err) => println!("{label:<28} {passage:<14} ERROR {err}"),
            Ok(html) => {
                let split = parse_chapters(&html, book, first, expected);
                let verses = match &split {
                    Some(chapters) => chapters
                        .iter()
                        .map(|c| verse_texts(&c.blocks).len())
                        .sum::<usize>(),
                    None => verse_texts(
                        &bible_youversion::parse_chapter(
                            &html,
                            bible_core::ChapterRef::new(book, first),
                        )
                        .blocks,
                    )
                    .len(),
                };
                println!(
                    "{label:<28} {passage:<14} {:>7} bytes, {verses:>4} verses, split into {expected} chapters: {}  ({} request)",
                    html.len(),
                    if split.is_some() { "YES" } else { "no" },
                    source.client().requests_made() - before
                );
            }
        }
    }
    println!(
        "\nRead it like this: if \"three chapters\" says YES, raise window::MAX_CHAPTERS. If \"whole book by id\" says YES with 879 verses, one request can fetch a whole book (and a \"Download this book\" button becomes worthwhile). Fewer verses than expected means the API truncated the passage: keep MAX_VERSES below that."
    );
    let _ = std::fs::remove_dir_all(dir);
}
