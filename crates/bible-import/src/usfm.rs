//! Minimal USFM parser covering the markers used by BSB, KJV and WEB
//! (eBible.org / bsb2usfm exports). Unknown markers are ignored with their
//! text kept, so new sources degrade gracefully instead of failing.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use bible_core::{
    Block, BlockKind, Book, ChapterRef, Note, NoteKind, Span, TextStyle, VerseId, book_by_code,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token<'a> {
    Marker {
        name: &'a str,
        closing: bool,
        ends_line: bool,
    },
    Text(&'a str),
}

fn tokenize(src: &str) -> Vec<Token<'_>> {
    let bytes = src.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut text_start = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            i += 1;
            continue;
        }
        if text_start < i {
            tokens.push(Token::Text(&src[text_start..i]));
        }
        let mut j = i + 1;
        if j < bytes.len() && bytes[j] == b'+' {
            j += 1;
        }
        let name_start = j;
        while j < bytes.len() && bytes[j].is_ascii_alphanumeric() {
            j += 1;
        }
        let name = &src[name_start..j];
        let closing = j < bytes.len() && bytes[j] == b'*';
        if closing {
            j += 1;
        }
        let mut ends_line = false;
        if !closing && j < bytes.len() && bytes[j].is_ascii_whitespace() {
            ends_line = bytes[j] == b'\n' || bytes[j] == b'\r';
            j += 1;
        }
        tokens.push(Token::Marker {
            name,
            closing,
            ends_line,
        });
        i = j;
        text_start = j;
    }
    if text_start < bytes.len() {
        tokens.push(Token::Text(&src[text_start..]));
    }
    tokens
}

fn paragraph_kind(name: &str) -> Option<BlockKind> {
    Some(match name {
        "p" | "pi" | "pi1" | "pi2" | "nb" | "lf" | "ph" | "ph1" => BlockKind::Paragraph,
        "m" | "mi" | "pm" | "pmo" => BlockKind::Continuation,
        "pc" => BlockKind::Centered,
        "q" | "q1" => BlockKind::Poetry(1),
        "q2" => BlockKind::Poetry(2),
        "q3" | "q4" => BlockKind::Poetry(3),
        "qm" | "qm1" => BlockKind::Poetry(1),
        "qm2" => BlockKind::Poetry(2),
        "qr" => BlockKind::PoetryRight,
        "qc" => BlockKind::Centered,
        "li" | "li1" => BlockKind::ListItem(1),
        "li2" | "li3" => BlockKind::ListItem(2),
        "b" => BlockKind::Blank,
        "d" => BlockKind::PsalmTitle,
        "s" | "s1" => BlockKind::Heading(1),
        "s2" | "s3" | "s4" => BlockKind::Heading(2),
        "ms" | "ms1" | "ms2" | "mr" => BlockKind::MajorHeading,
        "r" | "sr" => BlockKind::ParallelRefs,
        "qa" | "sp" => BlockKind::Label,
        _ => return None,
    })
}

/// Markers whose whole line is metadata or introduction we don't render.
fn is_line_ignored(name: &str) -> bool {
    matches!(
        name,
        "id" | "ide"
            | "h"
            | "h1"
            | "toc1"
            | "toc2"
            | "toc3"
            | "toca1"
            | "toca2"
            | "toca3"
            | "mt"
            | "mt1"
            | "mt2"
            | "mt3"
            | "mte"
            | "cl"
            | "cp"
            | "cd"
            | "rem"
            | "usfm"
            | "sts"
    ) || (name.starts_with('i') && name != "it")
}

/// Character markers whose content carries `|attr="..."` attributes to strip.
fn has_attributes(name: &str) -> bool {
    matches!(
        name,
        "w" | "wh" | "wg" | "wa" | "ref" | "rb" | "fig" | "jmp"
    )
}

fn style_for(stack: &[&str]) -> TextStyle {
    let mut style = TextStyle::default();
    for name in stack {
        match *name {
            "wj" => style.jesus = true,
            "add" => style.added = true,
            "nd" => style.divine_name = true,
            "tl" | "bk" | "k" | "qs" | "em" | "it" | "sls" => style.emphasis = true,
            _ => {}
        }
    }
    style
}

fn parse_verse_number(s: &str) -> Option<(u16, Option<u16>)> {
    let mut parts = s.split(['-', '–']);
    let digits = |p: &str| -> Option<u16> {
        let d: String = p.chars().take_while(char::is_ascii_digit).collect();
        d.parse().ok()
    };
    let start = digits(parts.next()?)?;
    let end = parts.next().and_then(digits).filter(|&e| e > start);
    Some((start, end))
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct NoteState {
    kind: NoteKind,
    text: String,
    /// Inside `\fr` / `\xo` (origin reference), whose text we drop.
    skipping: bool,
    caller_pending: bool,
}

#[derive(Debug)]
pub struct ParsedBook {
    pub book: &'static Book,
    pub chapters: Vec<(ChapterRef, Vec<Block>)>,
    pub verses: BTreeMap<VerseId, (Option<u16>, String)>,
    pub notes: Vec<Note>,
}

struct Parser<'n> {
    book: &'static Book,
    chapter: u16,
    blocks: Vec<Block>,
    block: Option<Block>,
    /// Body paragraph kind to resume after a chapter break or heading.
    last_body_kind: BlockKind,
    /// True when the next text in the current block should drop leading spaces.
    at_word_start: bool,
    styles: Vec<&'n str>,
    attr_skip: bool,
    ignore_line: bool,
    expect_number: Option<&'static str>,
    verse: Option<VerseId>,
    note: Option<NoteState>,
    next_note_id: &'n mut u32,
    out: ParsedBook,
}

impl<'n> Parser<'n> {
    fn flush_block(&mut self) {
        if let Some(mut block) = self.block.take() {
            if let Some(Span::Text { text, .. }) = block.spans.last_mut() {
                let trimmed = text.trim_end().len();
                text.truncate(trimmed);
                if text.is_empty() {
                    block.spans.pop();
                }
            }
            if !block.is_empty() || block.kind == BlockKind::Blank {
                self.blocks.push(block);
            }
        }
        self.styles.clear();
        self.attr_skip = false;
    }

    fn flush_chapter(&mut self) {
        self.flush_block();
        if self.chapter > 0 {
            let blocks = std::mem::take(&mut self.blocks);
            self.out
                .chapters
                .push((ChapterRef::new(self.book.num, self.chapter), blocks));
        }
    }

    fn start_block(&mut self, kind: BlockKind) {
        self.flush_block();
        if !kind.is_heading() && kind != BlockKind::Blank && kind != BlockKind::PsalmTitle {
            self.last_body_kind = kind.clone();
        }
        self.block = Some(Block::new(kind));
        self.at_word_start = true;
    }

    /// Make sure there is a block that can hold verse text.
    fn ensure_body_block(&mut self) {
        let needs_new = match &self.block {
            None => true,
            Some(b) => b.kind.is_heading() || b.kind == BlockKind::Blank,
        };
        if needs_new {
            let kind = self.last_body_kind.clone();
            self.start_block(kind);
        }
    }

    fn ensure_block(&mut self) {
        if self
            .block
            .as_ref()
            .is_none_or(|b| b.kind == BlockKind::Blank)
        {
            self.ensure_body_block();
        }
    }

    fn push_text(&mut self, raw: &str) {
        let normalized = raw.replace(['\n', '\r', '\t'], " ");
        let mut text = normalized.as_str();

        if let Some(note) = &mut self.note {
            if note.caller_pending {
                // First token after \f or \x is the caller ("+", "-", or a letter).
                text = text.trim_start();
                text = text
                    .get(text.chars().next().map_or(0, char::len_utf8)..)
                    .unwrap_or("");
                note.caller_pending = false;
            }
            if !note.skipping {
                note.text.push_str(text);
            }
            return;
        }

        self.ensure_block();

        // KJV marks paragraph starts with a pilcrow right after the verse number.
        if text.trim_start().starts_with('¶') {
            text = text.trim_start().trim_start_matches('¶');
            let block = self.block.as_mut().expect("block ensured");
            if block.spans.len() > 1 && matches!(block.spans.last(), Some(Span::Verse { .. })) {
                let verse_span = block.spans.pop().expect("checked");
                let kind = block.kind.clone();
                self.start_block(kind);
                self.block
                    .as_mut()
                    .expect("just started")
                    .spans
                    .push(verse_span);
            }
        }

        let mut text = text.to_string();
        while text.contains("  ") {
            text = text.replace("  ", " ");
        }
        if self.at_word_start {
            text = text.trim_start().to_string();
        }
        if text.is_empty() {
            return;
        }
        // Avoid double spaces across token boundaries.
        let block = self.block.as_mut().expect("block ensured");
        if text.starts_with(' ')
            && let Some(Span::Text { text: prev, .. }) = block.spans.last()
            && prev.ends_with(' ')
        {
            text.remove(0);
        }
        self.at_word_start = text.ends_with(' ');
        let style = style_for(&self.styles);
        let is_heading = block.kind.is_heading();
        block.push_text(&text, style);

        if !is_heading && let Some(verse) = self.verse {
            let entry = self.out.verses.entry(verse).or_default();
            entry.1.push_str(&text);
        }
    }

    fn text_token(&mut self, raw: &str) {
        let mut raw = raw;
        if self.ignore_line {
            match raw.find('\n') {
                Some(pos) => {
                    raw = &raw[pos + 1..];
                    self.ignore_line = false;
                }
                None => return,
            }
        }
        if let Some(marker) = self.expect_number.take() {
            let trimmed = raw.trim_start();
            let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
            let (number, rest) = trimmed.split_at(end);
            match marker {
                "c" => {
                    self.flush_chapter();
                    self.chapter = number.parse().unwrap_or(self.chapter + 1);
                    self.verse = None;
                }
                _ => {
                    if self.chapter == 0 {
                        return;
                    }
                    if let Some((num, end_verse)) = parse_verse_number(number) {
                        self.ensure_body_block();
                        let id = VerseId::new(self.book.num, self.chapter, num);
                        self.verse = Some(id);
                        self.out.verses.entry(id).or_default().0 = end_verse;
                        self.block
                            .as_mut()
                            .expect("block ensured")
                            .spans
                            .push(Span::Verse {
                                num,
                                end: end_verse,
                            });
                        self.at_word_start = true;
                    }
                }
            }
            raw = rest;
        }
        if self.chapter == 0 || raw.is_empty() {
            return;
        }
        if self.attr_skip {
            return;
        }
        if self.styles.last().is_some_and(|s| has_attributes(s))
            && let Some(pos) = raw.find('|')
        {
            self.attr_skip = true;
            raw = &raw[..pos];
        }
        if !raw.trim().is_empty() || !self.at_word_start {
            self.push_text(raw);
        }
    }

    fn marker(&mut self, name: &'n str, closing: bool, ends_line: bool) {
        if self.ignore_line {
            return;
        }
        if closing {
            self.close_marker(name);
            return;
        }
        match name {
            "c" | "v" => {
                self.expect_number = Some(if name == "c" { "c" } else { "v" });
                return;
            }
            "f" | "fe" | "x" => {
                self.note = Some(NoteState {
                    kind: if name == "x" {
                        NoteKind::CrossReference
                    } else {
                        NoteKind::Footnote
                    },
                    text: String::new(),
                    skipping: false,
                    caller_pending: true,
                });
                return;
            }
            _ => {}
        }
        if let Some(note) = &mut self.note {
            match name {
                "fr" | "xo" => note.skipping = true,
                "ref" => self.styles.push(name),
                _ => {
                    // \ft, \fq, \fqa, \xt, \fl, \fv, \fk ... all contribute text.
                    if !note.text.is_empty() && !note.text.ends_with(' ') && !note.skipping {
                        note.text.push(' ');
                    }
                    note.skipping = false;
                }
            }
            return;
        }
        if self.chapter == 0 {
            return;
        }
        if is_line_ignored(name) {
            self.ignore_line = !ends_line;
            return;
        }
        if let Some(kind) = paragraph_kind(name) {
            self.start_block(kind);
            return;
        }
        // Character marker.
        self.styles.push(name);
    }

    fn close_marker(&mut self, name: &str) {
        if matches!(name, "f" | "fe" | "x") {
            if let Some(note) = self.note.take() {
                let id = *self.next_note_id;
                *self.next_note_id += 1;
                let verse = self
                    .verse
                    .unwrap_or_else(|| VerseId::new(self.book.num, self.chapter, 0));
                self.out.notes.push(Note {
                    id,
                    kind: note.kind,
                    verse,
                    text: collapse_whitespace(&note.text),
                });
                self.ensure_block();
                self.block
                    .as_mut()
                    .expect("block ensured")
                    .spans
                    .push(Span::Note { id });
                self.at_word_start = false;
            }
            return;
        }
        if let Some(pos) = self.styles.iter().rposition(|s| *s == name) {
            self.styles.truncate(pos);
        }
        if has_attributes(name) {
            self.attr_skip = false;
        }
    }
}

/// Parse one USFM book. Returns `None` for books outside the 66-book canon
/// (deuterocanon, front matter, glossary).
pub fn parse_book(src: &str, next_note_id: &mut u32) -> Result<Option<ParsedBook>> {
    let tokens = tokenize(src);
    let code = match tokens.as_slice() {
        [Token::Marker { name: "id", .. }, Token::Text(t), ..] => {
            t.split_whitespace().next().unwrap_or_default()
        }
        _ => bail!("USFM file does not start with \\id"),
    };
    let Some(book) = book_by_code(code) else {
        return Ok(None);
    };

    let mut parser = Parser {
        book,
        chapter: 0,
        blocks: Vec::new(),
        block: None,
        last_body_kind: BlockKind::Paragraph,
        at_word_start: true,
        styles: Vec::new(),
        attr_skip: false,
        ignore_line: false,
        expect_number: None,
        verse: None,
        note: None,
        next_note_id,
        out: ParsedBook {
            book,
            chapters: Vec::new(),
            verses: BTreeMap::new(),
            notes: Vec::new(),
        },
    };
    for token in tokens {
        match token {
            Token::Marker {
                name,
                closing,
                ends_line,
            } => parser.marker(name, closing, ends_line),
            Token::Text(text) => parser.text_token(text),
        }
    }
    parser.flush_chapter();

    let mut out = parser.out;
    for (_, text) in out.verses.values_mut() {
        *text = collapse_whitespace(text);
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> ParsedBook {
        let mut id = 1;
        parse_book(src, &mut id).unwrap().unwrap()
    }

    fn texts(block: &Block) -> String {
        block
            .spans
            .iter()
            .map(|s| match s {
                Span::Verse { num, .. } => format!("[{num}]"),
                Span::Text { text, .. } => text.clone(),
                Span::Note { id } => format!("<{id}>"),
            })
            .collect()
    }

    #[test]
    fn headings_paragraphs_notes_and_red_letter() {
        let book = parse(
            "\\id JHN test\n\\h John\n\\mt1 John\n\\c 3\n\\s1 Jesus and Nicodemus\n\
             \\r (\\ref Genesis 22:1–10|GEN 22:1-10\\ref*)\n\
             \\p \\v 1 Now there was a man. \\v 2 He came to \\add Jesus\\add* at night.\n\
             \\p \\v 3 Jesus replied, \\wj “Truly, truly.\\f + \\fr 3:3 \\ft Or \\fqa born from above\\f*”\\wj*\n",
        );
        let (chapter, blocks) = &book.chapters[0];
        assert_eq!(*chapter, ChapterRef::new(43, 3));
        assert_eq!(blocks[0].kind, BlockKind::Heading(1));
        assert_eq!(texts(&blocks[0]), "Jesus and Nicodemus");
        assert_eq!(texts(&blocks[1]), "(Genesis 22:1–10)");
        assert_eq!(
            texts(&blocks[2]),
            "[1]Now there was a man. [2]He came to Jesus at night."
        );
        assert_eq!(texts(&blocks[3]), "[3]Jesus replied, “Truly, truly.<1>”");
        assert!(
            matches!(&blocks[2].spans[4], Span::Text { text, style } if text == "Jesus" && style.added)
        );
        assert!(matches!(&blocks[3].spans[2], Span::Text { style, .. } if style.jesus));

        assert_eq!(book.notes[0].text, "Or born from above");
        assert_eq!(book.notes[0].verse, VerseId::new(43, 3, 3));
        assert_eq!(
            book.verses[&VerseId::new(43, 3, 2)].1,
            "He came to Jesus at night."
        );
        assert_eq!(
            book.verses[&VerseId::new(43, 3, 3)].1,
            "Jesus replied, “Truly, truly.”"
        );
    }

    #[test]
    fn strongs_attributes_and_pilcrow() {
        let book = parse(
            "\\id JHN\n\\c 3\n\\p\n\\v 15 \\w That|strong=\"G2443\"\\w* whosoever.\n\
             \\v 16 \\wj ¶ \\+w For|strong=\"G1063\"\\+w* \\+w God|strong=\"G2316\"\\+w* so loved.\\wj*\n",
        );
        let blocks = &book.chapters[0].1;
        assert_eq!(blocks.len(), 2, "pilcrow starts a new paragraph");
        assert_eq!(texts(&blocks[0]), "[15]That whosoever.");
        assert_eq!(texts(&blocks[1]), "[16]For God so loved.");
        assert_eq!(book.verses[&VerseId::new(43, 3, 16)].1, "For God so loved.");
    }

    #[test]
    fn poetry_titles_ranges_and_chapter_continuation() {
        let book = parse(
            "\\id PSA\n\\c 23\n\\d \\v 1 A Psalm of David.\n\\q1 The LORD is my shepherd;\n\\q2 I shall not want.\n\
             \\b\n\\q1 \\v 2-3 He makes me lie down.\n\\c 24\n\\v 1 The earth is the \\nd Lord\\nd*’s.\n",
        );
        let c23 = &book.chapters[0].1;
        let kinds: Vec<_> = c23.iter().map(|b| b.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                BlockKind::PsalmTitle,
                BlockKind::Poetry(1),
                BlockKind::Poetry(2),
                BlockKind::Blank,
                BlockKind::Poetry(1)
            ]
        );
        assert_eq!(book.verses[&VerseId::new(19, 23, 2)].0, Some(3));
        let c24 = &book.chapters[1].1;
        assert_eq!(
            c24[0].kind,
            BlockKind::Poetry(1),
            "continues last body style"
        );
        assert!(
            matches!(&c24[0].spans[2], Span::Text { text, style } if text == "Lord" && style.divine_name)
        );
    }

    #[test]
    fn skips_non_canon_books() {
        let mut id = 1;
        assert!(
            parse_book("\\id TOB\n\\c 1\n\\p \\v 1 x", &mut id)
                .unwrap()
                .is_none()
        );
    }
}
