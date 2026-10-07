//! Builds `study.db` from the openbible.info topic and cross-reference data
//! (CC-BY) plus our own aliases. Trims it to what search can use: weak
//! topic entries and long tails of cross-references are dropped.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use anyhow::{Context as _, Result};
use bible_core::{VerseId, parse_osis, parse_osis_range};
use bible_store::{Passage, StudyWriter};

/// Topic entries scoring below this are noise, unless a topic would be left thin.
const MIN_TOPIC_SCORE: u32 = 3;
/// Always keep at least this many of a topic's best passages.
const KEEP_BEST: usize = 6;
const MAX_TOPIC_PASSAGES: usize = 40;
const MIN_TOPIC_PASSAGES: usize = 3;
const MIN_XREF_VOTES: u32 = 3;
const MAX_XREFS_PER_VERSE: usize = 8;

const ATTRIBUTION: &str = "Topics and cross-references from openbible.info, licensed CC-BY 4.0 (www.openbible.info/labs/cross-references). Cross-references derive from the Treasury of Scripture Knowledge.";

pub fn import(dir: &Path, out: &Path) -> Result<()> {
    let mut writer = StudyWriter::create(out, ATTRIBUTION)?;

    // Topics
    let text = std::fs::read_to_string(dir.join("topic-scores.txt"))
        .context("reading topic-scores.txt")?;
    let mut topics: BTreeMap<String, Vec<Passage>> = BTreeMap::new();
    for line in text.lines().skip(1) {
        let mut cols = line.split('\t');
        let (Some(name), Some(osis), Some(score)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        let (Some((start, end)), Ok(score)) = (parse_osis_range(osis), score.parse::<u32>()) else {
            continue;
        };
        topics
            .entry(name.trim().to_lowercase())
            .or_default()
            .push(Passage { start, end, score });
    }
    let mut ids: HashMap<String, u32> = HashMap::new();
    let mut kept = 0;
    for (name, mut passages) in topics {
        passages.sort_by(|a, b| b.score.cmp(&a.score).then(a.start.cmp(&b.start)));
        let strong = passages
            .iter()
            .take_while(|p| p.score >= MIN_TOPIC_SCORE)
            .count();
        passages.truncate(strong.clamp(KEEP_BEST, MAX_TOPIC_PASSAGES));
        if passages.len() < MIN_TOPIC_PASSAGES {
            continue;
        }
        ids.insert(name.clone(), writer.add_topic(&name, &passages)?);
        kept += 1;
    }

    // Aliases
    let mut aliases = 0;
    let mut orphans = Vec::new();
    let text = std::fs::read_to_string(dir.join("aliases.txt")).context("reading aliases.txt")?;
    for line in text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
    {
        let Some((topic, list)) = line.split_once(':') else {
            continue;
        };
        let Some(id) = ids.get(topic.trim()) else {
            orphans.push(topic.trim().to_string());
            continue;
        };
        for alias in list.split(',').map(str::trim).filter(|a| !a.is_empty()) {
            writer.add_alias(*id, alias)?;
            aliases += 1;
        }
    }

    // Cross-references: single-verse sources, best-voted targets only.
    let text = std::fs::read_to_string(dir.join("cross_references.txt"))
        .context("reading cross_references.txt")?;
    let mut by_source: BTreeMap<VerseId, Vec<Passage>> = BTreeMap::new();
    for line in text.lines().skip(1) {
        let mut cols = line.split('\t');
        let (Some(from), Some(to), Some(votes)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        let (Some(from), Some((start, end)), Ok(votes)) =
            (parse_osis(from), parse_osis_range(to), votes.parse::<u32>())
        else {
            continue;
        };
        if votes >= MIN_XREF_VOTES {
            by_source.entry(from).or_default().push(Passage {
                start,
                end,
                score: votes,
            });
        }
    }
    let mut xrefs = 0;
    for (from, mut targets) in by_source {
        targets.sort_by(|a, b| b.score.cmp(&a.score).then(a.start.cmp(&b.start)));
        let mut seen = HashSet::new();
        for target in targets
            .into_iter()
            .filter(|t| seen.insert(t.start))
            .take(MAX_XREFS_PER_VERSE)
        {
            writer.add_xref(from, target)?;
            xrefs += 1;
        }
    }
    writer.finish()?;

    println!(
        "study: {kept} topics, {aliases} aliases, {xrefs} cross-references -> {}",
        out.display()
    );
    for orphan in orphans {
        println!("  warning: alias topic not found: {orphan}");
    }
    Ok(())
}
