//! Dev tool: print ranked results. `cargo run -p bible-store --example probe -- "pride" [BSB]`
use std::path::Path;

use bible_store::{MatchMode, SearchRequest, Study, Translation, search};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let query = args.first().map_or("pride", String::as_str);
    let abbrev = args.get(1).map_or("bsb", |s| s.as_str()).to_lowercase();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/build");
    let tr = Translation::open(&dir.join(format!("{abbrev}.sqlite"))).unwrap();
    let study = Study::open(&dir.join("study.db")).ok();
    let mut req = SearchRequest::new(query, 15);
    if std::env::var("MODE").as_deref() == Ok("any") {
        req.mode = MatchMode::Any;
    }
    let started = std::time::Instant::now();
    let r = search(&tr, study.as_ref(), &req);
    println!("{:?} in {:?}", query, started.elapsed());
    for t in &r.topics {
        println!(
            "  topic {} ({}) strength {}",
            t.topic.name, t.topic.passages, t.strength
        );
    }
    println!("  {} matches", r.total);
    for h in &r.hits {
        let text: String = h.text.chars().take(80).collect();
        println!(
            "  {:5.2} {} {:<20} {text}",
            h.score,
            if h.in_topic { "T" } else { " " },
            h.verse.to_string()
        );
    }
}
