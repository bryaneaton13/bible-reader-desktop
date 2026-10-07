//! Bible domain types with no UI or storage dependencies.

pub mod books;
pub mod content;
pub mod reference;
pub mod text;
pub mod verse;

pub use books::{BOOKS, Book, Testament, book_by_code, book_by_num, parse_osis, parse_osis_range};
pub use content::{Block, BlockKind, Note, NoteKind, Span, TextStyle};
pub use reference::{Reference, find_book, parse_reference};
pub use verse::{ChapterRef, VerseId};
