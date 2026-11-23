//! haiyaku (配役, "the cast") stores what happens to a book's characters as a log of events in SQLite, and
//! answers who they are and how they relate as of the chapter a reader has reached, so that nothing from later
//! in the book shows; given a passage, it says which of those characters the passage names. It opens no database
//! of its own: every function that reads or writes one takes the caller's [`rusqlite::Connection`], and its
//! tables, all named `haiyaku_...`, live beside the caller's own.

#[cfg(feature = "extract")]
pub mod book;
pub mod cast;
mod error;
#[cfg(feature = "extract")]
pub mod extract;
pub mod fold;
#[cfg(feature = "extract")]
pub mod link;
#[cfg(feature = "extract")]
pub mod llm;
pub mod log;
pub mod lookup;
pub mod store;
mod words;

pub use cast::{Scope, cast_at, fold_series};
pub use error::{Error, Result};
pub use lookup::{Mention, NamePart, characters_named};
