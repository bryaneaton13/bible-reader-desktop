//! Builds `study.db` from topic and cross-reference data plus our own
//! aliases.
//!
//! Topics come from three sources and are merged by what they are about:
//! openbible.info (user-voted, CC-BY), Torrey's New Topical Textbook and
//! Nave's Topical Bible (both public domain, via CCEL). A verse listed by
//! several sources ranks above one listed by a single source. Topics that are
//! just names (people, places), thin, or only backed by a handful of weak
//! votes are dropped; Nave's "See X" entries become aliases.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use anyhow::{Context as _, Result};
use bible_core::text::words;
use bible_core::{VerseId, parse_osis, parse_osis_range};
use bible_store::{Passage, StudyWriter};

use crate::topical::{self, Vocab};

/// Topic entries scoring below this are noise, unless a topic would be left thin.
const MIN_TOPIC_SCORE: u32 = 3;
/// Always keep at least this many of a topic's best passages.
const KEEP_BEST: usize = 6;
const MAX_TOPIC_PASSAGES: usize = 40;
const MIN_TOPIC_PASSAGES: usize = 3;
/// A topic found only in Nave's or Torrey's needs this many distinct passages.
const MIN_CURATED_PASSAGES: usize = 4;
/// Longest run of verses taken from Nave's and Torrey's as one passage.
const MAX_CURATED_VERSES: u16 = 8;
/// Topic names with more words than this are sentences, not topics.
/// A new topic whose passages mostly contain its own word ("cup", "oak") is
/// already served by text search.
const MAX_LITERAL_SHARE: f32 = 0.6;
const MAX_NAME_WORDS: usize = 5;
const MIN_XREF_VOTES: u32 = 3;
const MAX_XREFS_PER_VERSE: usize = 8;

const ATTRIBUTION: &str = "Topics and cross-references from openbible.info, licensed CC-BY 4.0 (www.openbible.info/labs/cross-references). Cross-references derive from the Treasury of Scripture Knowledge. Topics are merged with Nave's Topical Bible and Torrey's New Topical Textbook (public domain, ccel.org).";

/// A verse or run of verses under a topic, and why it's there.
struct Cand {
    start: VerseId,
    end: VerseId,
    /// openbible.info's score, 0 when only the older books list it.
    votes: u32,
    /// Added by Nave's and Torrey's.
    curated: u32,
    /// How many sources beyond the first list it.
    hits: u32,
}

impl Cand {
    fn score(&self) -> u32 {
        self.votes + self.curated
    }
}

struct Draft {
    name: String,
    from_openbible: bool,
    cands: Vec<Cand>,
    index: HashMap<VerseId, usize>,
    /// Passages in the best-listed spelling, which names the topic.
    most: usize,
}

impl Draft {
    fn new(name: String, from_openbible: bool) -> Self {
        Self {
            name,
            from_openbible,
            cands: Vec::new(),
            index: HashMap::new(),
            most: 0,
        }
    }

    fn push(&mut self, cand: Cand) {
        let i = self.cands.len();
        for verse in (Passage {
            start: cand.start,
            end: cand.end,
            score: 0,
        })
        .verses()
        {
            self.index.entry(verse).or_insert(i);
        }
        self.cands.push(cand);
    }

    fn corroborated(&self) -> usize {
        self.cands.iter().filter(|c| c.hits > 0).count()
    }

    /// Fold in one older book's entry. `inbound` says how well known verses are.
    fn add_curated(
        &mut self,
        citations: &BTreeMap<(VerseId, VerseId), u32>,
        inbound: &HashMap<VerseId, u32>,
    ) {
        for (&(start, end), &times) in citations {
            // Cited again and again within an entry: central to it. Well-known
            // verses edge out obscure ones in the long entries.
            let known =
                ((1. + *inbound.get(&start).unwrap_or(&0) as f32).ln() / 2.5).min(3.) as u32;
            let score = 4 + 2 * (times - 1).min(3) + known;
            match self.index.get(&start).copied() {
                Some(i) => {
                    self.cands[i].curated += score;
                    self.cands[i].hits += 1;
                }
                None => self.push(Cand {
                    start,
                    end,
                    votes: 0,
                    curated: score,
                    hits: 0,
                }),
            }
        }
    }
}

pub fn import(sources: &Path, out: &Path) -> Result<()> {
    let dir = sources.join("openbible");
    let mut writer = StudyWriter::create(out, ATTRIBUTION)?;

    // Cross-references: single-verse sources, best-voted targets only.
    let text = std::fs::read_to_string(dir.join("cross_references.txt"))
        .context("reading cross_references.txt")?;
    let mut by_source: BTreeMap<VerseId, Vec<Passage>> = BTreeMap::new();
    let mut inbound: HashMap<VerseId, u32> = HashMap::new();
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
            *inbound.entry(start).or_default() += votes;
            by_source.entry(from).or_default().push(Passage {
                start,
                end,
                score: votes,
            });
        }
    }

    let dropped_names: HashSet<String> = std::fs::read_to_string(dir.join("drop.txt"))
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let vocab = Vocab::from_usfm(&sources.join("bsb")).context("reading BSB for names")?;
    // Nave's and Torrey's cite the King James wording, so check against that.
    let kjv = Vocab::from_usfm(&sources.join("kjv")).context("reading KJV")?;
    let mut drafts: BTreeMap<String, Draft> = BTreeMap::new();

    // openbible.info topics.
    let text = std::fs::read_to_string(dir.join("topic-scores.txt"))
        .context("reading topic-scores.txt")?;
    let mut ob_total = 0;
    let mut ob_names = 0;
    let mut ob_curated_out = 0;
    let mut by_name: BTreeMap<String, Vec<Passage>> = BTreeMap::new();
    for line in text.lines().skip(1) {
        let mut cols = line.split('\t');
        let (Some(name), Some(osis), Some(score)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        let (Some((start, end)), Ok(score)) = (parse_osis_range(osis), score.parse::<u32>()) else {
            continue;
        };
        by_name
            .entry(topical::topic_name(name))
            .or_default()
            .push(Passage { start, end, score });
    }
    for (name, passages) in by_name {
        ob_total += 1;
        if dropped_names.contains(&name) {
            ob_curated_out += 1;
            continue;
        }
        if name.len() < 3 || vocab.is_name(&name) {
            ob_names += 1;
            continue;
        }
        let draft = drafts
            .entry(topical::merge_key(&name))
            .or_insert_with(|| Draft::new(name.clone(), true));
        if passages.len() > draft.most {
            draft.most = passages.len();
            draft.name = name;
        }
        for p in passages {
            match draft.index.get(&p.start).copied() {
                Some(i) => draft.cands[i].votes = draft.cands[i].votes.max(p.score),
                None => draft.push(Cand {
                    start: p.start,
                    end: p.end,
                    votes: p.score,
                    curated: 0,
                    hits: 0,
                }),
            }
        }
    }

    // Torrey's, then Nave's: they back up topics openbible.info has and add
    // the ones it lacks.
    let topical_dir = sources.join("topical");
    let mut see_only: Vec<topical::Entry> = Vec::new();
    let (mut added, mut skipped_names, mut skipped_thin) = (0, 0, 0);
    for file in ["torrey.xml", "nave.xml"] {
        for entry in topical::load(&topical_dir.join(file))? {
            let name = topical::topic_name(&entry.term);
            if dropped_names.contains(&name) {
                ob_curated_out += 1;
                continue;
            }
            let citations = topical::citations(&entry, MAX_CURATED_VERSES);
            if entry.passages.is_empty() {
                see_only.push(entry);
                continue;
            }
            let key = topical::merge_key(&name);
            if key.is_empty() || name.len() < 3 {
                continue;
            }
            if let Some(draft) = drafts.get_mut(&key) {
                draft.add_curated(&citations, &inbound);
                continue;
            }
            if kjv.is_name(&name) {
                skipped_names += 1;
                continue;
            }
            let spans: Vec<_> = citations.keys().copied().collect();
            if citations.len() < MIN_CURATED_PASSAGES
                || words(&name).len() > MAX_NAME_WORDS
                || name == "select readings"
                || kjv.literal_share(&name, &spans) >= MAX_LITERAL_SHARE
            {
                skipped_thin += 1;
                continue;
            }
            let mut draft = Draft::new(name, false);
            draft.add_curated(&citations, &inbound);
            drafts.insert(key, draft);
            added += 1;
        }
    }

    // Trim and write.
    let mut ids: HashMap<String, u32> = HashMap::new();
    let (mut kept_ob, mut dropped_weak) = (0, 0);
    let mut report = Vec::new();
    for (key, mut draft) in drafts {
        if draft.from_openbible {
            let best = draft.cands.iter().map(|c| c.votes).max().unwrap_or(0);
            let backed = draft.corroborated();
            let long = words(&draft.name).len() > MAX_NAME_WORDS;
            // Votes alone can't tell "voodoo" from "doubt"; another book
            // listing the same verses can. Without that, only strong votes do.
            let trusted = backed >= 1 || best >= 5 || draft.cands.len() >= 8;
            if !trusted || long && backed < 2 {
                dropped_weak += 1;
                continue;
            }
        }
        draft
            .cands
            .sort_by(|a, b| b.score().cmp(&a.score()).then(a.start.cmp(&b.start)));
        let strong = draft
            .cands
            .iter()
            .take_while(|c| c.score() >= MIN_TOPIC_SCORE)
            .count();
        draft
            .cands
            .truncate(strong.clamp(KEEP_BEST, MAX_TOPIC_PASSAGES));
        if draft.cands.len() < MIN_TOPIC_PASSAGES {
            dropped_weak += 1;
            continue;
        }
        let passages: Vec<Passage> = draft
            .cands
            .iter()
            .map(|c| Passage {
                start: c.start,
                end: c.end,
                score: c.score(),
            })
            .collect();
        ids.insert(key, writer.add_topic(&draft.name, &passages)?);
        if draft.from_openbible {
            kept_ob += 1;
        }
        report.push(format!(
            "{}\t{}\t{}\t{}",
            draft.name,
            if draft.from_openbible { "ob" } else { "old" },
            draft.cands.len(),
            draft.corroborated()
        ));
    }
    let kept = ids.len();

    // Aliases: Nave's "See X" entries, then ours.
    let mut aliases = 0;
    let mut seen_alias: HashSet<(u32, String)> = HashSet::new();
    for entry in &see_only {
        let alias = topical::topic_name(&entry.term);
        if alias.len() < 3 || ids.contains_key(&topical::merge_key(&alias)) || kjv.is_name(&alias) {
            continue;
        }
        for target in &entry.see {
            if let Some(id) = ids.get(&topical::merge_key(&topical::topic_name(target)))
                && seen_alias.insert((*id, alias.clone()))
            {
                writer.add_alias(*id, &alias)?;
                aliases += 1;
            }
        }
    }
    let mut orphans = Vec::new();
    let text = std::fs::read_to_string(dir.join("aliases.txt")).context("reading aliases.txt")?;
    for line in text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
    {
        let Some((topic, list)) = line.split_once(':') else {
            continue;
        };
        let Some(id) = ids.get(&topical::merge_key(topic.trim())) else {
            orphans.push(topic.trim().to_string());
            continue;
        };
        for alias in list.split(',').map(str::trim).filter(|a| !a.is_empty()) {
            if seen_alias.insert((*id, alias.to_string())) {
                writer.add_alias(*id, alias)?;
                aliases += 1;
            }
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

    report.sort();
    std::fs::write(
        out.with_file_name("study-topics.txt"),
        format!(
            "name\torigin\tpassages\tcorroborated\n{}\n",
            report.join("\n")
        ),
    )?;
    println!(
        "study: {kept} topics ({kept_ob} openbible.info of {ob_total}; {ob_names} names, {ob_curated_out} listed in drop.txt and {dropped_weak} weak dropped; {added} added from Nave's and Torrey's, {skipped_names} names and {skipped_thin} thin skipped), {aliases} aliases, {xrefs} cross-references -> {}",
        out.display()
    );
    for orphan in orphans {
        println!("  warning: alias topic not found: {orphan}");
    }
    Ok(())
}
