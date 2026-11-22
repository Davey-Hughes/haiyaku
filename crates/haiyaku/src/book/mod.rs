//! The text model and the EPUB reader: a [`Book`] as extraction reads it, its spine documents in order and its
//! chapters, each a run of whole documents labelled from the table of contents. A document is a list of
//! [`Block`]s, the text of its headings and paragraphs, which [`blocks`] reads out of its XHTML.

mod epub;
mod xhtml;

pub use epub::{Book, ChapterText, PlacedBlock, SpineDocument, TocEntry};
pub use xhtml::blocks;

/// One heading or paragraph of a document, the unit a [`Position`](crate::log::Position) counts in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    /// The text with its markup gone: character references decoded, soft hyphens dropped, whitespace
    /// collapsed to single spaces and trimmed. Never empty.
    pub text: String,
}

/// What a [`Block`] is in its document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// Text that began inside `h1` to `h6`.
    Heading,
    /// Any other text of the body.
    Paragraph,
}
