//! Spelling correction for search queries: a curated list of common
//! misspellings first, then the closest word the translation actually uses
//! (Damerau-Levenshtein, commonest word wins ties).

use std::collections::HashMap;

/// `wrong right` per line; `#` starts a comment.
const MISSPELLINGS: &str = include_str!("misspellings.txt");

#[derive(Debug, Default)]
pub struct Vocabulary {
    freq: HashMap<String, u32>,
    curated: HashMap<&'static str, &'static str>,
}

impl Vocabulary {
    pub fn from_texts<'a>(texts: impl IntoIterator<Item = &'a str>) -> Self {
        let mut freq: HashMap<String, u32> = HashMap::new();
        for text in texts {
            for w in bible_core::text::words(text) {
                *freq.entry(w).or_default() += 1;
            }
        }
        let curated = MISSPELLINGS
            .lines()
            .filter_map(|l| l.split('#').next())
            .filter_map(|l| {
                let mut p = l.split_whitespace();
                Some((p.next()?, p.next()?))
            })
            .collect();
        Self { freq, curated }
    }

    pub fn contains(&self, word: &str) -> bool {
        self.freq.contains_key(word)
    }

    fn has_prefix(&self, prefix: &str) -> bool {
        self.freq.keys().any(|w| w.starts_with(prefix))
    }

    /// A better spelling for `word`, or `None` when it is fine as typed.
    /// `partial` marks the word still being typed, so prefixes are fine.
    pub fn correct(&self, word: &str, partial: bool) -> Option<String> {
        if word.chars().count() < 3 || word.chars().any(|c| c.is_ascii_digit()) {
            return None;
        }
        if self.contains(word) || (partial && self.has_prefix(word)) {
            return None;
        }
        if let Some(right) = self.curated.get(word) {
            return Some((*right).to_string());
        }
        let len = word.chars().count();
        let max = match len {
            0..=3 => 0,
            4..=5 => 1,
            _ => 2,
        };
        if max == 0 {
            return None;
        }
        let w: Vec<char> = word.chars().collect();
        let mut best: Option<(usize, u32, &str)> = None;
        for (cand, &n) in &self.freq {
            let cl = cand.chars().count();
            if cl.abs_diff(len) > max || cand.chars().next() != w.first().copied() && max == 1 {
                continue;
            }
            let c: Vec<char> = cand.chars().collect();
            let d = distance(&w, &c, max);
            if d <= max && best.is_none_or(|(bd, bn, bc)| (d, !n, cand.as_str()) < (bd, !bn, bc)) {
                best = Some((d, n, cand));
            }
        }
        best.map(|(_, _, c)| c.to_string())
    }
}

/// Optimal string alignment distance (insert, delete, substitute, swap),
/// giving up with `max + 1` once it can't come in under `max`.
fn distance(a: &[char], b: &[char], max: usize) -> usize {
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        let mut row_min = usize::MAX;
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
            row_min = row_min.min(v);
        }
        if row_min > max {
            return max + 1;
        }
    }
    d[n][m]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab() -> Vocabulary {
        Vocabulary::from_texts([
            "forgive us our sins as we forgive",
            "divorce is hateful and the sabbath is holy",
            "love anxiety anxious",
        ])
    }

    #[test]
    fn keeps_known_words_and_prefixes() {
        let v = vocab();
        assert_eq!(v.correct("forgive", false), None);
        assert_eq!(v.correct("forg", true), None);
    }

    #[test]
    fn fixes_typos_and_transpositions() {
        let v = vocab();
        assert_eq!(v.correct("anxietyy", false).as_deref(), Some("anxiety"));
        assert_eq!(v.correct("forgvie", false).as_deref(), Some("forgive"));
        assert_eq!(v.correct("sabbeth", false).as_deref(), Some("sabbath"));
    }

    #[test]
    fn curated_list_wins() {
        let v = vocab();
        assert_eq!(v.correct("devorce", false).as_deref(), Some("divorce"));
    }

    #[test]
    fn leaves_hopeless_words_alone() {
        assert_eq!(vocab().correct("zzzzzz", false), None);
    }
}
