//! Turns a chapter's `Block`s into styled GPUI text.
//!
//! GPUI text runs can't change font size. Verse numbers are ASCII digits
//! marked in `verse_numbers` so the caller can render them with the OpenType
//! `sups` feature; ASCII keeps GPUI's wrapper from breaking inside "17".
//! Footnote markers use Unicode superscript letters.

use std::ops::Range;

use bible_core::{Block, BlockKind, Span, VerseId};
use gpui_kit::{FontStyle, FontWeight, HighlightStyle, Hsla};

pub struct Palette {
    pub verse_number: Hsla,
    pub jesus: Hsla,
    pub note_marker: Hsla,
    pub target_background: Hsla,
}

pub struct Options {
    pub red_letter: bool,
    pub show_verse_numbers: bool,
    pub italic_added: bool,
    /// Verse to highlight after jumping to a reference.
    pub target: Option<VerseId>,
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
    };
    let mut target_start: Option<usize> = verse_in_progress
        .filter(|v| options.target == Some(chapter_verse(*v)))
        .map(|_| 0);

    for span in &block.spans {
        match span {
            Span::Verse { num, end } => {
                if let Some(start) = target_start.take() {
                    out.highlights
                        .push((start..out.text.len(), target_style(palette)));
                }
                let id = chapter_verse(*num);
                out.verses.push(*num);
                if options.target == Some(id) {
                    target_start = Some(out.text.len());
                }
                if options.show_verse_numbers {
                    if !out.text.is_empty() && !out.text.ends_with(' ') {
                        out.text.push(' ');
                    }
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
    if let Some(start) = target_start {
        out.highlights
            .push((start..out.text.len(), target_style(palette)));
    }
    out.highlights = merge_highlights(out.highlights);
    out
}

/// Build explicit text runs so verse numbers can carry the `sups` feature,
/// which highlights can't express.
pub fn build_runs(rendered: &RenderedBlock, base: &gpui_kit::TextStyle) -> Vec<gpui_kit::TextRun> {
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
        }
        runs.push(run);
    }
    runs
}

fn target_style(palette: &Palette) -> HighlightStyle {
    HighlightStyle {
        background_color: Some(palette.target_background),
        ..Default::default()
    }
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
}
