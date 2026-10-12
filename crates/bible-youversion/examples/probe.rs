//! Measure what the YouVersion passages endpoint accepts (ranges, whole
//! books), to see whether fewer, larger requests are possible. Prints sizes and counts only;
//! it writes nothing to disk and never prints Bible text.
//!
//!   YVP_APP_KEY=... cargo run -p bible-youversion --example probe -- 3034
//!
//! The argument is a Bible id licensed to your app (BSB is 3034). The probe
//! makes about 8 requests.

use bible_youversion::{Cache, Client, Source, parse_chapter, verse_texts};

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
    let cases = [
        ("one chapter", "JHN.3"),
        ("two chapters", "JHN.3-4"),
        ("three chapters", "JHN.3-5"),
        ("ten chapters", "JHN.1-10"),
        ("whole book by chapters", "JHN.1-21"),
        ("whole book by id", "JHN"),
        ("verse range across chapters", "JHN.3.30-4.5"),
    ];
    for (label, passage) in cases {
        match source.probe(bible, passage) {
            Err(err) => println!("{label:<28} {passage:<14} ERROR {err}"),
            Ok(html) => {
                let chapter = parse_chapter(&html, bible_core::ChapterRef::new(43, 1));
                println!(
                    "{label:<28} {passage:<14} {:>7} bytes, {:>4} verses",
                    html.len(),
                    verse_texts(&chapter.blocks).len()
                );
            }
        }
    }
    println!(
        "\nMeasured 2026-10-11 against Bible 3034: only the single-chapter form works; every range and whole-book id answers 404. If a row above says YES for several chapters, the API has changed and a multi-chapter request is worth building again."
    );
    let _ = std::fs::remove_dir_all(dir);
}
