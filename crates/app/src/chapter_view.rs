//! Turns a chapter's `Block`s into styled GPUI text.
//!
//! GPUI text runs can't change font size. Verse numbers are ASCII digits
//! marked in `verse_numbers` so the caller can render them with the OpenType
//! `sups` feature; ASCII keeps GPUI's wrapper from breaking inside "17".
//! Footnote markers use Unicode superscript letters.

use std::ops::Range;

use bible_core::{Block, BlockKind, Span, VerseId, VerseSet};
use gpui_kit::{FontStyle, FontWeight, HighlightStyle, Hsla};

pub struct Palette {
    pub verse_number: Hsla,
    pub jesus: Hsla,
    pub note_marker: Hsla,
    pub target_background: Hsla,
    /// Saved (highlighted) verses.
    pub saved_background: Hsla,
    /// Verses picked by clicking their numbers, before saving.
    pub selected_background: Hsla,
}

pub struct Options {
    pub red_letter: bool,
    pub show_verse_numbers: bool,
    pub italic_added: bool,
    /// Verse to highlight after jumping to a reference.
    pub target: Option<VerseId>,
    /// Saved verses of this chapter, shown highlighted.
    pub saved: VerseSet,
    /// Verses the user has selected.
    pub selected: VerseSet,
}

/// One block flattened to a string plus styling and clickable note ranges.
pub struct RenderedBlock {
    pub text: String,
    pub highlights: Vec<(Range<usize>, HighlightStyle)>,
    /// Byte range of each note marker, with the note id.
    pub notes: Vec<(Range<usize>, u32)>,
    /// Verses that start in this block.
    pub verses: Vec<u16>,
    /// Byte ranges of verse number labels.
    pub verse_numbers: Vec<Range<usize>>,
    /// Where each verse's text starts: (byte offset, first verse, last
    /// verse). A block that continues a verse starts with it at offset 0;
    /// merged verses ("17-18") cover more than one number.
    pub segments: Vec<(usize, u16, u16)>,
}

impl RenderedBlock {
    /// The verse (first, last) under byte `index`.
    pub fn verse_at(&self, index: usize) -> Option<(u16, u16)> {
        self.segments
            .iter()
            .rev()
            .find(|(start, _, _)| *start <= index)
            .map(|(_, first, last)| (*first, *last))
    }
}

/// a..z without q (no superscript q in Unicode), then repeats.
pub fn note_marker(index: usize) -> &'static str {
    const LETTERS: [&str; 25] = [
        "ᵃ", "ᵇ", "ᶜ", "ᵈ", "ᵉ", "ᶠ", "ᵍ", "ʰ", "ⁱ", "ʲ", "ᵏ", "ˡ", "ᵐ", "ⁿ", "ᵒ", "ᵖ", "ʳ", "ˢ",
        "ᵗ", "ᵘ", "ᵛ", "ʷ", "ˣ", "ʸ", "ᶻ",
    ];
    LETTERS[index % LETTERS.len()]
}

/// `verse_in_progress` is the verse that the previous block ended in, for
/// blocks that continue a verse without restating its number.
pub fn render_block(
    block: &Block,
    verse_in_progress: Option<u16>,
    chapter_verse: impl Fn(u16) -> VerseId,
    note_index: &dyn Fn(u32) -> usize,
    palette: &Palette,
    options: &Options,
) -> RenderedBlock {
    let mut out = RenderedBlock {
        text: String::new(),
        highlights: Vec::new(),
        notes: Vec::new(),
        verses: Vec::new(),
        verse_numbers: Vec::new(),
        segments: Vec::new(),
    };
    if let Some(v) = verse_in_progress {
        out.segments.push((0, v, v));
    }

    for span in &block.spans {
        match span {
            Span::Verse { num, end } => {
                out.verses.push(*num);
                if options.show_verse_numbers {
                    if !out.text.is_empty() && !out.text.ends_with(' ') {
                        out.text.push(' ');
                    }
                    out.segments
                        .push((out.text.len(), *num, end.unwrap_or(*num)));
                    let label = match end {
                        Some(e) => format!("{num}-{e}"),
                        None => num.to_string(),
                    };
                    let start = out.text.len();
                    out.text.push_str(&label);
                    out.highlights.push((
                        start..out.text.len(),
                        HighlightStyle {
                            color: Some(palette.verse_number),
                            ..Default::default()
                        },
                    ));
                    out.verse_numbers.push(start..out.text.len());
                    out.text.push('\u{202F}');
                } else {
                    out.segments
                        .push((out.text.len(), *num, end.unwrap_or(*num)));
                }
            }
            Span::Text { text, style } => {
                let start = out.text.len();
                out.text.push_str(text);
                let mut hl = HighlightStyle::default();
                let mut styled = false;
                if style.jesus && options.red_letter {
                    hl.color = Some(palette.jesus);
                    styled = true;
                }
                if (style.added && options.italic_added) || style.emphasis {
                    hl.font_style = Some(FontStyle::Italic);
                    styled = true;
                }
                if styled {
                    out.highlights.push((start..out.text.len(), hl));
                }
            }
            Span::Note { id } => {
                let start = out.text.len();
                out.text.push_str(note_marker(note_index(*id)));
                let range = start..out.text.len();
                out.highlights.push((
                    range.clone(),
                    HighlightStyle {
                        color: Some(palette.note_marker),
                        font_weight: Some(FontWeight::BOLD),
                        ..Default::default()
                    },
                ));
                out.notes.push((range, *id));
            }
        }
    }

    // Backgrounds per verse, weakest first: merging lets the later win.
    let len = out.text.len();
    let covers = |set: &VerseSet, first: u16, last: u16| (first..=last).any(|v| set.contains(v));
    // One range per run of consecutive verses, so the space between two
    // highlighted verses is covered too and they read as a single highlight.
    let mut spans: Vec<(Range<usize>, [bool; 3])> = Vec::new();
    for (ix, &(start, first, last)) in out.segments.iter().enumerate() {
        let end = out.segments.get(ix + 1).map_or(len, |s| s.0);
        // Leave out the space before the next verse number.
        let end = start + out.text[start..end].trim_end().len();
        if start >= end {
            continue;
        }
        let target = options
            .target
            .is_some_and(|t| (first..=last).any(|v| chapter_verse(v) == t));
        spans.push((
            start..end,
            [
                covers(&options.saved, first, last),
                target,
                covers(&options.selected, first, last),
            ],
        ));
    }
    let colors = [
        palette.saved_background,
        palette.target_background,
        palette.selected_background,
    ];
    for (kind, color) in colors.into_iter().enumerate() {
        let mut run: Option<Range<usize>> = None;
        for (range, on) in &spans {
            if on[kind] {
                run = Some(match run {
                    Some(r) => r.start..range.end,
                    None => range.clone(),
                });
            } else if let Some(r) = run.take() {
                out.highlights.push((r, background(color)));
            }
        }
        if let Some(r) = run {
            out.highlights.push((r, background(color)));
        }
    }
    out.highlights = merge_highlights(out.highlights);
    out
}

fn background(color: Hsla) -> HighlightStyle {
    HighlightStyle {
        background_color: Some(color),
        ..Default::default()
    }
}

/// Build explicit text runs so verse numbers can carry the `sups` feature,
/// which highlights can't express.
///
/// `number_family` sets the typeface for verse numbers (`None` keeps the body font).
pub fn build_runs(
    rendered: &RenderedBlock,
    base: &gpui_kit::TextStyle,
    number_family: Option<&gpui_kit::SharedString>,
) -> Vec<gpui_kit::TextRun> {
    let len = rendered.text.len();
    let mut points: Vec<usize> = vec![0, len];
    for (r, _) in &rendered.highlights {
        points.extend([r.start, r.end]);
    }
    for r in &rendered.verse_numbers {
        points.extend([r.start, r.end]);
    }
    points.sort_unstable();
    points.dedup();

    let mut sups = base.font();
    sups.features = gpui_kit::FontFeatures(std::sync::Arc::new(vec![("sups".into(), 1)]));

    let mut runs = Vec::new();
    for pair in points.windows(2) {
        let piece = pair[0]..pair[1];
        if piece.is_empty() {
            continue;
        }
        let mut style = base.clone();
        for (range, hl) in &rendered.highlights {
            if range.start <= piece.start && piece.end <= range.end {
                style = style.highlight(*hl);
            }
        }
        let mut run = style.to_run(piece.len());
        if rendered
            .verse_numbers
            .iter()
            .any(|r| r.start <= piece.start && piece.end <= r.end)
        {
            run.font.features = sups.features.clone();
            if let Some(family) = number_family {
                run.font.family = family.clone();
            }
        }
        runs.push(run);
    }
    runs
}

/// GPUI expects highlight ranges in order and non-overlapping. The target
/// background overlaps other styles, so split everything at boundaries and
/// combine the styles that cover each piece.
fn merge_highlights(
    highlights: Vec<(Range<usize>, HighlightStyle)>,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut points: Vec<usize> = highlights
        .iter()
        .flat_map(|(r, _)| [r.start, r.end])
        .collect();
    points.sort_unstable();
    points.dedup();
    let mut merged: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    for pair in points.windows(2) {
        let piece = pair[0]..pair[1];
        let mut style = HighlightStyle::default();
        let mut any = false;
        for (range, hl) in &highlights {
            if range.start <= piece.start && piece.end <= range.end {
                style = style.highlight(*hl);
                any = true;
            }
        }
        if any {
            merged.push((piece, style));
        }
    }
    merged
}

pub fn block_is_poetry(kind: &BlockKind) -> bool {
    matches!(kind, BlockKind::Poetry(_) | BlockKind::PoetryRight)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bible_core::TextStyle;

    fn palette() -> Palette {
        let c = gpui_kit::black();
        Palette {
            verse_number: c,
            jesus: c,
            note_marker: c,
            target_background: c,
            saved_background: c,
            selected_background: c,
        }
    }

    #[test]
    fn verse_numbers_and_notes() {
        let mut block = Block::new(BlockKind::Paragraph);
        block.spans.push(Span::Verse { num: 16, end: None });
        block.push_text(
            "For God so loved",
            TextStyle {
                jesus: true,
                ..Default::default()
            },
        );
        block.spans.push(Span::Note { id: 7 });
        block.spans.push(Span::Verse {
            num: 17,
            end: Some(18),
        });
        block.push_text("For God did not", TextStyle::default());
        let options = Options {
            red_letter: true,
            show_verse_numbers: true,
            italic_added: false,
            target: Some(VerseId::new(43, 3, 17)),
            saved: VerseSet::default(),
            selected: VerseSet::default(),
        };
        let r = render_block(
            &block,
            None,
            |v| VerseId::new(43, 3, v),
            &|_| 0,
            &palette(),
            &options,
        );
        assert_eq!(
            r.text,
            "16\u{202F}For God so lovedᵃ 17-18\u{202F}For God did not"
        );
        assert_eq!(&r.text[r.verse_numbers[1].clone()], "17-18");
        assert_eq!(r.verses, vec![16, 17]);
        assert_eq!(&r.text[r.notes[0].0.clone()], "ᵃ");
        // Ranges are sorted and non-overlapping.
        for pair in r.highlights.windows(2) {
            assert!(pair[0].0.end <= pair[1].0.start);
        }
        let target = r
            .highlights
            .iter()
            .filter(|(_, h)| h.background_color.is_some());
        assert!(target.count() >= 1);
    }

    #[test]
    fn target_verse_continues_into_next_block() {
        let mut block = Block::new(BlockKind::Poetry(2));
        block.push_text("He leads me beside quiet waters.", TextStyle::default());
        let options = Options {
            red_letter: false,
            show_verse_numbers: true,
            italic_added: false,
            target: Some(VerseId::new(19, 23, 2)),
            saved: VerseSet::default(),
            selected: VerseSet::default(),
        };
        let highlighted = |in_progress| {
            render_block(
                &block,
                in_progress,
                |v| VerseId::new(19, 23, v),
                &|_| 0,
                &palette(),
                &options,
            )
            .highlights
            .iter()
            .any(|(r, h)| h.background_color.is_some() && r.start == 0)
        };
        assert!(highlighted(Some(2)));
        assert!(!highlighted(Some(1)));
        assert!(!highlighted(None));
    }

    #[test]
    fn hiding_verse_numbers() {
        let mut block = Block::new(BlockKind::Paragraph);
        block.spans.push(Span::Verse { num: 1, end: None });
        block.push_text("In the beginning", TextStyle::default());
        let options = Options {
            red_letter: false,
            show_verse_numbers: false,
            italic_added: false,
            target: None,
            saved: VerseSet::default(),
            selected: VerseSet::default(),
        };
        let r = render_block(
            &block,
            None,
            |v| VerseId::new(1, 1, v),
            &|_| 0,
            &palette(),
            &options,
        );
        assert_eq!(r.text, "In the beginning");
        assert!(r.highlights.is_empty());
    }

    #[test]
    fn saved_and_selected_verses_get_backgrounds() {
        let mut block = Block::new(BlockKind::Paragraph);
        for (n, text) in [(1, "One."), (2, "Two."), (3, "Three.")] {
            block.spans.push(Span::Verse { num: n, end: None });
            block.push_text(text, TextStyle::default());
        }
        let mut palette = palette();
        palette.saved_background = gpui_kit::red();
        palette.selected_background = gpui_kit::blue();
        let options = Options {
            red_letter: false,
            show_verse_numbers: true,
            italic_added: false,
            target: None,
            saved: VerseSet::new([1, 2]),
            selected: VerseSet::new([2]),
        };
        let r = render_block(
            &block,
            None,
            |v| VerseId::new(43, 1, v),
            &|_| 0,
            &palette,
            &options,
        );
        let background_at = |needle: &str| {
            let at = r.text.find(needle).unwrap();
            r.highlights
                .iter()
                .find(|(range, h)| range.contains(&at) && h.background_color.is_some())
                .and_then(|(_, h)| h.background_color)
        };
        assert_eq!(background_at("One"), Some(gpui_kit::red()));
        // Selection wins over the saved highlight.
        assert_eq!(background_at("Two"), Some(gpui_kit::blue()));
        assert_eq!(background_at("Three"), None);
        // Saved verses 1 and 2 are one highlight: the space between is covered.
        let has_bg = |at: usize| {
            r.highlights
                .iter()
                .any(|(range, h)| range.contains(&at) && h.background_color.is_some())
        };
        let gap = r.text.find("One.").unwrap() + 4;
        assert_eq!(&r.text[gap..gap + 1], " ");
        assert!(has_bg(gap));
        // The space after the last highlighted verse isn't.
        let gap = r.text.find("Two.").unwrap() + 4;
        assert_eq!(&r.text[gap..gap + 1], " ");
        assert!(!has_bg(gap));
        assert_eq!(r.verse_at(r.text.find("Two").unwrap()), Some((2, 2)));
        assert_eq!(r.verse_at(0), Some((1, 1)));
    }
}
