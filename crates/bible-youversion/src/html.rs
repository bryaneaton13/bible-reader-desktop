//! Turns YouVersion passage HTML into the app's `Block` / `Span` model.
//!
//! The API returns USFM-styled markup: block `div`s whose class is the USFM
//! paragraph marker (`p`, `q1`, `s1`, `d`, ...), and inline `span`s such as
//! `yv-v` (verse start, `v` attribute), `yv-vlbl` (the printed number, which
//! we skip because the app draws its own), `wj`, `nd`, `add`, and `yv-n f` /
//! `yv-n x` for footnotes and cross-references. The parser is tolerant:
//! unknown classes keep their text, unknown wrappers are looked through.

use bible_core::{Block, BlockKind, ChapterRef, Note, NoteKind, Span, TextStyle, VerseId};

/// A chapter ready to render.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Chapter {
    pub blocks: Vec<Block>,
    pub notes: Vec<Note>,
}

#[derive(Debug)]
enum Token {
    Open {
        tag: String,
        classes: Vec<String>,
        verse: Option<String>,
        self_closing: bool,
    },
    Close(String),
    Text(String),
}

const VOID_TAGS: &[&str] = &["br", "hr", "img", "meta", "link", "input", "wbr"];

fn tokenize(html: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        if !rest.starts_with('<') {
            let at = rest.find('<').unwrap_or(rest.len());
            tokens.push(Token::Text(decode_entities(&rest[..at])));
            rest = &rest[at..];
            continue;
        }
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.split_once("-->").map_or("", |(_, r)| r);
            continue;
        }
        let Some(end) = find_tag_end(rest) else {
            // A stray '<' with no closing '>': keep it as text.
            tokens.push(Token::Text(decode_entities(rest)));
            break;
        };
        let inner = &rest[1..end];
        rest = &rest[end + 1..];
        if let Some(name) = inner.strip_prefix('/') {
            tokens.push(Token::Close(name.trim().to_ascii_lowercase()));
        } else if inner.starts_with('!') || inner.starts_with('?') {
            // doctype / processing instruction
        } else {
            tokens.push(parse_open(inner));
        }
    }
    tokens
}

/// Index of the `>` that ends the tag at the start of `s`, skipping quoted
/// attribute values.
fn find_tag_end(s: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

fn parse_open(inner: &str) -> Token {
    let inner = inner.trim_end();
    let self_closing = inner.ends_with('/');
    let inner = inner.trim_end_matches('/');
    let (tag, mut attrs) = match inner.find(char::is_whitespace) {
        Some(i) => (&inner[..i], inner[i..].trim_start()),
        None => (inner, ""),
    };
    let tag = tag.to_ascii_lowercase();
    let mut classes = Vec::new();
    let mut verse = None;
    while !attrs.is_empty() {
        let name_end = attrs
            .find(|c: char| c == '=' || c.is_whitespace())
            .unwrap_or(attrs.len());
        let name = attrs[..name_end].to_ascii_lowercase();
        attrs = attrs[name_end..].trim_start();
        let value = if let Some(after) = attrs.strip_prefix('=') {
            let after = after.trim_start();
            let (value, rest) = match after.chars().next() {
                Some(q @ ('"' | '\'')) => {
                    let body = &after[1..];
                    match body.find(q) {
                        Some(i) => (&body[..i], &body[i + 1..]),
                        None => (body, ""),
                    }
                }
                _ => {
                    let i = after.find(char::is_whitespace).unwrap_or(after.len());
                    (&after[..i], &after[i..])
                }
            };
            attrs = rest.trim_start();
            value.to_string()
        } else {
            String::new()
        };
        match name.as_str() {
            "class" => classes = value.split_whitespace().map(str::to_string).collect(),
            "v" => verse = Some(value),
            _ => {}
        }
    }
    Token::Open {
        self_closing: self_closing || VOID_TAGS.contains(&tag.as_str()),
        tag,
        classes,
        verse,
    }
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let decoded = rest.find(';').filter(|&i| i <= 10).and_then(|semi| {
            let name = &rest[1..semi];
            let c = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => ' ',
                _ => {
                    let num = name.strip_prefix('#')?;
                    let code = match num.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => num.parse().ok()?,
                    };
                    char::from_u32(code)?
                }
            };
            Some((c, semi))
        });
        match decoded {
            Some((c, semi)) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

enum BlockClass {
    Kind(BlockKind),
    /// Chapter labels and the like: the app draws its own.
    Skip,
}

fn numbered(class: &str, base: &str) -> bool {
    class
        .strip_prefix(base)
        .is_some_and(|n| n.parse::<u8>().is_ok())
}

fn block_class(classes: &[String]) -> Option<BlockClass> {
    use BlockClass::{Kind, Skip};
    for c in classes {
        let c = c.as_str();
        let found = match c {
            "p" | "po" | "ip" | "lh" | "lf" => Kind(BlockKind::Paragraph),
            "m" | "mi" | "nb" => Kind(BlockKind::Continuation),
            "q" | "q1" | "qm" | "qm1" => Kind(BlockKind::Poetry(1)),
            "q2" | "qm2" => Kind(BlockKind::Poetry(2)),
            "q3" | "q4" | "qm3" | "qm4" => Kind(BlockKind::Poetry(3)),
            "qr" => Kind(BlockKind::PoetryRight),
            "qc" | "pc" => Kind(BlockKind::Centered),
            "qa" | "sp" => Kind(BlockKind::Label),
            "s" | "s1" => Kind(BlockKind::Heading(1)),
            "s2" => Kind(BlockKind::Heading(2)),
            "r" | "mr" | "sr" => Kind(BlockKind::ParallelRefs),
            "d" => Kind(BlockKind::PsalmTitle),
            "b" => Kind(BlockKind::Blank),
            "li" | "li1" => Kind(BlockKind::ListItem(1)),
            "li2" => Kind(BlockKind::ListItem(2)),
            "li3" | "li4" => Kind(BlockKind::ListItem(3)),
            "c" | "cl" | "h" => Skip,
            _ if numbered(c, "s") => Kind(BlockKind::Heading(3)),
            _ if c.starts_with("ms") || c.starts_with("mt") => Kind(BlockKind::MajorHeading),
            _ if c.starts_with("pi") || c.starts_with("pm") => Kind(BlockKind::Paragraph),
            _ => continue,
        };
        return Some(found);
    }
    None
}

#[derive(Clone, Copy)]
enum StyleKind {
    Jesus,
    Added,
    DivineName,
    Emphasis,
}

impl StyleKind {
    fn slot(self) -> usize {
        self as usize
    }
}

fn style_kind(classes: &[String]) -> Option<StyleKind> {
    classes.iter().find_map(|c| match c.as_str() {
        "wj" => Some(StyleKind::Jesus),
        "add" => Some(StyleKind::Added),
        "nd" => Some(StyleKind::DivineName),
        "tl" | "bk" | "k" | "qs" | "em" | "it" => Some(StyleKind::Emphasis),
        _ => None,
    })
}

enum Frame {
    Block,
    Style(StyleKind),
    Skip,
    Note,
    Other,
}

struct Builder {
    chapter: ChapterRef,
    blocks: Vec<Block>,
    notes: Vec<Note>,
    current: Option<Block>,
    note: Option<(NoteKind, String)>,
    stack: Vec<(String, Frame)>,
    skip: u32,
    styles: [u32; 4],
    last_verse: Option<u16>,
}

impl Builder {
    fn style(&self) -> TextStyle {
        TextStyle {
            jesus: self.styles[StyleKind::Jesus.slot()] > 0,
            added: self.styles[StyleKind::Added.slot()] > 0,
            divine_name: self.styles[StyleKind::DivineName.slot()] > 0,
            emphasis: self.styles[StyleKind::Emphasis.slot()] > 0,
        }
    }

    fn finish_block(&mut self) {
        let Some(mut block) = self.current.take() else {
            return;
        };
        while let Some(Span::Text { text, .. }) = block.spans.last_mut() {
            let trimmed = text.trim_end().len();
            text.truncate(trimmed);
            if text.is_empty() {
                block.spans.pop();
            } else {
                break;
            }
        }
        if !block.is_empty() || block.kind == BlockKind::Blank {
            self.blocks.push(block);
        }
    }

    fn start_block(&mut self, kind: BlockKind) {
        self.finish_block();
        self.current = Some(Block::new(kind));
    }

    fn block(&mut self) -> &mut Block {
        self.current
            .get_or_insert_with(|| Block::new(BlockKind::Paragraph))
    }

    fn text(&mut self, text: &str) {
        if self.skip > 0 {
            return;
        }
        let collapsed = collapse_whitespace(text);
        if let Some((_, buf)) = self.note.as_mut() {
            let at_start = buf.is_empty() || buf.ends_with(' ');
            buf.push_str(if at_start {
                collapsed.trim_start()
            } else {
                &collapsed
            });
            return;
        }
        if collapsed.trim().is_empty() && self.current.is_none() {
            return;
        }
        let style = self.style();
        let block = self.block();
        let after_space = match block.spans.last() {
            None | Some(Span::Verse { .. }) => true,
            Some(Span::Text { text, .. }) => text.ends_with(' '),
            Some(Span::Note { .. }) => false,
        };
        let text = if after_space {
            collapsed.trim_start()
        } else {
            &collapsed
        };
        block.push_text(text, style);
    }

    fn verse(&mut self, raw: &str) {
        let mut parts = raw.split(['-', '–']);
        let Some(num) = parts.next().and_then(|n| n.trim().parse::<u16>().ok()) else {
            return;
        };
        let end = parts.next().and_then(|n| n.trim().parse::<u16>().ok());
        // Some markup repeats the marker when a verse continues in a new block.
        if self.last_verse == Some(num) {
            return;
        }
        self.last_verse = Some(num);
        self.block().spans.push(Span::Verse { num, end });
    }

    fn open(&mut self, tag: String, classes: Vec<String>, verse: Option<String>, empty: bool) {
        if tag == "br" {
            self.text(" ");
            return;
        }
        let has = |name: &str| classes.iter().any(|c| c == name);
        let frame = if tag == "span" && has("yv-v") {
            if let Some(v) = verse {
                self.verse(&v);
            }
            Frame::Other
        } else if tag == "span" && (has("yv-vlbl") || has("vlbl") || has("va") || has("vp")) {
            // The printed verse number, or an alternate one ("va", "vp"): the
            // app draws its own.
            self.skip += 1;
            Frame::Skip
        } else if tag == "span" && has("yv-n") {
            if self.note.is_none() {
                let kind = if has("x") {
                    NoteKind::CrossReference
                } else {
                    NoteKind::Footnote
                };
                self.note = Some((kind, String::new()));
                Frame::Note
            } else {
                Frame::Other
            }
        } else if self.note.is_some() {
            if has("fr") || has("xo") {
                // The note's own "3:16" origin label.
                self.skip += 1;
                Frame::Skip
            } else {
                Frame::Other
            }
        } else if let Some(kind) = style_kind(&classes).filter(|_| tag == "span") {
            self.styles[kind.slot()] += 1;
            Frame::Style(kind)
        } else if matches!(tag.as_str(), "div" | "p")
            && let Some(class) = block_class(&classes)
        {
            match class {
                BlockClass::Kind(kind) => {
                    self.start_block(kind);
                    Frame::Block
                }
                BlockClass::Skip => {
                    self.finish_block();
                    self.skip += 1;
                    Frame::Skip
                }
            }
        } else {
            Frame::Other
        };
        if empty {
            // Void elements get no close tag; undo any counters now.
            self.close_frame(frame);
        } else {
            self.stack.push((tag, frame));
        }
    }

    fn close_frame(&mut self, frame: Frame) {
        match frame {
            Frame::Block => self.finish_block(),
            Frame::Style(kind) => {
                let slot = &mut self.styles[kind.slot()];
                *slot = slot.saturating_sub(1);
            }
            Frame::Skip => self.skip = self.skip.saturating_sub(1),
            Frame::Note => {
                if let Some((kind, text)) = self.note.take() {
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        let id = self.notes.len() as u32 + 1;
                        let verse = VerseId::new(
                            self.chapter.book,
                            self.chapter.chapter,
                            self.last_verse.unwrap_or(1),
                        );
                        self.notes.push(Note {
                            id,
                            kind,
                            verse,
                            text,
                        });
                        self.block().spans.push(Span::Note { id });
                    }
                }
            }
            Frame::Other => {}
        }
    }

    fn close(&mut self, tag: &str) {
        let Some(at) = self.stack.iter().rposition(|(t, _)| t == tag) else {
            return;
        };
        while self.stack.len() > at {
            if let Some((_, frame)) = self.stack.pop() {
                self.close_frame(frame);
            }
        }
    }
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(c);
        }
    }
    if space {
        out.push(' ');
    }
    out
}

/// Parse one chapter's passage HTML. `chapter` tags footnotes with a verse.
pub fn parse_chapter(html: &str, chapter: ChapterRef) -> Chapter {
    let mut b = Builder {
        chapter,
        blocks: Vec::new(),
        notes: Vec::new(),
        current: None,
        note: None,
        stack: Vec::new(),
        skip: 0,
        styles: [0; 4],
        last_verse: None,
    };
    for token in tokenize(html) {
        match token {
            Token::Open {
                tag,
                classes,
                verse,
                self_closing,
            } => b.open(tag, classes, verse, self_closing),
            Token::Close(tag) => b.close(&tag),
            Token::Text(text) => b.text(&text),
        }
    }
    while let Some((_, frame)) = b.stack.pop() {
        b.close_frame(frame);
    }
    b.finish_block();
    Chapter {
        blocks: b.blocks,
        notes: b.notes,
    }
}

/// Plain text of a chapter's verses, for copy: `(verse, text)` in order.
/// Merged ranges are keyed by their first verse.
pub fn verse_texts(blocks: &[Block]) -> Vec<(u16, String)> {
    let mut out: Vec<(u16, String)> = Vec::new();
    for block in blocks.iter().filter(|b| !b.kind.is_heading()) {
        for span in &block.spans {
            match span {
                Span::Verse { num, .. } => out.push((*num, String::new())),
                Span::Text { text, .. } => {
                    if let Some((_, buf)) = out.last_mut() {
                        if !buf.is_empty() && !buf.ends_with(' ') && !text.starts_with(' ') {
                            buf.push(' ');
                        }
                        buf.push_str(text);
                    }
                }
                Span::Note { .. } => {}
            }
        }
    }
    for (_, text) in &mut out {
        *text = text.trim().to_string();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    // Public-domain KJV wording, laid out the way the API marks it up.
    const JOHN_3: &str = r#"<div><div class="s1">The New Birth</div><div class="p"><span class="yv-v" v="14"></span><span class="yv-vlbl">14</span>And as Moses lifted up the serpent in the wilderness, even so must the Son of man be lifted up:</div><div class="p"><span class="yv-v" v="16"></span><span class="yv-vlbl">16</span><span class="wj">For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish<span class="yv-n f"><span class="fr">3:16 </span><span class="ft">Or <span class="add">only</span> Son.</span></span>, but have everlasting life.</span></div></div>"#;

    const PSALM_23: &str = r#"<div><div class="d"><span class="yv-v" v="1"></span><span class="yv-vlbl">1</span>A Psalm of David.</div><div class="q1">The <span class="nd">Lord</span> is my shepherd;</div><div class="q2"><span class="yv-v" v="2"></span><span class="yv-vlbl">2</span>He maketh me to lie down in green pastures:</div><div class="b"></div><div class="q1">he leadeth me beside the still waters.</div></div>"#;

    fn kinds(c: &Chapter) -> Vec<BlockKind> {
        c.blocks.iter().map(|b| b.kind.clone()).collect()
    }

    #[test]
    fn parses_headings_verses_red_letter_and_notes() {
        let c = parse_chapter(JOHN_3, ChapterRef::new(43, 3));
        assert_eq!(
            kinds(&c),
            [
                BlockKind::Heading(1),
                BlockKind::Paragraph,
                BlockKind::Paragraph
            ]
        );
        assert_eq!(
            c.blocks[0].spans,
            [Span::Text {
                text: "The New Birth".into(),
                style: TextStyle::default()
            }]
        );
        // Verse marker first, printed label dropped.
        assert_eq!(c.blocks[1].spans[0], Span::Verse { num: 14, end: None });
        let Span::Text { text, style } = &c.blocks[2].spans[1] else {
            panic!("expected text after verse marker");
        };
        assert!(text.starts_with("For God so loved"));
        assert!(style.jesus);
        // Footnote anchor inside the red-letter run, text without "3:16".
        assert_eq!(c.notes.len(), 1);
        assert_eq!(c.notes[0].kind, NoteKind::Footnote);
        assert_eq!(c.notes[0].text, "Or only Son.");
        assert_eq!(c.notes[0].verse, VerseId::new(43, 3, 16));
        assert!(c.blocks[2].spans.contains(&Span::Note { id: 1 }));
        // Text after the note keeps the red-letter style and punctuation.
        assert!(c.blocks[2].spans.iter().any(|s| matches!(
            s,
            Span::Text { text, style } if text.starts_with(", but have") && style.jesus
        )));
    }

    #[test]
    fn parses_poetry_divine_name_and_stanza_break() {
        let c = parse_chapter(PSALM_23, ChapterRef::new(19, 23));
        assert_eq!(
            kinds(&c),
            [
                BlockKind::PsalmTitle,
                BlockKind::Poetry(1),
                BlockKind::Poetry(2),
                BlockKind::Blank,
                BlockKind::Poetry(1)
            ]
        );
        let line = &c.blocks[1].spans;
        assert!(matches!(&line[0], Span::Text { text, .. } if text == "The "));
        assert!(
            matches!(&line[1], Span::Text { text, style } if text == "Lord" && style.divine_name)
        );
        assert_eq!(c.blocks[0].spans[0], Span::Verse { num: 1, end: None });
        assert_eq!(c.blocks[2].spans[0], Span::Verse { num: 2, end: None });
    }

    #[test]
    fn alternate_verse_numbers_are_not_part_of_the_text() {
        let html = r#"<div class="q"><span class="va">1</span> <span class="nd">Hashem</span> is my Ro'eh;</div>"#;
        let c = parse_chapter(html, ChapterRef::new(19, 23));
        let text: String = c.blocks[0]
            .spans
            .iter()
            .filter_map(|s| match s {
                Span::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "Hashem is my Ro'eh;");
    }

    #[test]
    fn merged_verse_ranges_and_repeated_markers() {
        let html = r#"<div class="p"><span class="yv-v" v="15-16"></span><span class="yv-vlbl">15-16</span>Merged text.</div><div class="p"><span class="yv-v" v="15-16"></span>More of the same verse.</div>"#;
        let c = parse_chapter(html, ChapterRef::new(1, 1));
        assert_eq!(
            c.blocks[0].spans[0],
            Span::Verse {
                num: 15,
                end: Some(16)
            }
        );
        assert!(
            !c.blocks[1]
                .spans
                .iter()
                .any(|s| matches!(s, Span::Verse { .. }))
        );
    }

    #[test]
    fn decodes_entities_and_collapses_whitespace() {
        let html = "<div class=\"p\">  Tom &amp; Jerry&#39;s\n   &quot;day&quot; &#x2014; ok&nbsp;now  </div>";
        let c = parse_chapter(html, ChapterRef::new(1, 1));
        assert_eq!(
            c.blocks[0].spans,
            [Span::Text {
                text: "Tom & Jerry's \"day\" — ok now".into(),
                style: TextStyle::default()
            }]
        );
    }

    #[test]
    fn cross_reference_notes_and_unknown_markup() {
        let html = r#"<custom><div class="p">See<span class="yv-n x"><span class="xo">1:1 </span><span class="xt">Gen 1:1</span></span> this<br/>here <em>now</em></div></custom>"#;
        let c = parse_chapter(html, ChapterRef::new(43, 1));
        assert_eq!(c.notes.len(), 1);
        assert_eq!(c.notes[0].kind, NoteKind::CrossReference);
        assert_eq!(c.notes[0].text, "Gen 1:1");
        let text: String = c.blocks[0]
            .spans
            .iter()
            .filter_map(|s| match s {
                Span::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "See this here now");
    }

    #[test]
    fn chapter_labels_are_skipped_and_stray_text_becomes_a_paragraph() {
        let html = r#"<div><div class="c">3</div>Loose words<div class="m">after</div></div>"#;
        let c = parse_chapter(html, ChapterRef::new(1, 1));
        assert_eq!(kinds(&c), [BlockKind::Paragraph, BlockKind::Continuation]);
    }

    #[test]
    fn extracts_verse_texts_for_copy() {
        let c = parse_chapter(JOHN_3, ChapterRef::new(43, 3));
        let verses = verse_texts(&c.blocks);
        assert_eq!(verses.len(), 2);
        assert_eq!(verses[0].0, 14);
        assert!(verses[1].1.starts_with("For God so loved the world"));
        assert!(verses[1].1.ends_with("but have everlasting life."));
    }

    #[test]
    fn empty_and_garbage_input_do_not_panic() {
        assert_eq!(parse_chapter("", ChapterRef::new(1, 1)), Chapter::default());
        let c = parse_chapter("<div class=\"p\"><span", ChapterRef::new(1, 1));
        assert!(c.blocks.len() <= 1);
        let _ = parse_chapter("</div></span>text<", ChapterRef::new(1, 1));
    }
}
