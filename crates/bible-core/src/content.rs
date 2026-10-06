//! Translation-neutral model of a chapter's formatted text.
//!
//! The importer turns USFM into `Vec<Block>` per chapter; the app renders it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    /// Major section heading (`\ms`).
    MajorHeading,
    /// Section heading (`\s1`, `\s2`), with level.
    Heading(u8),
    /// Parallel passage references under a heading (`\r`).
    ParallelRefs,
    /// Psalm superscription (`\d`).
    PsalmTitle,
    /// Acrostic or speaker label (`\qa`, `\sp`).
    Label,
    /// Normal paragraph (`\p`, `\pi`, `\nb`, `\lf`).
    Paragraph,
    /// Flush-left paragraph continuing a previous one (`\m`, `\mi`).
    Continuation,
    /// Poetry line with indent level (`\q1`, `\q2`, `\q3`).
    Poetry(u8),
    /// Right-aligned poetry line (`\qr`).
    PoetryRight,
    /// List item with indent level (`\li1`, `\li2`).
    ListItem(u8),
    /// Centered paragraph (`\pc`).
    Centered,
    /// Stanza break (`\b`).
    Blank,
}

impl BlockKind {
    pub fn is_heading(&self) -> bool {
        matches!(
            self,
            BlockKind::MajorHeading
                | BlockKind::Heading(_)
                | BlockKind::ParallelRefs
                | BlockKind::Label
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextStyle {
    /// Words of Jesus (`\wj`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub jesus: bool,
    /// Translator-supplied words (`\add`), traditionally italic.
    #[serde(default, skip_serializing_if = "is_false")]
    pub added: bool,
    /// Divine name, e.g. LORD (`\nd`), traditionally small caps.
    #[serde(default, skip_serializing_if = "is_false")]
    pub divine_name: bool,
    /// Other emphasis (`\tl`, `\bk`, `\k`, `\qs`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub emphasis: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Span {
    /// Start of a verse. `end` is set for merged ranges like `\v 15-16`.
    Verse { num: u16, end: Option<u16> },
    Text {
        text: String,
        #[serde(default)]
        style: TextStyle,
    },
    /// Footnote or cross-reference marker; `id` indexes the translation's notes table.
    Note { id: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub kind: BlockKind,
    pub spans: Vec<Span>,
}

impl Block {
    pub fn new(kind: BlockKind) -> Self {
        Self {
            kind,
            spans: Vec::new(),
        }
    }

    /// Append text, merging with the previous span when the style matches.
    pub fn push_text(&mut self, text: &str, style: TextStyle) {
        if text.is_empty() {
            return;
        }
        if let Some(Span::Text {
            text: prev,
            style: prev_style,
        }) = self.spans.last_mut()
            && *prev_style == style
        {
            prev.push_str(text);
            return;
        }
        self.spans.push(Span::Text {
            text: text.to_string(),
            style,
        });
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    Footnote,
    CrossReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: u32,
    pub kind: NoteKind,
    pub verse: crate::VerseId,
    pub text: String,
}
