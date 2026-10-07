//! Suggested searches shown in the palette and on the empty search page.
//! Each one is known to return good results (a test checks them against the
//! built data), and a few are picked at a time so the list keeps changing.

/// Topic words, phrases people remember and questions, all of which find
/// plenty of verses. Keep them short enough to read at a glance.
pub const SUGGESTIONS: &[&str] = &[
    // Feelings and struggles
    "anxiety",
    "fear not",
    "loneliness",
    "grief",
    "depression",
    "anger",
    "doubt",
    "forgiveness",
    "money",
    "weakness",
    "comfort",
    "hope",
    // Character
    "pride",
    "humility",
    "patience",
    "wisdom",
    "gratitude",
    "generosity",
    "integrity",
    "self control",
    "faithfulness",
    "courage",
    // Faith and life
    "prayer",
    "grace",
    "salvation",
    "the holy spirit",
    "spiritual warfare",
    "eternal life",
    "what does the bible say about marriage",
    "how to forgive",
    "love your enemies",
    "friendship",
    "parenting",
    "work and rest",
    "temptation",
    "healing",
    "peace",
    "joy",
    "trust",
    // Well-known phrases
    "be still",
    "the good shepherd",
    "in the beginning",
    "faith hope love",
    "light and darkness",
    "fruit of the spirit",
    "armor of god",
    "ask and it will be given",
    "be anxious for nothing",
    "the lord is my shepherd",
    "jesus wept",
    "the beatitudes",
];

/// `count` suggestions starting at a spot that depends on `seed`. A step that
/// is coprime with the list length makes neighbours unrelated and visits every
/// entry before repeating.
pub fn pick(seed: usize, count: usize) -> Vec<&'static str> {
    let len = SUGGESTIONS.len();
    let step = 7; // coprime with the list length below
    debug_assert!(!len.is_multiple_of(step));
    (0..count.min(len))
        .map(|i| SUGGESTIONS[(seed * count * step + i * step) % len])
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::Path;

    use bible_store::{BOOST_STRENGTH, SearchRequest, Study, Translation, search};

    use super::*;

    #[test]
    fn picks_are_distinct_and_rotate() {
        let first = pick(0, 4);
        assert_eq!(first.len(), 4);
        assert_eq!(first.iter().collect::<HashSet<_>>().len(), 4);
        assert_ne!(first, pick(1, 4));
        // Every suggestion shows up within one full cycle.
        let seen: HashSet<_> = (0..SUGGESTIONS.len())
            .flat_map(|seed| pick(seed, 4))
            .collect();
        assert_eq!(seen.len(), SUGGESTIONS.len());
        assert_eq!(
            SUGGESTIONS.iter().collect::<HashSet<_>>().len(),
            SUGGESTIONS.len(),
            "duplicate suggestion"
        );
    }

    /// Every suggestion finds a topic or plenty of verses in the default
    /// translation (the wording of a quoted phrase varies between versions).
    #[test]
    fn every_suggestion_returns_good_results() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/build");
        let (Ok(bsb), Ok(study)) = (
            Translation::open(&dir.join("bsb.sqlite")),
            Study::open(&dir.join("study.db")),
        ) else {
            return;
        };
        let mut weak = Vec::new();
        for query in SUGGESTIONS {
            let results = search(&bsb, Some(&study), &SearchRequest::new(query, 50));
            let topic = results.topics.iter().any(|t| t.strength >= BOOST_STRENGTH);
            let enough = results.hits.len() >= if topic { 5 } else { 8 };
            if !(enough && (topic || results.hits[0].score > 1.2)) {
                weak.push(format!(
                    "{query}: {} hits, topic {topic}",
                    results.hits.len()
                ));
            }
        }
        assert!(weak.is_empty(), "weak suggestions: {weak:#?}");
    }
}
